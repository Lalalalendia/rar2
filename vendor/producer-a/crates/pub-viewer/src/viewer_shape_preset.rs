//! Admit only source-authoritative OfficeArt preset geometry into Viewer.
use super::*;

pub(super) fn viewer_preset_shape_from_canonical(
    node: &Node<PubResolvedNodePayload>,
) -> Option<ViewerPresetShape> {
    let authoritative_projection = |path: &str| {
        node.header.source_refs.iter().any(|source_ref| {
            source_ref.path.as_deref() == Some(path)
                && source_ref.authority == AuthorityClass::Authoritative
                && source_ref.confidence == Some(ReadConfidence::Exact)
                && matches!(source_ref.role, SourceRole::Projection)
        })
    };
    // Must precede legacy default-line; this source carries a physical
    // horizontal axis + stroke envelope, never a corner-to-corner diagonal.
    if authoritative_projection("SpContainer/ClientAnchor/source-horizontal-line-stroke") {
        Some(ViewerPresetShape::HorizontalLine)
    } else if authoritative_projection("SpContainer/FOPT/line-dashing-dash-gel")
        && authoritative_projection("SpContainer/FSP/default-line")
    {
        Some(ViewerPresetShape::LineDashGel)
    } else if authoritative_projection("SpContainer/FSP/default-line") {
        Some(ViewerPresetShape::Line)
    } else if authoritative_projection("SpContainer/FSP/default-roundrect") {
        Some(ViewerPresetShape::RoundRect)
    } else if authoritative_projection("SpContainer/FSP/default-ellipse") {
        Some(ViewerPresetShape::Ellipse)
    } else {
        None
    }
}
