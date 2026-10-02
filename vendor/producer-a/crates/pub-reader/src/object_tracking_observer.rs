use anyhow::{Context, Result, bail};
use pub_contents::{
    ContentsCursor, RawContentsBlock, RawContentsBlockBody, parse_0x2c_header,
    parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root, parse_confirmed_block,
    parse_confirmed_chunk_reference,
};
use pub_core::{RawSpan, StreamPath};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::CONTENTS_STREAM_PATH;

pub const PUB_OBJECT_TRACKING_WRAP_OBSERVER_SCHEMA_V1: &str =
    "pub-object-tracking-wrap-observer/v1";
const RAW_TYPE_OBJECT_TRACKING: u16 = 0x005A;

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubObjectTrackingWrapObserver {
    pub schema: &'static str,
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
            let children = match &field.body {
                RawContentsBlockBody::Container { content_source, .. } => {
                    parse_container_children(contents, content_source)?
                }
                _ => Vec::new(),
            };
            Ok(FieldNode { field, children })
        })
        .collect()
}

fn parse_container_children(contents: &[u8], source: &RawSpan) -> Result<Vec<FieldNode>> {
    let start = usize::try_from(source.offset).context("container offset does not fit usize")?;
    let len = usize::try_from(source.len).context("container length does not fit usize")?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), contents, start, len)
        .context("bound nested Contents container")?;
    let mut fields = Vec::new();
    while cursor.remaining() > 0 {
        let field = parse_confirmed_block(&mut cursor)
            .with_context(|| format!("parse nested Contents field at {}", cursor.position()))?;
        let children = match &field.body {
            RawContentsBlockBody::Container { content_source, .. } => {
                parse_container_children(contents, content_source)?
            }
            _ => Vec::new(),
        };
        fields.push(FieldNode { field, children });
    }
    Ok(fields)
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
