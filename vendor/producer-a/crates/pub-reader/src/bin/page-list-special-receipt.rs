use anyhow::{Context, Result, anyhow, bail};
use pub_contents::{
    BLOCK_TYPE_HANDLE_U32, BLOCK_TYPE_REFERENCE_U32, DOCUMENT_PAGE_LIST_ID,
    Contents0x2cChunkReference, RawContentsBlockBody, parse_0x2c_header,
    parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference, parse_confirmed_document_page_list,
};
use pub_core::StreamPath;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{BufWriter, Cursor},
    path::Path,
};

const CONTENTS_STREAM_PATH: &str = "/Contents";
const RAW_TYPE_PAGE: u16 = 0x43;
const RAW_TYPE_DOCUMENT: u16 = 0x44;
const RAW_TYPE_MARGINS: u16 = 0x4C;
const RAW_TYPE_PAGE_LIST_SPECIAL: u16 = 0x59;
const SCHEMA: &str = "chaptera.mature-029-special59-signature.v1";

fn single_raw_type(reference: &Contents0x2cChunkReference) -> Option<u16> {
    match reference.raw_types.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn build_reference_index(
    contents: &[u8],
    directory: &pub_contents::Contents0x2cDirectory,
) -> Result<BTreeMap<u32, Contents0x2cChunkReference>> {
    let mut references = BTreeMap::new();
    for seq_num in 0..directory.slots.len() {
        let Some(reference) = parse_confirmed_chunk_reference(contents, directory, seq_num)
            .with_context(|| format!("parse Contents directory reference ordinal {seq_num}"))?
        else {
            continue;
        };
        let key = u32::try_from(reference.seq_num)
            .map_err(|_| anyhow!("Contents seqNum does not fit u32"))?;
        if references.insert(key, reference).is_some() {
            bail!("duplicate Contents directory seqNum");
        }
    }
    Ok(references)
}

fn chunk_for_reference(
    contents: &[u8],
    reference: &Contents0x2cChunkReference,
) -> Result<pub_contents::Contents0x2cChunk> {
    if reference.chunk_offsets.len() != 1 {
        bail!("reference does not have exactly one chunk offset");
    }
    parse_confirmed_0x2c_chunk(
        StreamPath(CONTENTS_STREAM_PATH.into()),
        contents,
        reference.chunk_offsets[0].value,
    )
    .map_err(Into::into)
}

fn classify_target(
    value: u32,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_ordinals: &BTreeMap<u32, usize>,
    document_seq: u32,
    special_seq: u32,
) -> String {
    if value == special_seq {
        return "self_special".to_owned();
    }
    if value == document_seq {
        return "document".to_owned();
    }
    if let Some(ordinal) = page_ordinals.get(&value) {
        return format!("page_ordinal:{ordinal}");
    }
    let Some(reference) = references.get(&value) else {
        return "unresolved".to_owned();
    };
    match single_raw_type(reference) {
        Some(RAW_TYPE_PAGE_LIST_SPECIAL) => "other_special".to_owned(),
        Some(RAW_TYPE_MARGINS) => "margins".to_owned(),
        Some(RAW_TYPE_DOCUMENT) => "document_other".to_owned(),
        Some(RAW_TYPE_PAGE) => "page_outside_document_list".to_owned(),
        Some(raw_type) => format!("raw_type:0x{raw_type:02X}"),
        None => "raw_type:ambiguous".to_owned(),
    }
}

fn body_receipt(
    field: &pub_contents::RawContentsBlock,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_ordinals: &BTreeMap<u32, usize>,
    document_seq: u32,
    special_seq: u32,
) -> Value {
    match &field.body {
        RawContentsBlockBody::Empty => json!({"kind": "empty"}),
        RawContentsBlockBody::U16 { .. } => json!({"kind": "u16", "value_retained": false}),
        RawContentsBlockBody::U32 { value, .. } => {
            if matches!(field.block_type, BLOCK_TYPE_REFERENCE_U32 | BLOCK_TYPE_HANDLE_U32) {
                json!({
                    "kind": "u32_relation",
                    "target_class": classify_target(
                        *value,
                        references,
                        page_ordinals,
                        document_seq,
                        special_seq,
                    ),
                    "value_retained": false,
                })
            } else {
                json!({"kind": "u32", "value_retained": false})
            }
        }
        RawContentsBlockBody::Fixed8 { .. } => {
            json!({"kind": "fixed8", "bytes_retained": false})
        }
        RawContentsBlockBody::Fixed16 { .. } => {
            json!({"kind": "fixed16", "bytes_retained": false})
        }
        RawContentsBlockBody::Container {
            declared_length, ..
        } => json!({
            "kind": "container",
            "declared_length": declared_length,
            "bytes_retained": false,
        }),
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .context("usage: page-list-special-receipt SOURCE.pub OUTPUT.json")?;
    let output = args
        .next()
        .context("usage: page-list-special-receipt SOURCE.pub OUTPUT.json")?;
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }

    let pub_bytes = fs::read(&source).with_context(|| format!("read {:?}", source))?;
    let source_sha256 = Sha256::digest(&pub_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .context("read /Contents")?;
    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header =
        parse_0x2c_header(stream.clone(), &contents).context("parse mature 0x2C header")?;
    let trailer =
        parse_confirmed_0x2c_trailer_root(&contents, &header).context("parse mature trailer")?;
    let references = build_reference_index(&contents, &trailer.directory)?;

    let documents = references
        .iter()
        .filter(|(_, reference)| single_raw_type(reference) == Some(RAW_TYPE_DOCUMENT))
        .collect::<Vec<_>>();
    let [(document_seq_ref, document_reference_ref)] = documents.as_slice() else {
        bail!("expected exactly one DOCUMENT reference");
    };
    let document_seq = **document_seq_ref;
    let document_reference = *document_reference_ref;
    let document_chunk = chunk_for_reference(&contents, document_reference)?;
    let page_list_block = document_chunk
        .fields
        .iter()
        .filter(|field| field.id == DOCUMENT_PAGE_LIST_ID)
        .collect::<Vec<_>>();
    let [page_list_block] = page_list_block.as_slice() else {
        bail!("expected exactly one DOCUMENT PageList field");
    };
    let page_list = parse_confirmed_document_page_list(&contents, (**page_list_block).clone())
        .context("parse DOCUMENT PageList")?;

    let mut page_ordinals = BTreeMap::new();
    let mut specials = Vec::new();
    for (document_ordinal, entry) in page_list.entries.iter().enumerate() {
        let raw_type = references.get(&entry.handle).and_then(single_raw_type);
        match raw_type {
            Some(RAW_TYPE_PAGE) => {
                page_ordinals.insert(entry.handle, document_ordinal);
            }
            Some(RAW_TYPE_PAGE_LIST_SPECIAL) => {
                specials.push((document_ordinal, entry.handle));
            }
            _ => {}
        }
    }
    let [(special_document_ordinal_ref, special_seq_ref)] = specials.as_slice() else {
        bail!("expected exactly one raw0x59 PageList special entry");
    };
    let special_document_ordinal = *special_document_ordinal_ref;
    let special_seq = *special_seq_ref;
    let special_reference = references
        .get(&special_seq)
        .context("special PageList entry has no directory reference")?;
    let special_chunk = chunk_for_reference(&contents, special_reference)?;

    let fields = special_chunk
        .fields
        .iter()
        .map(|field| {
            json!({
                "field_id": field.id,
                "block_type": format!("0x{:02X}", field.block_type),
                "body": body_receipt(
                    field,
                    &references,
                    &page_ordinals,
                    document_seq,
                    special_seq,
                ),
            })
        })
        .collect::<Vec<_>>();

    let receipt = json!({
        "schema": SCHEMA,
        "source_sha256": source_sha256,
        "source_bytes": pub_bytes.len(),
        "document_page_list_entry_count": page_list.entries.len(),
        "confirmed_page_count": page_ordinals.len(),
        "special_entry_count": 1,
        "special_document_ordinal": special_document_ordinal,
        "special_chunk": {
            "declared_length": special_chunk.declared_length,
            "fully_decoded": special_chunk.is_fully_decoded(),
            "unsupported_tail_present": special_chunk.unsupported_tail.is_some(),
            "unsupported_tail_length": special_chunk.unsupported_tail.as_ref().map(|span| span.len),
            "fields": fields,
        },
        "claims": {
            "measurement_only": true,
            "raw_u16_values_retained": false,
            "raw_u32_values_retained": false,
            "raw_fixed_bytes_retained": false,
            "raw_seq_nums_retained": false,
            "story_text_retained": false,
            "page_relation_uses_document_ordinal_only": true,
            "source_graph_mutated": false,
            "viewer_semantics_changed": false,
        },
    });

    let file = File::create(Path::new(&output))
        .with_context(|| format!("create {:?}", output))?;
    serde_json::to_writer_pretty(BufWriter::new(file), &receipt)?;
    Ok(())
}
