//! T352: bounded in-place CFB stream patching, without touching the source PUB.
//!
//! Usage:
//!   cargo run --bin val_xproj_patch -- prepare BASE.pub STRUCTURAL.json OUT_DIR
//!   cargo run --bin val_xproj_patch -- fingerprint FILE.pub OUT.json
//!
//! The structural JSON is produced by the existing structural_base_manifest
//! binary on this *same exact* Publisher-normalized base. No semantic
//! classification is inferred here: this tool creates controls for the real
//! Publisher Open -> Save -> fresh Reopen experiment.
use anyhow::{Context, Result, bail};
use pub_contents::{
    RawContentsBlockBody, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE,
    PUBLISHER_FIELD_YS, PublisherFieldRecord, inspect_sp_containers,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

const DELTA_EMU: i64 = 127_000; // exactly +10 pt
const BASE_WIDTH_EMU: i64 = 5_076_000;
const BASE_XE_EMU: i64 = 2_376_000;

fn sha(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn require_i64<'a>(value: &'a Value, name: &str) -> Result<i64> {
    value
        .get(name)
        .and_then(Value::as_i64)
        .with_context(|| format!("missing signed integer {name}"))
}

fn require_str<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string {name}"))
}

fn uint32_bytes(value: i64) -> Result<[u8; 4]> {
    Ok(u32::try_from(value)
        .context("U32 value outside bounds")?
        .to_le_bytes())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Span {
    stream: String,
    offset: u64,
    len: u64,
}

fn span(value: &Value) -> Result<Span> {
    let stream = require_str(value, "stream")?.to_owned();
    let offset = value
        .get("offset")
        .and_then(Value::as_u64)
        .context("span missing offset")?;
    let len = value
        .get("len")
        .and_then(Value::as_u64)
        .context("span missing len")?;
    if !stream.starts_with('/') || stream.contains("..") {
        bail!("span stream must be an absolute CFB logical path");
    }
    Ok(Span {
        stream,
        offset,
        len,
    })
}

#[derive(Debug)]
struct Patch {
    span: Span, // the exact 4-byte value, not the surrounding tag
    expected: [u8; 4],
    replacement: [u8; 4],
}

fn plan(receipt: &Value) -> Result<(Patch, Patch)> {
    if require_str(receipt, "schema")? != "chaptera.modern-structural-base/v1" {
        bail!("unsupported structural receipt schema");
    }
    if require_i64(receipt, "stream_count")? != 10 || require_i64(receipt, "candidate_count")? != 6
    {
        bail!("T370 semantic contract: unexpected stream/candidate counts");
    }
    let target = receipt
        .get("selected_target")
        .context("missing selected_target")?;
    for (name, expected) in [
        ("contents_seq_num", 293),
        ("publisher_shape_id", 293),
        ("officeart_spid", 1025),
        ("officeart_shape_type", 202),
        ("contents_width_emu", BASE_WIDTH_EMU),
        ("contents_height_emu", 972_000),
        ("anchor_xs", -2_700_000),
        ("anchor_ys", -4_446_000),
        ("anchor_xe", BASE_XE_EMU),
        ("anchor_ye", -3_474_000),
    ] {
        if require_i64(target, name)? != expected {
            bail!("T370 target semantic contract mismatch: {name}");
        }
    }
    if target
        .get("contents_anchor_extent_exact")
        .and_then(Value::as_bool)
        != Some(true)
    {
        bail!("T370 Contents/Escher invariant absent");
    }
    if require_i64(target, "contents_width_wire_type")? != 0x20 {
        // The U32 tag's block_type is defined by pub-contents; a changed
        // encoding requires explicit review, never guessing a 4-byte patch.
        bail!("Contents width is not a known U32 wire type");
    }

    let cw = span(
        target
            .get("contents_width_source")
            .context("missing Contents width span")?,
    )?;
    if cw.stream != "/Contents" || cw.len != 4 {
        bail!("Contents width must be an exact four-byte /Contents value");
    }
    let candidates = receipt
        .pointer("/manifest/candidates")
        .and_then(Value::as_array)
        .context("missing structural candidates")?;
    let matched: Vec<_> = candidates
        .iter()
        .filter(|v| {
            v.get("contents_seq_num").and_then(Value::as_i64) == Some(293)
                && v.get("officeart_spid").and_then(Value::as_i64) == Some(1025)
        })
        .collect();
    if matched.len() != 1 {
        bail!("selected shape must have exactly one Contents/Escher joined candidate");
    }
    let fields = matched[0]
        .pointer("/escher_shape/client_anchor/fields")
        .and_then(Value::as_array)
        .context("missing selected ClientAnchor fields")?;
    let xe: Vec<_> = fields
        .iter()
        .filter(|v| v.get("id").and_then(Value::as_u64) == Some(0x2003))
        .collect();
    if xe.len() != 1 || xe[0].get("value").and_then(Value::as_u64) != Some(BASE_XE_EMU as u64) {
        bail!("Escher ClientAnchor XE must be present exactly once with expected value");
    }
    let tagged = span(xe[0].get("source").context("missing XE source")?)?;
    if tagged.len != 6 {
        bail!("Escher PublisherField must be exactly 2-byte ID + 4-byte value");
    }
    let value_offset = tagged.offset.checked_add(2).context("XE offset overflow")?;
    let ex = Span {
        stream: tagged.stream,
        offset: value_offset,
        len: 4,
    };
    if ex.stream != "/Escher/EscherStm" {
        bail!("Escher XE must belong to the pinned /Escher/EscherStm stream");
    }

    Ok((
        Patch {
            span: cw,
            expected: uint32_bytes(BASE_WIDTH_EMU)?,
            replacement: uint32_bytes(BASE_WIDTH_EMU + DELTA_EMU)?,
        },
        Patch {
            span: ex,
            expected: uint32_bytes(BASE_XE_EMU)?,
            replacement: uint32_bytes(BASE_XE_EMU + DELTA_EMU)?,
        },
    ))
}

fn all_streams(path: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    // CFB entry.path() is a std::path::Path and serializing it directly
    // produces platform-native separators (backslashes on Windows).
    // structural_base_manifest uses pub-cfb's canonical slash paths.
    // Reuse exactly that established inventory/stream reader.
    let inventory =
        pub_cfb::inspect_path(path).with_context(|| format!("inspect CFB {}", path.display()))?;
    let mut streams = BTreeMap::new();
    for entry in inventory
        .entries
        .into_iter()
        .filter(|entry| entry.kind == pub_cfb::EntryKind::Stream)
    {
        let content = pub_cfb::read_stream_path(path, &entry.path)?;
        if content.len() as u64 != entry.len {
            bail!("CFB inventory stream length mismatch: {}", entry.path);
        }
        if streams.insert(entry.path, content).is_some() {
            bail!("duplicate canonical CFB stream");
        }
    }
    Ok(streams)
}

fn check_structural_streams(receipt: &Value, actual: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let expected = receipt
        .pointer("/manifest/streams")
        .and_then(Value::as_array)
        .context("missing structural stream digests")?;
    if expected.len() != 10 || actual.len() != 10 {
        bail!("T370 requires exactly ten named CFB streams");
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in expected {
        let path = require_str(entry, "path")?;
        let expected_len = entry
            .get("len")
            .and_then(Value::as_u64)
            .context("missing stream length")?;
        let expected_sha = require_str(entry, "sha256")?;
        if !seen.insert(path) {
            bail!("duplicate structural stream path");
        }
        let bytes = actual
            .get(path)
            .context("structural stream absent from actual CFB")?;
        if bytes.len() as u64 != expected_len || sha(bytes) != expected_sha {
            bail!("structural stream hash or length drift: {path}");
        }
    }
    Ok(())
}

fn patch_one(path: &Path, plan: &Patch) -> Result<()> {
    let mut comp = cfb::open_rw(path).context("open copied PUB for exact-range patch")?;
    let mut stream = comp
        .open_stream(&plan.span.stream)
        .with_context(|| format!("missing planned stream {}", plan.span.stream))?;
    if plan.span.len != 4
        || plan
            .span
            .offset
            .checked_add(4)
            .is_none_or(|n| n > stream.len())
    {
        bail!("planned four-byte patch lies outside logical CFB stream");
    }
    stream.seek(SeekFrom::Start(plan.span.offset))?;
    let mut actual = [0u8; 4];
    stream.read_exact(&mut actual)?;
    if actual != plan.expected {
        bail!("expected original four-byte value does not match pinned structural receipt");
    }
    stream.seek(SeekFrom::Start(plan.span.offset))?;
    stream.write_all(&plan.replacement)?;
    stream.flush()?;
    Ok(())
}

fn changed_offsets(before: &[u8], after: &[u8]) -> Result<Vec<usize>> {
    if before.len() != after.len() {
        bail!("CFB logical stream size changed under four-byte patch");
    }
    Ok(before
        .iter()
        .zip(after)
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some(i))
        .collect())
}

#[derive(Serialize)]
struct StreamFingerprint {
    path: String,
    len: usize,
    sha256: String,
}

#[derive(Serialize)]
struct FileFingerprint {
    schema: &'static str,
    whole_file_sha256: String,
    whole_file_len: usize,
    streams: Vec<StreamFingerprint>,
}

fn fingerprint(path: &Path) -> Result<FileFingerprint> {
    let raw = fs::read(path)?;
    let streams = all_streams(path)?
        .into_iter()
        .map(|(path, data)| StreamFingerprint {
            path,
            len: data.len(),
            sha256: sha(&data),
        })
        .collect();
    Ok(FileFingerprint {
        schema: "chaptera.t352-cfb-fingerprint.v1",
        whole_file_sha256: sha(&raw),
        whole_file_len: raw.len(),
        streams,
    })
}

fn write_json<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let mut f = File::create(path)?;
    serde_json::to_writer_pretty(&mut f, data)?;
    f.write_all(b"\n")?;
    Ok(())
}

fn prepare(base: &Path, receipt_path: &Path, output: &Path) -> Result<()> {
    if output.exists() {
        bail!("output directory already exists: refusing to reuse or overwrite arms");
    }
    let base_raw = fs::read(base).context("read normalized PUB")?;
    if base_raw.len() < 50_000 || base_raw.len() > 10_000_000 {
        bail!("base PUB outside bounded size gate");
    }
    let manifest: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
    let base_sha = sha(&base_raw);
    if require_str(&manifest, "source_sha256")? != base_sha {
        bail!("structural receipt is not bound to the exact normalized PUB bytes");
    }
    let (contents, escher) = plan(&manifest)?;
    let baseline_streams = all_streams(base)?;
    check_structural_streams(&manifest, &baseline_streams)?;
    for patch in [&contents, &escher] {
        let bytes = baseline_streams
            .get(&patch.span.stream)
            .context("planned stream absent")?;
        let start = usize::try_from(patch.span.offset)?;
        if bytes.get(start..start + 4) != Some(patch.expected.as_slice()) {
            bail!("planned original range failed stream-byte check");
        }
    }

    fs::create_dir_all(output)?;
    let mut summaries = Vec::new();
    for (arm, use_contents, use_escher) in [
        ("control", false, false),
        ("both_consistent", true, true),
        ("contents_only", true, false),
        ("escher_only", false, true),
    ] {
        let filename = format!("{arm}.pub");
        let target = output.join(&filename);
        fs::copy(base, &target)?;
        if use_contents {
            patch_one(&target, &contents)?;
        }
        if use_escher {
            patch_one(&target, &escher)?;
        }

        let after = all_streams(&target)?;
        if after.keys().collect::<Vec<_>>() != baseline_streams.keys().collect::<Vec<_>>() {
            bail!("CFB stream topology changed while patching {arm}");
        }
        let mut changed = Vec::new();
        for (path, before_bytes) in &baseline_streams {
            let after_bytes = after.get(path).context("missing CFB stream after patch")?;
            let offsets = changed_offsets(before_bytes, after_bytes)?;
            if !offsets.is_empty() {
                let expected_span = if use_contents && *path == contents.span.stream {
                    Some(&contents.span)
                } else if use_escher && *path == escher.span.stream {
                    Some(&escher.span)
                } else {
                    None
                };
                let allowed = expected_span.context("unplanned CFB stream was changed")?;
                let start = usize::try_from(allowed.offset)?;
                if offsets.iter().any(|i| *i < start || *i >= start + 4) {
                    bail!("CFB stream changed outside exact planned four-byte range");
                }
                changed.push(json!({
                    "stream": path,
                    "before_sha256": sha(before_bytes),
                    "after_sha256": sha(after_bytes),
                    "changed_byte_count": offsets.len(),
                    "offset": start
                }));
            }
        }
        if changed.len() != usize::from(use_contents) + usize::from(use_escher) {
            bail!("patch did not modify exactly the expected logical streams in {arm}");
        }
        summaries.push(json!({
            "arm": arm, "private_file": filename,
            "file_sha256": sha(&fs::read(&target)?),
            "changed_streams": changed
        }));
    }
    if sha(&fs::read(base)?) != base_sha {
        bail!("immutable source base changed during patch preparation");
    }
    write_json(
        &output.join("t352-patch-receipt.json"),
        &json!({
            "schema": "chaptera.t352-exact-four-arm-patch.v1",
            "base_sha256": base_sha,
            "base_file_len": base_raw.len(),
            "selected_contents_seq_num": 293,
            "selected_spid": 1025,
            "width_before_emu": BASE_WIDTH_EMU,
            "width_after_emu": BASE_WIDTH_EMU + DELTA_EMU,
            "delta_emu": DELTA_EMU,
            "source_mutated": false,
            "arms": summaries
        }),
    )?;
    Ok(())
}

fn singleton_signed_field(record: &PublisherFieldRecord, id: u16) -> Result<i64> {
    let values: Vec<u32> = record.values(id).collect();
    let [value] = values.as_slice() else {
        bail!("required Escher anchor coordinate is not unique: {id:#x}");
    };
    Ok(i64::from(*value as i32))
}

/// Independent of Contents/Escher geometry equality. The normal source-graph
/// crosswalk requires equal extents and therefore omits the deliberately
/// contradictory object we must observe in T352.
fn inspect_projections(path: &Path) -> Result<Value> {
    let source = fs::read(path)?;
    let contents = pub_cfb::read_stream_reader(Cursor::new(&source), "/Contents")?;
    let stream = StreamPath("/Contents".into());
    let header = parse_0x2c_header(stream.clone(), &contents)?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)?;
    let reference = parse_confirmed_chunk_reference(&contents, &trailer.directory, 293)?
        .context("Contents shape seq293 missing")?;
    let offsets = reference
        .chunk_offsets
        .iter()
        .map(|x| x.value)
        .collect::<Vec<_>>();
    let [chunk_offset] = offsets.as_slice() else {
        bail!("Contents seq293 must have exactly one chunk offset");
    };
    let chunk = parse_confirmed_0x2c_chunk(stream, &contents, *chunk_offset)?;
    let widths = chunk
        .fields
        .iter()
        .filter(|f| f.id == 0x00AA)
        .collect::<Vec<_>>();
    let heights = chunk
        .fields
        .iter()
        .filter(|f| f.id == 0x00AB)
        .collect::<Vec<_>>();
    let ([width], [height]) = (widths.as_slice(), heights.as_slice()) else {
        bail!("Contents width/height must each have one field");
    };
    let decoded = |f: &pub_contents::RawContentsBlock| -> Result<i64> {
        match &f.body {
            RawContentsBlockBody::U32 { value, .. } => Ok(i64::from(*value)),
            _ => bail!("Contents dimensional field is not U32"),
        }
    };
    let contents_width = decoded(width)?;
    let contents_height = decoded(height)?;

    let escher_bytes = pub_cfb::read_stream_reader(Cursor::new(&source), "/Escher/EscherStm")?;
    let shapes = inspect_sp_containers(StreamPath("/Escher/EscherStm".into()), &escher_bytes)?;
    let matched = shapes
        .shapes
        .iter()
        .filter(|s| {
            s.fsp
                .as_ref()
                .is_some_and(|f| f.spid == 1025 && f.shape_type == 202)
                && s.client_data.as_ref().is_some_and(|d| {
                    let ids: Vec<u32> = d.values(PUBLISHER_FIELD_SHAPE_ID).collect();
                    ids.as_slice() == [293u32]
                })
        })
        .collect::<Vec<_>>();
    let [shape] = matched.as_slice() else {
        bail!("identity-linked Escher seq293 / SPID1025 must occur exactly once");
    };
    let anchor = shape
        .client_anchor
        .as_ref()
        .context("Escher client anchor missing")?;
    let xs = singleton_signed_field(anchor, PUBLISHER_FIELD_XS)?;
    let ys = singleton_signed_field(anchor, PUBLISHER_FIELD_YS)?;
    let xe = singleton_signed_field(anchor, PUBLISHER_FIELD_XE)?;
    let ye = singleton_signed_field(anchor, PUBLISHER_FIELD_YE)?;
    let escher_width = xe.checked_sub(xs).context("Escher width overflow")?;
    let escher_height = ye.checked_sub(ys).context("Escher height overflow")?;
    Ok(json!({
        "schema":"chaptera.t352-identity-projection-inspection.v1",
        "whole_file_sha256":sha(&source),
        "source_length":source.len(),
        "contents_shape_id":293,
        "escher_spid":1025,
        "escher_shape_type":202,
        "contents_width_emu":contents_width,
        "contents_height_emu":contents_height,
        "escher_width_emu":escher_width,
        "escher_height_emu":escher_height,
        "anchor_xs":xs, "anchor_ys":ys, "anchor_xe":xe, "anchor_ye":ye,
        "width_equal": contents_width == escher_width,
        "height_equal": contents_height == escher_height,
        "identity_join_independent_of_geometry":true
    }))
}

fn prepare_independent(
    base: &Path,
    census_path: &Path,
    shape_id: u32,
    output: &Path,
) -> Result<()> {
    if output.exists() {
        bail!("independent T352 output directory already exists");
    }
    let source_bytes = fs::read(base).context("read independent normalized base")?;
    if !(50_000..=10_000_000).contains(&source_bytes.len()) {
        bail!("independent normalized PUB size is outside bounded policy");
    }
    let source_sha = sha(&source_bytes);
    let census: Value = serde_json::from_slice(&fs::read(census_path)?)?;
    if require_str(&census, "schema")? != "chaptera.t352-independent-identity-first-preflight.v1"
        || require_str(&census, "source_sha256")? != source_sha
    {
        bail!("independent census schema or exact source SHA does not match");
    }
    let rows = census
        .get("rows")
        .and_then(Value::as_array)
        .context("missing census rows")?;
    let matching = rows
        .iter()
        .filter(|row| row.get("contents_seq").and_then(Value::as_u64) == Some(u64::from(shape_id)))
        .collect::<Vec<_>>();
    let [row] = matching.as_slice() else {
        bail!("requested independently joined shape does not have exactly one census row");
    };
    for flag in [
        "admitted_independent_target",
        "unique_join",
        "both_geometries_consistent",
        "identity_joined_even_if_extents_differ",
        "numeric_patchable",
        "different_from_original_t352_width",
    ] {
        if row.get(flag).and_then(Value::as_bool) != Some(true) {
            bail!("independent shape fails the bounded {flag} condition");
        }
    }
    if require_i64(row, "shape_type")? != 1 {
        bail!("independent falsifier requires a non-T352 rectangle shape class 1");
    }
    let spid = require_i64(row, "spid")?;
    let width = require_i64(row, "contents_width_emu")?;
    let anchor_width = require_i64(row, "anchor_width_emu")?;
    let anchor_xe = require_i64(row, "anchor_xe_emu")?;
    if width <= 0 || width != anchor_width || width == BASE_WIDTH_EMU {
        bail!("independent width fails baseline equality/novelty gate");
    }
    let new_width = width
        .checked_add(DELTA_EMU)
        .context("width delta overflow")?;
    let new_xe = anchor_xe
        .checked_add(DELTA_EMU)
        .context("XE delta overflow")?;
    let xe_old = i32::try_from(anchor_xe).context("XE not signed i32")?;
    let xe_new = i32::try_from(new_xe).context("XE + delta not signed i32")?;
    let cw_offset = u64::try_from(require_i64(row, "width_value_offset_in_contents")?)?;
    let xe_tag_offset = u64::try_from(require_i64(row, "xe_tagged_field_offset_in_escher")?)?;
    let xe_offset = xe_tag_offset
        .checked_add(2)
        .context("XE value offset overflow")?;
    let cw = Patch {
        span: Span {
            stream: "/Contents".to_owned(),
            offset: cw_offset,
            len: 4,
        },
        expected: uint32_bytes(width)?,
        replacement: uint32_bytes(new_width)?,
    };
    let ex = Patch {
        span: Span {
            stream: "/Escher/EscherStm".to_owned(),
            offset: xe_offset,
            len: 4,
        },
        expected: xe_old.to_le_bytes(),
        replacement: xe_new.to_le_bytes(),
    };
    if cw.expected == cw.replacement || ex.expected == ex.replacement {
        bail!("independent exact-range patch would be a no-op");
    }
    let baseline = all_streams(base)?;
    if baseline.len() < 2 {
        bail!("independent PUB lacks the required complete CFB inventory");
    }
    for patch in [&cw, &ex] {
        let bytes = baseline
            .get(&patch.span.stream)
            .context("patch stream missing")?;
        let start = usize::try_from(patch.span.offset)?;
        if bytes.get(start..start.saturating_add(4)) != Some(patch.expected.as_slice()) {
            bail!("census scalar span is not bound to current normalized CFB bytes");
        }
    }
    // Prove the exact wire ID tag, not just a plausible signed XE value.
    let escher = baseline
        .get("/Escher/EscherStm")
        .context("Escher not found")?;
    let tag_start = usize::try_from(xe_tag_offset)?;
    if escher.get(tag_start..tag_start.saturating_add(2))
        != Some(0x2003u16.to_le_bytes().as_slice())
    {
        bail!("selected Escher field tag is not XE 0x2003");
    }

    fs::create_dir_all(output)?;
    let mut arms = Vec::new();
    for (name, contents_change, escher_change) in [
        ("control", false, false),
        ("both_consistent", true, true),
        ("contents_only", true, false),
        ("escher_only", false, true),
    ] {
        let arm = output.join(format!("{name}.pub"));
        fs::copy(base, &arm)?;
        if contents_change {
            patch_one(&arm, &cw)?;
        }
        if escher_change {
            patch_one(&arm, &ex)?;
        }
        let next = all_streams(&arm)?;
        if baseline.keys().collect::<Vec<_>>() != next.keys().collect::<Vec<_>>() {
            bail!("independent patch unexpectedly changed logical CFB stream topology");
        }
        let mut changed = Vec::new();
        for (path, bytes) in &baseline {
            let next_bytes = next
                .get(path)
                .context("missing stream after exact-range mutation")?;
            let offsets = changed_offsets(bytes, next_bytes)?;
            if offsets.is_empty() {
                continue;
            }
            let planned = if contents_change && path == "/Contents" {
                Some(&cw)
            } else if escher_change && path == "/Escher/EscherStm" {
                Some(&ex)
            } else {
                None
            }
            .context("unplanned stream mutated by independent patch")?;
            let start = usize::try_from(planned.span.offset)?;
            if offsets
                .iter()
                .any(|offset| *offset < start || *offset >= start + 4)
            {
                bail!("independent patch changed bytes outside exact planned scalar range");
            }
            changed.push(json!({
                "stream": path,
                "sha256_before": sha(bytes),
                "sha256_after": sha(next_bytes),
                "value_offset": start,
                "changed_byte_count": offsets.len()
            }));
        }
        if changed.len() != usize::from(contents_change) + usize::from(escher_change) {
            bail!("independent arm does not have precisely the intended changed streams");
        }
        arms.push(json!({
            "arm": name,
            "file_sha256": sha(&fs::read(&arm)?),
            "changed_streams": changed
        }));
    }
    if sha(&fs::read(base)?) != source_sha {
        bail!("immutable independent normalized input was changed during experiment");
    }
    write_json(
        &output.join("t352-independent-patch-receipt.json"),
        &json!({
            "schema": "chaptera.t352-independent-four-arm-patch.v1",
            "source_sha256": source_sha,
            "source_length": source_bytes.len(),
            "contents_shape_id": shape_id,
            "spid": spid,
            "shape_type": 1,
            "width_before_emu": width,
            "width_after_emu": new_width,
            "xe_before_emu": anchor_xe,
            "xe_after_emu": new_xe,
            "delta_emu": DELTA_EMU,
            "source_mutated": false,
            "arms": arms,
            "native_result": "not_executed"
        }),
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let words: Vec<String> = args
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    match words.as_slice() {
        [command, base, census, id, output] if command == "prepare-independent" => {
            let id = id
                .parse::<u32>()
                .context("independent Contents shape id must be u32")?;
            prepare_independent(Path::new(base), Path::new(census), id, Path::new(output))
        }
        [command, base, receipt, output] if command == "prepare" => {
            prepare(Path::new(base), Path::new(receipt), Path::new(output))
        }
        [command, source, output] if command == "fingerprint" => {
            write_json(Path::new(output), &fingerprint(Path::new(source))?)
        }
        [command, source, output] if command == "inspect" => {
            write_json(Path::new(output), &inspect_projections(Path::new(source))?)
        }
        _ => bail!(
            "usage: val_xproj_patch prepare BASE.pub STRUCTURAL.json NEW_DIR | fingerprint FILE.pub OUT.json | inspect FILE.pub OUT.json | prepare-independent BASE.pub CENSUS.json SHAPE_ID NEW_DIR"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_width_and_anchor_delta() {
        assert_eq!(BASE_WIDTH_EMU + DELTA_EMU, 5_203_000);
        assert_eq!(BASE_XE_EMU + DELTA_EMU, 2_503_000);
        assert_eq!(
            uint32_bytes(BASE_WIDTH_EMU).unwrap(),
            5_076_000u32.to_le_bytes()
        );
    }
    #[test]
    fn tampered_structural_contract_fails_closed() {
        let r = json!({"schema": "chaptera.modern-structural-base/v1",
            "stream_count": 9, "candidate_count": 6,
            "selected_target": {}});
        assert!(plan(&r).is_err());
    }
    #[test]
    fn four_arms_modify_only_exact_logical_stream_ranges() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("base.pub");
        let receipt_path = tmp.path().join("structural.json");
        let arms = tmp.path().join("arms");
        let mut file = cfb::create(&source).unwrap();
        file.create_storage("/Escher").unwrap();
        let mut contents = vec![0x18u8; 60_000];
        contents[16..20].copy_from_slice(&uint32_bytes(BASE_WIDTH_EMU).unwrap());
        file.create_stream("/Contents")
            .unwrap()
            .write_all(&contents)
            .unwrap();
        let mut escher = vec![0x27u8; 2048];
        escher[8..10].copy_from_slice(&0x2003u16.to_le_bytes());
        escher[10..14].copy_from_slice(&uint32_bytes(BASE_XE_EMU).unwrap());
        file.create_stream("/Escher/EscherStm")
            .unwrap()
            .write_all(&escher)
            .unwrap();
        for i in 0u8..8u8 {
            let name = format!("/Ancillary{i}");
            file.create_stream(&name)
                .unwrap()
                .write_all(&[i; 50])
                .unwrap();
        }
        drop(file);

        let baseline = all_streams(&source).unwrap();
        for path in baseline.keys() {
            assert!(
                path.starts_with('/'),
                "CFB stream path is not canonical: {path}"
            );
            assert!(
                !path.contains('\\'),
                "Windows separator leaked into CFB path"
            );
        }
        let stream_digests: Vec<_> = baseline
            .iter()
            .map(|(path, bytes)| json!({"path":path,"len":bytes.len(),"sha256":sha(bytes)}))
            .collect();
        let receipt = json!({
            "schema": "chaptera.modern-structural-base/v1",
            "source_sha256": sha(&fs::read(&source).unwrap()),
            "stream_count": 10,
            "candidate_count": 6,
            "selected_target": {
                "contents_seq_num": 293,
                "publisher_shape_id": 293,
                "officeart_spid": 1025,
                "officeart_shape_type": 202,
                "contents_width_emu": BASE_WIDTH_EMU,
                "contents_height_emu": 972_000,
                "anchor_xs": -2_700_000,
                "anchor_ys": -4_446_000,
                "anchor_xe": BASE_XE_EMU,
                "anchor_ye": -3_474_000,
                "contents_anchor_extent_exact": true,
                "contents_width_wire_type": 0x20,
                "contents_width_source": { "stream":"/Contents","offset":16,"len":4 }
            },
            "manifest": {
                "streams": stream_digests,
                "candidates": [{
                    "contents_seq_num":293,
                    "officeart_spid":1025,
                    "escher_shape":{"client_anchor":{"fields":[{
                        "id":0x2003,"value": BASE_XE_EMU,
                        "source":{"stream":"/Escher/EscherStm","offset":8,"len":6}
                    }]}}
                }]
            }
        });
        write_json(&receipt_path, &receipt).unwrap();
        prepare(&source, &receipt_path, &arms).unwrap();
        for (name, contents_changed, escher_changed) in [
            ("control", false, false),
            ("both_consistent", true, true),
            ("contents_only", true, false),
            ("escher_only", false, true),
        ] {
            let current = all_streams(&arms.join(format!("{name}.pub"))).unwrap();
            for (path, original) in &baseline {
                let offsets = changed_offsets(original, &current[path]).unwrap();
                let expected: Vec<usize> = if path == "/Contents" && contents_changed {
                    changed_offsets(
                        &BASE_WIDTH_EMU.to_le_bytes(),
                        &(BASE_WIDTH_EMU + DELTA_EMU).to_le_bytes(),
                    )
                    .unwrap()
                    .iter()
                    .map(|x| x + 16)
                    .collect()
                } else if path == "/Escher/EscherStm" && escher_changed {
                    changed_offsets(
                        &BASE_XE_EMU.to_le_bytes(),
                        &(BASE_XE_EMU + DELTA_EMU).to_le_bytes(),
                    )
                    .unwrap()
                    .iter()
                    .map(|x| x + 10)
                    .collect()
                } else {
                    Vec::new()
                };
                assert_eq!(offsets, expected, "{name} unexpected changes to {path}");
            }
        }
        assert_eq!(
            sha(&fs::read(&source).unwrap()),
            receipt["source_sha256"].as_str().unwrap()
        );
    }

    #[test]
    fn independent_type1_arms_are_bound_to_unique_census_and_exact_stream_spans() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("normalized.pub");
        let census_path = tmp.path().join("census.json");
        let arms = tmp.path().join("arms");
        let width = 2_289_185i64;
        let xe = 3_144_900i64;
        let mut cfb = cfb::create(&source).unwrap();
        cfb.create_storage("/Escher").unwrap();
        let mut contents = vec![0x12u8; 60_000];
        contents[16..20].copy_from_slice(&uint32_bytes(width).unwrap());
        cfb.create_stream("/Contents")
            .unwrap()
            .write_all(&contents)
            .unwrap();
        let mut escher = vec![0x48u8; 2048];
        escher[8..10].copy_from_slice(&0x2003u16.to_le_bytes());
        escher[10..14].copy_from_slice(&(xe as i32).to_le_bytes());
        cfb.create_stream("/Escher/EscherStm")
            .unwrap()
            .write_all(&escher)
            .unwrap();
        drop(cfb);

        let original_sha = sha(&fs::read(&source).unwrap());
        let census = json!({
            "schema":"chaptera.t352-independent-identity-first-preflight.v1",
            "source_sha256": original_sha,
            "rows":[{
                "contents_seq": 305,
                "spid": 1035,
                "shape_type": 1,
                "admitted_independent_target": true,
                "unique_join": true,
                "both_geometries_consistent": true,
                "identity_joined_even_if_extents_differ": true,
                "numeric_patchable": true,
                "different_from_original_t352_width": true,
                "contents_width_emu": width,
                "anchor_width_emu": width,
                "anchor_xe_emu": xe,
                "width_value_offset_in_contents": 16,
                "xe_tagged_field_offset_in_escher": 8
            }]
        });
        write_json(&census_path, &census).unwrap();
        prepare_independent(&source, &census_path, 305, &arms).unwrap();
        assert_eq!(sha(&fs::read(&source).unwrap()), original_sha);
        let before = all_streams(&source).unwrap();
        for (name, contents_change, escher_change) in [
            ("control", false, false),
            ("both_consistent", true, true),
            ("contents_only", true, false),
            ("escher_only", false, true),
        ] {
            let changed = all_streams(&arms.join(format!("{name}.pub"))).unwrap();
            assert_eq!(
                before.keys().collect::<Vec<_>>(),
                changed.keys().collect::<Vec<_>>()
            );
            for (stream_name, bytes) in &before {
                let offsets = changed_offsets(bytes, &changed[stream_name]).unwrap();
                let expected = if stream_name == "/Contents" && contents_change {
                    changed_offsets(
                        &uint32_bytes(width).unwrap(),
                        &uint32_bytes(width + DELTA_EMU).unwrap(),
                    )
                    .unwrap()
                    .iter()
                    .map(|i| i + 16)
                    .collect::<Vec<_>>()
                } else if stream_name == "/Escher/EscherStm" && escher_change {
                    changed_offsets(
                        &(xe as i32).to_le_bytes(),
                        &((xe + DELTA_EMU) as i32).to_le_bytes(),
                    )
                    .unwrap()
                    .iter()
                    .map(|i| i + 10)
                    .collect::<Vec<_>>()
                } else {
                    vec![]
                };
                assert_eq!(
                    offsets, expected,
                    "unplanned source-stream mutation in {name}"
                );
            }
        }
        assert!(
            prepare_independent(&source, &census_path, 306, &tmp.path().join("wrong-id")).is_err()
        );
    }
}
