//! Source-safe candidate census for independent T352 fixtures.
//!
//! The core structural manifest already enforces a mature 0x2C source graph,
//! unique geometry crosswalk, and source-bound CFB stream provenance. This
//! preflight applies the additional ClientData identity/equality admission
//! without modifying either the PUB or the established T370 selected target.
use anyhow::{Context, Result, bail};
use pub_contents::{BLOCK_TYPE_U32, Contents0x2cChunk, RawContentsBlockBody};
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE,
    PUBLISHER_FIELD_YS, PublisherFieldRecord,
};
use pub_reader::build_mature_0x2c_structural_base_manifest;
use serde_json::json;
use std::{collections::BTreeMap, env, fs};

fn unique_dimension(chunk: &Contents0x2cChunk, id: u16) -> Option<i64> {
    let mut matches = chunk.fields.iter().filter(|f| f.id == id);
    let field = matches.next()?;
    if matches.next().is_some() || field.block_type != BLOCK_TYPE_U32 {
        return None;
    }
    match &field.body {
        RawContentsBlockBody::U32 { value, .. } => Some(i64::from(*value)),
        _ => None,
    }
}

fn unique_value(fields: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = fields.values(id);
    let value = matches.next()?;
    matches.next().is_none().then_some(value)
}

fn unique_signed(fields: &PublisherFieldRecord, id: u16) -> Option<i64> {
    unique_value(fields, id).map(|x| i64::from(x as i32))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = args.next().context("usage: t352_crosswalk_census SOURCE.pub OUTPUT.json")?;
    let output = args.next().context("usage: t352_crosswalk_census SOURCE.pub OUTPUT.json")?;
    if args.next().is_some() {
        bail!("usage: t352_crosswalk_census SOURCE.pub OUTPUT.json");
    }

    let source = fs::read(&source).context("read exact pinned source PUB")?;
    let manifest = build_mature_0x2c_structural_base_manifest(&source)
        .context("build pre-existing mature-0x2c candidate manifest")?;

    // Deliberately allow zero admitted candidates: it is a useful negative
    // preflight result rather than proof that Publisher refuses the source.
    let mut rows = Vec::new();
    let mut ids: BTreeMap<u32, u32> = BTreeMap::new();
    let mut spids: BTreeMap<u32, u32> = BTreeMap::new();
    for shape in &manifest.candidates {
        let Some(w) = unique_dimension(&shape.contents_chunk, 0x00AA) else { continue };
        let Some(h) = unique_dimension(&shape.contents_chunk, 0x00AB) else { continue };
        let (Some(anchor), Some(client_data), Some(fsp)) = (
            &shape.escher_shape.client_anchor,
            &shape.escher_shape.client_data,
            &shape.escher_shape.fsp,
        ) else { continue };
        let (Some(x0), Some(y0), Some(x1), Some(y1), Some(shape_id)) = (
            unique_signed(anchor, PUBLISHER_FIELD_XS),
            unique_signed(anchor, PUBLISHER_FIELD_YS),
            unique_signed(anchor, PUBLISHER_FIELD_XE),
            unique_signed(anchor, PUBLISHER_FIELD_YE),
            unique_value(client_data, PUBLISHER_FIELD_SHAPE_ID),
        ) else { continue };
        let Some(aw) = x1.checked_sub(x0) else { continue };
        let Some(ah) = y1.checked_sub(y0) else { continue };
        if w <= 0 || h <= 0 || aw <= 0 || ah <= 0 { continue };
        if u64::try_from(w + 127_000).is_err() || w + 127_000 > i64::from(u32::MAX) {
            continue;
        }
        // Explicit width/height identity and no duplicate shape/carrier across
        // accepted candidates; do not admit equal extents alone as identity.
        let joined = shape_id == shape.contents_seq_num && w == aw && h == ah;
        if joined {
            *ids.entry(shape.contents_seq_num).or_default() += 1;
            *spids.entry(fsp.spid).or_default() += 1;
        }
        rows.push(json!({
            "contents_seq": shape.contents_seq_num,
            "client_data_shape_id": shape_id,
            "spid": fsp.spid,
            "shape_type": fsp.shape_type,
            "contents_width_emu": w,
            "contents_height_emu": h,
            "anchor_width_emu": aw,
            "anchor_height_emu": ah,
            "anchor_xs_emu": x0,
            "anchor_xe_emu": x1,
            "joined_geometry_and_identity": joined,
            "different_from_t352_original_width": w != 5_076_000
        }));
    }

    for row in &mut rows {
        let seq = row["contents_seq"].as_u64().unwrap_or(u64::MAX) as u32;
        let spid = row["spid"].as_u64().unwrap_or(u64::MAX) as u32;
        let unique = ids.get(&seq) == Some(&1) && spids.get(&spid) == Some(&1);
        let novel_width = row["different_from_t352_original_width"].as_bool() == Some(true);
        row["admitted_independent_target"] = json!(
            unique && novel_width && row["joined_geometry_and_identity"] == true
        );
    }
    let admitted = rows.iter().filter(|r| r["admitted_independent_target"] == true).count();
    let data = json!({
        "schema": "chaptera.t352-independent-preflight.v1",
        "source_sha256": manifest.source_sha256,
        "source_length": source.len(),
        "logical_stream_count": manifest.streams.len(),
        "raw_geometry_candidate_count": manifest.candidates.len(),
        "joined_candidate_count": rows.iter().filter(|r| r["joined_geometry_and_identity"] == true).count(),
        "admitted_independent_target_count": admitted,
        "verdict": if admitted > 0 { "has_candidates_not_native_proven" } else { "no_admissible_target" },
        "candidates": rows,
        "native_publisher_result": "not_executed",
        "format_law_credit": "none"
    });
    fs::write(output, serde_json::to_vec_pretty(&data)?)?;
    Ok(())
}
