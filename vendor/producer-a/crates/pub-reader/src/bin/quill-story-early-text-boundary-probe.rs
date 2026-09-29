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
struct BtePlcCandidate {
    prefix_offset: usize,
    count: u32,
    data_size: u32,
    nonzero_flag_count: usize,
    position_count: usize,
    positions_monotonic: bool,
    positions_inside_text_count: usize,
    text_start_present: bool,
    text_end_present: bool,
    count_covers_story_count: bool,
    target_count: usize,
    targets_inside_format_chunk_count: Option<usize>,
}

#[derive(Debug, Serialize)]
struct WitnessRow {
    source_sha256: String,
    byte_len: usize,
    contents_serialization_revision: u16,
    grounded_story_count: u32,

    syid_descriptor_length: u32,
    syid_length_matches_grounded_count: bool,
    syid_chunk_all_ff: bool,

    strs_descriptor_length: u32,
    strs_length_matches_observed_22_plus_8n: bool,
    strs_chunk_all_ff: bool,
    strs_direct_generic_plc_candidate_count: usize,

    text_descriptor_length: u32,
    text_utf16_units: u64,

    btep_descriptor_length: Option<u32>,
    btec_descriptor_length: Option<u32>,
    fdpp_descriptor_length: Option<u32>,
    fdpc_descriptor_length: Option<u32>,
    btep_candidates: Vec<BtePlcCandidate>,
    btec_candidates: Vec<BtePlcCandidate>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
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

fn descriptor_all_ff(bytes: &[u8], descriptor: &Descriptor) -> Result<bool> {
    Ok(descriptor_range(bytes, descriptor)?
        .iter()
        .all(|byte| *byte == 0xff))
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

fn grounded_contents_story_count(contents: &[u8]) -> Result<(u16, u32)> {
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
    Ok((
        header.preamble.serialization_revision,
        catalog.declared_count,
    ))
}

fn scan_direct_generic_plc_count(payload: &[u8], grounded_count: u32) -> usize {
    let max_prefix = payload.len().saturating_sub(12).min(32);
    (0..=max_prefix)
        .filter(|prefix| u32_at(payload, *prefix) == Some(grounded_count))
        .count()
}

fn scan_bte_plc_candidates(
    quill: &[u8],
    descriptor: Option<&Descriptor>,
    format_descriptor: Option<&Descriptor>,
    text_descriptor: &Descriptor,
    grounded_story_count: u32,
) -> Result<Vec<BtePlcCandidate>> {
    let Some(descriptor) = descriptor else {
        return Ok(Vec::new());
    };
    let payload = descriptor_range(quill, descriptor)?;
    if payload.len() < 20 {
        return Ok(Vec::new());
    }

    let text_start = u64::from(text_descriptor.data_offset);
    let text_end = text_start
        .checked_add(u64::from(text_descriptor.data_length))
        .context("TEXT range overflow")?;

    let format_range = format_descriptor.map(|descriptor| {
        let start = u64::from(descriptor.data_offset);
        let end = start.saturating_add(u64::from(descriptor.data_length));
        (start, end)
    });

    let mut out = Vec::new();
    let max_prefix = payload.len().saturating_sub(20).min(64);
    for prefix in 0..=max_prefix {
        let Some(raw_count) = u32_at(payload, prefix) else {
            continue;
        };
        let Ok(count) = usize::try_from(raw_count) else {
            continue;
        };
        if count == 0 || count > 4096 {
            continue;
        }

        let Some(raw_data_size) = u32_at(payload, prefix + 4) else {
            continue;
        };
        if raw_data_size != 4 {
            continue;
        }
        let data_size = 4usize;
        let Some(flags) = payload.get(prefix + 8..prefix + 12) else {
            continue;
        };

        let Some(position_bytes) = count
            .checked_add(1)
            .and_then(|value| value.checked_mul(4))
        else {
            continue;
        };
        let Some(target_bytes) = count.checked_mul(data_size) else {
            continue;
        };
        let Some(expected_end) = prefix
            .checked_add(12)
            .and_then(|value| value.checked_add(position_bytes))
            .and_then(|value| value.checked_add(target_bytes))
        else {
            continue;
        };
        if expected_end != payload.len() {
            continue;
        }

        let positions_start = prefix + 12;
        let targets_start = positions_start + position_bytes;

        let mut positions = Vec::with_capacity(count + 1);
        for index in 0..=count {
            let Some(raw) = u32_at(payload, positions_start + index * 4) else {
                positions.clear();
                break;
            };
            let mapped = if raw == 0 {
                text_start
            } else {
                u64::from(raw)
            };
            positions.push(mapped);
        }
        if positions.len() != count + 1 {
            continue;
        }

        let positions_monotonic = positions.windows(2).all(|pair| pair[0] <= pair[1]);
        let positions_inside_text_count = positions
            .iter()
            .filter(|value| **value >= text_start && **value <= text_end)
            .count();

        let mut targets = Vec::with_capacity(count);
        for index in 0..count {
            let Some(raw) = u32_at(payload, targets_start + index * 4) else {
                targets.clear();
                break;
            };
            targets.push(u64::from(raw));
        }
        if targets.len() != count {
            continue;
        }

        let targets_inside_format_chunk_count = format_range.map(|(start, end)| {
            targets
                .iter()
                .filter(|value| **value >= start && **value < end)
                .count()
        });

        out.push(BtePlcCandidate {
            prefix_offset: prefix,
            count: raw_count,
            data_size: raw_data_size,
            nonzero_flag_count: flags.iter().filter(|byte| **byte != 0).count(),
            position_count: positions.len(),
            positions_monotonic,
            positions_inside_text_count,
            text_start_present: positions.contains(&text_start),
            text_end_present: positions.contains(&text_end),
            count_covers_story_count: raw_count >= grounded_story_count,
            target_count: targets.len(),
            targets_inside_format_chunk_count,
        });
    }

    Ok(out)
}

fn diagnose(bytes: &[u8]) -> Result<WitnessRow> {
    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS_STREAM)
        .context("read Contents stream")?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(bytes), QUILL_STREAM)
        .context("read Quill stream")?;

    let (revision, grounded_story_count) = grounded_contents_story_count(&contents)?;

    let descriptors = parse_descriptor_directory(&quill)?;
    let syid = unique_descriptor(&descriptors, *b"SYID")?;
    let strs = unique_descriptor(&descriptors, *b"STRS")?;
    let text = unique_descriptor(&descriptors, *b"TEXT")?;
    let btep = optional_unique_descriptor(&descriptors, *b"BTEP")?;
    let btec = optional_unique_descriptor(&descriptors, *b"BTEC")?;
    let fdpp = optional_unique_descriptor(&descriptors, *b"FDPP")?;
    let fdpc = optional_unique_descriptor(&descriptors, *b"FDPC")?;

    let syid_payload = descriptor_range(&quill, syid)?;
    let strs_payload = descriptor_range(&quill, strs)?;
    descriptor_range(&quill, text)?;

    let expected_syid_len = 8u64 + 4u64 * u64::from(grounded_story_count);
    let expected_strs_len = 22u64 + 8u64 * u64::from(grounded_story_count);

    let btep_candidates =
        scan_bte_plc_candidates(&quill, btep, fdpp, text, grounded_story_count)?;
    let btec_candidates =
        scan_bte_plc_candidates(&quill, btec, fdpc, text, grounded_story_count)?;

    Ok(WitnessRow {
        source_sha256: sha256_hex(bytes),
        byte_len: bytes.len(),
        contents_serialization_revision: revision,
        grounded_story_count,

        syid_descriptor_length: syid.data_length,
        syid_length_matches_grounded_count: u64::from(syid.data_length) == expected_syid_len,
        syid_chunk_all_ff: syid_payload.iter().all(|byte| *byte == 0xff),

        strs_descriptor_length: strs.data_length,
        strs_length_matches_observed_22_plus_8n: u64::from(strs.data_length) == expected_strs_len,
        strs_chunk_all_ff: strs_payload.iter().all(|byte| *byte == 0xff),
        strs_direct_generic_plc_candidate_count:
            scan_direct_generic_plc_count(strs_payload, grounded_story_count),

        text_descriptor_length: text.data_length,
        text_utf16_units: u64::from(text.data_length) / 2,

        btep_descriptor_length: btep.map(|value| value.data_length),
        btec_descriptor_length: btec.map(|value| value.data_length),
        fdpp_descriptor_length: fdpp.map(|value| value.data_length),
        fdpc_descriptor_length: fdpc.map(|value| value.data_length),
        btep_candidates,
        btec_candidates,
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
        "schema": "chaptera.quill-story-early-text-boundary.v2",
        "witness_count": rows.len(),
        "rows": rows,
        "evidence_boundary": "exact witness SHA plus source-safe structural counts, lengths and booleans only; no filenames, paths, document text, Story IDs, raw payload bytes, absolute offsets or parser error text",
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
