use std::{env, fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use pub_contents::{Contents0x2cChunk, RawContentsBlockBody, BLOCK_TYPE_U32};
use pub_core::RawSpan;
use pub_escher::{
    PublisherFieldRecord, PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE,
    PUBLISHER_FIELD_YS,
};
use pub_model::{NodeId, PageId, Sha256Digest};
use pub_reader::{
    build_mature_0x2c_structural_base_manifest, PubStructuralBaseManifest,
    PUB_STRUCTURAL_BASE_SCHEMA_V1,
};
use serde::Serialize;

const RECEIPT_SCHEMA: &str = "chaptera.modern-structural-base/v1";
const CONTENTS_SHAPE_WIDTH_ID: u16 = 0x00AA;
const CONTENTS_SHAPE_HEIGHT_ID: u16 = 0x00AB;

#[derive(Debug, Clone)]
struct ContentsDimension {
    value_emu: i64,
    source: RawSpan,
    wire_type: u8,
}

#[derive(Debug, Serialize)]
struct SelectedTarget {
    page_id: PageId,
    node_id: NodeId,
    contents_seq_num: u32,
    contents_width_emu: i64,
    contents_height_emu: i64,
    contents_width_source: RawSpan,
    contents_height_source: RawSpan,
    contents_width_wire_type: u8,
    contents_height_wire_type: u8,
    officeart_spid: u32,
    officeart_shape_type: u16,
    anchor_xs: i64,
    anchor_ys: i64,
    anchor_xe: i64,
    anchor_ye: i64,
    anchor_width_emu: i64,
    anchor_height_emu: i64,
    contents_anchor_extent_exact: bool,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: Sha256Digest,
    structural_schema: &'static str,
    stream_count: usize,
    candidate_count: usize,
    selected_target: SelectedTarget,
    manifest: PubStructuralBaseManifest,
}

fn unique_signed_anchor_field(record: &PublisherFieldRecord, id: u16) -> Option<i64> {
    let values = record.values(id).collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Some(i64::from(i32::from_le_bytes(value.to_le_bytes()))),
        _ => None,
    }
}

fn unique_contents_dimension(chunk: &Contents0x2cChunk, id: u16) -> Option<ContentsDimension> {
    let matches = chunk
        .fields
        .iter()
        .filter(|field| field.id == id)
        .collect::<Vec<_>>();
    let [field] = matches.as_slice() else {
        return None;
    };
    if field.block_type != BLOCK_TYPE_U32 {
        return None;
    }

    match &field.body {
        RawContentsBlockBody::U32 {
            value,
            value_source,
        } => Some(ContentsDimension {
            value_emu: i64::from(*value),
            source: value_source.clone(),
            wire_type: field.block_type,
        }),
        _ => None,
    }
}

fn select_target(manifest: &PubStructuralBaseManifest) -> Result<SelectedTarget> {
    for candidate in &manifest.candidates {
        let (Some(contents_width), Some(contents_height)) = (
            unique_contents_dimension(&candidate.contents_chunk, CONTENTS_SHAPE_WIDTH_ID),
            unique_contents_dimension(&candidate.contents_chunk, CONTENTS_SHAPE_HEIGHT_ID),
        ) else {
            continue;
        };
        if contents_width.value_emu <= 0 || contents_height.value_emu <= 0 {
            continue;
        }

        let Some(anchor) = candidate.escher_shape.client_anchor.as_ref() else {
            continue;
        };
        let (Some(xs), Some(ys), Some(xe), Some(ye)) = (
            unique_signed_anchor_field(anchor, PUBLISHER_FIELD_XS),
            unique_signed_anchor_field(anchor, PUBLISHER_FIELD_YS),
            unique_signed_anchor_field(anchor, PUBLISHER_FIELD_XE),
            unique_signed_anchor_field(anchor, PUBLISHER_FIELD_YE),
        ) else {
            continue;
        };

        let anchor_width = xe.checked_sub(xs).context("Escher anchor width overflow")?;
        let anchor_height = ye
            .checked_sub(ys)
            .context("Escher anchor height overflow")?;
        if anchor_width <= 0 || anchor_height <= 0 {
            continue;
        }

        let exact =
            contents_width.value_emu == anchor_width && contents_height.value_emu == anchor_height;
        if !exact {
            continue;
        }

        return Ok(SelectedTarget {
            page_id: candidate.page_id,
            node_id: candidate.node_id,
            contents_seq_num: candidate.contents_seq_num,
            contents_width_emu: contents_width.value_emu,
            contents_height_emu: contents_height.value_emu,
            contents_width_source: contents_width.source,
            contents_height_source: contents_height.source,
            contents_width_wire_type: contents_width.wire_type,
            contents_height_wire_type: contents_height.wire_type,
            officeart_spid: candidate.officeart_spid,
            officeart_shape_type: candidate.officeart_shape_type,
            anchor_xs: xs,
            anchor_ys: ys,
            anchor_xe: xe,
            anchor_ye: ye,
            anchor_width_emu: anchor_width,
            anchor_height_emu: anchor_height,
            contents_anchor_extent_exact: true,
        });
    }

    bail!(
        "no bounded ordinary shape candidate has exact Contents 0xAA/0xAB dimensions matching a unique signed Escher ClientAnchor extent"
    )
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .context("usage: structural_base_manifest <input.pub> <output.json>")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: structural_base_manifest <input.pub> <output.json>")?,
    );
    if args.next().is_some() {
        bail!("usage: structural_base_manifest <input.pub> <output.json>");
    }

    let bytes = fs::read(&input).with_context(|| format!("read {}", input.display()))?;
    let manifest = build_mature_0x2c_structural_base_manifest(&bytes)
        .with_context(|| format!("build structural base for {}", input.display()))?;
    let selected_target = select_target(&manifest)?;

    let receipt = Receipt {
        schema: RECEIPT_SCHEMA,
        source_sha256: manifest.source_sha256,
        structural_schema: PUB_STRUCTURAL_BASE_SCHEMA_V1,
        stream_count: manifest.streams.len(),
        candidate_count: manifest.candidates.len(),
        selected_target,
        manifest,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize structural base receipt")?,
    )
    .with_context(|| format!("write {}", output.display()))?;

    Ok(())
}
