use anyhow::{bail, Context, Result};
use pub_contents::{
    CONTENTS_RAW_TYPE_STORY_CATALOG, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
    parse_confirmed_mature_story_catalog,
};
use pub_core::StreamPath;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const CONTENTS_STREAM: &str = "/Contents";
const QUILL_STREAM: &str = "/Quill/QuillSub/CONTENTS";
const DESCRIPTOR_ROOT: u32 = 0x18;
const DESCRIPTOR_END: u32 = 0xffff_ffff;
const DESCRIPTOR_SIZE: usize = 24;
const DESCRIPTOR_PRESENT: u16 = 0x0018;

#[derive(Debug, Clone)]
struct Descriptor {
    name: [u8; 4],
    data_offset: u32,
    data_length: u32,
}

#[derive(Debug, Serialize)]
struct PlcCandidate {
    prefix_offset: usize,
    grounded_story_count: u32,
    header_count: u32,
    data_size: u32,
    nonzero_flag_count: usize,
    position_increment_count: usize,
    zero_story_increment_count: usize,
    first_n_increment_sum_utf16: u64,
    all_increment_sum_utf16: u64,
    trailing_increment_utf16: u32,
    text_utf16_units: u64,
    first_n_sum_matches_text: bool,
    all_sum_matches_text: bool,
    all_story_ends_within_text: bool,
    terminal_story_end_matches_text: bool,
    structured_record_count: usize,
    structured_records_consume_tail_exactly: bool,
    structured_record_size_sha256: String,
    story_increment_sha256: String,
    story_end_count: usize,
    story_ends_in_btep: Option<usize>,
    story_ends_in_btec: Option<usize>,
    story_ends_in_both: Option<usize>,
}

#[derive(Debug, Serialize)]
struct WitnessRow {
    source_sha256: String,
    byte_len: usize,
    contents_serialization_revision: u16,
    grounded_story_count: u32,
    contents_syid_order_matches: bool,
    syid_descriptor_length: u32,
    syid_length_matches_grounded_count: bool,
    strs_descriptor_length: u32,
    strs_length_matches_observed_22_plus_8n: bool,
    text_descriptor_length: u32,
    text_utf16_units: u64,
    btep_plc_valid: bool,
    btec_plc_valid: bool,
    candidate_count: usize,
    accepted_candidate_count: usize,
    candidates: Vec<PlcCandidate>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_u32s(values: &[u32]) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update(value.to_le_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_u16s(values: &[u16]) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update(value.to_le_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn descriptor_range<'a>(bytes: &'a [u8], descriptor: &Descriptor) -> Result<&'a [u8]> {
    let start = usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
    let len = usize::try_from(descriptor.data_length).context("descriptor length too large")?;
    let end = start.checked_add(len).context("descriptor range overflow")?;
    bytes
        .get(start..end)
        .with_context(|| format!("descriptor {:?} range outside Quill", descriptor.name))
}

fn parse_descriptor_directory(bytes: &[u8]) -> Result<Vec<Descriptor>> {
    let mut current = DESCRIPTOR_ROOT;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    while current != DESCRIPTOR_END {
        if !seen.insert(current) {
            bail!("descriptor list cycle");
        }
        let start = usize::try_from(current).context("descriptor node offset too large")?;
        let count = usize::from(
            u16_at(bytes, start + 2).context("descriptor node header is truncated")?,
        );
        let next = u32_at(bytes, start + 4).context("descriptor node next is truncated")?;
        let array_start = start.checked_add(8).context("descriptor array overflow")?;
        let array_len = count
            .checked_mul(DESCRIPTOR_SIZE)
            .context("descriptor array size overflow")?;
        let array_end = array_start
            .checked_add(array_len)
            .context("descriptor array end overflow")?;
        if array_end > bytes.len() {
            bail!("descriptor array outside Quill");
        }

        for index in 0..count {
            let offset = array_start + index * DESCRIPTOR_SIZE;
            let present = u16_at(bytes, offset).context("descriptor presence is truncated")?;
            if present != DESCRIPTOR_PRESENT {
                bail!("unexpected descriptor presence marker");
            }
            let name_raw = bytes
                .get(offset + 2..offset + 6)
                .context("descriptor name is truncated")?;
            let name = [name_raw[0], name_raw[1], name_raw[2], name_raw[3]];
            let data_offset =
                u32_at(bytes, offset + 16).context("descriptor data offset is truncated")?;
            let data_length =
                u32_at(bytes, offset + 20).context("descriptor data length is truncated")?;
            out.push(Descriptor {
                name,
                data_offset,
                data_length,
            });
        }

        current = next;
    }

    Ok(out)
}

fn unique_descriptor<'a>(descriptors: &'a [Descriptor], name: [u8; 4]) -> Result<&'a Descriptor> {
    let mut found = descriptors.iter().filter(|item| item.name == name);
    let first = found
        .next()
        .with_context(|| format!("missing Quill descriptor {:?}", name))?;
    if found.next().is_some() {
        bail!("duplicate Quill descriptor {:?}", name);
    }
    Ok(first)
}

fn optional_unique_descriptor<'a>(
    descriptors: &'a [Descriptor],
    name: [u8; 4],
) -> Result<Option<&'a Descriptor>> {
    let mut found = descriptors.iter().filter(|item| item.name == name);
    let first = found.next();
    if found.next().is_some() {
        bail!("duplicate Quill descriptor {:?}", name);
    }
    Ok(first)
}

fn parse_bte_positions(
    quill: &[u8],
    descriptor: Option<&Descriptor>,
    text_start: u32,
) -> Result<Option<BTreeSet<u32>>> {
    let Some(descriptor) = descriptor else {
        return Ok(None);
    };
    let payload = descriptor_range(quill, descriptor)?;
    if payload.len() < 16 {
        return Ok(None);
    }

    // BTEP/BTEC are only an independent research cross-check here. Early
    // Publisher variants may use a different local BTE framing, so a payload
    // that does not satisfy the already-confirmed ordinary BTE PLC grammar
    // must make the cross-check unavailable rather than abort the STRS probe.
    let Some(raw_count) = u32_at(payload, 0) else {
        return Ok(None);
    };
    let Ok(count) = usize::try_from(raw_count) else {
        return Ok(None);
    };
    let Some(raw_data_size) = u32_at(payload, 4) else {
        return Ok(None);
    };
    let Ok(data_size) = usize::try_from(raw_data_size) else {
        return Ok(None);
    };
    let Some(expected) = count
        .checked_add(1)
        .and_then(|value| value.checked_mul(4))
        .and_then(|position_bytes| 12usize.checked_add(position_bytes))
        .and_then(|base| {
            count
                .checked_mul(data_size)
                .and_then(|data_bytes| base.checked_add(data_bytes))
        })
    else {
        return Ok(None);
    };
    if expected != payload.len() || data_size != 4 {
        return Ok(None);
    }

    let mut positions = BTreeSet::new();
    for index in 0..=count {
        let Some(raw) = u32_at(payload, 12 + index * 4) else {
            return Ok(None);
        };
        positions.insert(if raw == 0 { text_start } else { raw });
    }
    Ok(Some(positions))
}

fn grounded_contents_story_ids(contents: &[u8]) -> Result<(u16, Vec<u32>)> {
    let stream = StreamPath(CONTENTS_STREAM.into());
    let header =
        parse_0x2c_header(stream.clone(), contents).context("parse mature Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature Contents trailer")?;

    let mut story_refs = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse chunk reference {seq_num}"))?
        else {
            continue;
        };
        if reference.raw_types.len() == 1
            && reference.raw_types[0].value == CONTENTS_RAW_TYPE_STORY_CATALOG
            && reference.chunk_offsets.len() == 1
        {
            story_refs.push(reference);
        }
    }
    if story_refs.len() != 1 {
        bail!(
            "expected exactly one strict 0x65 reference, found {}",
            story_refs.len()
        );
    }

    let reference = &story_refs[0];
    let chunk = parse_confirmed_0x2c_chunk(
        stream,
        contents,
        reference.chunk_offsets[0].value,
    )
    .context("parse Story catalog chunk")?;
    let catalog = parse_confirmed_mature_story_catalog(contents, &chunk)
        .context("parse grounded Story catalog")?;
    let ids = catalog.entries.iter().map(|entry| entry.text_id).collect();
    Ok((header.preamble.serialization_revision, ids))
}

fn parse_grounded_syid_ids(
    quill: &[u8],
    descriptor: &Descriptor,
    grounded_count: usize,
) -> Result<Vec<u32>> {
    let payload = descriptor_range(quill, descriptor)?;
    let expected = 8usize
        .checked_add(
            grounded_count
                .checked_mul(4)
                .context("grounded SYID size overflow")?,
        )
        .context("grounded SYID total overflow")?;
    if payload.len() != expected {
        bail!(
            "SYID descriptor length {} does not match grounded count {}",
            payload.len(),
            grounded_count
        );
    }
    let mut ids = Vec::with_capacity(grounded_count);
    for index in 0..grounded_count {
        ids.push(u32_at(payload, 8 + index * 4).context("grounded SYID id truncated")?);
    }
    Ok(ids)
}

fn structured_record_sizes_exact(
    payload: &[u8],
    start: usize,
    count: usize,
) -> (Vec<u16>, bool) {
    let mut pos = start;
    let mut sizes = Vec::with_capacity(count);
    for _ in 0..count {
        let Some(size) = u16_at(payload, pos) else {
            return (sizes, false);
        };
        let size_usize = usize::from(size);
        if size_usize < 2 {
            return (sizes, false);
        }
        let Some(end) = pos.checked_add(size_usize) else {
            return (sizes, false);
        };
        if end > payload.len() {
            return (sizes, false);
        }
        sizes.push(size);
        pos = end;
    }
    (sizes, pos == payload.len())
}

fn scan_strs_plc_candidates(
    strs: &[u8],
    grounded_count: usize,
    text_start: u32,
    text_length: u32,
    btep: Option<&BTreeSet<u32>>,
    btec: Option<&BTreeSet<u32>>,
) -> Result<Vec<PlcCandidate>> {
    let grounded_u32 = u32::try_from(grounded_count).context("grounded story count too large")?;
    let text_units = u64::from(text_length) / 2;
    if text_length % 2 != 0 {
        bail!("TEXT descriptor length is odd");
    }

    let mut candidates = Vec::new();
    let max_prefix = strs.len().saturating_sub(12).min(32);
    for prefix in 0..=max_prefix {
        let Some(header_count) = u32_at(strs, prefix) else {
            continue;
        };
        if header_count != grounded_u32 {
            continue;
        }
        let Some(data_size) = u32_at(strs, prefix + 4) else {
            continue;
        };
        let Some(flags) = strs.get(prefix + 8..prefix + 12) else {
            continue;
        };

        let positions_start = prefix + 12;
        let Some(position_bytes) = grounded_count
            .checked_add(1)
            .and_then(|value| value.checked_mul(4))
        else {
            continue;
        };
        let Some(positions_end) = positions_start.checked_add(position_bytes) else {
            continue;
        };
        if positions_end > strs.len() {
            continue;
        }

        let mut increments = Vec::with_capacity(grounded_count + 1);
        for index in 0..=grounded_count {
            let Some(value) = u32_at(strs, positions_start + index * 4) else {
                increments.clear();
                break;
            };
            increments.push(value);
        }
        if increments.len() != grounded_count + 1 {
            continue;
        }

        let first_n_sum = increments[..grounded_count]
            .iter()
            .fold(0u64, |sum, value| sum.saturating_add(u64::from(*value)));
        let all_sum = increments
            .iter()
            .fold(0u64, |sum, value| sum.saturating_add(u64::from(*value)));
        let trailing_increment = increments[grounded_count];
        let zero_story_increment_count =
            increments[..grounded_count].iter().filter(|value| **value == 0).count();

        let mut cumulative_units = 0u64;
        let mut story_ends = Vec::with_capacity(grounded_count);
        let mut all_story_ends_within_text = true;
        for increment in &increments[..grounded_count] {
            cumulative_units = cumulative_units.saturating_add(u64::from(*increment));
            let byte_delta = cumulative_units.saturating_mul(2);
            if byte_delta > u64::from(text_length) {
                all_story_ends_within_text = false;
            }
            let absolute = u64::from(text_start).saturating_add(byte_delta);
            story_ends.push(u32::try_from(absolute).unwrap_or(u32::MAX));
        }

        let (record_sizes, records_exact) =
            structured_record_sizes_exact(strs, positions_end, grounded_count);

        let in_btep = btep.map(|set| story_ends.iter().filter(|end| set.contains(*end)).count());
        let in_btec = btec.map(|set| story_ends.iter().filter(|end| set.contains(*end)).count());
        let in_both = match (btep, btec) {
            (Some(left), Some(right)) => Some(
                story_ends
                    .iter()
                    .filter(|end| left.contains(*end) && right.contains(*end))
                    .count(),
            ),
            _ => None,
        };

        candidates.push(PlcCandidate {
            prefix_offset: prefix,
            grounded_story_count: grounded_u32,
            header_count,
            data_size,
            nonzero_flag_count: flags.iter().filter(|byte| **byte != 0).count(),
            position_increment_count: increments.len(),
            zero_story_increment_count,
            first_n_increment_sum_utf16: first_n_sum,
            all_increment_sum_utf16: all_sum,
            trailing_increment_utf16: trailing_increment,
            text_utf16_units: text_units,
            first_n_sum_matches_text: first_n_sum == text_units,
            all_sum_matches_text: all_sum == text_units,
            all_story_ends_within_text,
            terminal_story_end_matches_text: first_n_sum == text_units,
            structured_record_count: record_sizes.len(),
            structured_records_consume_tail_exactly: records_exact,
            structured_record_size_sha256: hash_u16s(&record_sizes),
            story_increment_sha256: hash_u32s(&increments[..grounded_count]),
            story_end_count: story_ends.len(),
            story_ends_in_btep: in_btep,
            story_ends_in_btec: in_btec,
            story_ends_in_both: in_both,
        });
    }

    Ok(candidates)
}

fn diagnose(bytes: &[u8]) -> Result<WitnessRow> {
    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS_STREAM)
        .context("read Contents stream")?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(bytes), QUILL_STREAM)
        .context("read Quill stream")?;

    let (revision, contents_ids) = grounded_contents_story_ids(&contents)?;
    let grounded_count = contents_ids.len();
    let grounded_u32 = u32::try_from(grounded_count).context("grounded count too large")?;

    let descriptors = parse_descriptor_directory(&quill)?;
    let syid = unique_descriptor(&descriptors, *b"SYID")?;
    let strs = unique_descriptor(&descriptors, *b"STRS")?;
    let text = unique_descriptor(&descriptors, *b"TEXT")?;
    let btep = optional_unique_descriptor(&descriptors, *b"BTEP")?;
    let btec = optional_unique_descriptor(&descriptors, *b"BTEC")?;

    descriptor_range(&quill, syid)?;
    let strs_payload = descriptor_range(&quill, strs)?;
    descriptor_range(&quill, text)?;

    let syid_ids = parse_grounded_syid_ids(&quill, syid, grounded_count)?;
    let contents_syid_order_matches = contents_ids == syid_ids;

    let expected_syid_len = 8u64 + 4u64 * u64::from(grounded_u32);
    let expected_strs_len = 22u64 + 8u64 * u64::from(grounded_u32);

    let btep_positions = parse_bte_positions(&quill, btep, text.data_offset)?;
    let btec_positions = parse_bte_positions(&quill, btec, text.data_offset)?;
    let candidates = scan_strs_plc_candidates(
        strs_payload,
        grounded_count,
        text.data_offset,
        text.data_length,
        btep_positions.as_ref(),
        btec_positions.as_ref(),
    )?;
    let accepted_candidate_count = candidates
        .iter()
        .filter(|candidate| {
            candidate.first_n_sum_matches_text
                && candidate.all_story_ends_within_text
                && candidate.structured_records_consume_tail_exactly
        })
        .count();

    Ok(WitnessRow {
        source_sha256: sha256_hex(bytes),
        byte_len: bytes.len(),
        contents_serialization_revision: revision,
        grounded_story_count: grounded_u32,
        contents_syid_order_matches,
        syid_descriptor_length: syid.data_length,
        syid_length_matches_grounded_count: u64::from(syid.data_length) == expected_syid_len,
        strs_descriptor_length: strs.data_length,
        strs_length_matches_observed_22_plus_8n: u64::from(strs.data_length) == expected_strs_len,
        text_descriptor_length: text.data_length,
        text_utf16_units: u64::from(text.data_length) / 2,
        btep_plc_valid: btep_positions.is_some(),
        btec_plc_valid: btec_positions.is_some(),
        candidate_count: candidates.len(),
        accepted_candidate_count,
        candidates,
    })
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = fs::read_dir(root)
        .with_context(|| format!("read witness dir {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    out.sort();
    Ok(out)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: quill-story-early-text-boundary-probe WITNESS_DIR OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: quill-story-early-text-boundary-probe WITNESS_DIR OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("expected exactly WITNESS_DIR OUTPUT.json");
    }

    let paths = pub_paths(&root)?;
    if paths.len() != 7 {
        bail!("expected exactly 7 witness PUBs, found {}", paths.len());
    }

    let mut rows = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        rows.push(diagnose(&bytes)?);
    }
    rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));

    let report = serde_json::json!({
        "schema": "chaptera.quill-story-early-text-boundary.v1",
        "witness_count": rows.len(),
        "rows": rows,
        "evidence_boundary": "exact witness SHA plus source-safe structural counts, lengths, booleans and hashes only; no filenames, paths, document text, Story IDs, raw payload bytes or parser error text",
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
