use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use pub_escher::{
    PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE, PUBLISHER_FIELD_YS,
    PublisherFieldRecord,
};
use pub_model::{NodeId, PageId, RectEmu, Sha256Digest};
use pub_reader::{
    PUB_STRUCTURAL_BASE_SCHEMA_V1, PubStructuralBaseManifest,
    build_mature_0x2c_structural_base_manifest,
};
use serde::Serialize;

const RECEIPT_SCHEMA: &str = "chaptera.modern-structural-base/v1";

#[derive(Debug, Serialize)]
struct SelectedTarget {
    page_id: PageId,
    node_id: NodeId,
    contents_seq_num: u32,
    bounds_emu: RectEmu,
    officeart_spid: u32,
    officeart_shape_type: u16,
    anchor_xs: i64,
    anchor_ys: i64,
    anchor_xe: i64,
    anchor_ye: i64,
    anchor_width_emu: i64,
    anchor_height_emu: i64,
    bounds_anchor_exact: bool,
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

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<i64> {
    let values = record.values(id).collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Some(i64::from(*value)),
        _ => None,
    }
}

fn select_target(manifest: &PubStructuralBaseManifest) -> Result<SelectedTarget> {
    for candidate in &manifest.candidates {
        let Some(anchor) = candidate.escher_shape.client_anchor.as_ref() else {
            continue;
        };
        let (Some(xs), Some(ys), Some(xe), Some(ye)) = (
            unique_field(anchor, PUBLISHER_FIELD_XS),
            unique_field(anchor, PUBLISHER_FIELD_YS),
            unique_field(anchor, PUBLISHER_FIELD_XE),
            unique_field(anchor, PUBLISHER_FIELD_YE),
        ) else {
            continue;
        };

        let anchor_width = xe - xs;
        let anchor_height = ye - ys;
        let exact = candidate.bounds_emu.x.get() == xs
            && candidate.bounds_emu.y.get() == ys
            && candidate.bounds_emu.width.get() == anchor_width
            && candidate.bounds_emu.height.get() == anchor_height;
        if !exact {
            continue;
        }

        return Ok(SelectedTarget {
            page_id: candidate.page_id,
            node_id: candidate.node_id,
            contents_seq_num: candidate.contents_seq_num,
            bounds_emu: candidate.bounds_emu,
            officeart_spid: candidate.officeart_spid,
            officeart_shape_type: candidate.officeart_shape_type,
            anchor_xs: xs,
            anchor_ys: ys,
            anchor_xe: xe,
            anchor_ye: ye,
            anchor_width_emu: anchor_width,
            anchor_height_emu: anchor_height,
            bounds_anchor_exact: true,
        });
    }

    bail!(
        "no bounded ordinary shape candidate has an exact four-field ClientAnchor/bounds crosswalk"
    )
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(args.next().context("usage: structural_base_manifest <input.pub> <output.json>")?);
    let output = PathBuf::from(args.next().context("usage: structural_base_manifest <input.pub> <output.json>")?);
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
