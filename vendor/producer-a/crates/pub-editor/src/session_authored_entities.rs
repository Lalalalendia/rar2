//! Canonical authored overlay object state and DeleteNode replay helpers.
//!
//! Extracted unchanged from the EditorSession facade to preserve the
//! source-length ratchet while integrating a separate Page lifecycle operation.

use super::*;

pub(super) fn authored_shape_from_operation(operation: &EditOperation) -> Option<AuthoredShapeRuntimeV1> {
    match operation {
        EditOperation::CreateShape {
            node_id,
            page_id,
            parent_id,
            shape_kind,
            bounds,
            transform,
            paint,
            provenance,
        } => Some(AuthoredShapeRuntimeV1 {
            node_id: *node_id,
            page_id: *page_id,
            parent_id: *parent_id,
            shape_kind: *shape_kind,
            bounds: *bounds,
            transform: *transform,
            paint: paint.clone(),
            provenance: *provenance,
        }),
        _ => None,
    }
}

pub(super) fn authored_line_from_operation(operation: &EditOperation) -> Option<AuthoredLineRuntimeV1> {
    match operation {
        EditOperation::CreateLine {
            node_id,
            page_id,
            parent_id,
            geometry,
            stroke,
            provenance,
        } => Some(AuthoredLineRuntimeV1 {
            node_id: *node_id,
            page_id: *page_id,
            parent_id: *parent_id,
            geometry: *geometry,
            stroke: stroke.clone(),
            provenance: *provenance,
        }),
        _ => None,
    }
}

pub(super) fn apply_authored_line_inverse(
    authored_lines: &mut BTreeMap<NodeId, AuthoredLineRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let line = authored_line_from_operation(operation)
        .expect("CreateLine inverse receives CreateLine operation");
    if authored_lines.get(&line.node_id) != Some(&line) {
        return Err(EditorError::CreateLineIdCollision {
            node_id: line.node_id,
        });
    }
    authored_lines.remove(&line.node_id);
    Ok(())
}

pub(super) fn apply_authored_shape_inverse(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let shape = authored_shape_from_operation(operation)
        .expect("CreateShape inverse receives CreateShape operation");
    if authored_shapes.get(&shape.node_id) != Some(&shape) {
        return Err(EditorError::CreateShapeIdCollision {
            node_id: shape.node_id,
        });
    }
    authored_shapes.remove(&shape.node_id);
    Ok(())
}

pub(super) fn apply_authored_shape_delete_forward(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::DeleteNode {
        node_id,
        page_id,
        before,
        before_state_id,
    } = operation
    else {
        unreachable!("DeleteNode forward receives DeleteNode operation")
    };

    if before.node_id != *node_id || before.page_id != *page_id || before.parent_id != *page_id {
        return Err(EditorError::NodeDeletePageMismatch {
            node_id: *node_id,
            page_id: *page_id,
        });
    }
    if authored_shape_state_id_v1(before) != *before_state_id
        || authored_shapes.get(node_id) != Some(before)
    {
        return Err(EditorError::StaleNodeDelete { node_id: *node_id });
    }
    authored_shapes.remove(node_id);
    Ok(())
}

pub(super) fn apply_authored_shape_delete_inverse(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::DeleteNode {
        node_id,
        page_id,
        before,
        before_state_id,
    } = operation
    else {
        unreachable!("DeleteNode inverse receives DeleteNode operation")
    };

    if before.node_id != *node_id || before.page_id != *page_id || before.parent_id != *page_id {
        return Err(EditorError::NodeDeletePageMismatch {
            node_id: *node_id,
            page_id: *page_id,
        });
    }
    if authored_shape_state_id_v1(before) != *before_state_id
        || authored_shapes.contains_key(node_id)
    {
        return Err(EditorError::StaleNodeDelete { node_id: *node_id });
    }
    authored_shapes.insert(*node_id, before.clone());
    Ok(())
}
