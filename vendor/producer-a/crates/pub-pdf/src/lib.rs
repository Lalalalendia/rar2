//! Deterministic source-free PDF backend for the bounded resolved scene.
//!
//! This crate consumes only resolved physical geometry plus explicit render
//! resources and a target profile. It deliberately has no dependency on PUB
//! parser/raw crates and does not reconstruct authoring semantics.
//!
//! The current v0.1 slice closes page structure, explicit solid rectangle
//! paint, exact PNG/JPEG plus single-frame GIF placement, and bounded resolved
//! glyph text with explicit OutputFontPlan embed-full resources. Image crop/inner
//! transforms, animated GIF, broader font materializations, and richer vector
//! primitives remain explicit future capabilities under FIXED-RENDER-01.

mod text;

pub use text::{FixedFontResource, FixedTextRun, PdfTextPreparationError};

use image::{AnimationDecoder, ImageFormat};
use pub_layout::{BoundedResolvedScene, ResolvedPhysicalNode, ResolvedSurface};
use pub_model::{Affine2D, CanonicalId, NodeId, PageId, ResourceId};
use pub_output::{FontIdentity, OutputFontPlan};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Cursor;

pub const PDF_RENDER_SCHEMA_V0_1: &str = "fixed-pdf-v0.1";
pub const PDF_RENDERER_REVISION_V0_1: &str = "pub-pdf-v0.1";
const EMU_PER_POINT: i64 = 12_700;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfTargetProfile {
    pub renderer_revision: String,
    pub profile: String,
}

impl PdfTargetProfile {
    pub fn basic_geometry_v0_1() -> Self {
        Self {
            renderer_revision: PDF_RENDERER_REVISION_V0_1.into(),
            profile: "basic-fixed-geometry".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedStroke {
    pub rgb: [u8; 3],
    pub width_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedNodePaint {
    pub node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<FixedStroke>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedImagePlacement {
    pub node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_rotation_degrees: Option<i16>,
    #[serde(default)]
    pub source_window_present: bool,
    #[serde(default)]
    pub recolor_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedImageResource {
    pub resource_id: ResourceId,
    pub mime: String,
    /// True only for exact admitted embedded image bytes. Derived previews are
    /// useful Viewer evidence but do not inherit exact-image alpha authority.
    #[serde(default)]
    pub source_exact: bool,
    pub node_ids: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<FixedImagePlacement>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FixedPdfResources {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_paints: Vec<FixedNodePaint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<FixedImageResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_plan: Option<OutputFontPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<FixedFontResource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_runs: Vec<FixedTextRun>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfRenderDisposition {
    Painted,
    Partial,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfPageReport {
    pub origin: PageId,
    pub width_emu: i64,
    pub height_emu: i64,
    pub media_box_points: [String; 4],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfNodeReport {
    pub origin: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_origin: Option<PageId>,
    pub disposition: PdfRenderDisposition,
    pub code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfDiagnosticSeverity {
    FidelityWarning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfDiagnostic {
    pub code: String,
    pub severity: PdfDiagnosticSeverity,
    pub origin: CanonicalId,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfRenderCapabilities {
    pub page_bounds: bool,
    pub explicit_solid_rectangles: bool,
    pub resolved_text: bool,
    pub images: bool,
    pub richer_vectors_effects: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfRenderReport {
    pub schema_version: String,
    pub target: PdfTargetProfile,
    pub capabilities: PdfRenderCapabilities,
    pub pages: Vec<PdfPageReport>,
    pub nodes: Vec<PdfNodeReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PdfDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfRenderOutput {
    pub bytes: Vec<u8>,
    pub report: PdfRenderReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfRenderError {
    NonPositivePageSize {
        origin: PageId,
        width_emu: i64,
        height_emu: i64,
    },
    DuplicatePaint {
        node_id: NodeId,
    },
    PaintReferencesMissingNode {
        node_id: NodeId,
    },
    DuplicateImageResource {
        resource_id: ResourceId,
    },
    DuplicateImageUse {
        node_id: NodeId,
    },
    DuplicateImagePlacement {
        node_id: NodeId,
    },
    ImagePlacementReferencesMissingUse {
        resource_id: ResourceId,
        node_id: NodeId,
    },
    ImageReferencesMissingNode {
        resource_id: ResourceId,
        node_id: NodeId,
    },
    ImageDecodeFailed {
        resource_id: ResourceId,
        message: String,
    },
    Text(PdfTextPreparationError),
}

impl fmt::Display for PdfRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositivePageSize {
                origin,
                width_emu,
                height_emu,
            } => write!(
                f,
                "page {origin:?} has non-positive size {width_emu} x {height_emu} EMU"
            ),
            Self::DuplicatePaint { node_id } => {
                write!(f, "duplicate explicit paint for node {node_id:?}")
            }
            Self::PaintReferencesMissingNode { node_id } => {
                write!(
                    f,
                    "explicit paint references missing resolved node {node_id:?}"
                )
            }
            Self::DuplicateImageResource { resource_id } => {
                write!(f, "duplicate fixed image resource {resource_id:?}")
            }
            Self::DuplicateImageUse { node_id } => {
                write!(
                    f,
                    "more than one fixed image resource references node {node_id:?}"
                )
            }
            Self::DuplicateImagePlacement { node_id } => {
                write!(f, "duplicate fixed image placement for node {node_id:?}")
            }
            Self::ImagePlacementReferencesMissingUse {
                resource_id,
                node_id,
            } => write!(
                f,
                "fixed image placement for resource {resource_id:?} references non-image-use node {node_id:?}"
            ),
            Self::ImageReferencesMissingNode {
                resource_id,
                node_id,
            } => write!(
                f,
                "fixed image resource {resource_id:?} references missing resolved node {node_id:?}"
            ),
            Self::ImageDecodeFailed {
                resource_id,
                message,
            } => write!(
                f,
                "could not decode fixed image resource {resource_id:?}: {message}"
            ),
            Self::Text(error) => write!(f, "resolved text preparation failed: {error}"),
        }
    }
}

impl std::error::Error for PdfRenderError {}

impl From<PdfTextPreparationError> for PdfRenderError {
    fn from(value: PdfTextPreparationError) -> Self {
        Self::Text(value)
    }
}

/// Render the current bounded fixed-PDF subset.
///
/// v0.1 supports exact page creation, explicit fill/stroke rectangle paint,
/// exact opaque PNG/JPEG image XObjects, and bounded resolved glyph placement
/// using explicit embed-full OutputFontPlan resources. Unsupported or partial
/// nodes remain visible in the side-channel report rather than disappearing
/// silently.
pub fn render_bounded_pdf(
    scene: &BoundedResolvedScene,
    resources: &FixedPdfResources,
    target: &PdfTargetProfile,
) -> Result<PdfRenderOutput, PdfRenderError> {
    let mut surfaces = scene.surfaces.clone();
    surfaces.sort_by_key(|surface| surface.origin);

    for surface in &surfaces {
        if surface.size.width.get() <= 0 || surface.size.height.get() <= 0 {
            return Err(PdfRenderError::NonPositivePageSize {
                origin: surface.origin,
                width_emu: surface.size.width.get(),
                height_emu: surface.size.height.get(),
            });
        }
    }

    let mut nodes = scene.nodes.clone();
    nodes.sort_by_key(|node| node.origin);

    let node_ids = nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut paint_by_node = BTreeMap::new();
    for paint in &resources.node_paints {
        if !node_ids.contains(&paint.node_id) {
            return Err(PdfRenderError::PaintReferencesMissingNode {
                node_id: paint.node_id,
            });
        }
        if paint_by_node.insert(paint.node_id, paint).is_some() {
            return Err(PdfRenderError::DuplicatePaint {
                node_id: paint.node_id,
            });
        }
    }

    let mut prepared_images = BTreeMap::<ResourceId, PreparedImage>::new();
    let mut image_by_node = BTreeMap::<NodeId, ResourceId>::new();
    let mut image_placement_by_node = BTreeMap::<NodeId, &FixedImagePlacement>::new();
    for image in &resources.images {
        if prepared_images.contains_key(&image.resource_id) {
            return Err(PdfRenderError::DuplicateImageResource {
                resource_id: image.resource_id,
            });
        }

        let prepared = prepare_image(image)?;
        prepared_images.insert(image.resource_id, prepared);

        let mut uses = image.node_ids.clone();
        uses.sort();
        uses.dedup();
        for node_id in &uses {
            if !node_ids.contains(node_id) {
                return Err(PdfRenderError::ImageReferencesMissingNode {
                    resource_id: image.resource_id,
                    node_id: *node_id,
                });
            }
            if image_by_node.insert(*node_id, image.resource_id).is_some() {
                return Err(PdfRenderError::DuplicateImageUse { node_id: *node_id });
            }
        }
        let use_ids = uses.into_iter().collect::<BTreeSet<_>>();
        for placement in &image.placements {
            if !use_ids.contains(&placement.node_id) {
                return Err(PdfRenderError::ImagePlacementReferencesMissingUse {
                    resource_id: image.resource_id,
                    node_id: placement.node_id,
                });
            }
            if image_placement_by_node
                .insert(placement.node_id, placement)
                .is_some()
            {
                return Err(PdfRenderError::DuplicateImagePlacement {
                    node_id: placement.node_id,
                });
            }
        }
    }

    let prepared_text = text::prepare_text_resources(
        resources.font_plan.as_ref(),
        &resources.fonts,
        &resources.text_runs,
        &node_ids,
    )?;

    let page_by_parent = surfaces
        .iter()
        .map(|surface| (surface.origin.into_canonical(), surface))
        .collect::<BTreeMap<_, _>>();

    let mut content_by_page = surfaces
        .iter()
        .map(|surface| (surface.origin, String::new()))
        .collect::<BTreeMap<_, _>>();
    let mut reports = Vec::with_capacity(nodes.len());
    let mut diagnostics = Vec::new();
    let mut image_resources_by_page = surfaces
        .iter()
        .map(|surface| (surface.origin, BTreeSet::<ResourceId>::new()))
        .collect::<BTreeMap<_, _>>();
    let mut font_resources_by_page = surfaces
        .iter()
        .map(|surface| (surface.origin, BTreeSet::<FontIdentity>::new()))
        .collect::<BTreeMap<_, _>>();

    for node in &nodes {
        let Some(surface) = page_by_parent.get(&node.parent_origin).copied() else {
            unsupported_node(
                node,
                None,
                "pdf.node.page_ownership_unsupported",
                "resolved node is not directly owned by a page in the bounded PDF v0.1 slice",
                &mut reports,
                &mut diagnostics,
            );
            continue;
        };

        if node.transform != Affine2D::identity() {
            unsupported_node(
                node,
                Some(surface.origin),
                "pdf.node.transform_unsupported",
                "non-identity node transform is outside the bounded PDF v0.1 rectangle/image/text slice",
                &mut reports,
                &mut diagnostics,
            );
            continue;
        }

        let paint = paint_by_node.get(&node.origin).copied();
        let image_resource_id = image_by_node.get(&node.origin).copied();
        let image_placement = image_placement_by_node.get(&node.origin).copied();

        let valid_paint = paint.filter(|paint| {
            paint.fill_rgb.is_some()
                || paint
                    .stroke
                    .as_ref()
                    .is_some_and(|stroke| stroke.width_emu > 0)
        });

        if paint.is_some() && valid_paint.is_none() {
            diagnostics.push(PdfDiagnostic {
                code: "pdf.node.paint_empty".into(),
                severity: PdfDiagnosticSeverity::FidelityWarning,
                origin: node.origin.into_canonical(),
                message: "explicit paint has neither a fill nor a positive-width stroke".into(),
            });
        }

        let content = content_by_page
            .get_mut(&surface.origin)
            .expect("page content buffer must exist");

        if let Some(paint) = valid_paint {
            append_rectangle(content, node, paint);
        }

        let mut image_painted = false;
        let mut image_partial = false;
        if let Some(resource_id) = image_resource_id {
            let prepared = prepared_images
                .get(&resource_id)
                .expect("validated image resource must exist");
            match prepared {
                PreparedImage::Rgb { .. } => {
                    match image_placement {
                        Some(placement)
                            if placement.source_window_present || placement.recolor_present =>
                        {
                            diagnostics.push(PdfDiagnostic {
                                code: "pdf.image.placement_combination_unsupported".into(),
                                severity: PdfDiagnosticSeverity::FidelityWarning,
                                origin: node.origin.into_canonical(),
                                message: "cardinal image-content rotation combined with crop or recolor is outside the bounded fixed-PDF slice".into(),
                            });
                            image_partial = true;
                        }
                        Some(placement) => match placement.content_rotation_degrees.unwrap_or(0) {
                            0 => {
                                append_image(content, node, resource_id);
                                image_painted = true;
                            }
                            rotation @ (90 | 180 | 270) => {
                                append_image_cardinal(content, node, resource_id, rotation);
                                image_painted = true;
                            }
                            _ => {
                                diagnostics.push(PdfDiagnostic {
                                    code: "pdf.image.content_rotation_unsupported".into(),
                                    severity: PdfDiagnosticSeverity::FidelityWarning,
                                    origin: node.origin.into_canonical(),
                                    message: "image content rotation is not one of the bounded cardinal angles".into(),
                                });
                                image_partial = true;
                            }
                        },
                        None => {
                            append_image(content, node, resource_id);
                            image_painted = true;
                        }
                    }
                    if image_painted {
                        image_resources_by_page
                            .get_mut(&surface.origin)
                            .expect("page image resource set must exist")
                            .insert(resource_id);
                    }
                }
                PreparedImage::Unsupported { code, message } => {
                    diagnostics.push(PdfDiagnostic {
                        code: code.clone(),
                        severity: PdfDiagnosticSeverity::FidelityWarning,
                        origin: node.origin.into_canonical(),
                        message: message.clone(),
                    });
                    image_partial = true;
                }
            }
        }

        let mut text_painted = false;
        let mut text_partial = false;
        if let Some(runs) = prepared_text.runs_by_node.get(&node.origin) {
            for run in runs {
                match run {
                    text::PreparedTextRun::Painted { .. } => {
                        if let Some(identity) = text::append_text(
                            content,
                            node.bounds.x.get(),
                            node.bounds.y.get(),
                            run,
                        ) {
                            font_resources_by_page
                                .get_mut(&surface.origin)
                                .expect("page font resource set must exist")
                                .insert(identity);
                            text_painted = true;
                        }
                    }
                    text::PreparedTextRun::Unsupported { code, message } => {
                        diagnostics.push(PdfDiagnostic {
                            code: code.clone(),
                            severity: PdfDiagnosticSeverity::FidelityWarning,
                            origin: node.origin.into_canonical(),
                            message: message.clone(),
                        });
                        text_partial = true;
                    }
                }
            }
        }

        let any_partial = image_partial || text_partial;
        let painted_code = match (valid_paint.is_some(), image_painted, text_painted) {
            (true, true, true) => {
                Some("pdf.node.painted_explicit_rectangle_exact_image_and_resolved_text")
            }
            (true, true, false) => Some("pdf.node.painted_explicit_rectangle_and_exact_image"),
            (true, false, true) => Some("pdf.node.painted_explicit_rectangle_and_resolved_text"),
            (false, true, true) => Some("pdf.node.painted_exact_image_and_resolved_text"),
            (true, false, false) => Some("pdf.node.painted_explicit_rectangle"),
            (false, true, false) => Some("pdf.node.painted_exact_image"),
            (false, false, true) => Some("pdf.node.painted_resolved_text"),
            (false, false, false) => None,
        };

        match (painted_code, any_partial) {
            (Some(_), true) => reports.push(PdfNodeReport {
                origin: node.origin,
                page_origin: Some(surface.origin),
                disposition: PdfRenderDisposition::Partial,
                code: "pdf.node.painted_with_unsupported_resource".into(),
            }),
            (Some(code), false) => reports.push(PdfNodeReport {
                origin: node.origin,
                page_origin: Some(surface.origin),
                disposition: PdfRenderDisposition::Painted,
                code: code.into(),
            }),
            (None, true) => reports.push(PdfNodeReport {
                origin: node.origin,
                page_origin: Some(surface.origin),
                disposition: PdfRenderDisposition::Unsupported,
                code: "pdf.node.resource_unsupported".into(),
            }),
            (None, false) => unsupported_node(
                node,
                Some(surface.origin),
                "pdf.node.resource_missing",
                "resolved node has neither explicit fixed-output paint, exact image, nor resolved text resources",
                &mut reports,
                &mut diagnostics,
            ),
        }
    }
    reports.sort_by_key(|node| node.origin);
    diagnostics.sort_by(|left, right| (left.origin, &left.code).cmp(&(right.origin, &right.code)));

    let page_reports = surfaces.iter().map(page_report).collect::<Vec<_>>();

    let bytes = write_pdf(
        &surfaces,
        &content_by_page,
        &prepared_images,
        &image_resources_by_page,
        &prepared_text.fonts,
        &font_resources_by_page,
    );

    Ok(PdfRenderOutput {
        bytes,
        report: PdfRenderReport {
            schema_version: PDF_RENDER_SCHEMA_V0_1.into(),
            target: target.clone(),
            capabilities: PdfRenderCapabilities {
                page_bounds: true,
                explicit_solid_rectangles: true,
                resolved_text: true,
                images: true,
                richer_vectors_effects: false,
            },
            pages: page_reports,
            nodes: reports,
            diagnostics,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreparedImage {
    Rgb {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
        alpha: Option<Vec<u8>>,
    },
    Unsupported {
        code: String,
        message: String,
    },
}

fn prepare_image(image: &FixedImageResource) -> Result<PreparedImage, PdfRenderError> {
    let format = match image.mime.as_str() {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" if image.source_exact => ImageFormat::Gif,
        "image/gif" => {
            return Ok(PreparedImage::Unsupported {
                code: "pdf.image.preview_gif_unsupported".into(),
                message: "derived GIF preview is outside the exact-image PDF slice".into(),
            });
        }
        other => {
            return Ok(PreparedImage::Unsupported {
                code: "pdf.image.mime_unsupported".into(),
                message: format!(
                    "exact image MIME {other:?} is outside the bounded PNG/JPEG/single-frame-GIF PDF slice"
                ),
            });
        }
    };

    let decoded = if format == ImageFormat::Gif {
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(image.bytes.as_slice()))
            .map_err(|error| PdfRenderError::ImageDecodeFailed {
                resource_id: image.resource_id,
                message: error.to_string(),
            })?;
        let frames = decoder
            .into_frames()
            .collect_frames()
            .map_err(|error| PdfRenderError::ImageDecodeFailed {
                resource_id: image.resource_id,
                message: error.to_string(),
            })?;
        if frames.len() != 1 {
            return Ok(PreparedImage::Unsupported {
                code: "pdf.image.gif_animation_unsupported".into(),
                message: format!(
                    "exact GIF contains {} frames; bounded PDF supports exactly one",
                    frames.len()
                ),
            });
        }
        image::DynamicImage::ImageRgba8(
            frames
                .into_iter()
                .next()
                .expect("single-frame GIF count already checked")
                .into_buffer(),
        )
    } else {
        image::load_from_memory_with_format(&image.bytes, format).map_err(|error| {
            PdfRenderError::ImageDecodeFailed {
                resource_id: image.resource_id,
                message: error.to_string(),
            }
        })?
    };
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    let mut alpha = Vec::with_capacity(width as usize * height as usize);
    let mut has_alpha = false;
    for pixel in rgba.pixels() {
        rgb.extend_from_slice(&pixel.0[..3]);
        alpha.push(pixel.0[3]);
        has_alpha |= pixel.0[3] != 255;
    }
    if has_alpha && !image.source_exact {
        return Ok(PreparedImage::Unsupported {
            code: "pdf.image.preview_alpha_unsupported".into(),
            message: "alpha-bearing derived preview is outside the exact embedded-image PDF slice"
                .into(),
        });
    }
    Ok(PreparedImage::Rgb {
        width,
        height,
        bytes: rgb,
        alpha: has_alpha.then_some(alpha),
    })
}

fn image_name(resource_id: ResourceId) -> String {
    let canonical = resource_id.as_canonical().to_string().replace('-', "");
    format!("Im{}", &canonical[..16])
}

fn append_image(content: &mut String, node: &ResolvedPhysicalNode, resource_id: ResourceId) {
    let x = format_points(node.bounds.x.get());
    let width = format_points(node.bounds.width.get());
    let height = format_points(node.bounds.height.get());
    let y_plus_height = format_points(node.bounds.y.get() + node.bounds.height.get());
    content.push_str("q\n");
    content.push_str(&format!(
        "{width} 0 0 -{height} {x} {y_plus_height} cm\n/{} Do\n",
        image_name(resource_id)
    ));
    content.push_str("Q\n");
}

fn append_image_cardinal(
    content: &mut String,
    node: &ResolvedPhysicalNode,
    resource_id: ResourceId,
    rotation_degrees: i16,
) {
    let x = format_points(node.bounds.x.get());
    let y = format_points(node.bounds.y.get());
    let width = format_points(node.bounds.width.get());
    let height = format_points(node.bounds.height.get());
    let x_plus_width = format_points(node.bounds.x.get() + node.bounds.width.get());
    let y_plus_height = format_points(node.bounds.y.get() + node.bounds.height.get());
    let matrix = match rotation_degrees {
        90 => format!("0 {height} {width} 0 {x} {y}"),
        180 => format!("-{width} 0 0 {height} {x_plus_width} {y}"),
        270 => format!("0 -{height} -{width} 0 {x_plus_width} {y_plus_height}"),
        _ => unreachable!("cardinal image rotation validated before emission"),
    };
    content.push_str("q\n");
    content.push_str(&format!("{matrix} cm\n/{} Do\n", image_name(resource_id)));
    content.push_str("Q\n");
}

fn unsupported_node(
    node: &ResolvedPhysicalNode,
    page_origin: Option<PageId>,
    code: &str,
    message: &str,
    reports: &mut Vec<PdfNodeReport>,
    diagnostics: &mut Vec<PdfDiagnostic>,
) {
    reports.push(PdfNodeReport {
        origin: node.origin,
        page_origin,
        disposition: PdfRenderDisposition::Unsupported,
        code: code.into(),
    });
    diagnostics.push(PdfDiagnostic {
        code: code.into(),
        severity: PdfDiagnosticSeverity::FidelityWarning,
        origin: node.origin.into_canonical(),
        message: message.into(),
    });
}

fn page_report(surface: &ResolvedSurface) -> PdfPageReport {
    PdfPageReport {
        origin: surface.origin,
        width_emu: surface.size.width.get(),
        height_emu: surface.size.height.get(),
        media_box_points: [
            "0".into(),
            "0".into(),
            format_points(surface.size.width.get()),
            format_points(surface.size.height.get()),
        ],
    }
}

fn append_rectangle(content: &mut String, node: &ResolvedPhysicalNode, paint: &FixedNodePaint) {
    content.push_str("q\n");

    let mut has_fill = false;
    if let Some(rgb) = paint.fill_rgb {
        content.push_str(&format!(
            "{} {} {} rg\n",
            format_rgb(rgb[0]),
            format_rgb(rgb[1]),
            format_rgb(rgb[2])
        ));
        has_fill = true;
    }

    let mut has_stroke = false;
    if let Some(stroke) = &paint.stroke
        && stroke.width_emu > 0
    {
        content.push_str(&format!(
            "{} {} {} RG\n{} w\n",
            format_rgb(stroke.rgb[0]),
            format_rgb(stroke.rgb[1]),
            format_rgb(stroke.rgb[2]),
            format_points(stroke.width_emu)
        ));
        has_stroke = true;
    }

    content.push_str(&format!(
        "{} {} {} {} re\n",
        format_points(node.bounds.x.get()),
        format_points(node.bounds.y.get()),
        format_points(node.bounds.width.get()),
        format_points(node.bounds.height.get())
    ));

    content.push_str(match (has_fill, has_stroke) {
        (true, true) => "B\n",
        (true, false) => "f\n",
        (false, true) => "S\n",
        (false, false) => unreachable!("paint emptiness is checked before rectangle emission"),
    });
    content.push_str("Q\n");
}

fn write_pdf(
    surfaces: &[ResolvedSurface],
    content_by_page: &BTreeMap<PageId, String>,
    prepared_images: &BTreeMap<ResourceId, PreparedImage>,
    image_resources_by_page: &BTreeMap<PageId, BTreeSet<ResourceId>>,
    prepared_fonts: &BTreeMap<FontIdentity, text::PreparedFont>,
    font_resources_by_page: &BTreeMap<PageId, BTreeSet<FontIdentity>>,
) -> Vec<u8> {
    let image_ids = prepared_images
        .iter()
        .filter_map(|(resource_id, image)| {
            matches!(image, PreparedImage::Rgb { .. }).then_some(*resource_id)
        })
        .collect::<Vec<_>>();
    let font_ids = prepared_fonts.keys().cloned().collect::<Vec<_>>();

    let first_image_object_id = 3 + surfaces.len() * 2;
    let mut next_image_object_id = first_image_object_id;
    let mut image_object_ids = BTreeMap::<ResourceId, (usize, Option<usize>)>::new();
    for resource_id in &image_ids {
        let rgb_object_id = next_image_object_id;
        next_image_object_id += 1;
        let alpha_object_id = match &prepared_images[resource_id] {
            PreparedImage::Rgb { alpha: Some(_), .. } => {
                let object_id = next_image_object_id;
                next_image_object_id += 1;
                Some(object_id)
            }
            PreparedImage::Rgb { alpha: None, .. } => None,
            PreparedImage::Unsupported { .. } => {
                unreachable!("image object list contains only prepared RGB images")
            }
        };
        image_object_ids.insert(*resource_id, (rgb_object_id, alpha_object_id));
    }

    let first_font_object_id = next_image_object_id;
    let font_object_ids = font_ids
        .iter()
        .enumerate()
        .map(|(index, identity)| {
            let first = first_font_object_id + index * 5;
            (
                identity.clone(),
                [first, first + 1, first + 2, first + 3, first + 4],
            )
        })
        .collect::<BTreeMap<_, _>>();

    let image_object_count = image_object_ids
        .values()
        .map(|(_, alpha_object_id)| if alpha_object_id.is_some() { 2 } else { 1 })
        .sum::<usize>();
    let mut objects = Vec::<Vec<u8>>::with_capacity(
        2 + surfaces.len() * 2 + image_object_count + font_ids.len() * 5,
    );
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());

    let kids = surfaces
        .iter()
        .enumerate()
        .map(|(index, _)| format!("{} 0 R", 3 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            surfaces.len(),
            kids
        )
        .into_bytes(),
    );

    for (index, surface) in surfaces.iter().enumerate() {
        let content_id = 4 + index * 2;
        let width = format_points(surface.size.width.get());
        let height = format_points(surface.size.height.get());

        let page_image_ids = image_resources_by_page
            .get(&surface.origin)
            .expect("page image resource set must exist");
        let xobjects = page_image_ids
            .iter()
            .map(|resource_id| {
                format!(
                    "/{} {} 0 R",
                    image_name(*resource_id),
                    image_object_ids[resource_id].0
                )
            })
            .collect::<Vec<_>>()
            .join(" ");

        let page_font_ids = font_resources_by_page
            .get(&surface.origin)
            .expect("page font resource set must exist");
        let fonts = page_font_ids
            .iter()
            .map(|identity| {
                format!(
                    "/{} {} 0 R",
                    text::font_resource_name(identity),
                    font_object_ids[identity][4]
                )
            })
            .collect::<Vec<_>>()
            .join(" ");

        let mut resource_sections = Vec::new();
        if !xobjects.is_empty() {
            resource_sections.push(format!("/XObject << {xobjects} >>"));
        }
        if !fonts.is_empty() {
            resource_sections.push(format!("/Font << {fonts} >>"));
        }
        let resources = format!("<< {} >>", resource_sections.join(" "));

        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources {resources} /Contents {content_id} 0 R >>"
            )
            .into_bytes(),
        );

        let body = content_by_page
            .get(&surface.origin)
            .expect("page content buffer must exist");
        let page_height = format_points(surface.size.height.get());
        let transformed = format!("q\n1 0 0 -1 0 {page_height} cm\n{body}Q\n");
        let stream_body = transformed.as_bytes();
        let mut stream = format!("<< /Length {} >>\nstream\n", stream_body.len()).into_bytes();
        stream.extend_from_slice(stream_body);
        stream.extend_from_slice(b"endstream");
        objects.push(stream);
    }

    for resource_id in image_ids {
        let PreparedImage::Rgb {
            width,
            height,
            bytes,
            alpha,
        } = &prepared_images[&resource_id]
        else {
            unreachable!("image object list contains only RGB images")
        };
        let (_, alpha_object_id) = image_object_ids[&resource_id];
        let smask = alpha_object_id
            .map(|object_id| format!(" /SMask {object_id} 0 R"))
            .unwrap_or_default();
        let mut stream = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8{smask} /Length {} >>\nstream\n",
            bytes.len()
        )
        .into_bytes();
        stream.extend_from_slice(bytes);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);

        if let Some(alpha) = alpha {
            let mut mask_stream = format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                alpha.len()
            )
            .into_bytes();
            mask_stream.extend_from_slice(alpha);
            mask_stream.extend_from_slice(b"\nendstream");
            objects.push(mask_stream);
        }
    }

    for identity in font_ids {
        let font = &prepared_fonts[&identity];
        let [
            font_file_id,
            descriptor_id,
            cid_font_id,
            to_unicode_id,
            _type0_id,
        ] = font_object_ids[&identity];

        let mut font_file = format!(
            "<< /Length {} /Length1 {} >>\nstream\n",
            font.bytes.len(),
            font.bytes.len()
        )
        .into_bytes();
        font_file.extend_from_slice(&font.bytes);
        font_file.extend_from_slice(b"\nendstream");
        objects.push(font_file);

        objects.push(
            format!(
                "<< /Type /FontDescriptor /FontName /{} /Flags 32 /FontBBox [-2000 -2000 4000 4000] /ItalicAngle 0 /Ascent 2000 /Descent -1000 /CapHeight 1500 /StemV 80 /FontFile2 {font_file_id} 0 R >>",
                font.base_name
            )
            .into_bytes(),
        );

        objects.push(
            format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {descriptor_id} 0 R /DW 1000 /CIDToGIDMap /Identity >>",
                font.base_name
            )
            .into_bytes(),
        );

        let cmap = text::to_unicode_cmap(font);
        let mut cmap_stream = format!("<< /Length {} >>\nstream\n", cmap.len()).into_bytes();
        cmap_stream.extend_from_slice(&cmap);
        cmap_stream.extend_from_slice(b"endstream");
        objects.push(cmap_stream);

        objects.push(
            format!(
                "<< /Type /Font /Subtype /Type0 /BaseFont /{} /Encoding /Identity-H /DescendantFonts [{cid_font_id} 0 R] /ToUnicode {to_unicode_id} 0 R >>",
                font.base_name
            )
            .into_bytes(),
        );
    }

    assemble_pdf(objects)
}

fn assemble_pdf(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len() + 1);
    offsets.push(0usize);

    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Deterministic EMU -> PDF point decimal conversion.
///
/// Nine fractional digits keep the serialized error below 0.5 nanopt without
/// relying on floating-point formatting.
fn format_points(emu: i64) -> String {
    format_ratio(i128::from(emu), i128::from(EMU_PER_POINT), 9)
}

fn format_rgb(component: u8) -> String {
    format_ratio(i128::from(component), 255, 6)
}

fn format_ratio(numerator: i128, denominator: i128, fractional_digits: u32) -> String {
    debug_assert!(denominator > 0);

    if numerator == 0 {
        return "0".into();
    }

    let negative = numerator < 0;
    let absolute = numerator.abs();
    let scale = 10_i128.pow(fractional_digits);
    let mut integer = absolute / denominator;
    let remainder = absolute % denominator;
    let mut fraction = (remainder * scale + denominator / 2) / denominator;
    if fraction == scale {
        integer += 1;
        fraction = 0;
    }

    let sign = if negative { "-" } else { "" };
    if fraction == 0 {
        return format!("{sign}{integer}");
    }

    let mut fractional = format!("{fraction:0width$}", width = fractional_digits as usize);
    while fractional.ends_with('0') {
        fractional.pop();
    }
    format!("{sign}{integer}.{fractional}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_layout::{BoundedLayoutEnvironment, SceneOriginMapping};
    use pub_model::{LengthEmu, RectEmu, Size2D};

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn page_id(byte: u8) -> PageId {
        PageId::from_canonical(canonical(byte))
    }

    fn node_id(byte: u8) -> NodeId {
        NodeId::from_canonical(canonical(byte))
    }

    fn resource_id(byte: u8) -> ResourceId {
        ResourceId::from_canonical(canonical(byte))
    }

    fn scene() -> BoundedResolvedScene {
        let page_a = page_id(1);
        let page_b = page_id(2);
        let node_a = node_id(10);
        let node_b = node_id(11);
        let node_c = node_id(12);

        BoundedResolvedScene {
            environment: BoundedLayoutEnvironment {
                engine_revision: "test-layout".into(),
                font_set_fingerprint: "fonts:test".into(),
                resource_fingerprint: "resources:test".into(),
            },
            surfaces: vec![
                ResolvedSurface {
                    origin: page_b,
                    size: Size2D::new(LengthEmu::new(7_560_000), LengthEmu::new(10_692_000)),
                    bleed: None,
                    margins: None,
                },
                ResolvedSurface {
                    origin: page_a,
                    size: Size2D::new(LengthEmu::new(7_620_000), LengthEmu::new(9_906_000)),
                    bleed: None,
                    margins: None,
                },
            ],
            nodes: vec![
                ResolvedPhysicalNode {
                    origin: node_c,
                    parent_origin: page_b.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(127_000),
                        LengthEmu::new(254_000),
                        LengthEmu::new(1_270_000),
                        LengthEmu::new(635_000),
                    ),
                    transform: Affine2D::identity(),
                },
                ResolvedPhysicalNode {
                    origin: node_b,
                    parent_origin: page_a.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(381_000),
                        LengthEmu::new(508_000),
                        LengthEmu::new(889_000),
                        LengthEmu::new(762_000),
                    ),
                    transform: Affine2D::identity(),
                },
                ResolvedPhysicalNode {
                    origin: node_a,
                    parent_origin: page_a.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(127_000),
                        LengthEmu::new(254_000),
                        LengthEmu::new(1_270_000),
                        LengthEmu::new(635_000),
                    ),
                    transform: Affine2D::identity(),
                },
            ],
            origin_mapping: vec![
                SceneOriginMapping {
                    authoring_origin: node_a.into_canonical(),
                    resolved_node_origin: node_a,
                },
                SceneOriginMapping {
                    authoring_origin: node_b.into_canonical(),
                    resolved_node_origin: node_b,
                },
                SceneOriginMapping {
                    authoring_origin: node_c.into_canonical(),
                    resolved_node_origin: node_c,
                },
            ],
            diagnostics: Vec::new(),
        }
    }

    fn resources() -> FixedPdfResources {
        FixedPdfResources {
            node_paints: vec![
                FixedNodePaint {
                    node_id: node_id(10),
                    fill_rgb: Some([255, 0, 0]),
                    stroke: None,
                },
                FixedNodePaint {
                    node_id: node_id(11),
                    fill_rgb: None,
                    stroke: Some(FixedStroke {
                        rgb: [0, 0, 255],
                        width_emu: 12_700,
                    }),
                },
            ],
            images: Vec::new(),
            ..FixedPdfResources::default()
        }
    }

    #[test]
    fn deterministic_pdf_has_sorted_pages_exact_media_boxes_and_origin_report() {
        let target = PdfTargetProfile::basic_geometry_v0_1();
        let left = render_bounded_pdf(&scene(), &resources(), &target).unwrap();
        let right = render_bounded_pdf(&scene(), &resources(), &target).unwrap();

        assert_eq!(left, right);
        assert!(left.bytes.starts_with(b"%PDF-1.7"));
        assert_eq!(left.report.pages.len(), 2);
        assert_eq!(left.report.pages[0].origin, page_id(1));
        assert_eq!(
            left.report.pages[0].media_box_points,
            ["0", "0", "600", "780"]
        );
        assert_eq!(left.report.pages[1].origin, page_id(2));
        assert_eq!(
            left.report.pages[1].media_box_points,
            ["0", "0", "595.275590551", "841.88976378"]
        );
        assert_eq!(
            left.report
                .nodes
                .iter()
                .filter(|node| node.disposition == PdfRenderDisposition::Painted)
                .count(),
            2
        );
        assert!(left.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "pdf.node.resource_missing"
                && diagnostic.origin == node_id(12).into_canonical()
        }));
        assert!(left.report.capabilities.resolved_text);
        assert!(left.report.capabilities.images);
    }

    #[test]
    fn emitted_rectangle_coordinates_and_paint_are_stable() {
        let output = render_bounded_pdf(
            &scene(),
            &resources(),
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&output.bytes);

        assert!(text.contains("1 0 0 -1 0 780 cm"));
        assert!(text.contains("1 0 0 rg\n10 20 100 50 re\nf"));
        assert!(text.contains("0 0 1 RG\n1 w\n30 40 70 60 re\nS"));
    }

    #[test]
    fn exact_png_is_one_reused_xobject_for_two_node_uses() {
        let png = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x7b, 0x40, 0xe8, 0xdd, 0x00, 0x00, 0x00, 0x0f, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xc0, 0xf0, 0x9f, 0x01, 0x00, 0x07, 0xff, 0x01, 0xff,
            0x01, 0x7f, 0x89, 0xa7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42,
            0x60, 0x82,
        ];
        let resources = FixedPdfResources {
            node_paints: Vec::new(),
            images: vec![FixedImageResource {
                resource_id: resource_id(42),
                mime: "image/png".into(),
                source_exact: true,
                node_ids: vec![node_id(10), node_id(11)],
                placements: Vec::new(),
                bytes: png,
            }],
            ..FixedPdfResources::default()
        };

        let output = render_bounded_pdf(
            &scene(),
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();

        let text = String::from_utf8_lossy(&output.bytes);
        assert_eq!(text.matches("/Subtype /Image").count(), 1);
        assert_eq!(text.matches(" Do\n").count(), 2);
        assert!(text.contains("/Width 2 /Height 1 /ColorSpace /DeviceRGB"));
        assert_eq!(
            output
                .report
                .nodes
                .iter()
                .filter(|node| node.code == "pdf.node.painted_exact_image")
                .count(),
            2
        );
    }

    #[test]
    fn cardinal_image_content_rotation_uses_fixed_frame_matrix() {
        let png = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x7b, 0x40, 0xe8, 0xdd, 0x00, 0x00, 0x00, 0x0f, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xc0, 0xf0, 0x9f, 0x01, 0x00, 0x07, 0xff, 0x01, 0xff,
            0x01, 0x7f, 0x89, 0xa7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42,
            0x60, 0x82,
        ];
        let resources = FixedPdfResources {
            images: vec![FixedImageResource {
                resource_id: resource_id(42),
                mime: "image/png".into(),
                source_exact: true,
                node_ids: vec![node_id(10)],
                placements: vec![FixedImagePlacement {
                    node_id: node_id(10),
                    content_rotation_degrees: Some(90),
                    source_window_present: false,
                    recolor_present: false,
                }],
                bytes: png,
            }],
            ..FixedPdfResources::default()
        };

        let output = render_bounded_pdf(
            &scene(),
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&output.bytes);
        assert!(text.contains("0 50 100 0 10 20 cm"));
        assert_eq!(
            output
                .report
                .nodes
                .iter()
                .filter(|node| node.code == "pdf.node.painted_exact_image")
                .count(),
            1
        );
    }

    #[test]
    fn exact_rgba_png_uses_soft_mask_and_remains_painted() {
        use std::io::Cursor;

        let mut rgba = image::RgbaImage::new(1, 1);
        rgba.put_pixel(0, 0, image::Rgba([10, 20, 30, 128]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();

        let resources = FixedPdfResources {
            node_paints: Vec::new(),
            images: vec![FixedImageResource {
                resource_id: resource_id(43),
                mime: "image/png".into(),
                source_exact: true,
                node_ids: vec![node_id(10)],
                placements: Vec::new(),
                bytes: encoded.into_inner(),
            }],
            ..FixedPdfResources::default()
        };

        let output = render_bounded_pdf(
            &scene(),
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();

        let text = String::from_utf8_lossy(&output.bytes);
        assert!(text.contains("/SMask "));
        assert!(text.contains("/ColorSpace /DeviceGray"));
        assert_eq!(
            output
                .report
                .nodes
                .iter()
                .filter(|node| node.code == "pdf.node.painted_exact_image")
                .count(),
            1
        );
        assert!(
            output
                .report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "pdf.image.alpha_unsupported")
        );
    }

    #[test]
    fn exact_single_frame_gif_is_painted_and_preserves_transparency() {
        let mut rgba = image::RgbaImage::new(2, 1);
        rgba.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
        rgba.put_pixel(1, 0, image::Rgba([40, 50, 60, 0]));
        let mut encoded = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut encoded);
            encoder
                .encode_frame(image::Frame::new(rgba))
                .expect("encode single-frame GIF");
        }

        let resources = FixedPdfResources {
            node_paints: Vec::new(),
            images: vec![FixedImageResource {
                resource_id: resource_id(45),
                mime: "image/gif".into(),
                source_exact: true,
                node_ids: vec![node_id(10)],
                placements: Vec::new(),
                bytes: encoded,
            }],
            ..FixedPdfResources::default()
        };

        let output = render_bounded_pdf(
            &scene(),
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();

        let text = String::from_utf8_lossy(&output.bytes);
        assert!(text.contains("/SMask "));
        assert_eq!(
            output
                .report
                .nodes
                .iter()
                .filter(|node| node.code == "pdf.node.painted_exact_image")
                .count(),
            1
        );
        assert!(
            output
                .report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "pdf.image.mime_unsupported")
        );
    }

    #[test]
    fn exact_multi_frame_gif_remains_fail_closed() {
        let mut encoded = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut encoded);
            for rgb in [[10, 20, 30, 255], [40, 50, 60, 255]] {
                let mut rgba = image::RgbaImage::new(1, 1);
                rgba.put_pixel(0, 0, image::Rgba(rgb));
                encoder
                    .encode_frame(image::Frame::new(rgba))
                    .expect("encode GIF frame");
            }
        }

        let resources = FixedPdfResources {
            node_paints: Vec::new(),
            images: vec![FixedImageResource {
                resource_id: resource_id(46),
                mime: "image/gif".into(),
                source_exact: true,
                node_ids: vec![node_id(10)],
                placements: Vec::new(),
                bytes: encoded,
            }],
            ..FixedPdfResources::default()
        };

        let output = render_bounded_pdf(
            &scene(),
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();

        assert!(
            output
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "pdf.image.gif_animation_unsupported")
        );
        assert!(
            output
                .report
                .nodes
                .iter()
                .any(|node| node.code == "pdf.node.resource_unsupported")
        );
    }

    #[test]
    fn derived_preview_rgba_png_does_not_inherit_exact_alpha_authority() {
        use std::io::Cursor;

        let mut rgba = image::RgbaImage::new(1, 1);
        rgba.put_pixel(0, 0, image::Rgba([10, 20, 30, 128]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();

        let resources = FixedPdfResources {
            node_paints: Vec::new(),
            images: vec![FixedImageResource {
                resource_id: resource_id(44),
                mime: "image/png".into(),
                source_exact: false,
                node_ids: vec![node_id(10)],
                placements: Vec::new(),
                bytes: encoded.into_inner(),
            }],
            ..FixedPdfResources::default()
        };

        let output = render_bounded_pdf(
            &scene(),
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();

        let text = String::from_utf8_lossy(&output.bytes);
        assert!(!text.contains("/SMask "));
        assert!(
            output
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "pdf.image.preview_alpha_unsupported" })
        );
    }

    #[test]
    fn unsupported_transform_is_reported_not_silently_rendered() {
        let mut scene = scene();
        scene.nodes[0].transform = Affine2D::identity();
        scene.nodes[0].transform.tx = LengthEmu::new(1);

        let output = render_bounded_pdf(
            &scene,
            &resources(),
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
        .unwrap();

        assert!(output.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "pdf.node.transform_unsupported"
                && diagnostic.origin == node_id(12).into_canonical()
        }));
    }

    #[test]
    fn duplicate_or_dangling_paint_fails_closed() {
        let mut duplicate = resources();
        duplicate.node_paints.push(duplicate.node_paints[0].clone());
        assert!(matches!(
            render_bounded_pdf(
                &scene(),
                &duplicate,
                &PdfTargetProfile::basic_geometry_v0_1()
            ),
            Err(PdfRenderError::DuplicatePaint { .. })
        ));

        let dangling = FixedPdfResources {
            node_paints: vec![FixedNodePaint {
                node_id: node_id(99),
                fill_rgb: Some([1, 2, 3]),
                stroke: None,
            }],
            images: Vec::new(),
            ..FixedPdfResources::default()
        };
        assert!(matches!(
            render_bounded_pdf(
                &scene(),
                &dangling,
                &PdfTargetProfile::basic_geometry_v0_1()
            ),
            Err(PdfRenderError::PaintReferencesMissingNode { .. })
        ));
    }

    #[test]
    fn pdf_number_format_is_integer_arithmetic_and_canonical() {
        assert_eq!(format_points(12_700), "1");
        assert_eq!(format_points(7_560_000), "595.275590551");
        assert_eq!(format_points(-6_350), "-0.5");
        assert_eq!(format_rgb(0), "0");
        assert_eq!(format_rgb(255), "1");
        assert_eq!(format_rgb(128), "0.501961");
    }
}
