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

        let roots = parse_field_nodes(&contents, &chunk.fields)?;
        collect_matching_entries(
            u32::try_from(seq_num).context("ObjectTracking seq does not fit u32")?,
            target_oh_track,
            &roots,
            &mut observations,
        );
    }

    Ok(PubObjectTrackingWrapObserver {
        schema: PUB_OBJECT_TRACKING_WRAP_OBSERVER_SCHEMA_V1,
        serialization_revision: header.preamble.serialization_revision,
        target_oh_track,
        tracking_object_count,
        observations,
    })
}

fn parse_field_nodes(contents: &[u8], fields: &[RawContentsBlock]) -> Result<Vec<FieldNode>> {
    fields
        .iter()
        .cloned()
        .map(|field| {
            let children = nested_children(contents, &field)?;
            Ok(FieldNode { field, children })
        })
        .collect()
}

fn nested_children(contents: &[u8], field: &RawContentsBlock) -> Result<Vec<FieldNode>> {
    if !is_recursive_observer_wire(field.block_type) {
        return Ok(Vec::new());
    }
    match &field.body {
        RawContentsBlockBody::Container { content_source, .. } => {
            parse_container_children(contents, content_source)
        }
        _ => Ok(Vec::new()),
    }
}

fn is_recursive_observer_wire(block_type: u8) -> bool {
    matches!(
        block_type,
        0x88 | 0x90 | BLOCK_TYPE_TYPED_CONTAINER_98 | 0xA0
    )
}

fn parse_container_children(contents: &[u8], source: &RawSpan) -> Result<Vec<FieldNode>> {
    let start = usize::try_from(source.offset).context("container offset does not fit usize")?;
    let len = usize::try_from(source.len).context("container length does not fit usize")?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), contents, start, len)
        .context("bound nested Contents container")?;
    let mut fields = Vec::new();
    while cursor.remaining() > 0 {
        let offset = cursor.position();
        let field = parse_observer_block(&mut cursor)
            .with_context(|| format!("parse nested Contents field at {offset}"))?;
        let children = nested_children(contents, &field)?;
        fields.push(FieldNode { field, children });
    }
    Ok(fields)
}

/// Research-only fallback for the exact Publisher11 tracking oracle.
///
/// The paired Publisher11 hidden-XML/binary oracle proved that observed
/// variable wire classes >= 0x80 use the same bounded framing:
/// two-byte packed tag + little-endian u32 declared length, where the length
/// includes its own four-byte word. The production foundation intentionally
/// supports a narrower semantic set. Here we need only preserve framing while
/// walking a SHA-pinned rev19 fixture. Only known structural container wires
/// are recursed into; opaque variable payloads such as 0xC0 are skipped.
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

fn collect_matching_entries(
    tracking_seq_num: u32,
    target_oh_track: u32,
    nodes: &[FieldNode],
    output: &mut Vec<PubTrackingWrapObservation>,
) {
    for node in nodes {
        if direct_scalar(&node.children, 0x01).is_some_and(|value| value.value == target_oh_track) {
            let last_fmt = unique_child(&node.children, 0x12);
            let formatting = last_fmt.and_then(|value| unique_child(&value.children, 0x02));
            let group_shape = formatting.and_then(|value| unique_child(&value.children, 0x0E));
            let ecp_recolor = formatting.and_then(|value| unique_child(&value.children, 0x22));

            output.push(PubTrackingWrapObservation {
                tracking_seq_num,
                target_oh_track,
                entry_source: node.field.source.clone(),
                resolved_shape_type: formatting
                    .and_then(|value| direct_scalar(&value.children, 0x01)),
                dx_wrap_dist_left: group_shape
                    .and_then(|value| direct_scalar(&value.children, 0x05)),
                dy_wrap_dist_top: group_shape
                    .and_then(|value| direct_scalar(&value.children, 0x06)),
                dx_wrap_dist_right: group_shape
                    .and_then(|value| direct_scalar(&value.children, 0x07)),
                dy_wrap_dist_bottom: group_shape
                    .and_then(|value| direct_scalar(&value.children, 0x08)),
                ecp_recolor_scalars: ecp_recolor
                    .map(|value| direct_scalars(&value.children))
                    .unwrap_or_default(),
            });
        }
        collect_matching_entries(tracking_seq_num, target_oh_track, &node.children, output);
    }
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
    use pub_contents::{BLOCK_TYPE_CONTAINER_88, BLOCK_TYPE_REFERENCE_U32, BLOCK_TYPE_U32};

    fn scalar_block(id: u16, block_type: u8, value: u32, offset: u64) -> RawContentsBlock {
        let source = RawSpan {
            stream: StreamPath("/Contents".into()),
            offset,
            len: 6,
        };
        RawContentsBlock {
            id,
            block_type,
            raw_tag: [id as u8, block_type],
            tag_source: RawSpan {
                stream: source.stream.clone(),
                offset,
                len: 2,
            },
            source: source.clone(),
            body: RawContentsBlockBody::U32 {
                value,
                value_source: RawSpan {
                    stream: source.stream.clone(),
                    offset: offset + 2,
                    len: 4,
                },
            },
        }
    }

    fn container_node(id: u16, children: Vec<FieldNode>, offset: u64) -> FieldNode {
        FieldNode {
            field: RawContentsBlock {
                id,
                block_type: BLOCK_TYPE_CONTAINER_88,
                raw_tag: [id as u8, BLOCK_TYPE_CONTAINER_88],
                tag_source: RawSpan {
                    stream: StreamPath("/Contents".into()),
                    offset,
                    len: 2,
                },
                source: RawSpan {
                    stream: StreamPath("/Contents".into()),
                    offset,
                    len: 6,
                },
                body: RawContentsBlockBody::Container {
                    declared_length: 4,
                    length_source: RawSpan {
                        stream: StreamPath("/Contents".into()),
                        offset: offset + 2,
                        len: 4,
                    },
                    content_source: RawSpan {
                        stream: StreamPath("/Contents".into()),
                        offset: offset + 6,
                        len: 0,
                    },
                },
            },
            children,
        }
    }

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

        let fields =
            parse_container_children(&bytes, &source).expect("observer framing must parse");

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
    fn observer_recurses_into_typed_98_payload() {
        let bytes = [
            0x22,
            BLOCK_TYPE_TYPED_CONTAINER_98,
            0x0A,
            0x00,
            0x00,
            0x00,
            0x01,
            BLOCK_TYPE_U32,
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

        let fields = parse_container_children(&bytes, &source).expect("typed payload must parse");

        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field.block_type, BLOCK_TYPE_TYPED_CONTAINER_98);
        assert_eq!(
            direct_scalar(&fields[0].children, 0x01).map(|value| value.value),
            Some(0x08000003)
        );
    }

    #[test]
    fn extracts_exact_named_wrap_path_from_matching_oh_track_entry() {
        let group = container_node(
            0x0E,
            vec![
                FieldNode {
                    field: scalar_block(0x05, BLOCK_TYPE_U32, 111, 40),
                    children: Vec::new(),
                },
                FieldNode {
                    field: scalar_block(0x06, BLOCK_TYPE_U32, 222, 46),
                    children: Vec::new(),
                },
            ],
            34,
        );
        let formatting = container_node(
            0x02,
            vec![
                FieldNode {
                    field: scalar_block(0x01, BLOCK_TYPE_U32, 1, 28),
                    children: Vec::new(),
                },
                group,
            ],
            22,
        );
        let last_fmt = container_node(0x12, vec![formatting], 16);
        let entry = container_node(
            0x02,
            vec![
                FieldNode {
                    field: scalar_block(0x01, BLOCK_TYPE_REFERENCE_U32, 294, 10),
                    children: Vec::new(),
                },
                last_fmt,
            ],
            4,
        );

        let mut observations = Vec::new();
        collect_matching_entries(290, 294, &[entry], &mut observations);
        assert_eq!(observations.len(), 1);
        assert_eq!(
            observations[0]
                .dx_wrap_dist_left
                .as_ref()
                .map(|value| value.value),
            Some(111)
        );
        assert_eq!(
            observations[0]
                .dy_wrap_dist_top
                .as_ref()
                .map(|value| value.value),
            Some(222)
        );
        assert_eq!(
            observations[0]
                .resolved_shape_type
                .as_ref()
                .map(|value| value.value),
            Some(1)
        );
    }
}
