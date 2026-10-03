use anyhow::{bail, Context, Result};
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference, parse_confirmed_mature_story_catalog, MatureStoryCatalog,
    CONTENTS_RAW_TYPE_STORY_CATALOG,
};
use pub_core::StreamPath;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, env, fs, io::Cursor, path::Path};

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

#[derive(Debug)]
struct Doc {
    bytes: Vec<u8>,
    quill: Vec<u8>,
    story_catalog: MatureStoryCatalog,
    descriptors: Vec<Descriptor>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    variant_sha256: String,
    source_byte_len: usize,
    variant_byte_len: usize,
    source_story_count: u32,
    variant_story_count: u32,
    contents_story_count_equal: bool,
    contents_text_id_order_equal: bool,
    contents_text_id_set_equal: bool,
    source_text_sha256: String,
    variant_text_sha256: String,
    text_byte_identical: bool,
    source_text_utf16_units: u64,
    variant_text_utf16_units: u64,
    source_fdpp_boundary_count: usize,
    variant_fdpp_boundary_count: usize,
    variant_ordinary_quill_admitted: bool,
    variant_quill_story_count: usize,
    variant_story_utf16_lengths: Vec<u32>,
    variant_story_end_utf16: Vec<u64>,
    variant_story_ends_all_in_source_fdpp: bool,
    variant_story_ends_close_source_text: bool,
    variant_story_end_source_fdpp_ordinals_zero_based: Vec<usize>,
    exact_same_document_partition_donor: bool,
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
            let raw = bytes
                .get(offset + 2..offset + 6)
                .context("descriptor name truncated")?;
            out.push(Descriptor {
                name: [raw[0], raw[1], raw[2], raw[3]],
                data_offset: u32_at(bytes, offset + 16)
                    .context("descriptor data offset truncated")?,
                data_length: u32_at(bytes, offset + 20)
                    .context("descriptor data length truncated")?,
            });
        }
        current = next;
    }
    Ok(out)
}

fn unique_descriptor(descriptors: &[Descriptor], name: [u8; 4]) -> Result<&Descriptor> {
    let mut found = descriptors.iter().filter(|item| item.name == name);
    let first = found.next().context("required Quill descriptor missing")?;
    if found.next().is_some() {
        bail!("duplicate required Quill descriptor");
    }
    Ok(first)
}

fn descriptor_range<'a>(bytes: &'a [u8], descriptor: &Descriptor) -> Result<&'a [u8]> {
    let start = usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
    let len = usize::try_from(descriptor.data_length).context("descriptor length too large")?;
    let end = start
        .checked_add(len)
        .context("descriptor range overflow")?;
    bytes.get(start..end).context("descriptor outside Quill")
}

fn grounded_contents_story_catalog(contents: &[u8]) -> Result<MatureStoryCatalog> {
    let stream = StreamPath(CONTENTS_STREAM.into());
    let header =
        parse_0x2c_header(stream.clone(), contents).context("parse mature Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature Contents trailer")?;

    let mut found = Vec::new();
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
            found.push(reference);
        }
    }
    if found.len() != 1 {
        bail!("expected exactly one strict 0x65 Story catalog");
    }
    let chunk = parse_confirmed_0x2c_chunk(stream, contents, found[0].chunk_offsets[0].value)
        .context("parse Story catalog chunk")?;
    parse_confirmed_mature_story_catalog(contents, &chunk).context("parse grounded Story catalog")
}

fn load(path: &Path) -> Result<Doc> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let contents = pub_cfb::read_stream_reader(Cursor::new(&bytes), CONTENTS_STREAM)
        .context("read Contents")?;
    let quill =
        pub_cfb::read_stream_reader(Cursor::new(&bytes), QUILL_STREAM).context("read Quill")?;
    let story_catalog = grounded_contents_story_catalog(&contents)?;
    let descriptors = parse_descriptor_directory(&quill)?;
    Ok(Doc {
        bytes,
        quill,
        story_catalog,
        descriptors,
    })
}

fn fdpp_boundaries_utf16(doc: &Doc) -> Result<Vec<u64>> {
    let text = unique_descriptor(&doc.descriptors, *b"TEXT")?;
    let fdpp = unique_descriptor(&doc.descriptors, *b"FDPP")?;
    let payload = descriptor_range(&doc.quill, fdpp)?;
    let count = usize::from(u16_at(payload, 0).context("FDPP count missing")?);
    let table_end = 8usize
        .checked_add(count.checked_mul(4).context("FDPP table overflow")?)
        .context("FDPP table end overflow")?;
    if table_end > payload.len() {
        bail!("FDPP boundary table outside payload");
    }
    let text_start = u64::from(text.data_offset);
    let text_end = text_start
        .checked_add(u64::from(text.data_length))
        .context("TEXT range overflow")?;
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let absolute =
            u64::from(u32_at(payload, 8 + index * 4).context("FDPP boundary truncated")?);
        if absolute < text_start || absolute > text_end {
            bail!("FDPP boundary outside TEXT");
        }
        let delta = absolute - text_start;
        if delta % 2 != 0 {
            bail!("FDPP boundary not UTF-16 aligned");
        }
        out.push(delta / 2);
    }
    if !out.windows(2).all(|pair| pair[0] < pair[1]) {
        bail!("FDPP boundaries not strictly increasing");
    }
    Ok(out)
}

fn text_bytes(doc: &Doc) -> Result<&[u8]> {
    descriptor_range(&doc.quill, unique_descriptor(&doc.descriptors, *b"TEXT")?)
}

fn main() -> Result<()> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        bail!("usage: quill-story-lineage-compare SOURCE.pub VARIANT.pub OUTPUT.json");
    }
    let source = load(Path::new(&args[0]))?;
    let variant = load(Path::new(&args[1]))?;
    let output = Path::new(&args[2]);

    let source_text = text_bytes(&source)?;
    let variant_text = text_bytes(&variant)?;
    if source_text.len() % 2 != 0 || variant_text.len() % 2 != 0 {
        bail!("TEXT is not UTF-16 aligned");
    }

    let source_fdpp = fdpp_boundaries_utf16(&source)?;
    let variant_fdpp = fdpp_boundaries_utf16(&variant)?;
    let source_fdpp_set = source_fdpp.iter().copied().collect::<BTreeSet<_>>();

    let source_ids = source
        .story_catalog
        .entries
        .iter()
        .map(|entry| entry.text_id)
        .collect::<Vec<_>>();
    let variant_ids = variant
        .story_catalog
        .entries
        .iter()
        .map(|entry| entry.text_id)
        .collect::<Vec<_>>();
    let source_id_set = source_ids.iter().copied().collect::<BTreeSet<_>>();
    let variant_id_set = variant_ids.iter().copied().collect::<BTreeSet<_>>();

    let ordinary =
        pub_quill::parse_confirmed_story_catalog(StreamPath(QUILL_STREAM.into()), &variant.quill)
            .ok();

    let mut lengths = Vec::new();
    let mut ends = Vec::new();
    if let Some(catalog) = &ordinary {
        let mut cumulative = 0u64;
        for story in &catalog.stories {
            lengths.push(story.utf16_code_units);
            cumulative = cumulative
                .checked_add(u64::from(story.utf16_code_units))
                .context("Story length overflow")?;
            ends.push(cumulative);
        }
    }

    let all_in_source_fdpp =
        ordinary.is_some() && ends.iter().all(|end| source_fdpp_set.contains(end));
    let source_text_units = u64::try_from(source_text.len() / 2).context("TEXT units overflow")?;
    let variant_text_units =
        u64::try_from(variant_text.len() / 2).context("TEXT units overflow")?;
    let closes_source = ordinary.is_some() && ends.last().copied() == Some(source_text_units);
    let endpoint_ordinals = if all_in_source_fdpp {
        ends.iter()
            .map(|end| {
                source_fdpp
                    .iter()
                    .position(|value| value == end)
                    .context("donor endpoint missing from source FDPP")
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    let text_identical = source_text == variant_text;
    let counts_equal = source.story_catalog.declared_count == variant.story_catalog.declared_count;
    let order_equal = source_ids == variant_ids;
    let set_equal = source_id_set == variant_id_set;
    let variant_quill_story_count = ordinary.as_ref().map_or(0, |catalog| catalog.stories.len());
    let exact = text_identical
        && counts_equal
        && set_equal
        && ordinary.is_some()
        && variant_quill_story_count
            == usize::try_from(source.story_catalog.declared_count).unwrap_or(usize::MAX)
        && all_in_source_fdpp
        && closes_source;

    let receipt = Receipt {
        schema: "chaptera.quill-story-lineage-compare.v1",
        source_sha256: sha256_hex(&source.bytes),
        variant_sha256: sha256_hex(&variant.bytes),
        source_byte_len: source.bytes.len(),
        variant_byte_len: variant.bytes.len(),
        source_story_count: source.story_catalog.declared_count,
        variant_story_count: variant.story_catalog.declared_count,
        contents_story_count_equal: counts_equal,
        contents_text_id_order_equal: order_equal,
        contents_text_id_set_equal: set_equal,
        source_text_sha256: sha256_hex(source_text),
        variant_text_sha256: sha256_hex(variant_text),
        text_byte_identical: text_identical,
        source_text_utf16_units: source_text_units,
        variant_text_utf16_units: variant_text_units,
        source_fdpp_boundary_count: source_fdpp.len(),
        variant_fdpp_boundary_count: variant_fdpp.len(),
        variant_ordinary_quill_admitted: ordinary.is_some(),
        variant_quill_story_count,
        variant_story_utf16_lengths: lengths,
        variant_story_end_utf16: ends,
        variant_story_ends_all_in_source_fdpp: all_in_source_fdpp,
        variant_story_ends_close_source_text: closes_source,
        variant_story_end_source_fdpp_ordinals_zero_based: endpoint_ordinals,
        exact_same_document_partition_donor: exact,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
