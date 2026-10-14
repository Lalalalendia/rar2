use pub_model::{Affine2D, CanonicalId, NodeId, PageId, RectEmu};
use serde::{Deserialize, Serialize};

pub const MAX_MOVE_NODES_V1: usize = 1024;
pub const MAX_RESIZE_NODES_V1: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveNodeBatchEntry {
    pub node_id: NodeId,
    pub before: RectEmu,
    pub after: RectEmu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResizeNodeBatchEntry {
    pub node_id: NodeId,
    pub before: RectEmu,
    pub after: RectEmu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeometryNodeSnapshotV1 {
    pub node_id: NodeId,
    pub parent_id: CanonicalId,
    pub bounds: RectEmu,
    pub transform: Affine2D,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveNodesTransitionErrorV1 {
    Empty,
    TooLarge { found: usize },
    Duplicate { node_id: NodeId },
    SizeChanged { node_id: NodeId },
    NoChange { node_id: NodeId },
    NodeUnsupported { node_id: NodeId },
    PageMismatch { node_id: NodeId, page_id: PageId },
    Stale { node_id: NodeId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResizeNodesTransitionErrorV1 {
    InvalidCount { found: usize },
    Duplicate { node_id: NodeId },
    NotCanonical { node_id: NodeId },
    PageMismatch { node_id: NodeId, page_id: PageId },
    NodeUnsupported { node_id: NodeId },
    NonPositive { node_id: NodeId },
    Overflow { node_id: NodeId },
    Stale { node_id: NodeId },
    NoSizeChange,
}

pub fn validate_move_nodes_transition_v1(
    page_id: PageId,
    entries: &[MoveNodeBatchEntry],
    nodes: &[GeometryNodeSnapshotV1],
    forward: bool,
) -> Result<(), MoveNodesTransitionErrorV1> {
    if entries.is_empty() {
        return Err(MoveNodesTransitionErrorV1::Empty);
    }
    if entries.len() > MAX_MOVE_NODES_V1 {
        return Err(MoveNodesTransitionErrorV1::TooLarge {
            found: entries.len(),
        });
    }

    let page_parent = page_id.into_canonical();
    let mut previous = None;
    for entry in entries {
        if previous.is_some_and(|node_id| node_id >= entry.node_id) {
            return Err(MoveNodesTransitionErrorV1::Duplicate {
                node_id: entry.node_id,
            });
        }
        previous = Some(entry.node_id);

        if entry.before.width != entry.after.width || entry.before.height != entry.after.height {
            return Err(MoveNodesTransitionErrorV1::SizeChanged {
                node_id: entry.node_id,
            });
        }
        if entry.before == entry.after {
            return Err(MoveNodesTransitionErrorV1::NoChange {
                node_id: entry.node_id,
            });
        }

        let node = nodes
            .iter()
            .find(|node| node.node_id == entry.node_id)
            .ok_or(MoveNodesTransitionErrorV1::NodeUnsupported {
                node_id: entry.node_id,
            })?;
        if node.parent_id != page_parent {
            return Err(MoveNodesTransitionErrorV1::PageMismatch {
                node_id: entry.node_id,
                page_id,
            });
        }
        if node.transform != Affine2D::identity()
            || entry.before.width.get() <= 0
            || entry.before.height.get() <= 0
            || entry.before.right().is_none()
            || entry.before.bottom().is_none()
            || entry.after.right().is_none()
            || entry.after.bottom().is_none()
        {
            return Err(MoveNodesTransitionErrorV1::NodeUnsupported {
                node_id: entry.node_id,
            });
        }

        let expected = if forward { entry.before } else { entry.after };
        if node.bounds != expected {
            return Err(MoveNodesTransitionErrorV1::Stale {
                node_id: entry.node_id,
            });
        }
    }
    Ok(())
}

pub fn validate_resize_nodes_transition_v1(
    page_id: PageId,
    entries: &[ResizeNodeBatchEntry],
    nodes: &[GeometryNodeSnapshotV1],
    forward: bool,
) -> Result<(), ResizeNodesTransitionErrorV1> {
    if entries.len() < 2 || entries.len() > MAX_RESIZE_NODES_V1 {
        return Err(ResizeNodesTransitionErrorV1::InvalidCount {
            found: entries.len(),
        });
    }

    let page_parent = page_id.into_canonical();
    let mut previous = None;
    let mut has_size_change = false;
    for entry in entries {
        if let Some(previous_id) = previous {
            if previous_id == entry.node_id {
                return Err(ResizeNodesTransitionErrorV1::Duplicate {
                    node_id: entry.node_id,
                });
            }
            if previous_id > entry.node_id {
                return Err(ResizeNodesTransitionErrorV1::NotCanonical {
                    node_id: entry.node_id,
                });
            }
        }
        previous = Some(entry.node_id);

        let node = nodes
            .iter()
            .find(|node| node.node_id == entry.node_id)
            .ok_or(ResizeNodesTransitionErrorV1::NodeUnsupported {
                node_id: entry.node_id,
            })?;
        if node.parent_id != page_parent {
            return Err(ResizeNodesTransitionErrorV1::PageMismatch {
                node_id: entry.node_id,
                page_id,
            });
        }
        if node.transform != Affine2D::identity()
            || entry.before.width.get() <= 0
            || entry.before.height.get() <= 0
            || entry.before.right().is_none()
            || entry.before.bottom().is_none()
        {
            return Err(ResizeNodesTransitionErrorV1::NodeUnsupported {
                node_id: entry.node_id,
            });
        }
        if entry.after.width.get() <= 0 || entry.after.height.get() <= 0 {
            return Err(ResizeNodesTransitionErrorV1::NonPositive {
                node_id: entry.node_id,
            });
        }
        if entry.after.right().is_none() || entry.after.bottom().is_none() {
            return Err(ResizeNodesTransitionErrorV1::Overflow {
                node_id: entry.node_id,
            });
        }
        if entry.before.width != entry.after.width || entry.before.height != entry.after.height {
            has_size_change = true;
        }

        let expected = if forward { entry.before } else { entry.after };
        if node.bounds != expected {
            return Err(ResizeNodesTransitionErrorV1::Stale {
                node_id: entry.node_id,
            });
        }
    }

    if !has_size_change {
        return Err(ResizeNodesTransitionErrorV1::NoSizeChange);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::{CanonicalId, LengthEmu};

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn node_id(byte: u8) -> NodeId {
        NodeId::from_canonical(canonical(byte))
    }

    fn page_id(byte: u8) -> PageId {
        PageId::from_canonical(canonical(byte))
    }

    fn rect(x: i64, y: i64, width: i64, height: i64) -> RectEmu {
        RectEmu::new(
            LengthEmu::new(x),
            LengthEmu::new(y),
            LengthEmu::new(width),
            LengthEmu::new(height),
        )
    }

    fn snapshot(id: NodeId, page: PageId, bounds: RectEmu) -> GeometryNodeSnapshotV1 {
        GeometryNodeSnapshotV1 {
            node_id: id,
            parent_id: page.into_canonical(),
            bounds,
            transform: Affine2D::identity(),
        }
    }

    #[test]
    fn move_transition_accepts_forward_and_inverse_translation() {
        let page = page_id(1);
        let id = node_id(2);
        let before = rect(0, 0, 100, 80);
        let after = rect(10, 20, 100, 80);
        let entry = MoveNodeBatchEntry {
            node_id: id,
            before,
            after,
        };

        assert_eq!(
            validate_move_nodes_transition_v1(
                page,
                std::slice::from_ref(&entry),
                &[snapshot(id, page, before)],
                true,
            ),
            Ok(())
        );
        assert_eq!(
            validate_move_nodes_transition_v1(
                page,
                std::slice::from_ref(&entry),
                &[snapshot(id, page, after)],
                false,
            ),
            Ok(())
        );
    }

    #[test]
    fn move_transition_preserves_canonical_batch_failures() {
        let page = page_id(1);
        let id = node_id(2);
        let before = rect(0, 0, 100, 80);
        let translated = rect(10, 20, 100, 80);
        let resized = rect(10, 20, 120, 80);

        assert_eq!(
            validate_move_nodes_transition_v1(page, &[], &[], true),
            Err(MoveNodesTransitionErrorV1::Empty)
        );
        assert_eq!(
            validate_move_nodes_transition_v1(
                page,
                &[
                    MoveNodeBatchEntry {
                        node_id: id,
                        before,
                        after: translated,
                    },
                    MoveNodeBatchEntry {
                        node_id: id,
                        before,
                        after: translated,
                    },
                ],
                &[snapshot(id, page, before)],
                true,
            ),
            Err(MoveNodesTransitionErrorV1::Duplicate { node_id: id })
        );
        assert_eq!(
            validate_move_nodes_transition_v1(
                page,
                &[MoveNodeBatchEntry {
                    node_id: id,
                    before,
                    after: resized,
                }],
                &[snapshot(id, page, before)],
                true,
            ),
            Err(MoveNodesTransitionErrorV1::SizeChanged { node_id: id })
        );
    }

    #[test]
    fn move_transition_rejects_page_transform_and_stale_state() {
        let page = page_id(1);
        let other_page = page_id(3);
        let id = node_id(2);
        let before = rect(0, 0, 100, 80);
        let after = rect(10, 20, 100, 80);
        let entry = MoveNodeBatchEntry {
            node_id: id,
            before,
            after,
        };

        assert_eq!(
            validate_move_nodes_transition_v1(
                page,
                std::slice::from_ref(&entry),
                &[snapshot(id, other_page, before)],
                true,
            ),
            Err(MoveNodesTransitionErrorV1::PageMismatch {
                node_id: id,
                page_id: page,
            })
        );

        let mut transformed = snapshot(id, page, before);
        transformed.transform.tx = LengthEmu::new(1);
        assert_eq!(
            validate_move_nodes_transition_v1(
                page,
                std::slice::from_ref(&entry),
                &[transformed],
                true,
            ),
            Err(MoveNodesTransitionErrorV1::NodeUnsupported { node_id: id })
        );

        assert_eq!(
            validate_move_nodes_transition_v1(
                page,
                &[entry],
                &[snapshot(id, page, rect(1, 0, 100, 80))],
                true,
            ),
            Err(MoveNodesTransitionErrorV1::Stale { node_id: id })
        );
    }

    #[test]
    fn resize_transition_accepts_canonical_batch() {
        let page = page_id(1);
        let a = node_id(2);
        let b = node_id(3);
        let before_a = rect(0, 0, 100, 80);
        let before_b = rect(200, 0, 100, 80);
        let entries = [
            ResizeNodeBatchEntry {
                node_id: a,
                before: before_a,
                after: rect(0, 0, 120, 80),
            },
            ResizeNodeBatchEntry {
                node_id: b,
                before: before_b,
                after: rect(200, 0, 100, 90),
            },
        ];
        let nodes = [snapshot(a, page, before_a), snapshot(b, page, before_b)];

        assert_eq!(
            validate_resize_nodes_transition_v1(page, &entries, &nodes, true),
            Ok(())
        );
    }

    #[test]
    fn resize_transition_preserves_order_count_and_size_failures() {
        let page = page_id(1);
        let a = node_id(2);
        let b = node_id(3);
        let before_a = rect(0, 0, 100, 80);
        let before_b = rect(200, 0, 100, 80);

        assert_eq!(
            validate_resize_nodes_transition_v1(page, &[], &[], true),
            Err(ResizeNodesTransitionErrorV1::InvalidCount { found: 0 })
        );

        let reversed = [
            ResizeNodeBatchEntry {
                node_id: b,
                before: before_b,
                after: rect(200, 0, 120, 80),
            },
            ResizeNodeBatchEntry {
                node_id: a,
                before: before_a,
                after: rect(0, 0, 120, 80),
            },
        ];
        assert_eq!(
            validate_resize_nodes_transition_v1(
                page,
                &reversed,
                &[snapshot(a, page, before_a), snapshot(b, page, before_b)],
                true,
            ),
            Err(ResizeNodesTransitionErrorV1::NotCanonical { node_id: a })
        );

        let no_size_change = [
            ResizeNodeBatchEntry {
                node_id: a,
                before: before_a,
                after: rect(10, 0, 100, 80),
            },
            ResizeNodeBatchEntry {
                node_id: b,
                before: before_b,
                after: rect(210, 0, 100, 80),
            },
        ];
        assert_eq!(
            validate_resize_nodes_transition_v1(
                page,
                &no_size_change,
                &[snapshot(a, page, before_a), snapshot(b, page, before_b)],
                true,
            ),
            Err(ResizeNodesTransitionErrorV1::NoSizeChange)
        );
    }

    #[test]
    fn resize_transition_rejects_nonpositive_and_stale_targets() {
        let page = page_id(1);
        let a = node_id(2);
        let b = node_id(3);
        let before_a = rect(0, 0, 100, 80);
        let before_b = rect(200, 0, 100, 80);
        let nonpositive = [
            ResizeNodeBatchEntry {
                node_id: a,
                before: before_a,
                after: rect(0, 0, 0, 80),
            },
            ResizeNodeBatchEntry {
                node_id: b,
                before: before_b,
                after: rect(200, 0, 100, 90),
            },
        ];
        assert_eq!(
            validate_resize_nodes_transition_v1(
                page,
                &nonpositive,
                &[snapshot(a, page, before_a), snapshot(b, page, before_b)],
                true,
            ),
            Err(ResizeNodesTransitionErrorV1::NonPositive { node_id: a })
        );

        let valid = [
            ResizeNodeBatchEntry {
                node_id: a,
                before: before_a,
                after: rect(0, 0, 120, 80),
            },
            ResizeNodeBatchEntry {
                node_id: b,
                before: before_b,
                after: rect(200, 0, 100, 90),
            },
        ];
        assert_eq!(
            validate_resize_nodes_transition_v1(
                page,
                &valid,
                &[
                    snapshot(a, page, rect(1, 0, 100, 80)),
                    snapshot(b, page, before_b),
                ],
                true,
            ),
            Err(ResizeNodesTransitionErrorV1::Stale { node_id: a })
        );
    }
}
