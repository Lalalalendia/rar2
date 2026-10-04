use anyhow::{Context, Result, bail};
use pub_layout::{
    BoundedLayoutEnvironment, BoundedResolvedScene, BoundedShapedGlyph, BoundedShapedText,
    BoundedShapingDescriptor, ResolvedPhysicalNode, ResolvedSurface, SceneOriginMapping,
    font_fingerprint_sha256,
};
use pub_model::{Affine2D, CanonicalId, LengthEmu, NodeId, PageId, RectEmu, ResourceId, Size2D};
use pub_output::{
    ExplicitFontResource, FixedOutputFontProfile, FontIdentity, OutputFontRequest,
    PreferredEmbedding, plan_output_fonts, read_opentype_embedding_flags,
};
use pub_pdf::{
    FixedFontResource, FixedImageResource, FixedNodePaint, FixedPdfResources, FixedStroke,
    FixedTextRun, PdfTargetProfile,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};

const INPUT_VERSION: &str = "chaptera.current-viewer-fixed-pdf-input.v1";
const OUTPUT_VERSION: &str = "chaptera.current-viewer-yab-supported-request.v1";
const RENDER_REQUEST_VERSION: &str = "chaptera.fixed-pdf-packet-render-request.v1";

#[derive(Debug, Deserialize)]
struct CurrentViewerInput {
    protocol_version: String,
    binding: Value,
    pages: Vec<CurrentPage>,
    images: Vec<CurrentImageAsset>,
    font: CurrentFont,
    census: CurrentCensus,
}

#[derive(Debug, Deserialize)]
struct CurrentPage {
    page_id: PageId,
    page_size: Size2D,
    nodes: Vec<CurrentNode>,
}

#[derive(Debug, Deserialize)]
struct CurrentNode {
    node_id: NodeId,
    #[serde(default)]
    projected_scene_instance: Option<CurrentProjectedSceneInstance>,
    bounds: RectEmu,
    #[serde(default)]
    text_bounds: Option<RectEmu>,
    transform: Affine2D,
    #[serde(default)]
    solid_fill_rgb: Option<[u8; 3]>,
    #[serde(default)]
    solid_line: Option<CurrentSolidLine>,
    #[serde(default)]
    decorative_border: Option<Value>,
    #[serde(default)]
    image: Option<CurrentImageRef>,
    #[serde(default)]
    text: Option<CurrentText>,
    #[serde(default)]
    table: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct CurrentProjectedSceneInstance {
    instance_id: String,
    #[serde(default)]
    projection_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CurrentSolidLine {
    rgb: [u8; 3],
    width_emu: i64,
}

#[derive(Debug, Deserialize)]
struct CurrentImageRef {
    resource_id: ResourceId,
    mime: String,
    #[serde(default)]
    source_window: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct CurrentImageAsset {
    resource_id: ResourceId,
    mime: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct CurrentFont {
    resource_id: String,
    fingerprint_sha256: String,
    face_index: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct CurrentCensus {
    page_count: usize,
    node_count: usize,
    duplicate_node_id_count: usize,
}

#[derive(Debug, Deserialize)]
struct CurrentText {
    scalar_start: u32,
    scalar_end: u32,
    #[serde(default)]
    typography: Vec<CurrentTypographyRun>,
    #[serde(default)]
    layout: Option<CurrentTextLayout>,
}

#[derive(Debug, Deserialize)]
struct CurrentTypographyRun {
    scalar_start: u32,
    scalar_end: u32,
    text_size_emu: u32,
    #[serde(default)]
    color_rgb: Option<[u8; 3]>,
}

#[derive(Debug, Deserialize)]
struct CurrentTextLayout {
    disposition: CurrentTextLayoutDisposition,
    #[serde(default)]
    vertical_offset_emu: i64,
    #[serde(default)]
    lines: Vec<CurrentTextLine>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurrentTextLayoutDisposition {
    SharedResolved {
        font_resource_id: String,
        font_fingerprint_sha256: String,
        font_size_emu: i64,
        line_height_emu: i64,
    },
    BackendFallback {
        reason: String,
    },
}

#[derive(Debug, Deserialize)]
struct CurrentTextLine {
    scalar_start: u32,
    scalar_end: u32,
    text: String,
    measured_width_emu: i64,
    line_height_emu: i64,
    #[serde(default)]
    x_offset_emu: i64,
    #[serde(default)]
    spans: Vec<Value>,
    #[serde(default)]
    shaping: Option<CurrentShaping>,
}

#[derive(Debug, Clone, Deserialize)]
struct CurrentShaping {
    environment: BoundedShapingDescriptor,
    units_per_em: u32,
    glyphs: Vec<BoundedShapedGlyph>,
}

#[derive(Debug, Default, Serialize)]
struct MappingSummary {
    input_page_count: usize,
    input_node_count: usize,
    semantic_duplicate_node_id_count: usize,
    mapped_paint_node_count: usize,
    mapped_image_use_count: usize,
    mapped_text_node_count: usize,
    mapped_text_run_count: usize,
    mapped_resource_node_count: usize,
    text_node_count: usize,
    text_resource_residual_node_count: usize,
    text_resource_residual_signature_counts: BTreeMap<String, usize>,
    text_partial_residual_signature_counts: BTreeMap<String, usize>,
    text_resource_residual_cooccurrence_counts: BTreeMap<String, usize>,
    text_resource_residual_projection_lane_counts: BTreeMap<String, usize>,
    text_resource_residual_signature_lane_counts: BTreeMap<String, usize>,
    shared_resolved_line_count: usize,
    shared_resolved_line_outcome_counts: BTreeMap<String, usize>,
    backend_fallback_reason_counts: BTreeMap<String, usize>,
    cropped_image_use_count: usize,
    table_node_count: usize,
    decorative_border_node_count: usize,
    non_identity_transform_node_count: usize,
    shaped_span_line_count: usize,
    missing_shaping_line_count: usize,
    unresolved_text_color_line_count: usize,
    residual_node_signature_counts: BTreeMap<String, usize>,
    residual_projection_lane_counts: BTreeMap<String, usize>,
    residual_signature_lane_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct PacketRenderRequest {
    protocol_version: &'static str,
    binding: Value,
    scene: BoundedResolvedScene,
    resources: FixedPdfResources,
    target: PdfTargetProfile,
}

#[derive(Debug, Serialize)]
struct MaterializedEnvelope {
    protocol_version: &'static str,
    binding: Value,
    mapping: MappingSummary,
    request: PacketRenderRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorRangeDisposition {
    Resolved([u8; 3]),
    MissingRgb,
    MixedRgb,
    CoverageGap,
    InvalidRange,
}

impl ColorRangeDisposition {
    const fn code(self) -> &'static str {
        match self {
            Self::Resolved(_) => "resolved",
            Self::MissingRgb => "missing_rgb",
            Self::MixedRgb => "mixed_rgb",
            Self::CoverageGap => "coverage_gap",
            Self::InvalidRange => "invalid_range",
        }
    }
}

fn color_range_disposition(
    runs: &[CurrentTypographyRun],
    scalar_start: u32,
    scalar_end: u32,
) -> ColorRangeDisposition {
    if scalar_start >= scalar_end {
        return ColorRangeDisposition::InvalidRange;
    }

    let mut cursor = scalar_start;
    let mut colors = BTreeSet::<[u8; 3]>::new();
    let mut missing_rgb = false;
    for run in runs {
        if run.scalar_end <= scalar_start || run.scalar_start >= scalar_end {
            continue;
        }
        if run.scalar_end <= run.scalar_start {
            return ColorRangeDisposition::InvalidRange;
        }
        let start = run.scalar_start.max(scalar_start);
        let end = run.scalar_end.min(scalar_end);
        if start < cursor {
            return ColorRangeDisposition::InvalidRange;
        }
        if start > cursor {
            return ColorRangeDisposition::CoverageGap;
        }
        if let Some(color) = run.color_rgb {
            colors.insert(color);
        } else {
            missing_rgb = true;
        }
        cursor = end;
    }

    if cursor != scalar_end {
        return ColorRangeDisposition::CoverageGap;
    }
    if missing_rgb {
        return ColorRangeDisposition::MissingRgb;
    }
    match colors.len() {
        1 => ColorRangeDisposition::Resolved(*colors.iter().next().expect("one color exists")),
        0 => ColorRangeDisposition::MissingRgb,
        _ => ColorRangeDisposition::MixedRgb,
    }
}

fn text_size_profile_tag(text: &CurrentText) -> &'static str {
    if text.scalar_start >= text.scalar_end || text.typography.is_empty() {
        return "size_profile_unknown";
    }

    let mut cursor = text.scalar_start;
    let mut sizes = BTreeSet::<u32>::new();
    for run in &text.typography {
        if run.scalar_end <= text.scalar_start || run.scalar_start >= text.scalar_end {
            continue;
        }
        if run.scalar_end <= run.scalar_start {
            return "size_profile_unknown";
        }
        let start = run.scalar_start.max(text.scalar_start);
        let end = run.scalar_end.min(text.scalar_end);
        if start != cursor || run.text_size_emu == 0 {
            return "size_profile_unknown";
        }
        sizes.insert(run.text_size_emu);
        cursor = end;
    }
    if cursor != text.scalar_end {
        return "size_profile_unknown";
    }
    match sizes.len() {
        1 => "uniform_size",
        n if n > 1 => "mixed_size",
        _ => "size_profile_unknown",
    }
}

fn text_cooccurrence_tag(node: &CurrentNode) -> String {
    let mut tags = Vec::new();
    if node.solid_fill_rgb.is_some() || node.solid_line.is_some() {
        tags.push("paint");
    }
    if let Some(image) = &node.image {
        if image.source_window.is_some() {
            tags.push("cropped_image");
        } else {
            tags.push("image");
        }
    }
    if node.table.is_some() {
        tags.push("table");
    }
    if node.decorative_border.is_some() {
        tags.push("decorative_border");
    }
    if tags.is_empty() {
        "none".into()
    } else {
        tags.join("+")
    }
}

fn checked_add(left: i64, right: i64, label: &str) -> Result<i64> {
    left.checked_add(right)
        .with_context(|| format!("{label} overflow"))
}

fn checked_sub(left: i64, right: i64, label: &str) -> Result<i64> {
    left.checked_sub(right)
        .with_context(|| format!("{label} overflow"))
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn resolved_output_node_id_v1(node: &CurrentNode) -> Result<NodeId> {
    let Some(instance) = node.projected_scene_instance.as_ref() else {
        return Ok(node.node_id);
    };
    let digest = instance
        .instance_id
        .strip_prefix("sha256:")
        .context("projected scene instance id is not sha256-bound")?;
    if digest.len() != 64 {
        bail!("projected scene instance sha256 digest must contain 64 hex digits");
    }
    let raw = digest.as_bytes();
    let mut bytes = [0_u8; 16];
    for (index, slot) in bytes.iter_mut().enumerate() {
        let high = hex_nibble(raw[index * 2])
            .context("projected scene instance sha256 digest contains invalid hex")?;
        let low = hex_nibble(raw[index * 2 + 1])
            .context("projected scene instance sha256 digest contains invalid hex")?;
        *slot = (high << 4) | low;
    }
    Ok(NodeId::from_canonical(CanonicalId::from_bytes(bytes)))
}

fn main() -> Result<()> {
    let mut raw = String::new();
    io::stdin()
        .read_to_string(&mut raw)
        .context("read current Viewer fixed-PDF packet")?;
    let input: CurrentViewerInput =
        serde_json::from_str(&raw).context("parse current Viewer fixed-PDF packet")?;
    if input.protocol_version != INPUT_VERSION {
        bail!("unsupported current Viewer fixed-PDF input protocol");
    }
    if input.census.page_count != input.pages.len() {
        bail!("current Viewer packet page census differs from page vector");
    }
    let actual_node_count = input.pages.iter().map(|page| page.nodes.len()).sum::<usize>();
    if input.census.node_count != actual_node_count {
        bail!("current Viewer packet node census differs from page plans");
    }
    if input.font.bytes.is_empty() {
        bail!("current Viewer explicit font bytes are required");
    }
    let actual_font_fingerprint = font_fingerprint_sha256(&input.font.bytes);
    if actual_font_fingerprint != input.font.fingerprint_sha256 {
        bail!("current Viewer explicit font fingerprint mismatch");
    }

    let mut seen_nodes = BTreeSet::new();
    let mut surfaces = Vec::with_capacity(input.pages.len());
    let mut nodes = Vec::with_capacity(actual_node_count);
    let mut origin_mapping = Vec::with_capacity(actual_node_count);
    let mut node_paints = Vec::new();
    let mut image_uses = BTreeMap::<ResourceId, Vec<NodeId>>::new();
    let mut text_runs = Vec::<FixedTextRun>::new();
    let mut used_glyph_ids = BTreeSet::<u32>::new();
    let mut mapped_text_nodes = BTreeSet::<NodeId>::new();
    let mut mapped_resource_nodes = BTreeSet::<NodeId>::new();
    let mut text_node_ids = BTreeSet::<NodeId>::new();
    let mut residual_reasons = BTreeMap::<NodeId, BTreeSet<String>>::new();
    let mut text_residual_reasons = BTreeMap::<NodeId, BTreeSet<String>>::new();
    let mut text_cooccurrence_by_node = BTreeMap::<NodeId, String>::new();
    let mut text_projection_lane_by_node = BTreeMap::<NodeId, String>::new();
    let mut summary = MappingSummary {
        input_page_count: input.pages.len(),
        input_node_count: actual_node_count,
        semantic_duplicate_node_id_count: input.census.duplicate_node_id_count,
        ..MappingSummary::default()
    };

    let image_assets = input
        .images
        .iter()
        .map(|asset| (asset.resource_id, asset))
        .collect::<BTreeMap<_, _>>();
    if image_assets.len() != input.images.len() {
        bail!("current Viewer packet contains duplicate image resource identities");
    }

    for page in &input.pages {
        surfaces.push(ResolvedSurface {
            origin: page.page_id,
            size: page.page_size,
            bleed: None,
            margins: None,
        });

        for node in &page.nodes {
            let resolved_node_id = resolved_output_node_id_v1(node)?;
            if !seen_nodes.insert(resolved_node_id) {
                bail!("current Viewer packet contains duplicate resolved output NodeId");
            }
            nodes.push(ResolvedPhysicalNode {
                origin: resolved_node_id,
                parent_origin: page.page_id.into_canonical(),
                bounds: node.bounds,
                transform: node.transform.clone(),
            });
            origin_mapping.push(SceneOriginMapping {
                authoring_origin: node.node_id.into_canonical(),
                resolved_node_origin: resolved_node_id,
            });

            if node.transform != Affine2D::identity() {
                summary.non_identity_transform_node_count += 1;
            }
            if node.decorative_border.is_some() {
                summary.decorative_border_node_count += 1;
            }
            if node.table.is_some() {
                summary.table_node_count += 1;
                residual_reasons
                    .entry(resolved_node_id)
                    .or_default()
                    .insert("table".into());
            }

            if node.solid_fill_rgb.is_some() || node.solid_line.is_some() {
                node_paints.push(FixedNodePaint {
                    node_id: resolved_node_id,
                    fill_rgb: node.solid_fill_rgb,
                    stroke: node.solid_line.as_ref().map(|line| FixedStroke {
                        rgb: line.rgb,
                        width_emu: line.width_emu,
                    }),
                });
                summary.mapped_paint_node_count += 1;
                mapped_resource_nodes.insert(resolved_node_id);
            }

            if let Some(image) = &node.image {
                if image.source_window.is_some() {
                    summary.cropped_image_use_count += 1;
                    residual_reasons
                        .entry(resolved_node_id)
                        .or_default()
                        .insert("cropped_image".into());
                } else {
                    let asset = image_assets.get(&image.resource_id).copied().ok_or_else(|| {
                        anyhow::anyhow!(
                            "current Viewer image use references missing resource {:?}",
                            image.resource_id
                        )
                    })?;
                    if asset.mime != image.mime {
                        bail!(
                            "current Viewer image MIME differs between use and resource for {:?}",
                            image.resource_id
                        );
                    }
                    image_uses
                        .entry(image.resource_id)
                        .or_default()
                        .push(resolved_node_id);
                    summary.mapped_image_use_count += 1;
                    mapped_resource_nodes.insert(resolved_node_id);
                }
            }

            let Some(text) = &node.text else {
                continue;
            };
            text_node_ids.insert(resolved_node_id);
            text_cooccurrence_by_node.insert(resolved_node_id, text_cooccurrence_tag(node));
            text_projection_lane_by_node.insert(
                resolved_node_id,
                node.projected_scene_instance
                    .as_ref()
                    .and_then(|instance| instance.projection_kind.clone())
                    .unwrap_or_else(|| "base".into()),
            );
            let Some(layout) = &text.layout else {
                summary.missing_shaping_line_count += 1;
                residual_reasons
                    .entry(resolved_node_id)
                    .or_default()
                    .insert("text_layout_missing".into());
                text_residual_reasons
                    .entry(resolved_node_id)
                    .or_default()
                    .insert("text_layout_missing".into());
                continue;
            };

            match &layout.disposition {
                CurrentTextLayoutDisposition::BackendFallback { reason } => {
                    *summary
                        .backend_fallback_reason_counts
                        .entry(reason.clone())
                        .or_default() += 1;
                    let tag = if reason == "shared_layout_incomplete" {
                        format!("backend_fallback:{reason}:{}", text_size_profile_tag(text))
                    } else {
                        format!("backend_fallback:{reason}")
                    };
                    residual_reasons
                        .entry(resolved_node_id)
                        .or_default()
                        .insert(tag.clone());
                    text_residual_reasons
                        .entry(resolved_node_id)
                        .or_default()
                        .insert(tag);
                }
                CurrentTextLayoutDisposition::SharedResolved {
                    font_resource_id,
                    font_fingerprint_sha256,
                    font_size_emu,
                    line_height_emu,
                } => {
                    if font_resource_id != &input.font.resource_id
                        || font_fingerprint_sha256 != &input.font.fingerprint_sha256
                    {
                        bail!("SharedResolved text is not bound to the packet explicit font");
                    }
                    if *font_size_emu <= 0 || *line_height_emu <= 0 {
                        bail!("SharedResolved text has non-positive font metrics");
                    }

                    let text_bounds = node.text_bounds.unwrap_or(node.bounds);
                    let base_x = checked_sub(
                        text_bounds.x.get(),
                        node.bounds.x.get(),
                        "text bounds x offset",
                    )?;
                    let base_y = checked_sub(
                        text_bounds.y.get(),
                        node.bounds.y.get(),
                        "text bounds y offset",
                    )?;
                    let mut line_top = 0_i64;
                    let mut node_mapped = false;

                    for line in &layout.lines {
                        summary.shared_resolved_line_count += 1;
                        let current_line_top = line_top;
                        line_top = checked_add(line_top, line.line_height_emu, "text line top")?;

                        if line.text.is_empty() {
                            *summary
                                .shared_resolved_line_outcome_counts
                                .entry("empty_text".into())
                                .or_default() += 1;
                            continue;
                        }
                        if !line.spans.is_empty() {
                            summary.shaped_span_line_count += 1;
                            *summary
                                .shared_resolved_line_outcome_counts
                                .entry("shaped_span".into())
                                .or_default() += 1;
                            residual_reasons
                                .entry(resolved_node_id)
                                .or_default()
                                .insert("shaped_span".into());
                            text_residual_reasons
                                .entry(resolved_node_id)
                                .or_default()
                                .insert("shaped_span".into());
                            continue;
                        }
                        let Some(shaping) = &line.shaping else {
                            summary.missing_shaping_line_count += 1;
                            *summary
                                .shared_resolved_line_outcome_counts
                                .entry("missing_shaping".into())
                                .or_default() += 1;
                            residual_reasons
                                .entry(resolved_node_id)
                                .or_default()
                                .insert("missing_shaping".into());
                            text_residual_reasons
                                .entry(resolved_node_id)
                                .or_default()
                                .insert("missing_shaping".into());
                            continue;
                        };
                        if shaping.units_per_em == 0 {
                            bail!("SharedResolved shaped line has zero units_per_em");
                        }
                        if shaping.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
                            bail!("SharedResolved shaped line contains a missing glyph");
                        }
                        if shaping.environment.face_index != input.font.face_index
                            || shaping.environment.layout.font_set_fingerprint
                                != input.font.fingerprint_sha256
                            || shaping.environment.layout.resource_fingerprint
                                != input.font.resource_id
                        {
                            bail!("SharedResolved shaping environment differs from packet font");
                        }

                        let fill_rgb = match color_range_disposition(
                            &text.typography,
                            line.scalar_start,
                            line.scalar_end,
                        ) {
                            ColorRangeDisposition::Resolved(color) => color,
                            disposition => {
                                summary.unresolved_text_color_line_count += 1;
                                let tag = format!(
                                    "unresolved_text_color:{}",
                                    disposition.code()
                                );
                                *summary
                                    .shared_resolved_line_outcome_counts
                                    .entry(tag.clone())
                                    .or_default() += 1;
                                residual_reasons
                                    .entry(resolved_node_id)
                                    .or_default()
                                    .insert(tag.clone());
                                text_residual_reasons
                                    .entry(resolved_node_id)
                                    .or_default()
                                    .insert(tag);
                                continue;
                            }
                        };

                        let baseline_x = checked_add(
                            base_x,
                            line.x_offset_emu,
                            "text baseline x",
                        )?;
                        let baseline_y = checked_add(
                            checked_add(
                                checked_add(
                                    base_y,
                                    layout.vertical_offset_emu,
                                    "text vertical offset",
                                )?,
                                current_line_top,
                                "text line vertical placement",
                            )?,
                            shaping.environment.font_size_emu.get(),
                            "text baseline y",
                        )?;

                        used_glyph_ids.extend(shaping.glyphs.iter().map(|glyph| glyph.glyph_id));
                        text_runs.push(FixedTextRun {
                            node_id: resolved_node_id,
                            scalar_base: line.scalar_start,
                            logical_text: line.text.clone(),
                            shaped: BoundedShapedText {
                                environment: shaping.environment.clone(),
                                units_per_em: shaping.units_per_em,
                                glyphs: shaping.glyphs.clone(),
                                total_x_advance: LengthEmu::new(line.measured_width_emu),
                            },
                            baseline_x: LengthEmu::new(baseline_x),
                            baseline_y: LengthEmu::new(baseline_y),
                            fill_rgb,
                        });
                        summary.mapped_text_run_count += 1;
                        *summary
                            .shared_resolved_line_outcome_counts
                            .entry("emitted".into())
                            .or_default() += 1;
                        node_mapped = true;
                    }

                    if node_mapped {
                        mapped_text_nodes.insert(resolved_node_id);
                        mapped_resource_nodes.insert(resolved_node_id);
                    } else if !text_residual_reasons.contains_key(&resolved_node_id) {
                        let tag = if layout.lines.is_empty() {
                            "shared_resolved:no_lines"
                        } else if layout.lines.iter().all(|line| line.text.is_empty()) {
                            "shared_resolved:empty_only"
                        } else {
                            "shared_resolved:no_emittable_run"
                        };
                        residual_reasons
                            .entry(resolved_node_id)
                            .or_default()
                            .insert(tag.into());
                        text_residual_reasons
                            .entry(resolved_node_id)
                            .or_default()
                            .insert(tag.into());
                    }
                }
            }
        }
    }
    summary.mapped_text_node_count = mapped_text_nodes.len();
    summary.mapped_resource_node_count = mapped_resource_nodes.len();
    summary.text_node_count = text_node_ids.len();

    for node_id in &text_node_ids {
        let Some(reasons) = text_residual_reasons.get(node_id) else {
            continue;
        };
        let signature = reasons.iter().cloned().collect::<Vec<_>>().join("+");
        if mapped_text_nodes.contains(node_id) {
            *summary
                .text_partial_residual_signature_counts
                .entry(signature)
                .or_default() += 1;
        } else {
            *summary
                .text_resource_residual_signature_counts
                .entry(signature)
                .or_default() += 1;
            let cooccurrence = text_cooccurrence_by_node
                .get(node_id)
                .cloned()
                .unwrap_or_else(|| "unknown".into());
            *summary
                .text_resource_residual_cooccurrence_counts
                .entry(cooccurrence)
                .or_default() += 1;
            let lane = text_projection_lane_by_node
                .get(node_id)
                .cloned()
                .unwrap_or_else(|| "unknown".into());
            *summary
                .text_resource_residual_projection_lane_counts
                .entry(lane.clone())
                .or_default() += 1;
            *summary
                .text_resource_residual_signature_lane_counts
                .entry(format!("{signature}@{lane}"))
                .or_default() += 1;
            summary.text_resource_residual_node_count += 1;
        }
    }
    if mapped_text_nodes.len() + summary.text_resource_residual_node_count != text_node_ids.len() {
        bail!("current Viewer text-resource partition does not cover every text node exactly once");
    }
    let line_outcome_total = summary
        .shared_resolved_line_outcome_counts
        .values()
        .copied()
        .sum::<usize>();
    if line_outcome_total != summary.shared_resolved_line_count {
        bail!("current Viewer SharedResolved line outcomes do not partition every line exactly once");
    }

    for page in &input.pages {
        for node in &page.nodes {
            let resolved_node_id = resolved_output_node_id_v1(node)?;
            if mapped_resource_nodes.contains(&resolved_node_id) {
                continue;
            }
            let signature = residual_reasons
                .get(&resolved_node_id)
                .filter(|reasons| !reasons.is_empty())
                .map(|reasons| reasons.iter().cloned().collect::<Vec<_>>().join("+"))
                .unwrap_or_else(|| {
                    if node.text.is_some() {
                        "text:no_supported_text_resource".into()
                    } else if node.decorative_border.is_some() {
                        "decorative_border".into()
                    } else {
                        "geometry_only".into()
                    }
                });
            *summary
                .residual_node_signature_counts
                .entry(signature)
                .or_default() += 1;

            let lane = node
                .projected_scene_instance
                .as_ref()
                .and_then(|instance| instance.projection_kind.as_deref())
                .unwrap_or("base")
                .to_owned();
            *summary
                .residual_projection_lane_counts
                .entry(lane.clone())
                .or_default() += 1;
            *summary
                .residual_signature_lane_counts
                .entry(format!("{signature}@{lane}"))
                .or_default() += 1;
        }
    }
    let residual_total = summary
        .residual_node_signature_counts
        .values()
        .copied()
        .sum::<usize>();
    if residual_total + mapped_resource_nodes.len() != actual_node_count {
        bail!("current Viewer residual partition does not cover every node exactly once");
    }

    let image_resources = image_uses
        .into_iter()
        .map(|(resource_id, node_ids)| {
            let asset = image_assets
                .get(&resource_id)
                .copied()
                .expect("validated image use must have an exact asset");
            FixedImageResource {
                resource_id,
                mime: asset.mime.clone(),
                node_ids,
                bytes: asset.bytes.clone(),
            }
        })
        .collect::<Vec<_>>();

    let (font_plan, fonts) = if text_runs.is_empty() {
        (None, Vec::new())
    } else {
        let embedding = read_opentype_embedding_flags(&input.font.bytes, input.font.face_index)
            .context("read current Viewer font embedding flags")?;
        let identity = FontIdentity {
            fingerprint_sha256: input.font.fingerprint_sha256.clone(),
            face_index: input.font.face_index,
        };
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.preferred_embedding = PreferredEmbedding::Full;
        let plan = plan_output_fonts(
            &profile,
            vec![OutputFontRequest {
                source: identity.clone(),
                source_resource: Some(ExplicitFontResource {
                    identity: identity.clone(),
                    bytes: &input.font.bytes,
                    embedding,
                }),
                fallback_resource: None,
                used_glyph_ids,
            }],
        );
        if !plan.can_serialize() {
            let codes = plan
                .blockers
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("current Viewer font cannot be embedded: {codes}");
        }
        (
            Some(plan),
            vec![FixedFontResource {
                identity,
                bytes: input.font.bytes.clone(),
            }],
        )
    };

    let scene = BoundedResolvedScene {
        environment: BoundedLayoutEnvironment {
            engine_revision: "chaptera.current-viewer-fixed-pdf.v1".into(),
            font_set_fingerprint: input.font.fingerprint_sha256.clone(),
            resource_fingerprint: input.font.resource_id.clone(),
        },
        surfaces,
        nodes,
        origin_mapping,
        diagnostics: Vec::new(),
    };
    let request = PacketRenderRequest {
        protocol_version: RENDER_REQUEST_VERSION,
        binding: input.binding.clone(),
        scene,
        resources: FixedPdfResources {
            node_paints,
            images: image_resources,
            font_plan,
            fonts,
            text_runs,
        },
        target: PdfTargetProfile::basic_geometry_v0_1(),
    };

    let envelope = MaterializedEnvelope {
        protocol_version: OUTPUT_VERSION,
        binding: input.binding,
        mapping: summary,
        request,
    };
    println!("{}", serde_json::to_string(&envelope)?);
    Ok(())
}
