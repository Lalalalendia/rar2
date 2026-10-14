use crate::{
    IMAGE_FRAME_GEOMETRY_FEATURE, ODG_CONTENT_PATH, ODG_SCHEMA_FENCE_ODF_1_4, OdgPackage,
    OdgPackageBuilder, OdgPackageError, OdgPartKind,
};
use pub_export::{CapabilityLevel, ExportPlan, LossKind};
use pub_model::{
    CanonicalId, EMU_PER_POINT, LengthEmu, NodeId, PageId, ResolvedGraph, Story, StoryFlowError,
    StoryFrame, StoryId, validate_story_frames,
};
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;

const OFFICE_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const DRAW_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const TEXT_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const STYLE_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const SVG_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const FO_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OdgSemanticError {
    Package(OdgPackageError),
    SchemaFenceMismatch {
        found: Option<String>,
    },
    MissingPage {
        page_id: PageId,
    },
    MissingNode {
        page_id: PageId,
        node_id: NodeId,
    },
    UnplannedPageGeometry {
        page_id: PageId,
    },
    UnplannedPageObjectOrder {
        page_id: PageId,
    },
    UnplannedStoryText {
        story_id: StoryId,
    },
    UnplannedFrameFlow {
        node_id: NodeId,
    },
    UnplannedNodeOmission {
        node_id: NodeId,
    },
    FrameIdMismatch {
        node_id: NodeId,
        frame_id: NodeId,
    },
    MissingStory {
        node_id: NodeId,
        story_id: StoryId,
    },
    FrameParentIsNotPage {
        node_id: NodeId,
    },
    InvalidFrameBounds {
        node_id: NodeId,
    },
    InvalidStoryFlow {
        errors: Vec<StoryFlowError<StoryId, NodeId>>,
    },
    SharedStoryMaterializedWithoutLoss {
        story_id: StoryId,
    },
    InvalidXmlCharacter {
        story_id: StoryId,
        scalar: u32,
    },
}

impl fmt::Display for OdgSemanticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => write!(formatter, "{error}"),
            Self::SchemaFenceMismatch { found } => write!(
                formatter,
                "ODG semantic projection requires ODF 1.4 fence, found {found:?}"
            ),
            Self::MissingPage { page_id } => write!(
                formatter,
                "document page {} is missing from resolved graph",
                page_id.as_canonical()
            ),
            Self::MissingNode { page_id, node_id } => write!(
                formatter,
                "page {} references missing node {}",
                page_id.as_canonical(),
                node_id.as_canonical()
            ),
            Self::UnplannedPageGeometry { page_id } => write!(
                formatter,
                "page {} has no preserved page.geometry in ExportPlan",
                page_id.as_canonical()
            ),
            Self::UnplannedPageObjectOrder { page_id } => write!(
                formatter,
                "page {} has no grounded Page.children order and no page.object_order loss",
                page_id.as_canonical()
            ),
            Self::UnplannedStoryText { story_id } => write!(
                formatter,
                "Story {} has no preserved story.text in ExportPlan",
                story_id.as_canonical()
            ),
            Self::UnplannedFrameFlow { node_id } => write!(
                formatter,
                "frame {} has no preserved story.linked_frames in ExportPlan",
                node_id.as_canonical()
            ),
            Self::UnplannedNodeOmission { node_id } => write!(
                formatter,
                "node {} is not mapped to ODG and has no explicit unsupported loss",
                node_id.as_canonical()
            ),
            Self::FrameIdMismatch { node_id, frame_id } => write!(
                formatter,
                "frame extractor returned {} for node {}",
                frame_id.as_canonical(),
                node_id.as_canonical()
            ),
            Self::MissingStory { node_id, story_id } => write!(
                formatter,
                "frame {} references missing Story {}",
                node_id.as_canonical(),
                story_id.as_canonical()
            ),
            Self::FrameParentIsNotPage { node_id } => write!(
                formatter,
                "frame {} is not directly authored on a page",
                node_id.as_canonical()
            ),
            Self::InvalidFrameBounds { node_id } => write!(
                formatter,
                "frame {} has invalid authored bounds",
                node_id.as_canonical()
            ),
            Self::InvalidStoryFlow { errors } => {
                write!(formatter, "linked Story graph is invalid: {errors:?}")
            }
            Self::SharedStoryMaterializedWithoutLoss { story_id } => write!(
                formatter,
                "Story {} has multiple independent placements but no story.shared_identity loss",
                story_id.as_canonical()
            ),
            Self::InvalidXmlCharacter { story_id, scalar } => write!(
                formatter,
                "Story {} contains invalid XML 1.0 scalar U+{scalar:04X}",
                story_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for OdgSemanticError {}

impl From<OdgPackageError> for OdgSemanticError {
    fn from(value: OdgPackageError) -> Self {
        Self::Package(value)
    }
}

#[derive(Debug, Clone)]
struct FrameProjection {
    page_id: PageId,
    frame: StoryFrame<StoryId, NodeId>,
    x: LengthEmu,
    y: LengthEmu,
    width: LengthEmu,
    height: LengthEmu,
    z_index: usize,
}

pub fn project_resolved_graph_to_odg<
    NodePayload,
    Resource,
    StyleKind,
    StyleProperties,
    ExtensionStorage,
    ParagraphProperties,
    RunProperties,
    FrameExtractor,
>(
    plan: &ExportPlan,
    graph: &ResolvedGraph<
        NodePayload,
        Resource,
        StyleKind,
        StyleProperties,
        ExtensionStorage,
        ParagraphProperties,
        RunProperties,
    >,
    mut frame_for: FrameExtractor,
) -> Result<OdgPackage, OdgSemanticError>
where
    FrameExtractor: FnMut(NodeId, &NodePayload) -> Option<StoryFrame<StoryId, NodeId>>,
{
    if plan.target.schema_fence.as_deref() != Some(ODG_SCHEMA_FENCE_ODF_1_4) {
        return Err(OdgSemanticError::SchemaFenceMismatch {
            found: plan.target.schema_fence.clone(),
        });
    }

    let mut builder = OdgPackageBuilder::from_export_plan(plan)?;
    let mut frames = Vec::new();

    for page_id in &graph.document.pages {
        let page = graph
            .pages
            .get(page_id)
            .ok_or(OdgSemanticError::MissingPage { page_id: *page_id })?;

        if !page.size.is_positive()
            || !has_preserved_feature(plan, page_id.into_canonical(), "page.geometry")
        {
            return Err(OdgSemanticError::UnplannedPageGeometry { page_id: *page_id });
        }

        let page_node_ids = ordered_page_node_ids(plan, graph, page)?;
        for (z_index, node_id) in page_node_ids.into_iter().enumerate() {
            let node = graph
                .nodes
                .get(&node_id)
                .ok_or(OdgSemanticError::MissingNode {
                    page_id: *page_id,
                    node_id,
                })?;

            let Some(frame) = frame_for(node_id, &node.payload) else {
                if has_preserved_feature(
                    plan,
                    node_id.into_canonical(),
                    IMAGE_FRAME_GEOMETRY_FEATURE,
                ) {
                    continue;
                }
                if !has_reported_unsupported_omission(plan, node_id.into_canonical()) {
                    return Err(OdgSemanticError::UnplannedNodeOmission { node_id });
                }
                continue;
            };

            if frame.frame_id != node_id {
                return Err(OdgSemanticError::FrameIdMismatch {
                    node_id,
                    frame_id: frame.frame_id,
                });
            }
            if node.header.parent_id != page.id.into_canonical() {
                return Err(OdgSemanticError::FrameParentIsNotPage { node_id });
            }
            if !has_preserved_feature(plan, node_id.into_canonical(), "story.linked_frames") {
                return Err(OdgSemanticError::UnplannedFrameFlow { node_id });
            }
            if !graph.stories.contains_key(&frame.story_id) {
                return Err(OdgSemanticError::MissingStory {
                    node_id,
                    story_id: frame.story_id,
                });
            }
            if node.header.bounds.width.get() <= 0 || node.header.bounds.height.get() <= 0 {
                return Err(OdgSemanticError::InvalidFrameBounds { node_id });
            }

            frames.push(FrameProjection {
                page_id: *page_id,
                frame,
                x: node.header.bounds.x,
                y: node.header.bounds.y,
                width: node.header.bounds.width,
                height: node.header.bounds.height,
                z_index,
            });
        }
    }

    for story_id in graph.stories.keys() {
        if !has_preserved_feature(plan, story_id.into_canonical(), "story.text") {
            return Err(OdgSemanticError::UnplannedStoryText {
                story_id: *story_id,
            });
        }
    }

    let flow_frames = frames
        .iter()
        .map(|projection| projection.frame.clone())
        .collect::<Vec<_>>();
    let flow_errors = validate_story_frames(&flow_frames);
    if !flow_errors.is_empty() {
        return Err(OdgSemanticError::InvalidStoryFlow {
            errors: flow_errors,
        });
    }

    let roots_by_story = roots_by_story(&frames);
    for (story_id, roots) in &roots_by_story {
        if roots.len() > 1
            && !has_reported_feature_loss(plan, story_id.into_canonical(), "story.shared_identity")
        {
            return Err(OdgSemanticError::SharedStoryMaterializedWithoutLoss {
                story_id: *story_id,
            });
        }
    }

    let content = content_xml(graph, &frames)?;
    builder.add_xml_part(ODG_CONTENT_PATH, OdgPartKind::Content, content)?;
    builder.add_xml_part("styles.xml", OdgPartKind::Styles, styles_xml(graph))?;
    builder.finish().map_err(Into::into)
}

fn ordered_page_node_ids<
    NodePayload,
    Resource,
    StyleKind,
    StyleProperties,
    ExtensionStorage,
    ParagraphProperties,
    RunProperties,
>(
    plan: &ExportPlan,
    graph: &ResolvedGraph<
        NodePayload,
        Resource,
        StyleKind,
        StyleProperties,
        ExtensionStorage,
        ParagraphProperties,
        RunProperties,
    >,
    page: &pub_model::Page,
) -> Result<Vec<NodeId>, OdgSemanticError> {
    let authored = graph
        .nodes
        .iter()
        .filter_map(|(node_id, node)| {
            (node.header.parent_id == page.id.into_canonical()).then_some(*node_id)
        })
        .collect::<Vec<_>>();

    if page.children.len() == authored.len()
        && page
            .children
            .iter()
            .zip(authored.iter())
            .all(|(left, right)| left == right)
    {
        return Ok(page.children.clone());
    }

    if !has_reported_feature_loss(plan, page.id.into_canonical(), "page.object_order") {
        return Err(OdgSemanticError::UnplannedPageObjectOrder { page_id: page.id });
    }

    Ok(authored)
}

fn has_preserved_feature(plan: &ExportPlan, origin: CanonicalId, feature: &str) -> bool {
    plan.features.iter().any(|planned| {
        planned.request.origin == Some(origin)
            && planned.request.feature == feature
            && planned.disposition == CapabilityLevel::Preserved
    })
}

fn has_reported_feature_loss(plan: &ExportPlan, origin: CanonicalId, feature: &str) -> bool {
    plan.losses
        .iter()
        .any(|loss| loss.origin == Some(origin) && loss.feature == feature)
}

fn has_reported_unsupported_omission(plan: &ExportPlan, origin: CanonicalId) -> bool {
    plan.losses.iter().any(|loss| {
        loss.origin == Some(origin)
            && matches!(
                loss.kind,
                LossKind::Unsupported | LossKind::SourceExtensionLost
            )
    })
}

fn roots_by_story(frames: &[FrameProjection]) -> BTreeMap<StoryId, Vec<NodeId>> {
    let mut roots = BTreeMap::<StoryId, Vec<NodeId>>::new();
    for projection in frames {
        if projection.frame.previous.is_none() {
            roots
                .entry(projection.frame.story_id)
                .or_default()
                .push(projection.frame.frame_id);
        }
    }
    roots
}

fn content_xml<
    NodePayload,
    Resource,
    StyleKind,
    StyleProperties,
    ExtensionStorage,
    ParagraphProperties,
    RunProperties,
>(
    graph: &ResolvedGraph<
        NodePayload,
        Resource,
        StyleKind,
        StyleProperties,
        ExtensionStorage,
        ParagraphProperties,
        RunProperties,
    >,
    frames: &[FrameProjection],
) -> Result<String, OdgSemanticError> {
    let mut xml = String::new();
    writeln!(xml, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    writeln!(
        xml,
        "<office:document-content xmlns:office=\"{OFFICE_NS}\" xmlns:draw=\"{DRAW_NS}\" xmlns:text=\"{TEXT_NS}\" xmlns:style=\"{STYLE_NS}\" xmlns:svg=\"{SVG_NS}\" xmlns:fo=\"{FO_NS}\" xmlns:xlink=\"{XLINK_NS}\" office:version=\"1.4\">"
    )
    .unwrap();
    xml.push_str("  <office:automatic-styles/>\n");
    xml.push_str("  <office:body>\n");
    xml.push_str("    <office:drawing>\n");

    for page_id in &graph.document.pages {
        let page = graph
            .pages
            .get(page_id)
            .ok_or(OdgSemanticError::MissingPage { page_id: *page_id })?;
        writeln!(
            xml,
            "      <draw:page draw:name=\"{}\" draw:master-page-name=\"{}\">",
            page_name(*page_id),
            master_name(*page_id)
        )
        .unwrap();

        for projection in frames.iter().filter(|frame| frame.page_id == *page_id) {
            let story = graph.stories.get(&projection.frame.story_id).ok_or(
                OdgSemanticError::MissingStory {
                    node_id: projection.frame.frame_id,
                    story_id: projection.frame.story_id,
                },
            )?;
            write_frame(&mut xml, projection, story)?;
        }

        xml.push_str("      </draw:page>\n");
        let _ = page;
    }

    xml.push_str("    </office:drawing>\n");
    xml.push_str("  </office:body>\n");
    xml.push_str("</office:document-content>\n");
    Ok(xml)
}

fn styles_xml<
    NodePayload,
    Resource,
    StyleKind,
    StyleProperties,
    ExtensionStorage,
    ParagraphProperties,
    RunProperties,
>(
    graph: &ResolvedGraph<
        NodePayload,
        Resource,
        StyleKind,
        StyleProperties,
        ExtensionStorage,
        ParagraphProperties,
        RunProperties,
    >,
) -> String {
    let mut xml = String::new();
    writeln!(xml, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    writeln!(
        xml,
        "<office:document-styles xmlns:office=\"{OFFICE_NS}\" xmlns:style=\"{STYLE_NS}\" xmlns:fo=\"{FO_NS}\" office:version=\"1.4\">"
    )
    .unwrap();
    xml.push_str("  <office:styles/>\n");
    xml.push_str("  <office:automatic-styles>\n");

    for page_id in &graph.document.pages {
        if let Some(page) = graph.pages.get(page_id) {
            writeln!(
                xml,
                "    <style:page-layout style:name=\"{}\">",
                page_layout_name(*page_id)
            )
            .unwrap();
            writeln!(
                xml,
                "      <style:page-layout-properties fo:page-width=\"{}pt\" fo:page-height=\"{}pt\"/>",
                format_emu_points(page.size.width),
                format_emu_points(page.size.height)
            )
            .unwrap();
            xml.push_str("    </style:page-layout>\n");
        }
    }

    xml.push_str("  </office:automatic-styles>\n");
    xml.push_str("  <office:master-styles>\n");
    for page_id in &graph.document.pages {
        writeln!(
            xml,
            "    <style:master-page style:name=\"{}\" style:page-layout-name=\"{}\"/>",
            master_name(*page_id),
            page_layout_name(*page_id)
        )
        .unwrap();
    }
    xml.push_str("  </office:master-styles>\n");
    xml.push_str("</office:document-styles>\n");
    xml
}

fn write_frame(
    xml: &mut String,
    projection: &FrameProjection,
    story: &Story,
) -> Result<(), OdgSemanticError> {
    let name = frame_name(projection.frame.frame_id);
    let chain = projection
        .frame
        .next
        .map(frame_name)
        .map(|next| format!(" draw:chain-next-name=\"{next}\""))
        .unwrap_or_default();

    writeln!(
        xml,
        "        <draw:frame draw:name=\"{name}\" draw:z-index=\"{}\" svg:x=\"{}pt\" svg:y=\"{}pt\" svg:width=\"{}pt\" svg:height=\"{}pt\"{chain}>",
        projection.z_index,
        format_emu_points(projection.x),
        format_emu_points(projection.y),
        format_emu_points(projection.width),
        format_emu_points(projection.height)
    )
    .unwrap();

    if projection.frame.previous.is_none() {
        xml.push_str("          <draw:text-box>\n");
        write_story_paragraphs(xml, story)?;
        xml.push_str("          </draw:text-box>\n");
    } else {
        xml.push_str("          <draw:text-box/>\n");
    }

    xml.push_str("        </draw:frame>\n");
    Ok(())
}

fn write_story_paragraphs(xml: &mut String, story: &Story) -> Result<(), OdgSemanticError> {
    let paragraphs = story.text.split('\r').collect::<Vec<_>>();
    for paragraph in paragraphs {
        xml.push_str("            <text:p>");
        xml.push_str(&escape_odf_text(story.id, paragraph)?);
        xml.push_str("</text:p>\n");
    }
    Ok(())
}

fn escape_odf_text(story_id: StoryId, input: &str) -> Result<String, OdgSemanticError> {
    let mut output = String::new();
    let mut pending_spaces = 0_u32;

    let flush_spaces = |output: &mut String, count: &mut u32| {
        if *count == 0 {
            return;
        }
        if *count == 1 {
            output.push(' ');
        } else {
            write!(output, "<text:s text:c=\"{}\"/>", *count).unwrap();
        }
        *count = 0;
    };

    for character in input.chars() {
        let scalar = u32::from(character);
        if !is_xml_10_scalar(scalar) {
            return Err(OdgSemanticError::InvalidXmlCharacter { story_id, scalar });
        }

        match character {
            ' ' => pending_spaces += 1,
            '\t' => {
                flush_spaces(&mut output, &mut pending_spaces);
                output.push_str("<text:tab/>");
            }
            '\n' => {
                flush_spaces(&mut output, &mut pending_spaces);
                output.push_str("<text:line-break/>");
            }
            '&' => {
                flush_spaces(&mut output, &mut pending_spaces);
                output.push_str("&amp;");
            }
            '<' => {
                flush_spaces(&mut output, &mut pending_spaces);
                output.push_str("&lt;");
            }
            '>' => {
                flush_spaces(&mut output, &mut pending_spaces);
                output.push_str("&gt;");
            }
            _ => {
                flush_spaces(&mut output, &mut pending_spaces);
                output.push(character);
            }
        }
    }
    flush_spaces(&mut output, &mut pending_spaces);
    Ok(output)
}

fn is_xml_10_scalar(value: u32) -> bool {
    matches!(
        value,
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

fn frame_name(id: NodeId) -> String {
    stable_name("Frame", id.into_canonical())
}

pub(crate) fn page_name(id: PageId) -> String {
    stable_name("Page", id.into_canonical())
}

fn master_name(id: PageId) -> String {
    stable_name("Master", id.into_canonical())
}

fn page_layout_name(id: PageId) -> String {
    stable_name("PM", id.into_canonical())
}

fn stable_name(prefix: &str, id: CanonicalId) -> String {
    let mut value = String::with_capacity(prefix.len() + 33);
    value.push_str(prefix);
    value.push('_');
    for byte in id.into_bytes() {
        write!(value, "{byte:02x}").unwrap();
    }
    value
}

pub(crate) fn format_emu_points(value: LengthEmu) -> String {
    let numerator = i128::from(value.get());
    let denominator = i128::from(EMU_PER_POINT);
    let negative = numerator < 0;
    let numerator = numerator.abs();
    let whole = numerator / denominator;
    let mut remainder = numerator % denominator;
    let mut result = String::new();

    if negative {
        result.push('-');
    }
    write!(result, "{whole}").unwrap();
    if remainder == 0 {
        return result;
    }

    result.push('.');
    for _ in 0..15 {
        remainder *= 10;
        let digit = remainder / denominator;
        result.push(char::from(
            b'0' + u8::try_from(digit).expect("decimal digit"),
        ));
        remainder %= denominator;
        if remainder == 0 {
            break;
        }
    }
    while result.ends_with('0') {
        result.pop();
    }
    if result.ends_with('.') {
        result.pop();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_export::{
        SemanticFeatureRequest, TargetCapabilityManifest, TargetProfile, plan_export,
    };
    use pub_model::{
        Affine2D, Document, DocumentId, Node, NodeHeader, NodeKind, Page, RectEmu, Sha256Digest,
        Size2D, SourceDescriptor,
    };
    use std::collections::BTreeMap;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Payload {
        frame: Option<StoryFrame<StoryId, NodeId>>,
    }

    type TestGraph = ResolvedGraph<Payload, (), (), (), String>;

    fn id(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn page_id(byte: u8) -> PageId {
        PageId::from_canonical(id(byte))
    }

    fn node_id(byte: u8) -> NodeId {
        NodeId::from_canonical(id(byte))
    }

    fn story_id(byte: u8) -> StoryId {
        StoryId::from_canonical(id(byte))
    }

    fn hash() -> Sha256Digest {
        Sha256Digest::from_bytes([0x11; 32])
    }

    fn graph(linked: bool) -> TestGraph {
        let page1 = page_id(1);
        let page2 = page_id(2);
        let frame1 = node_id(11);
        let frame2 = node_id(12);
        let story = story_id(21);

        let document = Document {
            id: DocumentId::from_canonical(id(30)),
            format_origin: "pub".into(),
            source_hash: hash(),
            pages: vec![page1, page2],
            resources: Vec::new(),
            styles: Vec::new(),
        };

        let mut pages = BTreeMap::new();
        pages.insert(
            page1,
            Page {
                id: page1,
                size: Size2D::new(LengthEmu::new(7_315_200), LengthEmu::new(9_753_600)),
                bleed: None,
                margins: None,
                children: vec![frame1],
                extensions: Vec::new(),
            },
        );
        pages.insert(
            page2,
            Page {
                id: page2,
                size: Size2D::new(LengthEmu::new(7_315_200), LengthEmu::new(9_753_600)),
                bleed: None,
                margins: None,
                children: vec![frame2],
                extensions: Vec::new(),
            },
        );

        let frame_payload = |frame_id, previous, next| Payload {
            frame: Some(StoryFrame {
                story_id: story,
                frame_id,
                ordinal: if frame_id == frame1 { 0 } else { 1 },
                previous,
                next,
            }),
        };

        let mut nodes = BTreeMap::new();
        nodes.insert(
            frame1,
            Node {
                kind: NodeKind::TextFrame,
                header: NodeHeader {
                    id: frame1,
                    parent_id: page1.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(914_400),
                        LengthEmu::new(914_400),
                        LengthEmu::new(3_657_600),
                        LengthEmu::new(1_828_800),
                    ),
                    transform: Affine2D::identity(),
                    source_refs: Vec::new(),
                    extensions: Vec::new(),
                },
                payload: frame_payload(frame1, None, linked.then_some(frame2)),
            },
        );
        nodes.insert(
            frame2,
            Node {
                kind: NodeKind::TextFrame,
                header: NodeHeader {
                    id: frame2,
                    parent_id: page2.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(914_400),
                        LengthEmu::new(914_400),
                        LengthEmu::new(3_657_600),
                        LengthEmu::new(1_828_800),
                    ),
                    transform: Affine2D::identity(),
                    source_refs: Vec::new(),
                    extensions: Vec::new(),
                },
                payload: frame_payload(frame2, linked.then_some(frame1), None),
            },
        );

        let mut stories = BTreeMap::new();
        stories.insert(
            story,
            Story {
                id: story,
                text: "Hello  Publisher\rSecond paragraph".into(),
                paragraphs: Vec::new(),
                runs: Vec::new(),
                fields: Vec::new(),
                hyperlinks: Vec::new(),
                source_refs: Vec::new(),
            },
        );

        ResolvedGraph {
            cdm_version: "0.1".into(),
            resolver_version: "test".into(),
            source: SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                adapter_version: "test".into(),
                source_hash: hash(),
            },
            document,
            pages,
            nodes,
            stories,
            paragraphs: BTreeMap::new(),
            text_runs: BTreeMap::new(),
            resources: BTreeMap::new(),
            styles: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }

    fn plan(graph: &TestGraph, linked: bool) -> ExportPlan {
        let mut features = BTreeMap::new();
        features.insert("page.geometry".into(), CapabilityLevel::Preserved);
        features.insert("story.text".into(), CapabilityLevel::Preserved);
        features.insert("story.linked_frames".into(), CapabilityLevel::Preserved);
        let manifest = TargetCapabilityManifest {
            target: TargetProfile {
                format: "odg".into(),
                adapter_version: crate::ODG_ADAPTER_VERSION_V0_1.into(),
                profile: "bounded-editable".into(),
                schema_fence: Some(ODG_SCHEMA_FENCE_ODF_1_4.into()),
            },
            features,
        };

        let mut requests = Vec::new();
        for page_id in &graph.document.pages {
            requests.push(SemanticFeatureRequest {
                feature: "page.geometry".into(),
                origin: Some(page_id.into_canonical()),
                property_path: Some("page.size".into()),
                require_preserved: true,
            });
        }
        for story_id in graph.stories.keys() {
            requests.push(SemanticFeatureRequest {
                feature: "story.text".into(),
                origin: Some(story_id.into_canonical()),
                property_path: Some("story.text".into()),
                require_preserved: true,
            });
            if !linked {
                requests.push(SemanticFeatureRequest {
                    feature: "story.shared_identity".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.placements".into()),
                    require_preserved: false,
                });
            }
        }
        for node_id in graph.nodes.keys() {
            requests.push(SemanticFeatureRequest {
                feature: "story.linked_frames".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.story_frame".into()),
                require_preserved: true,
            });
        }
        plan_export(&manifest, requests)
    }

    #[test]
    fn linked_story_maps_to_chain_next_and_only_head_carries_text() {
        let graph = graph(true);
        let package = project_resolved_graph_to_odg(&plan(&graph, true), &graph, |_, payload| {
            payload.frame.clone()
        })
        .expect("ODG projection");

        let content = package
            .parts
            .iter()
            .find(|part| part.path == ODG_CONTENT_PATH)
            .expect("content.xml");
        let xml = String::from_utf8(content.content.clone()).unwrap();

        assert_eq!(xml.matches("<draw:page ").count(), 2);
        assert_eq!(xml.matches("<draw:frame ").count(), 2);
        assert_eq!(xml.matches("Hello").count(), 1);
        assert!(xml.contains("draw:chain-next-name="));
        assert!(xml.contains("<text:s text:c=\"2\"/>"));
        assert!(xml.contains("<text:p>Second paragraph</text:p>"));
    }

    #[test]
    fn independent_shared_story_requires_explicit_loss_and_is_not_chained() {
        let graph = graph(false);
        let package = project_resolved_graph_to_odg(&plan(&graph, false), &graph, |_, payload| {
            payload.frame.clone()
        })
        .expect("explicit shared-identity materialization loss permits projection");

        let content = package
            .parts
            .iter()
            .find(|part| part.path == ODG_CONTENT_PATH)
            .expect("content.xml");
        let xml = String::from_utf8(content.content.clone()).unwrap();

        assert_eq!(xml.matches("Hello").count(), 2);
        assert!(!xml.contains("draw:chain-next-name="));
    }

    #[test]
    fn shared_story_without_loss_fails_closed() {
        let graph = graph(false);
        let mut plan = plan(&graph, false);
        plan.losses
            .retain(|loss| loss.feature != "story.shared_identity");

        assert!(matches!(
            project_resolved_graph_to_odg(&plan, &graph, |_, payload| payload.frame.clone()),
            Err(OdgSemanticError::SharedStoryMaterializedWithoutLoss { .. })
        ));
    }
}

mod authored_rectangle_overlay_v1 {
    use crate::{ODG_CONTENT_PATH, OdgPackage, OdgPartKind};
    use pub_export::{
        AUTHORED_SHAPE_FILL_FEATURE, AUTHORED_SHAPE_GEOMETRY_FEATURE, AUTHORED_SHAPE_STROKE_FEATURE,
        AUTHORED_SHAPE_Z_ORDER_FEATURE, AuthoredRectangleExportV1, CapabilityLevel, ExportPlan,
        ExportSrgb8V1,
    };
    use pub_model::{CanonicalId, EMU_PER_POINT, LengthEmu, NodeId, PageId};
    use std::collections::BTreeSet;
    use std::fmt;
    use std::fmt::Write as _;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum OdgAuthoredShapeError {
        NonOdgPackage,
        MissingContent,
        InvalidContentUtf8,
        DuplicateRectangle {
            node_id: NodeId,
        },
        InvalidBounds {
            node_id: NodeId,
        },
        InvalidStroke {
            node_id: NodeId,
        },
        MissingPreservedFeature {
            node_id: NodeId,
            feature: &'static str,
        },
        MissingZOrderLoss {
            node_id: NodeId,
        },
        MissingAutomaticStyles,
        MissingPage {
            page_id: PageId,
        },
    }

    impl fmt::Display for OdgAuthoredShapeError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::NonOdgPackage => f.write_str("authored rectangle projection requires ODG"),
                Self::MissingContent => f.write_str("ODG content.xml missing"),
                Self::InvalidContentUtf8 => f.write_str("ODG content.xml is not UTF-8"),
                Self::DuplicateRectangle { node_id } => {
                    write!(f, "duplicate authored rectangle {node_id}")
                }
                Self::InvalidBounds { node_id } => {
                    write!(f, "authored rectangle {node_id} has invalid bounds")
                }
                Self::InvalidStroke { node_id } => {
                    write!(f, "authored rectangle {node_id} has invalid stroke")
                }
                Self::MissingPreservedFeature { node_id, feature } => write!(
                    f,
                    "authored rectangle {node_id} lacks preserved {feature} in ExportPlan"
                ),
                Self::MissingZOrderLoss { node_id } => write!(
                    f,
                    "authored rectangle {node_id} lacks explicit authored_shape.z_order loss"
                ),
                Self::MissingAutomaticStyles => f.write_str("ODG automatic-styles container missing"),
                Self::MissingPage { page_id } => write!(f, "ODG page {page_id} missing"),
            }
        }
    }

    impl std::error::Error for OdgAuthoredShapeError {}

    pub fn add_authored_rectangles_to_odg(
        plan: &ExportPlan,
        package: &mut OdgPackage,
        rectangles: &[AuthoredRectangleExportV1],
    ) -> Result<(), OdgAuthoredShapeError> {
        if plan.target.format != "odg" || package.target.format != "odg" {
            return Err(OdgAuthoredShapeError::NonOdgPackage);
        }
        let content_index = package
            .parts
            .iter()
            .position(|part| part.kind == OdgPartKind::Content && part.path == ODG_CONTENT_PATH)
            .ok_or(OdgAuthoredShapeError::MissingContent)?;
        let mut xml = String::from_utf8(package.parts[content_index].content.clone())
            .map_err(|_| OdgAuthoredShapeError::InvalidContentUtf8)?;

        let mut ordered = rectangles.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|item| (item.page_id, item.node_id));
        let mut seen = BTreeSet::new();
        for item in &ordered {
            if !seen.insert(item.node_id) {
                return Err(OdgAuthoredShapeError::DuplicateRectangle {
                    node_id: item.node_id,
                });
            }
            validate(plan, item)?;
        }

        if ordered.is_empty() {
            return Ok(());
        }

        let mut styles = String::new();
        for item in &ordered {
            let style_name = style_name(item.node_id);
            write!(
                styles,
                "    <style:style style:name=\"{style_name}\" style:family=\"graphic\"><style:graphic-properties"
            )
            .unwrap();
            if item.fill.visible {
                write!(
                    styles,
                    " draw:fill=\"solid\" draw:fill-color=\"{}\"",
                    hex_color(item.fill.color)
                )
                .unwrap();
            } else {
                styles.push_str(" draw:fill=\"none\"");
            }
            if item.stroke.visible {
                write!(
                    styles,
                    " draw:stroke=\"solid\" svg:stroke-color=\"{}\" svg:stroke-width=\"{}pt\"",
                    hex_color(item.stroke.color),
                    format_emu_points(item.stroke.width_emu)
                )
                .unwrap();
            } else {
                styles.push_str(" draw:stroke=\"none\"");
            }
            styles.push_str("/></style:style>\n");
        }
        insert_automatic_styles(&mut xml, &styles)?;

        for item in ordered {
            let page_marker = format!("<draw:page draw:name=\"{}\"", page_name(item.page_id));
            let Some(page_start) = xml.find(&page_marker) else {
                return Err(OdgAuthoredShapeError::MissingPage {
                    page_id: item.page_id,
                });
            };
            let close = "      </draw:page>";
            let Some(relative_close) = xml[page_start..].find(close) else {
                return Err(OdgAuthoredShapeError::MissingPage {
                    page_id: item.page_id,
                });
            };
            let insert_at = page_start + relative_close;
            let fragment = format!(
                "        <draw:rect draw:name=\"{}\" draw:style-name=\"{}\" svg:x=\"{}pt\" svg:y=\"{}pt\" svg:width=\"{}pt\" svg:height=\"{}pt\"/>\n",
                rect_name(item.node_id),
                style_name(item.node_id),
                format_emu_points(item.bounds.x),
                format_emu_points(item.bounds.y),
                format_emu_points(item.bounds.width),
                format_emu_points(item.bounds.height),
            );
            xml.insert_str(insert_at, &fragment);
        }

        package.parts[content_index].content = xml.into_bytes();
        Ok(())
    }

    fn validate(
        plan: &ExportPlan,
        item: &AuthoredRectangleExportV1,
    ) -> Result<(), OdgAuthoredShapeError> {
        if item.bounds.width.get() <= 0
            || item.bounds.height.get() <= 0
            || item.bounds.right().is_none()
            || item.bounds.bottom().is_none()
        {
            return Err(OdgAuthoredShapeError::InvalidBounds {
                node_id: item.node_id,
            });
        }
        if item.stroke.width_emu.get() <= 0 {
            return Err(OdgAuthoredShapeError::InvalidStroke {
                node_id: item.node_id,
            });
        }
        for feature in [
            AUTHORED_SHAPE_GEOMETRY_FEATURE,
            AUTHORED_SHAPE_FILL_FEATURE,
            AUTHORED_SHAPE_STROKE_FEATURE,
        ] {
            if !has_preserved_feature(plan, item.node_id.into_canonical(), feature) {
                return Err(OdgAuthoredShapeError::MissingPreservedFeature {
                    node_id: item.node_id,
                    feature,
                });
            }
        }
        if !has_reported_loss(
            plan,
            item.node_id.into_canonical(),
            AUTHORED_SHAPE_Z_ORDER_FEATURE,
        ) {
            return Err(OdgAuthoredShapeError::MissingZOrderLoss {
                node_id: item.node_id,
            });
        }
        Ok(())
    }

    fn insert_automatic_styles(xml: &mut String, styles: &str) -> Result<(), OdgAuthoredShapeError> {
        let empty = "  <office:automatic-styles/>\n";
        if xml.matches(empty).count() == 1 {
            let replacement =
                format!("  <office:automatic-styles>\n{styles}  </office:automatic-styles>\n");
            *xml = xml.replacen(empty, &replacement, 1);
            return Ok(());
        }
        let close = "  </office:automatic-styles>";
        let Some(index) = xml.find(close) else {
            return Err(OdgAuthoredShapeError::MissingAutomaticStyles);
        };
        xml.insert_str(index, styles);
        Ok(())
    }

    fn has_preserved_feature(plan: &ExportPlan, origin: CanonicalId, feature: &str) -> bool {
        plan.features.iter().any(|planned| {
            planned.request.origin == Some(origin)
                && planned.request.feature == feature
                && planned.disposition == CapabilityLevel::Preserved
        })
    }

    fn has_reported_loss(plan: &ExportPlan, origin: CanonicalId, feature: &str) -> bool {
        plan.losses
            .iter()
            .any(|loss| loss.origin == Some(origin) && loss.feature == feature)
    }

    fn style_name(id: NodeId) -> String {
        stable_name("AuthoredRectStyle", id.into_canonical())
    }

    fn rect_name(id: NodeId) -> String {
        stable_name("AuthoredRect", id.into_canonical())
    }

    fn page_name(id: PageId) -> String {
        stable_name("Page", id.into_canonical())
    }

    fn stable_name(prefix: &str, id: CanonicalId) -> String {
        let mut value = String::with_capacity(prefix.len() + 33);
        value.push_str(prefix);
        value.push('_');
        for byte in id.into_bytes() {
            write!(value, "{byte:02x}").unwrap();
        }
        value
    }

    fn hex_color(color: ExportSrgb8V1) -> String {
        format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
    }

    fn format_emu_points(value: LengthEmu) -> String {
        let numerator = i128::from(value.get());
        let denominator = i128::from(EMU_PER_POINT);
        let negative = numerator < 0;
        let numerator = numerator.abs();
        let whole = numerator / denominator;
        let mut remainder = numerator % denominator;
        let mut result = String::new();
        if negative {
            result.push('-');
        }
        write!(result, "{whole}").unwrap();
        if remainder == 0 {
            return result;
        }
        result.push('.');
        for _ in 0..15 {
            remainder *= 10;
            let digit = remainder / denominator;
            result.push(char::from(
                b'0' + u8::try_from(digit).expect("decimal digit"),
            ));
            remainder %= denominator;
            if remainder == 0 {
                break;
            }
        }
        while result.ends_with('0') {
            result.pop();
        }
        if result.ends_with('.') {
            result.pop();
        }
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{ODG_ADAPTER_VERSION_V0_1, ODG_SCHEMA_FENCE_ODF_1_4, OdgPart};
        use pub_export::{
            AUTHORED_SHAPE_FILL_FEATURE, AUTHORED_SHAPE_GEOMETRY_FEATURE,
            AUTHORED_SHAPE_STROKE_FEATURE, AUTHORED_SHAPE_Z_ORDER_FEATURE, ExportSolidPaintV1,
            ExportSolidStrokeV1, SemanticFeatureRequest, TargetCapabilityManifest, TargetProfile,
            plan_export,
        };
        use pub_model::{CanonicalId, RectEmu, Size2D};
        use std::collections::BTreeMap;

        fn id(byte: u8) -> CanonicalId {
            CanonicalId::from_bytes([byte; 16])
        }

        fn rectangle() -> AuthoredRectangleExportV1 {
            AuthoredRectangleExportV1 {
                node_id: NodeId::from_canonical(id(2)),
                page_id: PageId::from_canonical(id(1)),
                page_size: Size2D::new(LengthEmu::new(7_620_000), LengthEmu::new(9_906_000)),
                bounds: RectEmu::new(
                    LengthEmu::new(1_270_000),
                    LengthEmu::new(2_540_000),
                    LengthEmu::new(2_540_000),
                    LengthEmu::new(1_270_000),
                ),
                fill: ExportSolidPaintV1 {
                    visible: true,
                    color: ExportSrgb8V1 {
                        r: 0x12,
                        g: 0x34,
                        b: 0x56,
                    },
                },
                stroke: ExportSolidStrokeV1 {
                    visible: true,
                    color: ExportSrgb8V1 {
                        r: 0xAA,
                        g: 0xBB,
                        b: 0xCC,
                    },
                    width_emu: LengthEmu::new(12_700),
                },
            }
        }

        fn plan(item: &AuthoredRectangleExportV1) -> ExportPlan {
            let mut features = BTreeMap::new();
            for feature in [
                AUTHORED_SHAPE_GEOMETRY_FEATURE,
                AUTHORED_SHAPE_FILL_FEATURE,
                AUTHORED_SHAPE_STROKE_FEATURE,
            ] {
                features.insert(feature.into(), CapabilityLevel::Preserved);
            }
            let manifest = TargetCapabilityManifest {
                target: TargetProfile {
                    format: "odg".into(),
                    adapter_version: ODG_ADAPTER_VERSION_V0_1.into(),
                    profile: "bounded-editable".into(),
                    schema_fence: Some(ODG_SCHEMA_FENCE_ODF_1_4.into()),
                },
                features,
            };
            let request = |feature: &str, required| SemanticFeatureRequest {
                feature: feature.into(),
                origin: Some(item.node_id.into_canonical()),
                property_path: None,
                require_preserved: required,
            };
            plan_export(
                &manifest,
                vec![
                    request(AUTHORED_SHAPE_GEOMETRY_FEATURE, true),
                    request(AUTHORED_SHAPE_FILL_FEATURE, true),
                    request(AUTHORED_SHAPE_STROKE_FEATURE, true),
                    request(AUTHORED_SHAPE_Z_ORDER_FEATURE, false),
                ],
            )
        }

        fn package(item: &AuthoredRectangleExportV1, plan: &ExportPlan) -> OdgPackage {
            let content = format!(
                "<?xml version=\"1.0\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\">\n  <office:automatic-styles/>\n  <office:body><office:drawing><draw:page draw:name=\"{}\">\n      </draw:page></office:drawing></office:body></office:document-content>",
                page_name(item.page_id)
            );
            OdgPackage {
                target: plan.target.clone(),
                conversion_fence: None,
                parts: vec![OdgPart {
                    path: ODG_CONTENT_PATH.into(),
                    kind: OdgPartKind::Content,
                    media_type: "text/xml".into(),
                    content: content.into_bytes(),
                }],
            }
        }

        #[test]
        fn writes_native_graphic_style_rectangle_and_no_fake_z_index() {
            let item = rectangle();
            let plan = plan(&item);
            assert!(plan.losses.iter().any(|loss| {
                loss.origin == Some(item.node_id.into_canonical())
                    && loss.feature == AUTHORED_SHAPE_Z_ORDER_FEATURE
            }));
            let mut package = package(&item, &plan);
            add_authored_rectangles_to_odg(&plan, &mut package, std::slice::from_ref(&item))
                .expect("authored rectangle");
            let xml = std::str::from_utf8(&package.parts[0].content).unwrap();
            assert!(xml.contains("style:family=\"graphic\""));
            assert!(xml.contains("draw:fill=\"solid\" draw:fill-color=\"#123456\""));
            assert!(xml.contains(
                "draw:stroke=\"solid\" svg:stroke-color=\"#AABBCC\" svg:stroke-width=\"1pt\""
            ));
            assert!(xml.contains("<draw:rect draw:name=\"AuthoredRect_"));
            assert!(
                xml.contains(
                    "svg:x=\"100pt\" svg:y=\"200pt\" svg:width=\"200pt\" svg:height=\"100pt\""
                )
            );
            assert!(!xml.contains("draw:z-index="));
        }

        #[test]
        fn invisible_fill_and_stroke_remain_explicit_none() {
            let mut item = rectangle();
            item.fill.visible = false;
            item.stroke.visible = false;
            let plan = plan(&item);
            let mut package = package(&item, &plan);
            add_authored_rectangles_to_odg(&plan, &mut package, std::slice::from_ref(&item))
                .expect("authored rectangle");
            let xml = std::str::from_utf8(&package.parts[0].content).unwrap();
            assert!(xml.contains("draw:fill=\"none\""));
            assert!(xml.contains("draw:stroke=\"none\""));
        }
    }
}

pub use authored_rectangle_overlay_v1::{
    OdgAuthoredShapeError, add_authored_rectangles_to_odg,
};
