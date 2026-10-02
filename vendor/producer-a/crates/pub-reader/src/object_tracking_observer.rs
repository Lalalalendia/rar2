use anyhow::{Context, Result, bail};
use pub_contents::{
    BlockReadError, ContentsCursor, RawContentsBlock, RawContentsBlockBody,
    decode_packed_field_tag, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_block, parse_confirmed_chunk_reference,
};
use pub_core::{RawSpan, StreamPath};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::CONTENTS_STREAM_PATH;

pub const PUB_OBJECT_TRACKING_WRAP_OBSERVER_SCHEMA_V1: &str =
    "pub-object-tracking-wrap-observer/v1";
const RAW_TYPE_OBJECT_TRACKING: u16 = 0x005A;
const BLOCK_TYPE_CONTAINER_88: u8 = 0x88;
const BLOCK_TYPE_TYPED_CONTAINER_98: u8 = 0x98;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTrackingScalar {
    pub value: u32,
    pub source: RawSpan,
    pub wire_type: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTrackingWrapObservation {
    pub tracking_seq_num: u32,
    pub target_oh_track: u32,
    pub entry_source: RawSpan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_shape_type: Option<PubTrackingScalar>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dx_wrap_dist_left: Option<PubTrackingScalar>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dy_wrap_dist_top: Option<PubTrackingScalar>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dx_wrap_dist_right: Option<PubTrackingScalar>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dy_wrap_dist_bottom: Option<PubTrackingScalar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ecp_recolor_scalars: Vec<PubTrackingScalar>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubObjectTrackingWrapObserver {
    pub schema: &'static str,
    pub serialization_revision: u16,
    pub target_oh_track: u32,
    pub tracking_object_count: usize,
    pub observations: Vec<PubTrackingWrapObservation>,
}

#[derive(Debug, Clone)]
struct FieldNode {
    field: RawContentsBlock,
    children: Vec<FieldNode>,
}

pub fn observe_object_tracking_wrap_state(
    pub_bytes: &[u8],
    target_oh_track: u32,
) -> Result<PubObjectTrackingWrapObserver> {
    let contents = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), CONTENTS_STREAM_PATH)
        .with_context(|| format!("read {CONTENTS_STREAM_PATH}"))?;
    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(stream.clone(), &contents)
        .context("parse mature-0x2C Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse mature-0x2C Contents trailer")?;

    let mut tracking_object_count = 0_usize;
    let mut observations = Vec::new();

    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(&contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse Contents reference seq {seq_num}"))?
        else {
            continue;
        };
        let raw_types = reference
            .raw_types
            .iter()
            .map(|value| value.value)
            .collect::<Vec<_>>();
        if raw_types.as_slice() != [RAW_TYPE_OBJECT_TRACKING] {
            continue;
        }
        let offsets = reference
            .chunk_offsets
            .iter()
            .map(|value| value.value)
            .collect::<Vec<_>>();
        let [offset] = offsets.as_slice() else {
            bail!(
                "ObjectTracking seq {seq_num} must have exactly one chunk offset; got {}",
                offsets.len()
            );
        };
        tracking_object_count += 1;
        let chunk = parse_confirmed_0x2c_chunk(stream.clone(), &contents, *offset)
            .with_context(|| format!("parse ObjectTracking chunk seq {seq_num}"))?;

        if let Some(entry) =
            find_target_entry_by_oh_track_marker(&contents, &chunk.source, target_oh_track)?
        {
            let observation = observe_target_entry(
                &contents,
                u32::try_from(seq_num).context("ObjectTracking seq does not fit u32")?,
                target_oh_track,
                &entry,
            )?;
            observations.push(observation);
        }
    }

    Ok(PubObjectTrackingWrapObserver {
        schema: PUB_OBJECT_TRACKING_WRAP_OBSERVER_SCHEMA_V1,
        serialization_revision: header.preamble.serialization_revision,
        target_oh_track,
        tracking_object_count,
        observations,
    })
}

fn parse_container_prefix(contents: &[u8], source: &RawSpan) -> Result<Vec<FieldNode>> {
    let start = usize::try_from(source.offset).context("container offset does not fit usize")?;
    let len = usize::try_from(source.len).context("container length does not fit usize")?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), contents, start, len)
        .context("bound nested Contents container")?;
    let mut fields = Vec::new();
    while cursor.remaining() > 0 {
        match parse_observer_block(&mut cursor) {
            Ok(field) => fields.push(FieldNode {
                field,
                children: Vec::new(),
            }),
            Err(_) => {
                // Research-only prefix behavior: once the current payload stops
                // looking like a field sequence, keep the already-framed prefix
                // and leave the remainder opaque. Target admission below requires
                // all named fields it needs to occur inside this framed prefix.
                break;
            }
        }
    }
    Ok(fields)
}

fn find_target_entry_by_oh_track_marker(
    contents: &[u8],
    chunk_source: &RawSpan,
    target_oh_track: u32,
) -> Result<Option<FieldNode>> {
    let chunk_start =
        usize::try_from(chunk_source.offset).context("ObjectTracking chunk offset too large")?;
    let chunk_len =
        usize::try_from(chunk_source.len).context("ObjectTracking chunk length too large")?;
    let chunk_end = chunk_start
        .checked_add(chunk_len)
        .filter(|end| *end <= contents.len())
        .context("ObjectTracking chunk source is out of bounds")?;
    let scan_start = chunk_start.saturating_add(4);
    let target = target_oh_track.to_le_bytes();
    let marker = [0x01_u8, 0x68_u8, target[0], target[1], target[2], target[3]];

    let mut hits = Vec::new();
    if scan_start + marker.len() <= chunk_end {
        for offset in scan_start..=chunk_end - marker.len() {
            if contents[offset..offset + marker.len()] == marker {
                hits.push(offset);
            }
        }
    }

    let mut matches = Vec::new();
    for hit in hits {
        for start in scan_start..=hit {
            if start + 6 > chunk_end {
                break;
            }
            let raw_tag = [contents[start], contents[start + 1]];
            let (_, block_type) = decode_packed_field_tag(raw_tag);
            if block_type < 0x80 {
                continue;
            }
            let declared_length = u32::from_le_bytes([
                contents[start + 2],
                contents[start + 3],
                contents[start + 4],
                contents[start + 5],
            ]);
            if declared_length < 4 {
                continue;
            }
            let Ok(declared_usize) = usize::try_from(declared_length) else {
                continue;
            };
            let Some(end) = start
                .checked_add(2)
                .and_then(|value| value.checked_add(declared_usize))
            else {
                continue;
            };
            if end > chunk_end || start + 6 > hit || hit + marker.len() > end {
                continue;
            }

            let content_source = RawSpan {
                stream: chunk_source.stream.clone(),
                offset: (start + 6) as u64,
                len: (end - (start + 6)) as u64,
            };
            let children = parse_container_prefix(contents, &content_source)?;
            let direct_hit = children.iter().any(|node| {
                node.field.tag_source.offset == hit as u64
                    && node.field.id == 0x01
                    && scalar(&node.field).is_some_and(|value| value.value == target_oh_track)
            });
            if !direct_hit {
                continue;
            }

            let tag_source = RawSpan {
                stream: chunk_source.stream.clone(),
                offset: start as u64,
                len: 2,
            };
            let length_source = RawSpan {
                stream: chunk_source.stream.clone(),
                offset: (start + 2) as u64,
                len: 4,
            };
            let source = RawSpan {
                stream: chunk_source.stream.clone(),
                offset: start as u64,
                len: (end - start) as u64,
            };
            matches.push(FieldNode {
                field: RawContentsBlock {
                    id: decode_packed_field_tag(raw_tag).0,
                    block_type,
                    raw_tag,
                    tag_source,
                    source,
                    body: RawContentsBlockBody::Container {
                        declared_length,
                        length_source,
                        content_source,
                    },
                },
                children,
            });
        }
    }

    matches.sort_by_key(|node| node.field.source.len);
    matches.dedup_by_key(|node| node.field.source.offset);

    match matches.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(only.clone())),
        many => {
            let min_len = many[0].field.source.len;
            let smallest = many
                .iter()
                .filter(|node| node.field.source.len == min_len)
                .collect::<Vec<_>>();
            if smallest.len() == 1 {
                Ok(Some((*smallest[0]).clone()))
            } else {
                bail!(
                    "OhTrack={target_oh_track} marker has {} equally-small direct parent candidates",
                    smallest.len()
                )
            }
        }
    }
}

/// Research-only fallback for the exact Publisher11 tracking oracle.
///
/// The paired Publisher11 hidden-XML/binary oracle proved that observed
/// variable wire classes >= 0x80 use the same bounded framing:
/// two-byte packed tag + little-endian u32 declared length, where the length
/// includes its own four-byte word. This fallback only establishes framing;
/// semantic payload opening remains owner/member path specific.
fn parse_observer_block(cursor: &mut ContentsCursor<'_>) -> Result<RawContentsBlock> {
    match parse_confirmed_block(cursor) {
        Ok(block) => Ok(block),
        Err(BlockReadError::UnsupportedType { block_type, .. }) if block_type >= 0x80 => {
            parse_bounded_variable_block(cursor, block_type)
        }
        Err(error) => Err(error.into()),
    }
}

fn parse_bounded_variable_block(
    cursor: &mut ContentsCursor<'_>,
    expected_block_type: u8,
) -> Result<RawContentsBlock> {
    let start = cursor.position();
    let (tag0, tag0_source) = cursor.read_u8()?;
    let (tag1, _) = cursor.read_u8()?;
    let raw_tag = [tag0, tag1];
    let (id, block_type) = decode_packed_field_tag(raw_tag);
    if block_type != expected_block_type {
        bail!(
            "variable wire changed while reparsing at {start}: expected 0x{expected_block_type:02X}, got 0x{block_type:02X}"
        );
    }

    let tag_source = RawSpan {
        stream: tag0_source.stream.clone(),
        offset: tag0_source.offset,
        len: 2,
    };
    let (declared_length, length_source) = cursor.read_u32_le()?;
    if declared_length < 4 {
        bail!(
            "invalid variable Contents length at {start}: wire=0x{block_type:02X}, declared={declared_length}"
        );
    }
    let content_len = usize::try_from(declared_length - 4)
        .context("variable Contents length does not fit usize")?;
    let (_, content_source) = cursor.take(content_len)?;
    let end = cursor.position();

    Ok(RawContentsBlock {
        id,
        block_type,
        raw_tag,
        tag_source,
        source: RawSpan {
            stream: tag0_source.stream,
            offset: tag0_source.offset,
            len: (end - start) as u64,
        },
        body: RawContentsBlockBody::Container {
            declared_length,
            length_source,
            content_source,
        },
    })
}

fn explicit_container_children(
    contents: &[u8],
    node: &FieldNode,
    semantic_path: &str,
) -> Result<Vec<FieldNode>> {
    match &node.field.body {
        RawContentsBlockBody::Container { content_source, .. } => {
            let children = parse_container_prefix(contents, content_source)?;
            if children.is_empty() && content_source.len != 0 {
                bail!("{semantic_path} did not expose a decodable field prefix");
            }
            Ok(children)
        }
        _ => bail!("{semantic_path} is not a bounded variable payload"),
    }
}

fn explicit_container_88_children(
    contents: &[u8],
    node: &FieldNode,
    semantic_path: &str,
) -> Result<Vec<FieldNode>> {
    if node.field.block_type != BLOCK_TYPE_CONTAINER_88 {
        bail!(
            "{semantic_path} must use Publisher11 nested wire 0x88; got 0x{:02X}",
            node.field.block_type
        );
    }
    explicit_container_children(contents, node, semantic_path)
}

fn explicit_typed_98_children(
    contents: &[u8],
    node: &FieldNode,
    semantic_path: &str,
) -> Result<Vec<FieldNode>> {
    if node.field.block_type != BLOCK_TYPE_TYPED_CONTAINER_98 {
        bail!(
            "{semantic_path} must use Publisher11 typed wire 0x98; got 0x{:02X}",
            node.field.block_type
        );
    }
    explicit_container_children(contents, node, semantic_path)
}

fn observe_target_entry(
    contents: &[u8],
    tracking_seq_num: u32,
    target_oh_track: u32,
    entry: &FieldNode,
) -> Result<PubTrackingWrapObservation> {
    if !direct_scalar(&entry.children, 0x01).is_some_and(|value| value.value == target_oh_track) {
        bail!("selected ObjectTracking entry does not carry OhTrack={target_oh_track}");
    }

    let last_fmt = unique_child(&entry.children, 0x12)
        .context("target OplOt lacks unique OplLastFmt field0x12")?;
    let last_fmt_children = explicit_container_88_children(contents, last_fmt, "OplOt.OplLastFmt")?;
    let formatting = unique_child(&last_fmt_children, 0x02)
        .context("target OplLastFmt lacks unique PoFormatting field0x02")?;
    let formatting_children =
        explicit_container_88_children(contents, formatting, "OplLastFmt.PoFormatting")?;

    let group_shape = unique_child(&formatting_children, 0x0E)
        .context("target PoFormatting lacks unique GroupShape field0x0E")?;
    let ecp_recolor = unique_child(&formatting_children, 0x22)
        .context("target PoFormatting lacks unique EcpRecolor field0x22")?;

    let group_shape_children =
        explicit_typed_98_children(contents, group_shape, "OplOdpo.GroupShape")?;
    let ecp_recolor_children =
        explicit_typed_98_children(contents, ecp_recolor, "OplOdpo.EcpRecolor")?;

    Ok(PubTrackingWrapObservation {
        tracking_seq_num,
        target_oh_track,
        entry_source: entry.field.source.clone(),
        resolved_shape_type: direct_scalar(&formatting_children, 0x01),
        dx_wrap_dist_left: direct_scalar(&group_shape_children, 0x05),
        dy_wrap_dist_top: direct_scalar(&group_shape_children, 0x06),
        dx_wrap_dist_right: direct_scalar(&group_shape_children, 0x07),
        dy_wrap_dist_bottom: direct_scalar(&group_shape_children, 0x08),
        ecp_recolor_scalars: direct_scalars(&ecp_recolor_children),
    })
}

fn unique_child(nodes: &[FieldNode], id: u16) -> Option<&FieldNode> {
    let mut matches = nodes.iter().filter(|node| node.field.id == id);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn direct_scalar(nodes: &[FieldNode], id: u16) -> Option<PubTrackingScalar> {
    let node = unique_child(nodes, id)?;
    scalar(&node.field)
}

fn direct_scalars(nodes: &[FieldNode]) -> Vec<PubTrackingScalar> {
    nodes
        .iter()
        .filter_map(|node| scalar(&node.field))
        .collect()
}

fn scalar(field: &RawContentsBlock) -> Option<PubTrackingScalar> {
    match &field.body {
        RawContentsBlockBody::U16 {
            value,
            value_source,
        } => Some(PubTrackingScalar {
            value: u32::from(*value),
            source: value_source.clone(),
            wire_type: field.block_type,
        }),
        RawContentsBlockBody::U32 {
            value,
            value_source,
        } => Some(PubTrackingScalar {
            value: *value,
            source: value_source.clone(),
            wire_type: field.block_type,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_contents::{BLOCK_TYPE_REFERENCE_U32, BLOCK_TYPE_U32};

    #[test]
    fn observer_skips_opaque_c0_with_proven_variable_framing() {
        let bytes = [
            0x0E,
            0xC0,
            0x08,
            0x00,
            0x00,
            0x00,
            0x41,
            0x00,
            0x00,
            0x00,
            0x01,
            BLOCK_TYPE_U32,
            0x7B,
            0x00,
            0x00,
            0x00,
        ];
        let source = RawSpan {
            stream: StreamPath("/Contents".into()),
            offset: 0,
            len: bytes.len() as u64,
        };

        let fields = parse_container_prefix(&bytes, &source).expect("observer framing must parse");

        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].field.block_type, 0xC0);
        assert!(fields[0].children.is_empty());
        assert_eq!(fields[1].field.id, 0x01);
        assert_eq!(
            direct_scalar(&fields, 0x01).map(|value| value.value),
            Some(123)
        );
    }

    #[test]
    fn observer_opens_typed_98_only_on_explicit_schema_path() {
        let bytes = [
            0x22,
            BLOCK_TYPE_TYPED_CONTAINER_98,
            0x0A,
            0x00,
            0x00,
            0x00,
            0x01,
            0x22,
            0x03,
            0x00,
            0x00,
            0x08,
        ];
        let source = RawSpan {
            stream: StreamPath("/Contents".into()),
            offset: 0,
            len: bytes.len() as u64,
        };

        let fields = parse_container_prefix(&bytes, &source).expect("typed payload must frame");

        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field.block_type, BLOCK_TYPE_TYPED_CONTAINER_98);
        assert!(fields[0].children.is_empty());

        let typed = explicit_typed_98_children(&bytes, &fields[0], "test.OplEcp")
            .expect("explicit typed payload path must parse");
        assert_eq!(typed.len(), 1);
        assert_eq!(typed[0].field.id, 0x0201);
        assert_eq!(
            scalar(&typed[0].field).map(|value| value.value),
            Some(0x08000003)
        );
    }

    #[test]
    fn marker_guided_observer_ignores_unrelated_bad_branch_and_extracts_target_path() {
        // A fake unrelated 0x88 payload contains bytes that are not a valid field
        // sequence. The target OplOt entry follows it in the same chunk.
        let payload = [
            0x09,
            0x88,
            0x08,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x02,
            0x88,
            0x46,
            0x00,
            0x00,
            0x00,
            0x01,
            BLOCK_TYPE_REFERENCE_U32,
            0x26,
            0x01,
            0x00,
            0x00,
            0x12,
            0x88,
            0x3A,
            0x00,
            0x00,
            0x00,
            0x02,
            BLOCK_TYPE_CONTAINER_88,
            0x34,
            0x00,
            0x00,
            0x00,
            0x01,
            BLOCK_TYPE_U32,
            0x01,
            0x00,
            0x00,
            0x00,
            0x0E,
            BLOCK_TYPE_TYPED_CONTAINER_98,
            0x1C,
            0x00,
            0x00,
            0x00,
            0x05,
            BLOCK_TYPE_U32,
            0x6F,
            0x00,
            0x00,
            0x00,
            0x06,
            BLOCK_TYPE_U32,
            0xDE,
            0x00,
            0x00,
            0x00,
            0x07,
            BLOCK_TYPE_U32,
            0x4D,
            0x01,
            0x00,
            0x00,
            0x08,
            BLOCK_TYPE_U32,
            0xBC,
            0x01,
            0x00,
            0x00,
            0x22,
            BLOCK_TYPE_TYPED_CONTAINER_98,
            0x0A,
            0x00,
            0x00,
            0x00,
            0x01,
            0x22,
            0x03,
            0x00,
            0x00,
            0x08,
        ];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((payload.len() + 4) as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        let chunk_source = RawSpan {
            stream: StreamPath("/Contents".into()),
            offset: 0,
            len: bytes.len() as u64,
        };

        let entry = find_target_entry_by_oh_track_marker(&bytes, &chunk_source, 294)
            .expect("marker search must succeed")
            .expect("target entry must exist");
        let observation = observe_target_entry(&bytes, 290, 294, &entry)
            .expect("schema-guided target path must parse");

        assert_eq!(
            observation
                .dx_wrap_dist_left
                .as_ref()
                .map(|value| value.value),
            Some(111)
        );
        assert_eq!(
            observation
                .dy_wrap_dist_top
                .as_ref()
                .map(|value| value.value),
            Some(222)
        );
        assert_eq!(
            observation
                .dx_wrap_dist_right
                .as_ref()
                .map(|value| value.value),
            Some(333)
        );
        assert_eq!(
            observation
                .dy_wrap_dist_bottom
                .as_ref()
                .map(|value| value.value),
            Some(444)
        );
        assert_eq!(
            observation
                .resolved_shape_type
                .as_ref()
                .map(|value| value.value),
            Some(1)
        );
        assert_eq!(
            observation
                .ecp_recolor_scalars
                .iter()
                .map(|value| value.value)
                .collect::<Vec<_>>(),
            vec![0x08000003]
        );
    }
}
