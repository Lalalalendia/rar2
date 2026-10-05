//! Canonical Chaptera primitive-Line authoring state.
//!
//! Primitive Line is intentionally separate from Rectangle bounds and from
//! VectorPath topology. Ordered endpoints are canonical truth; the normalized
//! rectangle returned by line_bounds_v1 is derived geometry.

use pub_model::{LengthEmu, NodeId, PageId, RectEmu};
use serde::{Deserialize, Serialize};

use crate::{AuthoredEntityProvenanceV1, AuthoredSolidStrokeV1};

pub const MAX_SAFE_LINE_EMU_V1: i64 = 9_007_199_254_740_991;
pub const MIN_SAFE_LINE_EMU_V1: i64 = -MAX_SAFE_LINE_EMU_V1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointEmuV1 {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineGeometryV1 {
    pub begin: PointEmuV1,
    pub end: PointEmuV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredLineRuntimeV1 {
    pub node_id: NodeId,
    pub page_id: PageId,
    pub parent_id: PageId,
    pub geometry: LineGeometryV1,
    pub stroke: AuthoredSolidStrokeV1,
    pub provenance: AuthoredEntityProvenanceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateLineRuntimeValidationError {
    NodeIdNotUuidV7,
    ParentPageMismatch,
    NonAuthorCreatedProvenance,
    CoordinateOutOfRange,
    DerivedBoundsOverflow,
    InvalidStroke,
}

pub fn line_bounds_v1(
    geometry: LineGeometryV1,
) -> Result<RectEmu, CreateLineRuntimeValidationError> {
    for value in [
        geometry.begin.x,
        geometry.begin.y,
        geometry.end.x,
        geometry.end.y,
    ] {
        if !(MIN_SAFE_LINE_EMU_V1..=MAX_SAFE_LINE_EMU_V1).contains(&value) {
            return Err(CreateLineRuntimeValidationError::CoordinateOutOfRange);
        }
    }

    let x = geometry.begin.x.min(geometry.end.x);
    let y = geometry.begin.y.min(geometry.end.y);
    let right = geometry.begin.x.max(geometry.end.x);
    let bottom = geometry.begin.y.max(geometry.end.y);
    let width = right
        .checked_sub(x)
        .ok_or(CreateLineRuntimeValidationError::DerivedBoundsOverflow)?;
    let height = bottom
        .checked_sub(y)
        .ok_or(CreateLineRuntimeValidationError::DerivedBoundsOverflow)?;

    Ok(RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    ))
}

pub fn validate_authored_line_runtime_v1(
    line: &AuthoredLineRuntimeV1,
) -> Result<(), CreateLineRuntimeValidationError> {
    if !is_editor_created_uuid_v7_node_id(line.node_id) {
        return Err(CreateLineRuntimeValidationError::NodeIdNotUuidV7);
    }
    if line.parent_id != line.page_id {
        return Err(CreateLineRuntimeValidationError::ParentPageMismatch);
    }
    if line.provenance != AuthoredEntityProvenanceV1::AuthorCreated {
        return Err(CreateLineRuntimeValidationError::NonAuthorCreatedProvenance);
    }

    let _ = line_bounds_v1(line.geometry)?;

    if line.stroke.width_emu <= 0 || line.stroke.width_emu > MAX_SAFE_LINE_EMU_V1 {
        return Err(CreateLineRuntimeValidationError::InvalidStroke);
    }

    Ok(())
}

fn is_editor_created_uuid_v7_node_id(node_id: NodeId) -> bool {
    let bytes = node_id.as_canonical().as_bytes();
    (bytes[6] & 0xf0) == 0x70 && (bytes[8] & 0xc0) == 0x80
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Srgb8V1;

    fn node_id() -> NodeId {
        serde_json::from_str("\"01890f47-0c00-7abc-8def-0123456789ab\"")
            .expect("canonical editor UUIDv7 NodeId")
    }

    fn page_id() -> PageId {
        serde_json::from_str("\"11000000-0000-4000-8000-000000000001\"").expect("canonical PageId")
    }

    fn stroke() -> AuthoredSolidStrokeV1 {
        AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 { r: 4, g: 5, b: 6 },
            width_emu: 25_400,
        }
    }

    fn line(begin: PointEmuV1, end: PointEmuV1) -> AuthoredLineRuntimeV1 {
        AuthoredLineRuntimeV1 {
            node_id: node_id(),
            page_id: page_id(),
            parent_id: page_id(),
            geometry: LineGeometryV1 { begin, end },
            stroke: stroke(),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    #[test]
    fn ordered_endpoints_remain_canonical_while_bounds_normalize() {
        let forward = line(PointEmuV1 { x: 100, y: 200 }, PointEmuV1 { x: 400, y: 500 });
        let reverse = line(forward.geometry.end, forward.geometry.begin);

        assert_ne!(forward.geometry, reverse.geometry);
        assert_eq!(
            line_bounds_v1(forward.geometry).expect("forward bounds"),
            line_bounds_v1(reverse.geometry).expect("reverse bounds")
        );
    }

    #[test]
    fn horizontal_vertical_and_zero_length_lines_are_valid() {
        for candidate in [
            line(PointEmuV1 { x: 100, y: 100 }, PointEmuV1 { x: 220, y: 100 }),
            line(PointEmuV1 { x: 100, y: 100 }, PointEmuV1 { x: 100, y: 220 }),
            line(PointEmuV1 { x: 140, y: 140 }, PointEmuV1 { x: 140, y: 140 }),
        ] {
            validate_authored_line_runtime_v1(&candidate).expect("Publisher-compatible Line");
        }

        let zero = line(PointEmuV1 { x: 140, y: 140 }, PointEmuV1 { x: 140, y: 140 });
        assert_eq!(
            line_bounds_v1(zero.geometry).expect("zero bounds"),
            RectEmu::new(
                LengthEmu::new(140),
                LengthEmu::new(140),
                LengthEmu::ZERO,
                LengthEmu::ZERO,
            )
        );
    }

    #[test]
    fn line_rejects_non_positive_stroke_and_unsafe_coordinates() {
        let mut bad_stroke = line(
            PointEmuV1 { x: 0, y: 0 },
            PointEmuV1 { x: 1, y: 1 },
        );
        bad_stroke.stroke.width_emu = 0;
        assert_eq!(
            validate_authored_line_runtime_v1(&bad_stroke),
            Err(CreateLineRuntimeValidationError::InvalidStroke)
        );

        let unsafe_geometry = LineGeometryV1 {
            begin: PointEmuV1 {
                x: MAX_SAFE_LINE_EMU_V1 + 1,
                y: 0,
            },
            end: PointEmuV1 { x: 0, y: 0 },
        };
        assert_eq!(
            line_bounds_v1(unsafe_geometry),
            Err(CreateLineRuntimeValidationError::CoordinateOutOfRange)
        );
    }
}
