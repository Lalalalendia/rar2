use anyhow::{Context, Result, bail};
use pub_contents::{
    BLOCK_TYPE_CONTAINER_88, BLOCK_TYPE_CONTAINER_A0, BLOCK_TYPE_U16, BLOCK_TYPE_U32,
    Contents0x2cChunkReference, ContentsCursor, RawContentsBlock, RawContentsBlockBody,
    decode_packed_field_tag, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_block, parse_confirmed_chunk_reference,
};
use pub_core::{RawSpan, StreamPath};
use serde::{Deserialize, Serialize};

const CONTENTS_STREAM_PATH: &str = "/Contents";
const RAW_TYPE_SHAPE: u16 = 0x01;
const RAW_TYPE_FANCY_BORDERS: u16 = 0x46;
const OPLPO_FBID_FIELD: u16 = 0x09;
const OPLPLBFB_IFBMAX_FIELD: u16 = 0x01;
const OPLPLBFB_RGFB_FIELD: u16 = 0x02;
const OPLFB_SZ_FBRD_NAME_FIELD: u16 = 0x03;
const BLOCK_TYPE_UTF16_Z: u8 = 0xC0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtCatalogEntryV1 {
    pub ordinal: u32,
    pub name: String,
    pub source: RawSpan,
    pub name_source: RawSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtCatalogV1 {
    pub ifbmax: u32,
    pub source: RawSpan,
    pub entries: Vec<PubBorderArtCatalogEntryV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtShapeUseV1 {
    pub contents_seq_num: u32,
    pub fbid: u16,
    pub source: RawSpan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtCatalogDiagnosticV1 {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents_seq_num: Option<u32>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtCatalogReadV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<PubBorderArtCatalogV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape_uses: Vec<PubBorderArtShapeUseV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PubBorderArtCatalogDiagnosticV1>,
}

#[derive(Debug)]
struct CatalogCandidate {
    seq_num: u32,
    catalog: PubBorderArtCatalogV1,
}

fn exact_raw_type(reference: &Contents0x2cChunkReference) -> Option<u16> {
    match reference.raw_types.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn seq_u32(value: usize) -> Result<u32> {
    u32::try_from(value).context("Contents seqNum exceeds u32")
}

fn decode_utf16_z_field(
    contents: &[u8],
    stream: StreamPath,
    offset: usize,
    limit: usize,
) -> Result<(String, usize, RawSpan)> {
    let tag_end = offset
        .checked_add(2)
        .filter(|end| *end <= limit)
        .context("BorderArt UTF-16 field tag crosses parent boundary")?;
    let (field_id, wire_type) = decode_packed_field_tag([contents[offset], contents[offset + 1]]);
    if field_id != OPLFB_SZ_FBRD_NAME_FIELD || wire_type != BLOCK_TYPE_UTF16_Z {
        bail!(
            "expected BorderArt catalog name coordinate at 0x{offset:X}, got field0x{field_id:03X}/wire0x{wire_type:02X}"
        );
    }

    let length_end = tag_end
        .checked_add(4)
        .filter(|end| *end <= limit)
        .context("BorderArt UTF-16 field length crosses parent boundary")?;
    let declared_length = u32::from_le_bytes(
        contents[tag_end..length_end]
            .try_into()
            .expect("four-byte BorderArt string length"),
    );
    if declared_length < 4 {
        bail!("invalid BorderArt UTF-16 declared length {declared_length}");
    }

    let payload_len =
        usize::try_from(declared_length - 4).context("BorderArt UTF-16 payload length")?;
    if payload_len == 0 || payload_len % 2 != 0 {
        bail!("invalid BorderArt UTF-16 payload length {payload_len}");
    }
    let payload_end = length_end
        .checked_add(payload_len)
        .filter(|end| *end <= limit)
        .context("BorderArt UTF-16 payload crosses parent boundary")?;

    let mut units = contents[length_end..payload_end]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    if units.last() != Some(&0) {
        bail!("BorderArt catalog name is not NUL-terminated UTF-16");
    }
    units.pop();
    if units.is_empty() || units.contains(&0) {
        bail!("BorderArt catalog name is empty or contains embedded NUL");
    }

    let name = String::from_utf16(&units).context("decode BorderArt catalog name UTF-16")?;
    let source = RawSpan {
        stream,
        offset: length_end as u64,
        len: payload_len as u64,
    };
    Ok((name, payload_end, source))
}

fn parse_oplfb_name(
    contents: &[u8],
    child: &RawContentsBlock,
    ordinal: u32,
) -> Result<PubBorderArtCatalogEntryV1> {
    let RawContentsBlockBody::Container { content_source, .. } = &child.body else {
        bail!("BorderArt catalog child is not a bounded container");
    };
    if child.block_type != BLOCK_TYPE_CONTAINER_88 {
        bail!(
            "BorderArt catalog child has wire0x{:02X}, expected 0x{BLOCK_TYPE_CONTAINER_88:02X}",
            child.block_type
        );
    }

    let start = usize::try_from(content_source.offset).context("BorderArt child start")?;
    let len = usize::try_from(content_source.len).context("BorderArt child length")?;
    let limit = start
        .checked_add(len)
        .context("BorderArt child boundary overflow")?;
    let mut cursor = ContentsCursor::bounded(content_source.stream.clone(), contents, start, len)?;

    while cursor.remaining() >= 2 {
        let position = cursor.position();
        let (field_id, wire_type) =
            decode_packed_field_tag([contents[position], contents[position + 1]]);
        if field_id == OPLFB_SZ_FBRD_NAME_FIELD && wire_type == BLOCK_TYPE_UTF16_Z {
            let (name, end, name_source) =
                decode_utf16_z_field(contents, content_source.stream.clone(), position, limit)?;
            if end > limit {
                bail!("BorderArt catalog name crosses OplFb boundary");
            }
            return Ok(PubBorderArtCatalogEntryV1 {
                ordinal,
                name,
                source: child.source.clone(),
                name_source,
            });
        }

        parse_confirmed_block(&mut cursor).with_context(|| {
            format!(
                "parse bounded OplFb field0x{field_id:03X}/wire0x{wire_type:02X} before catalog name"
            )
        })?;
    }

    bail!("BorderArt OplFb child has no bounded SzFBrdName")
}

fn parse_catalog_names(
    contents: &[u8],
    rgfb: &RawContentsBlock,
) -> Result<Vec<PubBorderArtCatalogEntryV1>> {
    if rgfb.id != OPLPLBFB_RGFB_FIELD || rgfb.block_type != BLOCK_TYPE_CONTAINER_A0 {
        bail!(
            "expected FancyBorders Rgfb field0x{OPLPLBFB_RGFB_FIELD:02X}/wire0x{BLOCK_TYPE_CONTAINER_A0:02X}"
        );
    }
    let RawContentsBlockBody::Container { content_source, .. } = &rgfb.body else {
        bail!("FancyBorders Rgfb is not a bounded container");
    };

    let start = usize::try_from(content_source.offset).context("FancyBorders Rgfb start")?;
    let len = usize::try_from(content_source.len).context("FancyBorders Rgfb length")?;
    let mut cursor = ContentsCursor::bounded(content_source.stream.clone(), contents, start, len)?;
    let mut entries = Vec::new();

    while cursor.remaining() > 0 {
        let child = parse_confirmed_block(&mut cursor).context("parse bounded OplFb child")?;
        let ordinal = u32::try_from(entries.len()).context("BorderArt catalog ordinal overflow")?;
        entries.push(parse_oplfb_name(contents, &child, ordinal)?);
    }

    Ok(entries)
}

fn parse_catalog_candidate(
    contents: &[u8],
    stream: StreamPath,
    reference: &Contents0x2cChunkReference,
) -> Result<PubBorderArtCatalogV1> {
    let [offset] = reference.chunk_offsets.as_slice() else {
        bail!("FancyBorders reference does not expose one exact chunk offset");
    };
    let chunk = parse_confirmed_0x2c_chunk(stream, contents, offset.value)
        .context("parse FancyBorders 0x46 chunk")?;
    if chunk.unsupported_tail.is_some() {
        bail!("FancyBorders 0x46 chunk has an unsupported tail");
    }

    let ifbmax_fields = chunk
        .fields
        .iter()
        .filter(|field| field.id == OPLPLBFB_IFBMAX_FIELD)
        .collect::<Vec<_>>();
    if ifbmax_fields.len() != 1 {
        bail!(
            "FancyBorders 0x46 exposes {} IfbMax candidates, expected exactly one",
            ifbmax_fields.len()
        );
    }
    let ifbmax_field = ifbmax_fields[0];
    if ifbmax_field.block_type != BLOCK_TYPE_U32 {
        bail!(
            "FancyBorders IfbMax uses wire0x{:02X}, expected 0x{BLOCK_TYPE_U32:02X}",
            ifbmax_field.block_type
        );
    }
    let RawContentsBlockBody::U32 { value: ifbmax, .. } = &ifbmax_field.body else {
        bail!("FancyBorders IfbMax body is not U32");
    };

    let rgfb_fields = chunk
        .fields
        .iter()
        .filter(|field| field.id == OPLPLBFB_RGFB_FIELD)
        .collect::<Vec<_>>();
    if rgfb_fields.len() != 1 {
        bail!(
            "FancyBorders 0x46 exposes {} Rgfb candidates, expected exactly one",
            rgfb_fields.len()
        );
    }
    let entries = parse_catalog_names(contents, rgfb_fields[0])?;
    if entries.len() != *ifbmax as usize {
        bail!(
            "FancyBorders catalog cardinality mismatch: IfbMax={}, decoded entries={}",
            ifbmax,
            entries.len()
        );
    }

    Ok(PubBorderArtCatalogV1 {
        ifbmax: *ifbmax,
        source: chunk.source,
        entries,
    })
}

fn parse_shape_fbid(
    chunk: &pub_contents::Contents0x2cChunk,
    seq_num: u32,
    diagnostics: &mut Vec<PubBorderArtCatalogDiagnosticV1>,
) -> Option<PubBorderArtShapeUseV1> {
    let fields = chunk
        .fields
        .iter()
        .filter(|field| field.id == OPLPO_FBID_FIELD)
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return None;
    }
    if fields.len() != 1 {
        diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
            code: "borderart_shape_fbid_ambiguous".into(),
            contents_seq_num: Some(seq_num),
            detail: format!("shape exposes {} Fbid candidates", fields.len()),
        });
        return None;
    }

    let field = fields[0];
    if field.block_type != BLOCK_TYPE_U16 {
        diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
            code: "borderart_shape_fbid_wrong_wire".into(),
            contents_seq_num: Some(seq_num),
            detail: format!(
                "shape Fbid uses wire0x{:02X}, expected 0x{BLOCK_TYPE_U16:02X}",
                field.block_type
            ),
        });
        return None;
    }
    let RawContentsBlockBody::U16 {
        value,
        value_source,
    } = &field.body
    else {
        diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
            code: "borderart_shape_fbid_wrong_body".into(),
            contents_seq_num: Some(seq_num),
            detail: "shape Fbid body is not U16".into(),
        });
        return None;
    };

    Some(PubBorderArtShapeUseV1 {
        contents_seq_num: seq_num,
        fbid: *value,
        source: value_source.clone(),
        catalog_name: None,
    })
}

pub fn read_mature_0x2c_borderart_catalog_v1(contents: &[u8]) -> Result<PubBorderArtCatalogReadV1> {
    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header =
        parse_0x2c_header(stream.clone(), contents).context("parse mature 0x2C Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature 0x2C Contents trailer")?;

    let mut diagnostics = Vec::new();
    let mut shape_uses = Vec::new();
    let mut catalog_candidates = Vec::<CatalogCandidate>::new();

    for seq_num in 0..trailer.directory.slots.len() {
        let reference = match parse_confirmed_chunk_reference(contents, &trailer.directory, seq_num)
            .with_context(|| format!("parse Contents reference seq {seq_num}"))?
        {
            Some(reference) => reference,
            None => continue,
        };
        let Some(raw_type) = exact_raw_type(&reference) else {
            continue;
        };
        if raw_type != RAW_TYPE_SHAPE && raw_type != RAW_TYPE_FANCY_BORDERS {
            continue;
        }

        let seq_num_u32 = seq_u32(seq_num)?;
        match raw_type {
            RAW_TYPE_SHAPE => {
                let [offset] = reference.chunk_offsets.as_slice() else {
                    diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
                        code: "borderart_shape_offset_ambiguous".into(),
                        contents_seq_num: Some(seq_num_u32),
                        detail: "shape reference does not expose one exact chunk offset".into(),
                    });
                    continue;
                };
                match parse_confirmed_0x2c_chunk(stream.clone(), contents, offset.value) {
                    Ok(chunk) => {
                        if let Some(use_) = parse_shape_fbid(&chunk, seq_num_u32, &mut diagnostics)
                        {
                            shape_uses.push(use_);
                        }
                    }
                    Err(error) => diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
                        code: "borderart_shape_chunk_unavailable".into(),
                        contents_seq_num: Some(seq_num_u32),
                        detail: error.to_string(),
                    }),
                }
            }
            RAW_TYPE_FANCY_BORDERS => {
                match parse_catalog_candidate(contents, stream.clone(), &reference) {
                    Ok(catalog) => catalog_candidates.push(CatalogCandidate {
                        seq_num: seq_num_u32,
                        catalog,
                    }),
                    Err(error) => diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
                        code: "borderart_catalog_unavailable".into(),
                        contents_seq_num: Some(seq_num_u32),
                        detail: error.to_string(),
                    }),
                }
            }
            _ => unreachable!(),
        }
    }

    let catalog = match catalog_candidates.len() {
        0 => None,
        1 => Some(catalog_candidates.remove(0).catalog),
        count => {
            diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
                code: "borderart_catalog_ambiguous".into(),
                contents_seq_num: None,
                detail: format!(
                    "document exposes {count} usable FancyBorders catalogs at seqNum {:?}",
                    catalog_candidates
                        .iter()
                        .map(|candidate| candidate.seq_num)
                        .collect::<Vec<_>>()
                ),
            });
            None
        }
    };

    if let Some(catalog) = catalog.as_ref() {
        for shape_use in &mut shape_uses {
            if let Some(entry) = catalog.entries.get(usize::from(shape_use.fbid)) {
                shape_use.catalog_name = Some(entry.name.clone());
            } else {
                diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
                    code: "borderart_shape_fbid_out_of_range".into(),
                    contents_seq_num: Some(shape_use.contents_seq_num),
                    detail: format!(
                        "shape Fbid={} is outside catalog cardinality {}",
                        shape_use.fbid,
                        catalog.entries.len()
                    ),
                });
            }
        }
    } else if !shape_uses.is_empty() {
        diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
            code: "borderart_shape_usage_without_usable_catalog".into(),
            contents_seq_num: None,
            detail: format!(
                "{} shape Fbid observations cannot be resolved without one usable catalog",
                shape_uses.len()
            ),
        });
    }

    Ok(PubBorderArtCatalogReadV1 {
        catalog,
        shape_uses,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_z_field(name: &str) -> Vec<u8> {
        let mut payload = name.encode_utf16().collect::<Vec<_>>();
        payload.push(0);
        let payload_bytes = payload.len() * 2;
        let declared = u32::try_from(payload_bytes + 4).expect("test string length");

        let mut bytes = vec![0x03, 0xC0];
        bytes.extend_from_slice(&declared.to_le_bytes());
        for unit in payload {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn oplfb_child(name: &str, field_id: u8) -> Vec<u8> {
        let content = utf16_z_field(name);
        let declared = u32::try_from(content.len() + 4).expect("test child length");
        let mut bytes = vec![field_id, BLOCK_TYPE_CONTAINER_88];
        bytes.extend_from_slice(&declared.to_le_bytes());
        bytes.extend_from_slice(&content);
        bytes
    }

    fn rgfb_block(names: &[&str]) -> Vec<u8> {
        let mut content = Vec::new();
        for (ordinal, name) in names.iter().enumerate() {
            content.extend_from_slice(&oplfb_child(
                name,
                u8::try_from(ordinal).expect("small test ordinal"),
            ));
        }
        let declared = u32::try_from(content.len() + 4).expect("test Rgfb length");
        let mut bytes = vec![0x02, BLOCK_TYPE_CONTAINER_A0];
        bytes.extend_from_slice(&declared.to_le_bytes());
        bytes.extend_from_slice(&content);
        bytes
    }

    #[test]
    fn bounded_rgfb_parser_preserves_zero_based_catalog_order() {
        let bytes = rgfb_block(&["Gingerbread Man", "Confetti", "Holly"]);
        let mut cursor = ContentsCursor::new(StreamPath(CONTENTS_STREAM_PATH.into()), &bytes);
        let rgfb = parse_confirmed_block(&mut cursor).expect("bounded Rgfb");
        let entries = parse_catalog_names(&bytes, &rgfb).expect("catalog names");

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].ordinal, 0);
        assert_eq!(entries[0].name, "Gingerbread Man");
        assert_eq!(entries[1].ordinal, 1);
        assert_eq!(entries[1].name, "Confetti");
        assert_eq!(entries[2].ordinal, 2);
        assert_eq!(entries[2].name, "Holly");
        assert_eq!(cursor.remaining(), 0);
    }

    #[test]
    fn catalog_name_requires_nul_terminated_utf16() {
        let mut bytes = utf16_z_field("Wide Inline");
        bytes.truncate(bytes.len() - 2);
        let declared = u32::from_le_bytes(bytes[2..6].try_into().expect("length"));
        let new_declared = declared - 2;
        bytes[2..6].copy_from_slice(&new_declared.to_le_bytes());

        let error = decode_utf16_z_field(
            &bytes,
            StreamPath(CONTENTS_STREAM_PATH.into()),
            0,
            bytes.len(),
        )
        .expect_err("missing UTF-16 NUL must fail closed");
        assert!(error.to_string().contains("not NUL-terminated"));
    }

    #[test]
    fn rgfb_parser_rejects_non_88_catalog_child() {
        let mut bytes = rgfb_block(&["Wide Inline"]);
        bytes[6 + 1] = 0x90;

        let mut cursor = ContentsCursor::new(StreamPath(CONTENTS_STREAM_PATH.into()), &bytes);
        let rgfb = parse_confirmed_block(&mut cursor).expect("bounded Rgfb");
        let error = parse_catalog_names(&bytes, &rgfb)
            .expect_err("wrong OplFb child wire must fail closed");
        assert!(
            error.to_string().contains("wire")
                || format!("{error:#}").contains("BorderArt catalog child")
        );
    }
}
