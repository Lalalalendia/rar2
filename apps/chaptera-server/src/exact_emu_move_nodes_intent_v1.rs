//! Private, source-bound planning seam for a single atomic MoveNodes event.
//!
//! This module never treats an untrusted ReaderScene or browser rectangle as
//! authoring authority. Only an isolated EditorSession supplies before-bounds.
//! The caller must bind the exact immutable PUB, canonical project and revision.
use std::collections::BTreeSet;

use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredShapeKindV1, AuthoredShapeTransformV1, EditOperation,
    EditorSession, LengthEmu, MAX_MOVE_NODES_V1, MoveNodeBatchEntry, NodeId, PageId,
    RectEmu as AuthoringRectEmu, validate_authored_shape_runtime_v1,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::exact_emu_align_distribute_v1::{
    Member, Mode, RectEmu as PlanRectEmu, changed_only, plan,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IsolatedMoveNodesIntentV1 {
    pub page_id: String,
    pub selected_node_ids: Vec<String>,
    pub mode: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveNodesIntentErrorV1 {
    InvalidIdentity,
    InvalidCount,
    DuplicateNode,
    UnsupportedMode,
    NotAuthorCreated,
    GeometryRejected,
    NoChange,
}

fn parse_mode(value: &str) -> Result<Mode, MoveNodesIntentErrorV1> {
    match value {
        "align_left" => Ok(Mode::AlignLeft),
        "align_right" => Ok(Mode::AlignRight),
        "align_top" => Ok(Mode::AlignTop),
        "align_bottom" => Ok(Mode::AlignBottom),
        "align_horizontal_center" => Ok(Mode::AlignHorizontalCenter),
        "align_vertical_center" => Ok(Mode::AlignVerticalCenter),
        "distribute_horizontal" => Ok(Mode::DistributeHorizontal),
        "distribute_vertical" => Ok(Mode::DistributeVertical),
        _ => Err(MoveNodesIntentErrorV1::UnsupportedMode),
    }
}

/// Validate *all* selected identities before reading any geometry or history.
/// The source-neutral caller is responsible for the session/revision grant.
pub fn validate_move_nodes_intent(
    intent: &IsolatedMoveNodesIntentV1,
) -> Result<(PageId, Vec<NodeId>, Mode), MoveNodesIntentErrorV1> {
    let count = intent.selected_node_ids.len();
    if !(2..=MAX_MOVE_NODES_V1).contains(&count) {
        return Err(MoveNodesIntentErrorV1::InvalidCount);
    }
    let page_id: PageId = serde_json::from_value(Value::String(intent.page_id.clone()))
        .map_err(|_| MoveNodesIntentErrorV1::InvalidIdentity)?;
    if page_id.as_canonical().to_string() != intent.page_id {
        return Err(MoveNodesIntentErrorV1::InvalidIdentity);
    }
    let mode = parse_mode(&intent.mode)?;
    if matches!(mode, Mode::DistributeHorizontal | Mode::DistributeVertical) && count < 3 {
        return Err(MoveNodesIntentErrorV1::InvalidCount);
    }
    let mut unique = BTreeSet::new();
    for raw in &intent.selected_node_ids {
        let node_id: NodeId = serde_json::from_value(Value::String(raw.clone()))
            .map_err(|_| MoveNodesIntentErrorV1::InvalidIdentity)?;
        // This first Guest slice admits only authored v7 direct shapes.
        // A source-backed identity must never be inferred from a scene hit.
        if node_id.as_canonical().to_string() != *raw
            || node_id.as_canonical().as_bytes()[6] & 0xf0 != 0x70
        {
            return Err(MoveNodesIntentErrorV1::InvalidIdentity);
        }
        if !unique.insert(node_id) {
            return Err(MoveNodesIntentErrorV1::DuplicateNode);
        }
    }
    Ok((page_id, unique.into_iter().collect(), mode))
}

fn to_authoring_rect(rect: PlanRectEmu) -> AuthoringRectEmu {
    AuthoringRectEmu::new(
        LengthEmu::new(rect.x),
        LengthEmu::new(rect.y),
        LengthEmu::new(rect.width),
        LengthEmu::new(rect.height),
    )
}

/// Derive the entire mutation from source-owned canonical graph state.
/// A no-op produces an explicit error: no event/revision/TTL may be issued.
pub fn derive_move_nodes_operation(
    session: &EditorSession,
    intent: &IsolatedMoveNodesIntentV1,
) -> Result<EditOperation, MoveNodesIntentErrorV1> {
    let (page_id, selected_ids, mode) = validate_move_nodes_intent(intent)?;
    let graph = session.graph();
    let page = graph
        .pages
        .get(&page_id)
        .ok_or(MoveNodesIntentErrorV1::NotAuthorCreated)?;
    let mut members = Vec::with_capacity(selected_ids.len());

    for node_id in selected_ids {
        let shape = session
            .authored_shape(node_id)
            .ok_or(MoveNodesIntentErrorV1::NotAuthorCreated)?;
        if shape.page_id != page_id
            || shape.parent_id != page_id
            || shape.node_id != node_id
            || shape.shape_kind != AuthoredShapeKindV1::Rectangle
            || shape.transform != AuthoredShapeTransformV1::Identity
            || shape.provenance != AuthoredEntityProvenanceV1::AuthorCreated
            || shape.paint.provenance != AuthoredEntityProvenanceV1::AuthorCreated
            || validate_authored_shape_runtime_v1(shape).is_err()
            || page.children.iter().filter(|child| **child == node_id).count() != 1
        {
            return Err(MoveNodesIntentErrorV1::NotAuthorCreated);
        }
        let node = graph
            .nodes
            .get(&node_id)
            .ok_or(MoveNodesIntentErrorV1::NotAuthorCreated)?;
        if node.header.parent_id != page_id.into_canonical() {
            return Err(MoveNodesIntentErrorV1::NotAuthorCreated);
        }
        // The provenance overlay preserves author-created identity, whereas
        // the graph tracks CURRENT geometry after earlier MoveNode/MoveNodes.
        // Authoring from shape.bounds would silently replay initial geometry.
        let r = node.header.bounds;
        members.push(Member {
            node_id: node_id.as_canonical().to_string(),
            bounds: PlanRectEmu {
                x: r.x.get(),
                y: r.y.get(),
                width: r.width.get(),
                height: r.height.get(),
            },
        });
    }
    let planned = plan(members, mode).map_err(|_| MoveNodesIntentErrorV1::GeometryRejected)?;
    let changed = changed_only(planned);
    if changed.is_empty() {
        return Err(MoveNodesIntentErrorV1::NoChange);
    }
    let mut entries = Vec::with_capacity(changed.len());
    for item in changed {
        let node_id: NodeId = serde_json::from_value(Value::String(item.node_id))
            .map_err(|_| MoveNodesIntentErrorV1::InvalidIdentity)?;
        entries.push(MoveNodeBatchEntry {
            node_id,
            before: to_authoring_rect(item.before),
            after: to_authoring_rect(item.after),
        });
    }
    entries.sort_by_key(|entry| entry.node_id);
    Ok(EditOperation::MoveNodes { page_id, entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(ids: Vec<&str>, mode: &str) -> IsolatedMoveNodesIntentV1 {
        IsolatedMoveNodesIntentV1 {
            page_id: "20000000-0000-4000-8000-000000000001".into(),
            selected_node_ids: ids.into_iter().map(str::to_string).collect(),
            mode: mode.into(),
        }
    }

    const A: &str = "30000000-0000-7000-8000-000000000001";
    const B: &str = "30000000-0000-7000-8000-000000000002";
    const C: &str = "30000000-0000-7000-8000-000000000003";

    #[test]
    fn accepts_canonical_page_and_authored_v7_selection() {
        let (page, ids, mode) =
            validate_move_nodes_intent(&intent(vec![C, A, B], "distribute_horizontal")).unwrap();
        assert_eq!(page.as_canonical().to_string(), "20000000-0000-4000-8000-000000000001");
        assert_eq!(
            ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            vec![A, B, C]
        );
        assert_eq!(mode, Mode::DistributeHorizontal);
    }

    #[test]
    fn denies_duplicates_and_undersized_distribution_before_source_access() {
        assert_eq!(
            validate_move_nodes_intent(&intent(vec![A, A], "align_left")),
            Err(MoveNodesIntentErrorV1::DuplicateNode)
        );
        assert_eq!(
            validate_move_nodes_intent(&intent(vec![A, B], "distribute_vertical")),
            Err(MoveNodesIntentErrorV1::InvalidCount)
        );
        assert_eq!(
            validate_move_nodes_intent(&intent(vec![A], "align_left")),
            Err(MoveNodesIntentErrorV1::InvalidCount)
        );
    }

    #[test]
    fn rejects_imported_identity_and_unrecognized_command() {
        let imported = "30000000-0000-4000-8000-000000000001";
        assert_eq!(
            validate_move_nodes_intent(&intent(vec![A, imported], "align_left")),
            Err(MoveNodesIntentErrorV1::InvalidIdentity)
        );
        assert_eq!(
            validate_move_nodes_intent(&intent(vec![A, B], "magic")),
            Err(MoveNodesIntentErrorV1::UnsupportedMode)
        );
    }

    #[test]
    fn accepts_only_strict_private_json_envelope() {
        let raw = serde_json::json!({
            "page_id": "20000000-0000-4000-8000-000000000001",
            "selected_node_ids": [A, B],
            "mode": "align_right",
            "source_bytes": "not admitted"
        });
        assert!(serde_json::from_value::<IsolatedMoveNodesIntentV1>(raw).is_err());
    }
}
