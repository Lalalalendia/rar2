use anyhow::{Context, Result, bail};
use pub_model::{Node, NodeId, NodeKind};
use pub_reader::PubResolvedNodePayload;
use pub_viewer::{ViewerNodePaint, ViewerPresetShape, open_pub_bundle, viewer_geometry_environment_v0_1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::PathBuf;

const SCHEMA: &str = "chaptera.pub-pdf-resource-missing-census.v2";
const MAX_BOUNDED_SOURCE_LINE_WIDTH_EMU_V1: i64 = 0x0132_F540;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Shape => "shape",
        NodeKind::TextFrame => "text_frame",
        NodeKind::ImageFrame => "image_frame",
        NodeKind::VectorPath => "vector_path",
        NodeKind::Group => "group",
        NodeKind::Connector => "connector",
        NodeKind::Table => "table",
        NodeKind::PlacedArtifact => "placed_artifact",
        NodeKind::Unsupported => "unsupported",
    }
}

fn explicit_paint_signal(node: &Node<PubResolvedNodePayload>) -> bool {
    let paint = &node.payload.explicit_paint;
    paint.fill.solid
        || paint.fill.color_rgb.is_some()
        || paint.fill.visible.is_some()
        || paint.line.color_rgb.is_some()
        || paint.line.width_emu.is_some()
        || paint.line.visible.is_some()
}

fn bounded_visible_paint(node: &Node<PubResolvedNodePayload>) -> (bool, bool) {
    if let Some(effective) = node.payload.effective_paint.as_ref() {
        let fill = matches!(
            (
                effective.fill.solid.as_ref(),
                effective.fill.color_rgb.as_ref(),
                effective.fill.visible.as_ref(),
            ),
            (Some(solid), Some(_), Some(visible)) if solid.value && visible.value
        );
        let line = matches!(
            (
                effective.line.color_rgb.as_ref(),
                effective.line.width_emu.as_ref(),
                effective.line.visible.as_ref(),
            ),
            (Some(_), Some(width), Some(visible))
                if visible.value
                    && width.value > 0
                    && width.value <= MAX_BOUNDED_SOURCE_LINE_WIDTH_EMU_V1
        );
        return (fill, line);
    }

    let paint = &node.payload.explicit_paint;
    let fill = paint.fill.solid && paint.fill.visible == Some(true) && paint.fill.color_rgb.is_some();
    let line = matches!(
        (paint.line.visible, paint.line.color_rgb, paint.line.width_emu),
        (Some(true), Some(_), Some(width))
            if width > 0 && width <= MAX_BOUNDED_SOURCE_LINE_WIDTH_EMU_V1
    );
    (fill, line)
}

fn preset_name(paint: &ViewerNodePaint) -> Option<&'static str> {
    match paint.preset_shape {
        Some(ViewerPresetShape::RoundRect) => Some("round_rect"),
        Some(ViewerPresetShape::Ellipse) => Some("ellipse"),
        Some(ViewerPresetShape::Line) => Some("line"),
        Some(ViewerPresetShape::LineDashGel) => Some("line_dash_gel"),
        None => None,
    }
}

struct Membership<'a> {
    paints: BTreeMap<NodeId, &'a ViewerNodePaint>,
    story_frames: BTreeSet<NodeId>,
    text_fragments: BTreeSet<NodeId>,
    tables: BTreeSet<NodeId>,
    images: BTreeSet<NodeId>,
    borders: BTreeSet<NodeId>,
}

fn classify(
    node: &Node<PubResolvedNodePayload>,
    membership: &Membership<'_>,
) -> &'static str {
    let id = node.header.id;
    if membership.borders.contains(&id) {
        return "decorative_border";
    }
    if membership.tables.contains(&id) || node.payload.table.is_some() || node.kind == NodeKind::Table {
        return "table";
    }
    if membership.images.contains(&id)
        || node.payload.image_slot.is_some()
        || node.payload.legacy_ole.is_some()
        || node.kind == NodeKind::ImageFrame
    {
        return "image_or_ole";
    }
    if membership.story_frames.contains(&id)
        || membership.text_fragments.contains(&id)
        || node.payload.story_frame.is_some()
        || node.kind == NodeKind::TextFrame
    {
        return "text_frame";
    }

    match node.kind {
        NodeKind::VectorPath => "vector_path",
        NodeKind::Connector => "connector",
        NodeKind::Group => "group",
        NodeKind::PlacedArtifact => "placed_artifact",
        NodeKind::Table => "table",
        NodeKind::ImageFrame => "image_or_ole",
        NodeKind::TextFrame => "text_frame",
        NodeKind::Unsupported => "unsupported_node_kind",
        NodeKind::Shape => {
            if let Some(paint) = membership.paints.get(&id) {
                match paint.preset_shape {
                    Some(ViewerPresetShape::Line | ViewerPresetShape::LineDashGel) => "line_shape_empty_fixed_paint",
                    Some(ViewerPresetShape::Ellipse) => "ellipse_shape_empty_fixed_paint",
                    Some(ViewerPresetShape::RoundRect) => "round_rect_empty_fixed_paint",
                    None if paint.solid_fill_rgb.is_some() || paint.solid_line.is_some() => {
                        "shape_viewer_paint_present_but_fixed_missing"
                    }
                    None => "shape_empty_viewer_paint",
                }
            } else if node.payload.effective_paint.is_some() {
                let (fill, line) = bounded_visible_paint(node);
                if fill || line {
                    "shape_visible_effective_paint_not_projected"
                } else {
                    "shape_effective_paint_nonpaintable"
                }
            } else if explicit_paint_signal(node) {
                let (fill, line) = bounded_visible_paint(node);
                if fill || line {
                    "shape_visible_explicit_paint_not_projected"
                } else {
                    "shape_explicit_paint_nonpaintable"
                }
            } else {
                "shape_without_paint_semantics"
            }
        }
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let pub_path = PathBuf::from(
        args.next()
            .context("usage: resource-missing-census SOURCE.pub LOSS.json OUTPUT.json")?,
    );
    let loss_path = PathBuf::from(
        args.next()
            .context("usage: resource-missing-census SOURCE.pub LOSS.json OUTPUT.json")?,
    );
    let output_path = PathBuf::from(
        args.next()
            .context("usage: resource-missing-census SOURCE.pub LOSS.json OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("resource-missing-census accepts exactly SOURCE.pub LOSS.json OUTPUT.json");
    }

    let bytes = fs::read(&pub_path).with_context(|| format!("read {}", pub_path.display()))?;
    let source_sha256 = sha256_hex(&bytes);
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .context("open source through Viewer")?;
    let loss: Value = serde_json::from_slice(
        &fs::read(&loss_path).with_context(|| format!("read {}", loss_path.display()))?,
    )
    .context("parse fixed-PDF loss report")?;

    let receipt_source = loss
        .pointer("/conversion_profile/source/source_sha256")
        .and_then(Value::as_str)
        .context("loss report lacks conversion source identity")?;
    if receipt_source != source_sha256 {
        bail!("loss report/source identity mismatch");
    }

    let membership = Membership {
        paints: bundle
            .geometry
            .paints
            .iter()
            .map(|paint| (paint.node_id, paint))
            .collect(),
        story_frames: bundle.geometry.story_frames.iter().map(|frame| frame.frame_id).collect(),
        text_fragments: bundle.geometry.text_fragments.iter().map(|fragment| fragment.frame_id).collect(),
        tables: bundle.geometry.tables.iter().map(|table| table.node_id).collect(),
        images: bundle
            .geometry
            .images
            .iter()
            .flat_map(|image| image.node_ids.iter().copied())
            .collect(),
        borders: bundle
            .geometry
            .decorative_borders
            .iter()
            .map(|border| border.node_id)
            .collect(),
    };

    let report_nodes = loss
        .pointer("/pdf/nodes")
        .and_then(Value::as_array)
        .context("loss report lacks pdf.nodes")?;

    let mut missing_origins = Vec::<NodeId>::new();
    for row in report_nodes {
        if row.get("code").and_then(Value::as_str) != Some("pdf.node.resource_missing") {
            continue;
        }
        let origin: NodeId = serde_json::from_value(
            row.get("origin")
                .cloned()
                .context("resource-missing node lacks origin")?,
        )
        .context("decode resource-missing node identity")?;
        missing_origins.push(origin);
    }
    missing_origins.sort();
    missing_origins.dedup();

    let mut semantic_class_counts = BTreeMap::<String, usize>::new();
    let mut node_kind_counts = BTreeMap::<String, usize>::new();
    let mut feature_counts = BTreeMap::<String, usize>::new();
    let mut graph_lookup_missing = 0usize;

    for origin in &missing_origins {
        let Some(node) = bundle.resolved_graph.nodes.get(origin) else {
            graph_lookup_missing += 1;
            continue;
        };
        bump(&mut node_kind_counts, node_kind_name(node.kind));
        bump(&mut semantic_class_counts, classify(node, &membership));

        if let Some(paint) = membership.paints.get(origin) {
            bump(&mut feature_counts, "viewer_paint_present");
            if paint.solid_fill_rgb.is_some() {
                bump(&mut feature_counts, "viewer_solid_fill_present");
            }
            if paint.solid_line.is_some() {
                bump(&mut feature_counts, "viewer_solid_line_present");
            }
            if let Some(name) = preset_name(paint) {
                bump(&mut feature_counts, format!("viewer_preset_{name}"));
            }
        }
        if membership.story_frames.contains(origin) {
            bump(&mut feature_counts, "viewer_story_frame");
        }
        if membership.text_fragments.contains(origin) {
            bump(&mut feature_counts, "viewer_text_fragment");
        }
        if membership.tables.contains(origin) {
            bump(&mut feature_counts, "viewer_table");
        }
        if membership.images.contains(origin) {
            bump(&mut feature_counts, "viewer_image_use");
        }
        if membership.borders.contains(origin) {
            bump(&mut feature_counts, "viewer_decorative_border");
        }
        if node.payload.effective_paint.is_some() {
            bump(&mut feature_counts, "payload_effective_paint");
        }
        if explicit_paint_signal(node) {
            bump(&mut feature_counts, "payload_explicit_paint_signal");
        }
        let (bounded_fill, bounded_line) = bounded_visible_paint(node);
        if bounded_fill {
            bump(&mut feature_counts, "payload_bounded_visible_fill");
        }
        if bounded_line {
            bump(&mut feature_counts, "payload_bounded_visible_line");
        }
        if bounded_fill || bounded_line {
            bump(&mut feature_counts, "payload_bounded_visible_paint");
        }
        if node.payload.image_slot.is_some() {
            bump(&mut feature_counts, "payload_image_slot");
        }
        if node.payload.legacy_ole.is_some() {
            bump(&mut feature_counts, "payload_legacy_ole");
        }
        if node.payload.story_frame.is_some() {
            bump(&mut feature_counts, "payload_story_frame");
        }
        if node.payload.table.is_some() {
            bump(&mut feature_counts, "payload_table");
        }
        if node.payload.officeart_shape_type.is_some() {
            bump(&mut feature_counts, "payload_officeart_shape_type");
        }
    }

    let receipt = json!({
        "schema": SCHEMA,
        "source_sha256": source_sha256,
        "format": bundle.geometry.document.source.format,
        "format_version": bundle.geometry.document.source.format_version,
        "resource_missing_count": missing_origins.len(),
        "resolved_graph_lookup_missing_count": graph_lookup_missing,
        "semantic_class_counts": semantic_class_counts,
        "node_kind_counts": node_kind_counts,
        "feature_counts": feature_counts,
        "claims": {
            "raw_node_ids_emitted": false,
            "source_text_emitted": false,
            "classification_is_source_viewer_semantic_not_raster_inferred": true,
            "classification_changes_pdf_output": false
        }
    });

    fs::write(
        &output_path,
        serde_json::to_vec_pretty(&receipt).context("serialize resource-missing census")?,
    )
    .with_context(|| format!("write {}", output_path.display()))?;
    Ok(())
}
