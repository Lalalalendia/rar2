use anyhow::{Context, Result, bail};
use pub_contents::{
    Contents0x2cChunkReference, RawContentsBlockBody, parse_0x2c_header,
    parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference, parse_confirmed_margins_page_extent,
};
use pub_core::StreamPath;
use pub_model::Sha256Digest;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

const CONTENTS_STREAM: &str = "/Contents";
const RAW_TYPE_DOCUMENT: u16 = 0x44;
const RAW_TYPE_PAGE_MANAGER_CANDIDATE: u16 = 0x4A;
const RAW_TYPE_MARGINS: u16 = 0x4C;
const RAW_TYPE_IMPOSITION: u16 = 0x8A;
const RAW_TYPE_IMPOSITION_HELPER_A: u16 = 0x8B;
const RAW_TYPE_IMPOSITION_HELPER_B: u16 = 0x8C;

const A4_SHORT: u32 = 7_560_000;
const A4_LONG: u32 = 10_692_000;
const LETTER_SHORT: u32 = 7_772_400;
const LETTER_LONG: u32 = 10_058_400;

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    fixtures: Vec<FixtureReceipt>,
}

#[derive(Debug, Serialize)]
struct FixtureReceipt {
    fixture: String,
    source_sha256: String,
    source_byte_len: usize,
    serialization_revision: u16,
    canonical_page_sizes_emu: Vec<[i64; 2]>,
    carriers: Vec<CarrierReceipt>,
}

#[derive(Debug, Serialize)]
struct CarrierReceipt {
    seq_num: u32,
    raw_type: u16,
    parent_seq_num: Option<u32>,
    declared_length: u32,
    fully_decoded_prefix: bool,
    supported_u32_fields: Vec<U32Field>,
    target_u32_matches: Vec<TargetMatch>,
    confirmed_margins_extent_emu: Option<[u32; 2]>,
}

#[derive(Debug, Serialize)]
struct U32Field {
    id: u16,
    block_type: u8,
    value: u32,
}

#[derive(Debug, Serialize)]
struct TargetMatch {
    label: &'static str,
    value: u32,
    relative_offset: usize,
    absolute_offset: u64,
}

fn main() -> Result<()> {
    let paths = std::env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        bail!("usage: page_extent_authority_census <fixture.pub>...");
    }

    let mut fixtures = Vec::with_capacity(paths.len());
    for path in paths {
        fixtures.push(inspect_fixture(&path)?);
    }

    let receipt = Receipt {
        schema: "chaptera.page-extent-authority-census.v1",
        fixtures,
    };
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}

fn inspect_fixture(path: &Path) -> Result<FixtureReceipt> {
    let source = std::fs::read(path)
        .with_context(|| format!("read {}", path.display()))?;
    let source_sha256 = sha256_hex(&source);
    let source_hash: Sha256Digest = source_sha256
        .parse()
        .context("parse source SHA-256 as canonical digest")?;

    let contents = pub_cfb::read_stream_reader(Cursor::new(source.as_slice()), CONTENTS_STREAM)
        .with_context(|| format!("read {CONTENTS_STREAM} from {}", path.display()))?;
    let stream = StreamPath(CONTENTS_STREAM.to_owned());
    let header = parse_0x2c_header(stream.clone(), &contents)
        .context("parse mature Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse mature Contents trailer")?;
    let references = build_reference_index(&contents, &trailer.directory)?;

    let build = pub_reader::build_mature_0x2c_source_graph(
        Cursor::new(source.as_slice()),
        source_hash,
    )
    .context("build current canonical source graph")?;
    let canonical_page_sizes_emu = build
        .graph
        .pages
        .values()
        .map(|page| [page.size.width.get(), page.size.height.get()])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let interesting = [
        RAW_TYPE_DOCUMENT,
        RAW_TYPE_PAGE_MANAGER_CANDIDATE,
        RAW_TYPE_MARGINS,
        RAW_TYPE_IMPOSITION,
        RAW_TYPE_IMPOSITION_HELPER_A,
        RAW_TYPE_IMPOSITION_HELPER_B,
    ];

    let mut carriers = Vec::new();
    for reference in references.values() {
        let Some(raw_type) = single_raw_type(reference) else {
            continue;
        };
        if !interesting.contains(&raw_type) {
            continue;
        }

        let seq_num = u32::try_from(reference.seq_num)
            .context("Contents seqNum does not fit u32")?;
        let offset = match reference.chunk_offsets.as_slice() {
            [field] => field.value,
            many => bail!(
                "seq {seq_num} raw0x{raw_type:02X} has {} chunk offsets",
                many.len()
            ),
        };
        let chunk = parse_confirmed_0x2c_chunk(stream.clone(), &contents, offset)
            .with_context(|| format!("parse seq {seq_num} raw0x{raw_type:02X}"))?;

        let supported_u32_fields = chunk
            .fields
            .iter()
            .filter_map(|field| match field.body {
                RawContentsBlockBody::U32 { value, .. } => Some(U32Field {
                    id: field.id,
                    block_type: field.block_type,
                    value,
                }),
                _ => None,
            })
            .collect::<Vec<_>>();

        let start = usize::try_from(chunk.source.offset).context("chunk start too large")?;
        let len = usize::try_from(chunk.source.len).context("chunk length too large")?;
        let end = start.checked_add(len).context("chunk range overflow")?;
        let raw = contents
            .get(start..end)
            .with_context(|| format!("chunk range outside Contents for seq {seq_num}"))?;
        let target_u32_matches = scan_target_u32(raw, chunk.source.offset);

        let confirmed_margins_extent_emu = if raw_type == RAW_TYPE_MARGINS {
            let extent = parse_confirmed_margins_page_extent(&contents, &chunk)
                .with_context(|| format!("parse confirmed OplMg extent seq {seq_num}"))?;
            Some([extent.width_emu, extent.height_emu])
        } else {
            None
        };

        carriers.push(CarrierReceipt {
            seq_num,
            raw_type,
            parent_seq_num: single_parent_seq(reference),
            declared_length: chunk.declared_length,
            fully_decoded_prefix: chunk.is_fully_decoded(),
            supported_u32_fields,
            target_u32_matches,
            confirmed_margins_extent_emu,
        });
    }
    carriers.sort_by_key(|carrier| (carrier.raw_type, carrier.seq_num));

    Ok(FixtureReceipt {
        fixture: path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("fixture")
            .to_owned(),
        source_sha256,
        source_byte_len: source.len(),
        serialization_revision: header.preamble.serialization_revision,
        canonical_page_sizes_emu,
        carriers,
    })
}

fn build_reference_index(
    contents: &[u8],
    directory: &pub_contents::Contents0x2cDirectory,
) -> Result<BTreeMap<u32, Contents0x2cChunkReference>> {
    let mut references = BTreeMap::new();
    for seq_num in 0..directory.slots.len() {
        let Some(reference) = parse_confirmed_chunk_reference(contents, directory, seq_num)
            .with_context(|| format!("parse directory reference seq {seq_num}"))?
        else {
            continue;
        };
        let key = u32::try_from(reference.seq_num).context("reference seqNum does not fit u32")?;
        if references.insert(key, reference).is_some() {
            bail!("duplicate Contents directory seq {key}");
        }
    }
    Ok(references)
}

fn single_raw_type(reference: &Contents0x2cChunkReference) -> Option<u16> {
    match reference.raw_types.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn single_parent_seq(reference: &Contents0x2cChunkReference) -> Option<u32> {
    match reference.parent_seq_nums.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn scan_target_u32(raw: &[u8], absolute_start: u64) -> Vec<TargetMatch> {
    let targets = [
        ("a4_short", A4_SHORT),
        ("a4_long", A4_LONG),
        ("letter_short", LETTER_SHORT),
        ("letter_long", LETTER_LONG),
    ];
    let mut matches = Vec::new();
    for (relative_offset, bytes) in raw.windows(4).enumerate() {
        let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        for (label, target) in targets {
            if value == target {
                matches.push(TargetMatch {
                    label,
                    value,
                    relative_offset,
                    absolute_offset: absolute_start + relative_offset as u64,
                });
            }
        }
    }
    matches
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
