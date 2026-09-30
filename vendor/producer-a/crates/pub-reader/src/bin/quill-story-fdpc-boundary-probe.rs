use anyhow::{bail, Context, Result};
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference, parse_confirmed_mature_story_catalog,
    CONTENTS_RAW_TYPE_STORY_CATALOG,
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
struct FdpcPageProfile {
    descriptor_index: usize,
    descriptor_length: u32,
    payload_all_ff: bool,
    stored_count: Option<u16>,
    tables_fit: bool,
    boundary_count: usize,
}

#[derive(Debug, Serialize)]
struct Row {
    source_sha256: String,
    grounded_story_count: u32,
    text_utf16_units: u64,
    fdpp_boundary_count: usize,
    fdpc_descriptor_count: usize,
    fdpc_valid_page_count: usize,
    fdpc_boundary_count: usize,
    fdpp_fdpc_intersection_count: usize,
    fdpp_fdpc_intersection_closes_text: bool,
    intersection_count_equals_grounded_story_count: bool,
    intersection_fdpp_ordinals_zero_based: Vec<usize>,
    ordinary_story_catalog_admitted: bool,
    ordinary_story_end_count: usize,
    ordinary_story_ends_all_in_fdpp: bool,
    ordinary_story_ends_all_in_fdpc: bool,
    ordinary_story_ends_all_in_intersection: bool,
    intersection_equals_ordinary_story_end_set: bool,
    fdpc_pages: Vec<FdpcPageProfile>,
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

fn parse_descriptor_directory(bytes: &[u8]) -> Result<Vec<Descriptor>> {
    let mut current = DESCRIPTOR_ROOT;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    while current != DESCRIPTOR_END {
        if !seen.insert(current) {
            bail!("descriptor list cycle");
        }
        let start = usize::try_from(current).context("descriptor node offset too large")?;
        let count =
            usize::from(u16_at(bytes, start + 2).context("descriptor node header truncated")?);
        let next = u32_at(bytes, start + 4).context("descriptor node next truncated")?;
        let array_start = start.checked_add(8).context("descriptor array overflow")?;
        let array_end = array_start
            .checked_add(
                count
                    .checked_mul(DESCRIPTOR_SIZE)
                    .context("descriptor array size overflow")?,
            )
            .context("descriptor array end overflow")?;
        if array_end > bytes.len() {
            bail!("descriptor array outside Quill");
        }

        for index in 0..count {
            let offset = array_start + index * DESCRIPTOR_SIZE;
            if u16_at(bytes, offset) != Some(DESCRIPTOR_PRESENT) {
                bail!("unexpected descriptor presence marker");
            }
            let name_raw = bytes
                .get(offset + 2..offset + 6)
                .context("descriptor name truncated")?;
            let name = [name_raw[0], name_raw[1], name_raw[2], name_raw[3]];
            let data_offset =
                u32_at(bytes, offset + 16).context("descriptor data offset truncated")?;
            let data_length =
                u32_at(bytes, offset + 20).context("descriptor data length truncated")?;
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

fn descriptor_range<'a>(bytes: &'a [u8], descriptor: &Descriptor) -> Result<&'a [u8]> {
    let start = usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
    let len = usize::try_from(descriptor.data_length).context("descriptor length too large")?;
    let end = start.checked_add(len).context("descriptor range overflow")?;
    bytes.get(start..end).context("descriptor outside Quill")
}

fn unique_descriptor(descriptors: &[Descriptor], name: [u8; 4]) -> Result<&Descriptor> {
    let mut found = descriptors.iter().filter(|item| item.name == name);
    let first = found.next().context("missing required Quill descriptor")?;
    if found.next().is_some() {
        bail!("duplicate required Quill descriptor");
    }
    Ok(first)
}

fn descriptors_named(descriptors: &[Descriptor], name: [u8; 4]) -> Vec<&Descriptor> {
    descriptors.iter().filter(|item| item.name == name).collect()
}

fn grounded_story_count(contents: &[u8]) -> Result<u32> {
    let stream = StreamPath(CONTENTS_STREAM.into());
    let header = parse_0x2c_header(stream.clone(), contents)?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)?;
    let mut found = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(contents, &trailer.directory, seq_num)?
        else {
            continue;
        };
        if reference.raw_types.len() == 1
            && reference.raw_types[0].value == CONTENTS_RAW_TYPE_STORY_CATALOG
            && reference.chunk_offsets.len() == 1
        {
            found.push(reference);
        }
    }
    if found.len() != 1 {
        bail!("expected exactly one strict Story catalog");
    }
    let chunk = parse_confirmed_0x2c_chunk(stream, contents, found[0].chunk_offsets[0].value)?;
    Ok(parse_confirmed_mature_story_catalog(contents, &chunk)?.declared_count)
}

fn boundary_set(
    quill: &[u8],
    descriptor: &Descriptor,
    text: &Descriptor,
) -> Result<Option<BTreeSet<u64>>> {
    let payload = descriptor_range(quill, descriptor)?;
    let Some(count_u16) = u16_at(payload, 0) else {
        return Ok(None);
    };
    if count_u16 == u16::MAX {
        return Ok(None);
    }
    let count = usize::from(count_u16);
    let table_end = 8usize
        .checked_add(count.checked_mul(4).context("boundary table size overflow")?)
        .context("boundary table end overflow")?;
    let chunk_table_end = table_end
        .checked_add(count.checked_mul(2).context("chunk table size overflow")?)
        .context("chunk table end overflow")?;
    if chunk_table_end > payload.len() {
        return Ok(None);
    }

    let text_start = u64::from(text.data_offset);
    let text_end = text_start
        .checked_add(u64::from(text.data_length))
        .context("TEXT range overflow")?;
    let mut out = BTreeSet::new();
    for index in 0..count {
        let absolute = u64::from(
            u32_at(payload, 8 + index * 4).context("boundary word truncated")?,
        );
        if absolute < text_start || absolute > text_end {
            return Ok(None);
        }
        let delta = absolute - text_start;
        if delta % 2 != 0 {
            return Ok(None);
        }
        out.insert(delta / 2);
    }
    Ok(Some(out))
}

fn diagnose(path: &Path) -> Result<Row> {
    let bytes = fs::read(path)?;
    let source_sha256 = sha256_hex(&bytes);
    let contents = pub_cfb::read_stream_reader(Cursor::new(&bytes), CONTENTS_STREAM)?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(&bytes), QUILL_STREAM)?;

    let grounded_story_count = grounded_story_count(&contents)?;
    let descriptors = parse_descriptor_directory(&quill)?;
    let text = unique_descriptor(&descriptors, *b"TEXT")?;
    if text.data_length % 2 != 0 {
        bail!("TEXT is not UTF-16 aligned");
    }
    let text_utf16_units = u64::from(text.data_length) / 2;

    let fdpp = unique_descriptor(&descriptors, *b"FDPP")?;
    let fdpp_set = boundary_set(&quill, fdpp, text)?.context("invalid FDPP boundary table")?;
    let fdpc_descriptors = descriptors_named(&descriptors, *b"FDPC");

    let mut fdpc_union = BTreeSet::new();
    let mut fdpc_pages = Vec::new();
    let mut fdpc_valid_page_count = 0usize;
    for (descriptor_index, descriptor) in fdpc_descriptors.iter().enumerate() {
        let payload = descriptor_range(&quill, descriptor)?;
        let stored_count = u16_at(payload, 0);
        let payload_all_ff = payload.iter().all(|byte| *byte == 0xff);
        let parsed = boundary_set(&quill, descriptor, text)?;
        let tables_fit = parsed.is_some();
        let boundary_count = parsed.as_ref().map_or(0, BTreeSet::len);
        if let Some(set) = parsed {
            fdpc_valid_page_count += 1;
            fdpc_union.extend(set);
        }
        fdpc_pages.push(FdpcPageProfile {
            descriptor_index,
            descriptor_length: descriptor.data_length,
            payload_all_ff,
            stored_count,
            tables_fit,
            boundary_count,
        });
    }

    let intersection = fdpp_set
        .intersection(&fdpc_union)
        .copied()
        .collect::<BTreeSet<_>>();
    let intersection_fdpp_ordinals_zero_based = fdpp_set
        .iter()
        .enumerate()
        .filter_map(|(ordinal, value)| intersection.contains(value).then_some(ordinal))
        .collect::<Vec<_>>();

    let ordinary = pub_quill::parse_confirmed_story_catalog(
        StreamPath(QUILL_STREAM.into()),
        &quill,
    )
    .ok();
    let mut ordinary_story_ends = BTreeSet::new();
    if let Some(catalog) = &ordinary {
        let mut cumulative = 0u64;
        for story in &catalog.stories {
            cumulative = cumulative
                .checked_add(u64::from(story.utf16_code_units))
                .context("ordinary Story length overflow")?;
            ordinary_story_ends.insert(cumulative);
        }
    }

    Ok(Row {
        source_sha256,
        grounded_story_count,
        text_utf16_units,
        fdpp_boundary_count: fdpp_set.len(),
        fdpc_descriptor_count: fdpc_descriptors.len(),
        fdpc_valid_page_count,
        fdpc_boundary_count: fdpc_union.len(),
        fdpp_fdpc_intersection_count: intersection.len(),
        fdpp_fdpc_intersection_closes_text: intersection.contains(&text_utf16_units),
        intersection_count_equals_grounded_story_count: intersection.len()
            == usize::try_from(grounded_story_count).unwrap_or(usize::MAX),
        intersection_fdpp_ordinals_zero_based,
        ordinary_story_catalog_admitted: ordinary.is_some(),
        ordinary_story_end_count: ordinary_story_ends.len(),
        ordinary_story_ends_all_in_fdpp: ordinary.is_some()
            && ordinary_story_ends.iter().all(|value| fdpp_set.contains(value)),
        ordinary_story_ends_all_in_fdpc: ordinary.is_some()
            && ordinary_story_ends.iter().all(|value| fdpc_union.contains(value)),
        ordinary_story_ends_all_in_intersection: ordinary.is_some()
            && ordinary_story_ends.iter().all(|value| intersection.contains(value)),
        intersection_equals_ordinary_story_end_set: ordinary.is_some()
            && intersection == ordinary_story_ends,
        fdpc_pages,
    })
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = fs::read_dir(root)?
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
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        bail!("usage: quill-story-fdpc-boundary-probe WITNESS_DIR OUTPUT.json");
    }
    let root = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let paths = pub_paths(&root)?;
    if paths.len() != 28 {
        bail!("expected exactly 28 same-source siblings, found {}", paths.len());
    }

    let mut rows = Vec::new();
    let mut skipped = 0usize;
    for path in paths {
        match diagnose(&path) {
            Ok(row) => rows.push(row),
            Err(_) => skipped += 1,
        }
    }
    rows.sort_by(|a, b| a.source_sha256.cmp(&b.source_sha256));

    let ordinary_rows = rows
        .iter()
        .filter(|row| row.ordinary_story_catalog_admitted)
        .count();
    let exact_intersection_controls = rows
        .iter()
        .filter(|row| row.intersection_equals_ordinary_story_end_set)
        .count();

    let report = serde_json::json!({
        "schema": "chaptera.quill-story-fdpc-boundary.v1",
        "witness_count": 28,
        "admitted_count": rows.len(),
        "skipped_count": skipped,
        "ordinary_story_catalog_admitted_count": ordinary_rows,
        "intersection_equals_ordinary_story_end_set_count": exact_intersection_controls,
        "rows": rows,
        "evidence_boundary": "28 exact SHA-addressed same-source siblings only; no full corpus; source-safe counts, booleans and FDPP ordinals only; no filenames, document text, raw payload bytes or absolute offsets"
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
