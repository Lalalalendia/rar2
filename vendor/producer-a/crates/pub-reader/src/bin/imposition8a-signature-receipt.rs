use anyhow::{Context, Result, anyhow, bail};
use pub_contents::{
    BLOCK_TYPE_CONTAINER_88, BLOCK_TYPE_CONTAINER_90, BLOCK_TYPE_CONTAINER_A0, BLOCK_TYPE_DUMMY,
    BLOCK_TYPE_EMPTY, BLOCK_TYPE_FIXED_8, BLOCK_TYPE_FIXED_16, BLOCK_TYPE_HANDLE_U32,
    BLOCK_TYPE_REFERENCE_U32, BLOCK_TYPE_U16, BLOCK_TYPE_U16_SERVICE, BLOCK_TYPE_U32,
    Contents0x2cChunkReference, DOCUMENT_PAGE_LIST_ID, RawContentsBlockBody,
    decode_packed_field_tag, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
    parse_confirmed_document_page_list,
};
use pub_core::StreamPath;
use serde_json::{Value, json};
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
const RAW_TYPE_IMPOSITION_ENGINE: u16 = 0x8A;
const SCHEMA: &str = "chaptera.mature-imposition8a-signature.v1";

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

fn relation_class(
    value: u32,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_ordinals: &BTreeMap<u32, usize>,
    document_seq: Option<u32>,
) -> String {
    if Some(value) == document_seq {
        return "document".to_owned();
    }
    if let Some(ordinal) = page_ordinals.get(&value) {
        return format!("page_ordinal:{ordinal}");
    }
    let Some(reference) = references.get(&value) else {
        return "unresolved".to_owned();
    };
    match single_raw_type(reference) {
        Some(RAW_TYPE_IMPOSITION_ENGINE) => "imposition_engine".to_owned(),
        Some(RAW_TYPE_PAGE_LIST_SPECIAL) => "page_list_special".to_owned(),
        Some(RAW_TYPE_MARGINS) => "margins".to_owned(),
        Some(RAW_TYPE_DOCUMENT) => "document_other".to_owned(),
        Some(RAW_TYPE_PAGE) => "page_outside_document_list".to_owned(),
        Some(raw_type) => format!("raw_type:0x{raw_type:02X}"),
        None => "raw_type:ambiguous".to_owned(),
    }
}

fn take_u16(bytes: &[u8], pos: &mut usize) -> Option<u16> {
    let end = pos.checked_add(2)?;
    let raw = bytes.get(*pos..end)?;
    *pos = end;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn take_u32(bytes: &[u8], pos: &mut usize) -> Option<u32> {
    let end = pos.checked_add(4)?;
    let raw = bytes.get(*pos..end)?;
    *pos = end;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn take_bytes<'a>(bytes: &'a [u8], pos: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = pos.checked_add(len)?;
    let raw = bytes.get(*pos..end)?;
    *pos = end;
    Some(raw)
}

fn parse_nested_sequence(
    bytes: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_ordinals: &BTreeMap<u32, usize>,
    document_seq: Option<u32>,
    depth: usize,
) -> Value {
    if depth > 12 {
        return json!({
            "fully_decoded": false,
            "terminal": "depth_limit",
            "remaining_bytes": bytes.len(),
        });
    }

    let mut pos = 0usize;
    let mut fields = Vec::new();
    let mut terminal = None::<String>;

    while pos < bytes.len() {
        if bytes.len() - pos < 2 {
            terminal = Some("truncated_tag".to_owned());
            break;
        }
        let raw_tag = [bytes[pos], bytes[pos + 1]];
        pos += 2;
        let (field_id, block_type) = decode_packed_field_tag(raw_tag);

        let body = match block_type {
            BLOCK_TYPE_EMPTY | BLOCK_TYPE_DUMMY => json!({"kind": "empty"}),
            BLOCK_TYPE_U16 | BLOCK_TYPE_U16_SERVICE => {
                if take_u16(bytes, &mut pos).is_none() {
                    terminal = Some("truncated_u16".to_owned());
                    break;
                }
                json!({"kind": "u16", "value_retained": false})
            }
            BLOCK_TYPE_U32 | BLOCK_TYPE_REFERENCE_U32 | BLOCK_TYPE_HANDLE_U32 => {
                let Some(value) = take_u32(bytes, &mut pos) else {
                    terminal = Some("truncated_u32".to_owned());
                    break;
                };
                if matches!(block_type, BLOCK_TYPE_REFERENCE_U32 | BLOCK_TYPE_HANDLE_U32) {
                    json!({
                        "kind": "u32_relation",
                        "target_class": relation_class(
                            value,
                            references,
                            page_ordinals,
                            document_seq,
                        ),
                        "value_retained": false,
                    })
                } else {
                    json!({"kind": "u32", "value_retained": false})
                }
            }
            BLOCK_TYPE_FIXED_8 => {
                if take_bytes(bytes, &mut pos, 8).is_none() {
                    terminal = Some("truncated_fixed8".to_owned());
                    break;
                }
                json!({"kind": "fixed8", "bytes_retained": false})
            }
            BLOCK_TYPE_FIXED_16 => {
                if take_bytes(bytes, &mut pos, 16).is_none() {
                    terminal = Some("truncated_fixed16".to_owned());
                    break;
                }
                json!({"kind": "fixed16", "bytes_retained": false})
            }
            BLOCK_TYPE_CONTAINER_88 | BLOCK_TYPE_CONTAINER_90 | BLOCK_TYPE_CONTAINER_A0 => {
                let Some(declared_length) = take_u32(bytes, &mut pos) else {
                    terminal = Some("truncated_container_length".to_owned());
                    break;
                };
                if declared_length < 4 {
                    terminal = Some("invalid_container_length".to_owned());
                    break;
                }
                let Ok(content_len) = usize::try_from(declared_length - 4) else {
                    terminal = Some("container_length_overflow".to_owned());
                    break;
                };
                let Some(content) = take_bytes(bytes, &mut pos, content_len) else {
                    terminal = Some("truncated_container_body".to_owned());
                    break;
                };
                json!({
                    "kind": "container",
                    "declared_length": declared_length,
                    "children": parse_nested_sequence(
                        content,
                        references,
                        page_ordinals,
                        document_seq,
                        depth + 1,
                    ),
                })
            }
            _ => {
                terminal = Some(format!("unsupported_wire:0x{block_type:02X}"));
                break;
            }
        };

        fields.push(json!({
            "field_id": field_id,
            "block_type": format!("0x{block_type:02X}"),
            "body": body,
        }));
    }

    json!({
        "fully_decoded": terminal.is_none() && pos == bytes.len(),
        "decoded_bytes": pos,
        "remaining_bytes": bytes.len().saturating_sub(pos),
        "terminal": terminal,
        "fields": fields,
    })
}

fn top_level_field_receipt(
    field: &pub_contents::RawContentsBlock,
    contents: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_ordinals: &BTreeMap<u32, usize>,
    document_seq: Option<u32>,
) -> Result<Value> {
    let body = match &field.body {
        RawContentsBlockBody::Empty => json!({"kind": "empty"}),
        RawContentsBlockBody::U16 { .. } => json!({"kind": "u16", "value_retained": false}),
        RawContentsBlockBody::U32 { value, .. } => {
            if matches!(
                field.block_type,
                BLOCK_TYPE_REFERENCE_U32 | BLOCK_TYPE_HANDLE_U32
            ) {
                json!({
                    "kind": "u32_relation",
                    "target_class": relation_class(
                        *value,
                        references,
                        page_ordinals,
                        document_seq,
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
            declared_length,
            content_source,
            ..
        } => {
            let start = usize::try_from(content_source.offset)
                .context("container content offset does not fit usize")?;
            let len = usize::try_from(content_source.len)
                .context("container content length does not fit usize")?;
            let end = start.checked_add(len).context("container slice overflow")?;
            let content = contents
                .get(start..end)
                .context("container content source outside /Contents")?;
            json!({
                "kind": "container",
                "declared_length": declared_length,
                "children": parse_nested_sequence(
                    content,
                    references,
                    page_ordinals,
                    document_seq,
                    1,
                ),
            })
        }
    };

    Ok(json!({
        "field_id": field.id,
        "block_type": format!("0x{:02X}", field.block_type),
        "body": body,
    }))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .context("usage: imposition8a-signature-receipt SOURCE.pub OUTPUT.json")?;
    let output = args
        .next()
        .context("usage: imposition8a-signature-receipt SOURCE.pub OUTPUT.json")?;
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }

    let pub_bytes = fs::read(&source).with_context(|| format!("read {:?}", source))?;
    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .context("read /Contents")?;
    let header = parse_0x2c_header(StreamPath(CONTENTS_STREAM_PATH.into()), &contents)
        .context("parse mature 0x2C header")?;
    let trailer =
        parse_confirmed_0x2c_trailer_root(&contents, &header).context("parse mature trailer")?;
    let references = build_reference_index(&contents, &trailer.directory)?;

    let documents = references
        .iter()
        .filter(|(_, reference)| single_raw_type(reference) == Some(RAW_TYPE_DOCUMENT))
        .collect::<Vec<_>>();
    let document_seq = if let [(seq, _)] = documents.as_slice() {
        Some(**seq)
    } else {
        None
    };

    let mut page_ordinals = BTreeMap::new();
    if let [(document_seq_ref, document_reference_ref)] = documents.as_slice() {
        let document_chunk = chunk_for_reference(&contents, document_reference_ref)?;
        let page_list_blocks = document_chunk
            .fields
            .iter()
            .filter(|field| field.id == DOCUMENT_PAGE_LIST_ID)
            .collect::<Vec<_>>();
        if let [page_list_block] = page_list_blocks.as_slice() {
            if let Ok(page_list) =
                parse_confirmed_document_page_list(&contents, (**page_list_block).clone())
            {
                for (document_ordinal, entry) in page_list.entries.iter().enumerate() {
                    if references.get(&entry.handle).and_then(single_raw_type)
                        == Some(RAW_TYPE_PAGE)
                    {
                        page_ordinals.insert(entry.handle, document_ordinal);
                    }
                }
            }
        }
        let _ = document_seq_ref;
    }

    let mut objects = Vec::new();
    for (seq_num, reference) in references
        .iter()
        .filter(|(_, reference)| single_raw_type(reference) == Some(RAW_TYPE_IMPOSITION_ENGINE))
    {
        let chunk = chunk_for_reference(&contents, reference)?;
        let fields = chunk
            .fields
            .iter()
            .map(|field| {
                top_level_field_receipt(field, &contents, &references, &page_ordinals, document_seq)
            })
            .collect::<Result<Vec<_>>>()?;
        objects.push(json!({
            "object_ordinal": objects.len(),
            "source_seq_num_retained": false,
            "declared_length": chunk.declared_length,
            "fully_decoded": chunk.is_fully_decoded(),
            "unsupported_tail_present": chunk.unsupported_tail.is_some(),
            "unsupported_tail_length": chunk.unsupported_tail.as_ref().map(|span| span.len),
            "fields": fields,
        }));
        let _ = seq_num;
    }

    let receipt = json!({
        "schema": SCHEMA,
        "source_bytes": pub_bytes.len(),
        "confirmed_page_count": page_ordinals.len(),
        "imposition_object_count": objects.len(),
        "imposition_objects": objects,
        "claims": {
            "measurement_only": true,
            "raw_scalar_values_retained": false,
            "raw_fixed_bytes_retained": false,
            "raw_seq_nums_retained": false,
            "story_text_retained": false,
            "reference_values_reduced_to_target_classes": true,
            "source_graph_mutated": false,
            "viewer_semantics_changed": false,
            "raster_or_pdf_used": false,
        },
    });

    let file = File::create(Path::new(&output)).with_context(|| format!("create {:?}", output))?;
    serde_json::to_writer_pretty(BufWriter::new(file), &receipt)?;
    Ok(())
}
