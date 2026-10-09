//! Identity-first source-safe crosswalk census for independent T352 PUBs.
//!
//! Uses the existing pub-contents / pub-escher parsers. In particular it does
//! NOT require equal geometric extents to *find* a shape: geometry is checked
//! only after the unique Contents positional ID == Escher ClientData shape ID
//! join. This avoids the known same-size ambiguity on SampleNewsletter.
use anyhow::{Context, Result, bail};
use pub_contents::{
    BLOCK_TYPE_U32, RawContentsBlock, RawContentsBlockBody, parse_0x2c_header,
    parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference,
};
use pub_core::{RawSpan, StreamPath};
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE,
    PUBLISHER_FIELD_YS, PublisherField, PublisherFieldRecord, inspect_sp_containers,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, io::Cursor};

const CONTENTS_PATH: &str = "/Contents";
const ESCHER_PATH: &str = "/Escher/EscherStm";
const DELTA_EMU: i64 = 127_000;
const MAX_SLOTS: usize = 40_000;
const MAX_PUB_BYTES: usize = 32 * 1024 * 1024;

fn unique_dimension(fields: &[RawContentsBlock], id: u16) -> Option<(i64, RawSpan)> {
    let mut matches = fields.iter().filter(|f| f.id == id);
    let field = matches.next()?;
    if matches.next().is_some() || field.block_type != BLOCK_TYPE_U32 {
        return None;
    }
    match &field.body {
        RawContentsBlockBody::U32 { value, value_source } if value_source.len == 4 =>
            Some((i64::from(*value), value_source.clone())),
        _ => None,
    }
}

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<&PublisherField> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let field = matches.next()?;
    matches.next().is_none().then_some(field)
}

fn unique_signed(record: &PublisherFieldRecord, id: u16) -> Option<i64> {
    unique_field(record, id).map(|field| i64::from(field.value as i32))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = args.next().context("usage: t352_crosswalk_census INPUT.pub OUTPUT.json")?;
    let output = args.next().context("usage: t352_crosswalk_census INPUT.pub OUTPUT.json")?;
    if args.next().is_some() {
        bail!("usage: t352_crosswalk_census INPUT.pub OUTPUT.json");
    }
    let raw = fs::read(&source).context("read exact fixture bytes")?;
    if raw.len() < 512 || raw.len() > MAX_PUB_BYTES {
        bail!("source byte size outside bounded PUB census policy");
    }
    let digest = Sha256::digest(&raw);
    let source_sha = digest.iter().map(|x| format!("{x:02x}")).collect::<String>();

    let contents = pub_cfb::read_stream_reader(Cursor::new(&raw), CONTENTS_PATH)
        .context("read /Contents using canonical CFB reader")?;
    let header = parse_0x2c_header(StreamPath(CONTENTS_PATH.into()), &contents)
        .context("parse mature 0x2C Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse mature 0x2C Contents trailer")?;
    let slot_count = trailer.directory.slots.len();
    if slot_count > MAX_SLOTS {
        bail!("Contents directory slot_count exceeds bounded T352 preflight");
    }
    // One *unique positional* shape identifier maps to at most one decoded
    // /Contents AA/AB pair and its exact scalar source spans.
    let mut content_dims: BTreeMap<u32, ((i64, RawSpan), (i64, RawSpan))> = BTreeMap::new();
    for index in 0..slot_count {
        let Some(reference) =
            parse_confirmed_chunk_reference(&contents, &trailer.directory, index)
                .with_context(|| format!("read occupied Contents slot {index}"))?
        else {
            continue;
        };
        if reference.raw_types.len() != 1
            || reference.raw_types[0].value != 0x01
            || reference.chunk_offsets.len() != 1
        {
            continue;
        }
        let chunk = parse_confirmed_0x2c_chunk(
            StreamPath(CONTENTS_PATH.into()),
            &contents,
            reference.chunk_offsets[0].value,
        )?;
        if !chunk.is_fully_decoded() {
            continue;
        }
        let (Some(width), Some(height)) = (
            unique_dimension(&chunk.fields, 0x00AA),
            unique_dimension(&chunk.fields, 0x00AB),
        ) else {
            continue;
        };
        if width.0 <= 0 || height.0 <= 0 {
            continue;
        }
        let seq = u32::try_from(index).context("Contents seq outside u32 bounds")?;
        content_dims.insert(seq, (width, height));
    }

    let escher = pub_cfb::read_stream_reader(Cursor::new(&raw), ESCHER_PATH)
        .context("read /Escher/EscherStm using canonical CFB reader")?;
    let escher_shapes =
        inspect_sp_containers(StreamPath(ESCHER_PATH.into()), &escher)
            .context("parse Escher SpContainer inventory")?;
    let mut rows: Vec<Value> = Vec::new();
    let mut id_counts: BTreeMap<u32, u32> = BTreeMap::new();
    let mut spid_counts: BTreeMap<u32, u32> = BTreeMap::new();
    for shape in &escher_shapes.shapes {
        let (Some(anchor), Some(data), Some(fsp)) =
            (&shape.client_anchor, &shape.client_data, &shape.fsp)
        else {
            continue;
        };
        let Some(identity_field) = unique_field(data, PUBLISHER_FIELD_SHAPE_ID) else {
            continue;
        };
        let id = identity_field.value;
        let Some(((width, width_span), (height, height_span))) = content_dims.get(&id)
        else {
            continue;
        };
        let (Some(xs), Some(ys), Some(xe), Some(ye), Some(xe_field)) = (
            unique_signed(anchor, PUBLISHER_FIELD_XS),
            unique_signed(anchor, PUBLISHER_FIELD_YS),
            unique_signed(anchor, PUBLISHER_FIELD_XE),
            unique_signed(anchor, PUBLISHER_FIELD_YE),
            unique_field(anchor, PUBLISHER_FIELD_XE),
        ) else {
            continue;
        };
        let (Some(aw), Some(ah)) = (xe.checked_sub(xs), ye.checked_sub(ys))
        else {
            continue;
        };
        if aw <= 0 || ah <= 0 || xe_field.source.len != 6
            || width_span.len != 4 || height_span.len != 4
        {
            continue;
        }
        // The T352 mutation must remain positive, bounded U32 width and a
        // signed 32-bit Escher coordinate; it must not alter stream lengths.
        let patchable = width.checked_add(DELTA_EMU).is_some_and(|next|
            next <= i64::from(u32::MAX))
            && xe.checked_add(DELTA_EMU).is_some_and(|next|
                next <= i64::from(i32::MAX));
        *id_counts.entry(id).or_default() += 1;
        *spid_counts.entry(fsp.spid).or_default() += 1;
        rows.push(json!({
            "contents_seq": id,
            "client_data_shape_id": id,
            "spid": fsp.spid,
            "shape_type": fsp.shape_type,
            "contents_width_emu": width,
            "contents_height_emu": height,
            "anchor_width_emu": aw,
            "anchor_height_emu": ah,
            "anchor_xs_emu": xs,
            "anchor_xe_emu": xe,
            "width_value_offset_in_contents": width_span.offset,
            "height_value_offset_in_contents": height_span.offset,
            "xe_tagged_field_offset_in_escher": xe_field.source.offset,
            "identity_joined_even_if_extents_differ": true,
            "both_geometries_consistent": *width == aw && *height == ah,
            "different_from_original_t352_width": *width != 5_076_000,
            "numeric_patchable": patchable
        }));
    }
    for row in &mut rows {
        let id = row["contents_seq"].as_u64().context("missing content id")? as u32;
        let spid = row["spid"].as_u64().context("missing Escher spid")? as u32;
        let unique = id_counts.get(&id) == Some(&1) && spid_counts.get(&spid) == Some(&1);
        row["unique_join"] = json!(unique);
        row["admitted_independent_target"] = json!(
            unique
                && row["both_geometries_consistent"] == true
                && row["different_from_original_t352_width"] == true
                && row["numeric_patchable"] == true
        );
    }
    let admitted = rows.iter().filter(|row| row["admitted_independent_target"] == true).count();
    let result = json!({
        "schema": "chaptera.t352-independent-identity-first-preflight.v1",
        "source_sha256": source_sha,
        "source_length": raw.len(),
        "contents_shape_with_dimensions_count": content_dims.len(),
        "escher_shape_container_count": escher_shapes.shapes.len(),
        "identity_joined_pair_count": rows.len(),
        "admitted_independent_target_count": admitted,
        "verdict": if admitted > 0 { "admitted_raw_candidates_require_native_normalization" }
                   else { "no_raw_candidate_require_independent_native_check" },
        "rows": rows,
        "native_publisher_result": "not_executed",
        "format_law_credit": "none"
    });
    fs::write(output, serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
