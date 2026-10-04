//! Bounded single-Rectangle Duplicate semantics for Chaptera-authored objects.
//! CI negative-control note: this feature-owned module must not require Continuity V2 Windows.
//!
//! Duplicate is intentionally lowered to the existing canonical CreateShape
//! operation. This module owns only admission and the named placement policy;
//! persistence, Undo/Redo, AuthoredStack membership and replay stay owned by
//! the existing CreateShape runtime.

use crate::create_shape_runtime_v1::is_editor_created_uuid_v7_node_id;
use crate::{
    AuthoredEntityProvenanceV1, AuthoredShapeKindV1, AuthoredShapePaintV1, AuthoredShapeRuntimeV1,
    AuthoredShapeTransformV1, EditorError, LengthEmu, NodeId, PageId, RectEmu,
    validate_authored_shape_runtime_v1,
};
use std::fmt;

pub const DUPLICATE_PLACEMENT_POLICY_V1: &str = "chaptera.duplicate-placement.10pt-down-right.v1";
pub const DUPLICATE_OFFSET_EMU_V1: i64 = 127_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateAuthoredRectanglePlanV1 {
    pub source_node_id: NodeId,
    pub destination_node_id: NodeId,
    pub page_id: PageId,
    pub bounds: RectEmu,
    pub paint: AuthoredShapePaintV1,
}

#[derive(Debug)]
pub enum DuplicateAuthoredRectangleErrorV1 {
    UnsupportedPlacementPolicy,
    SourceUnsupported { node_id: NodeId },
    DestinationInvalid { node_id: NodeId },
    SameIdentity { node_id: NodeId },
    BoundsOverflow { node_id: NodeId },
    Commit(EditorError),
}

impl fmt::Display for DuplicateAuthoredRectangleErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlacementPolicy => formatter
                .write_str("Duplicate requires chaptera.duplicate-placement.10pt-down-right.v1"),
            Self::SourceUnsupported { node_id } => write!(
                formatter,
                "node {} is not an admitted author-created direct page-owned Rectangle",
                node_id.as_canonical()
            ),
            Self::DestinationInvalid { node_id } => write!(
                formatter,
                "Duplicate destination {} is not a fresh editor-created UUIDv7",
                node_id.as_canonical()
            ),
            Self::SameIdentity { node_id } => write!(
                formatter,
                "Duplicate source and destination cannot both be {}",
                node_id.as_canonical()
            ),
            Self::BoundsOverflow { node_id } => write!(
                formatter,
                "Duplicate placement for source {} exceeds safe geometry bounds",
                node_id.as_canonical()
            ),
            Self::Commit(error) => {
                write!(formatter, "Duplicate CreateShape commit failed: {error}")
            }
        }
    }
}

impl std::error::Error for DuplicateAuthoredRectangleErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Commit(error) => Some(error),
            _ => None,
        }
    }
}

pub fn validate_duplicate_authored_rectangle_source_v1(
    source: &AuthoredShapeRuntimeV1,
) -> Result<(), DuplicateAuthoredRectangleErrorV1> {
    if source.shape_kind != AuthoredShapeKindV1::Rectangle
        || source.transform != AuthoredShapeTransformV1::Identity
        || source.parent_id != source.page_id
        || source.provenance != AuthoredEntityProvenanceV1::AuthorCreated
        || source.paint.provenance != AuthoredEntityProvenanceV1::AuthorCreated
        || validate_authored_shape_runtime_v1(source).is_err()
    {
        return Err(DuplicateAuthoredRectangleErrorV1::SourceUnsupported {
            node_id: source.node_id,
        });
    }
    Ok(())
}

pub fn plan_duplicate_authored_rectangle_v1(
    source: &AuthoredShapeRuntimeV1,
    destination_node_id: NodeId,
    placement_policy: &str,
) -> Result<DuplicateAuthoredRectanglePlanV1, DuplicateAuthoredRectangleErrorV1> {
    if placement_policy != DUPLICATE_PLACEMENT_POLICY_V1 {
        return Err(DuplicateAuthoredRectangleErrorV1::UnsupportedPlacementPolicy);
    }
    validate_duplicate_authored_rectangle_source_v1(source)?;
    if source.node_id == destination_node_id {
        return Err(DuplicateAuthoredRectangleErrorV1::SameIdentity {
            node_id: source.node_id,
        });
    }
    if !is_editor_created_uuid_v7_node_id(destination_node_id) {
        return Err(DuplicateAuthoredRectangleErrorV1::DestinationInvalid {
            node_id: destination_node_id,
        });
    }

    let x = source
        .bounds
        .x
        .checked_add(LengthEmu::new(DUPLICATE_OFFSET_EMU_V1))
        .ok_or(DuplicateAuthoredRectangleErrorV1::BoundsOverflow {
            node_id: source.node_id,
        })?;
    let y = source
        .bounds
        .y
        .checked_add(LengthEmu::new(DUPLICATE_OFFSET_EMU_V1))
        .ok_or(DuplicateAuthoredRectangleErrorV1::BoundsOverflow {
            node_id: source.node_id,
        })?;
    let bounds = RectEmu::new(x, y, source.bounds.width, source.bounds.height);

    let candidate = AuthoredShapeRuntimeV1 {
        node_id: destination_node_id,
        page_id: source.page_id,
        parent_id: source.page_id,
        shape_kind: source.shape_kind,
        bounds,
        transform: source.transform,
        paint: source.paint.clone(),
        provenance: source.provenance,
    };
    if validate_authored_shape_runtime_v1(&candidate).is_err() {
        return Err(DuplicateAuthoredRectangleErrorV1::BoundsOverflow {
            node_id: source.node_id,
        });
    }

    Ok(DuplicateAuthoredRectanglePlanV1 {
        source_node_id: source.node_id,
        destination_node_id,
        page_id: source.page_id,
        bounds,
        paint: source.paint.clone(),
    })
}
