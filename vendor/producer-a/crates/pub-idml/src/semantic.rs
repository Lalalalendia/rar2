use crate::{
    IdmlPackage, IdmlPackageBuilder, IdmlPackageError, IdmlPartKind, IdmlSimpleTable,
    IdmlTableError,
};
use pub_export::{CapabilityLevel, ExportPlan, LossKind};
use pub_model::{
    CanonicalId, EMU_PER_POINT, LengthEmu, NodeId, PageId, ResolvedGraph, Story, StoryFlowError,
    StoryFrame, StoryId, validate_story_frames,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;

pub const IDML_PACKAGING_NAMESPACE: &str = "http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";
pub const IDML_SCHEMA_FENCE_LEGACY_DOM_7: &str = "legacy-spec-8.02/dom-7.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdmlWireProfile {
    pub dom_version: String,
    pub aid_reader_version: String,
    pub aid_feature_set: String,
    pub schema_fence: String,
}

impl IdmlWireProfile {
    pub fn legacy_dom_7() -> Self {
        Self {
            dom_version: "7.0".into(),
            aid_reader_version: "6.0".into(),
            aid_feature_set: "257".into(),
            schema_fence: IDML_SCHEMA_FENCE_LEGACY_DOM_7.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdmlSemanticError {
    Package(IdmlPackageError),
    SchemaFenceMismatch {
        expected: String,
        found: Option<String>,
    },
    MissingPage {
        page_id: PageId,
    },
    InvalidPageSize {
        page_id: PageId,
    },
    FrameParentIsNotPage {
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
    InvalidFrameBounds {
        node_id: NodeId,
    },
    InvalidStoryFlow {
        errors: Vec<StoryFlowError<StoryId, NodeId>>,
    },
    UnplannedNodeOmission {
        node_id: NodeId,
    },
    UnreportedRichStoryDowngrade {
        story_id: StoryId,
    },
    InvalidXmlCharacter {
        story_id: StoryId,
        scalar: u32,
    },
    Table(IdmlTableError),
    DuplicateTableStory {
        story_id: StoryId,
    },
    TableStoryAlsoLinked {
        story_id: StoryId,
    },
    UnplannedTableStructure {
        node_id: NodeId,
    },
    UnreportedTableStyleDowngrade {
        node_id: NodeId,
    },
}

impl fmt::Display for IdmlSemanticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => write!(formatter, "{error}"),
            Self::SchemaFenceMismatch { expected, found } => {
                write!(
                    formatter,
                    "IDML schema fence mismatch: expected {expected}, found {found:?}"
                )
            }
            Self::MissingPage { page_id } => write!(
                formatter,
                "document page {} отсутствует в resolved graph",
                page_id.as_canonical()
            ),
            Self::InvalidPageSize { page_id } => write!(
                formatter,
                "page {} имеет неположительный размер",
                page_id.as_canonical()
            ),
            Self::FrameParentIsNotPage { node_id } => write!(
                formatter,
                "text frame {} не принадлежит authored page напрямую",
                node_id.as_canonical()
            ),
            Self::FrameIdMismatch { node_id, frame_id } => write!(
                formatter,
                "text-frame extractor вернул frame {} для node {}",
                frame_id.as_canonical(),
                node_id.as_canonical()
            ),
            Self::MissingStory { node_id, story_id } => write!(
                formatter,
                "text frame {} ссылается на отсутствующую Story {}",
                node_id.as_canonical(),
                story_id.as_canonical()
            ),
            Self::InvalidFrameBounds { node_id } => write!(
                formatter,
                "text frame {} имеет невалидную геометрию",
                node_id.as_canonical()
            ),
            Self::InvalidStoryFlow { errors } => {
                write!(formatter, "linked-story graph невалиден: {errors:?}")
            }
            Self::UnplannedNodeOmission { node_id } => write!(
                formatter,
                "node {} не отображён в IDML и не покрыт explicit unsupported loss",
                node_id.as_canonical()
            ),
            Self::UnreportedRichStoryDowngrade { story_id } => write!(
                formatter,
                "Story {} содержит rich semantics, но export plan не сообщает downgrade",
                story_id.as_canonical()
            ),
            Self::InvalidXmlCharacter { story_id, scalar } => write!(
                formatter,
                "Story {} содержит недопустимый XML 1.0 scalar U+{scalar:04X}",
                story_id.as_canonical()
            ),
            Self::Table(error) => write!(formatter, "{error}"),
            Self::DuplicateTableStory { story_id } => write!(
                formatter,
                "несколько TABLE nodes претендуют на Story {} в bounded IDML projection",
                story_id.as_canonical()
            ),
            Self::TableStoryAlsoLinked { story_id } => write!(
                formatter,
                "table Story {} одновременно используется ordinary linked text frame",
                story_id.as_canonical()
            ),
            Self::UnplannedTableStructure { node_id } => write!(
                formatter,
                "TABLE node {} не имеет preserved table.structure в ExportPlan",
                node_id.as_canonical()
            ),
            Self::UnreportedTableStyleDowngrade { node_id } => write!(
                formatter,
                "TABLE node {} materializes default IDML styles without explicit table.style loss",
                node_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for IdmlSemanticError {}

impl From<IdmlPackageError> for IdmlSemanticError {
    fn from(value: IdmlPackageError) -> Self {
        Self::Package(value)
    }
}

impl From<IdmlTableError> for IdmlSemanticError {
    fn from(value: IdmlTableError) -> Self {
        Self::Table(value)
    }
}

#[derive(Debug, Clone)]
struct FrameProjection {
    page_id: PageId,
    frame: StoryFrame<StoryId, NodeId>,
    x: LengthEmu,
    y: LengthEmu,
    right: LengthEmu,
    bottom: LengthEmu,
}

pub fn project_resolved_graph_to_idml<
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
    profile: &IdmlWireProfile,
    mut frame_for: FrameExtractor,
) -> Result<IdmlPackage, IdmlSemanticError>
where
    FrameExtractor: FnMut(NodeId, &NodePayload) -> Option<StoryFrame<StoryId, NodeId>>,
{
    if plan.target.schema_fence.as_deref() != Some(profile.schema_fence.as_str()) {
        return Err(IdmlSemanticError::SchemaFenceMismatch {
            expected: profile.schema_fence.clone(),
            found: plan.target.schema_fence.clone(),
        });
    }

    let mut builder = IdmlPackageBuilder::from_export_plan(plan)?;
    let mut frames = Vec::new();

    for page_id in &graph.document.pages {
        let page = graph
            .pages
            .get(page_id)
            .ok_or(IdmlSemanticError::MissingPage { page_id: *page_id })?;
        if !page.size.is_positive() {
            return Err(IdmlSemanticError::InvalidPageSize { page_id: *page_id });
        }
    }

    for (node_id, node) in &graph.nodes {
        let Some(frame) = frame_for(*node_id, &node.payload) else {
            if !has_reported_unsupported_omission(plan, node_id.into_canonical()) {
                return Err(IdmlSemanticError::UnplannedNodeOmission { node_id: *node_id });
            }
            continue;
        };

        if frame.frame_id != *node_id {
            return Err(IdmlSemanticError::FrameIdMismatch {
                node_id: *node_id,
                frame_id: frame.frame_id,
            });
        }
        if !graph.stories.contains_key(&frame.story_id) {
            return Err(IdmlSemanticError::MissingStory {
                node_id: *node_id,
                story_id: frame.story_id,
            });
        }

        let page_id = graph
            .pages
            .iter()
            .find_map(|(page_id, page)| {
                (page.id.into_canonical() == node.header.parent_id).then_some(*page_id)
            })
            .ok_or(IdmlSemanticError::FrameParentIsNotPage { node_id: *node_id })?;

        if node.header.bounds.width.get() <= 0 || node.header.bounds.height.get() <= 0 {
            return Err(IdmlSemanticError::InvalidFrameBounds { node_id: *node_id });
        }
        let right = node
            .header
            .bounds
            .right()
            .ok_or(IdmlSemanticError::InvalidFrameBounds { node_id: *node_id })?;
        let bottom = node
            .header
            .bounds
            .bottom()
            .ok_or(IdmlSemanticError::InvalidFrameBounds { node_id: *node_id })?;

        frames.push(FrameProjection {
            page_id,
            frame,
            x: node.header.bounds.x,
            y: node.header.bounds.y,
            right,
            bottom,
        });
    }

    let flow_frames = frames
        .iter()
        .map(|item| item.frame.clone())
        .collect::<Vec<_>>();
    let flow_errors = validate_story_frames(&flow_frames);
    if !flow_errors.is_empty() {
        return Err(IdmlSemanticError::InvalidStoryFlow {
            errors: flow_errors,
        });
    }

    for story in graph.stories.values() {
        validate_story_downgrade(plan, story)?;
        let path = story_path(story.id);
        let xml = story_xml(story, profile)?;
        builder.add_part(path, IdmlPartKind::Story, xml)?;
    }

    for page_id in &graph.document.pages {
        let page = graph
            .pages
            .get(page_id)
            .ok_or(IdmlSemanticError::MissingPage { page_id: *page_id })?;
        let page_frames = frames
            .iter()
            .filter(|item| item.page_id == *page_id)
            .collect::<Vec<_>>();
        let path = spread_path(*page_id);
        let xml = spread_xml(page, &page_frames, profile);
        builder.add_part(path, IdmlPartKind::Spread, xml)?;
    }

    let designmap = designmap_xml(graph, profile);
    builder.add_part("designmap.xml", IdmlPartKind::DesignMap, designmap)?;

    builder.finish().map_err(Into::into)
}

pub fn project_resolved_graph_to_idml_with_tables<
    NodePayload,
    Resource,
    StyleKind,
    StyleProperties,
    ExtensionStorage,
    ParagraphProperties,
    RunProperties,
    FrameExtractor,
    TableExtractor,
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
    profile: &IdmlWireProfile,
    mut frame_for: FrameExtractor,
    mut table_for: TableExtractor,
) -> Result<IdmlPackage, IdmlSemanticError>
where
    FrameExtractor: FnMut(NodeId, &NodePayload) -> Option<StoryFrame<StoryId, NodeId>>,
    TableExtractor: FnMut(NodeId, &NodePayload) -> Option<IdmlSimpleTable>,
{
    if plan.target.schema_fence.as_deref() != Some(profile.schema_fence.as_str()) {
        return Err(IdmlSemanticError::SchemaFenceMismatch {
            expected: profile.schema_fence.clone(),
            found: plan.target.schema_fence.clone(),
        });
    }

    let mut builder = IdmlPackageBuilder::from_export_plan(plan)?;
    let mut frames = Vec::new();
    let mut ordinary_frame_ids = BTreeSet::new();
    let mut tables = BTreeMap::<StoryId, (NodeId, IdmlSimpleTable)>::new();

    for page_id in &graph.document.pages {
        let page = graph
            .pages
            .get(page_id)
            .ok_or(IdmlSemanticError::MissingPage { page_id: *page_id })?;
        if !page.size.is_positive() {
            return Err(IdmlSemanticError::InvalidPageSize { page_id: *page_id });
        }
    }

    for (node_id, node) in &graph.nodes {
        let ordinary_frame = frame_for(*node_id, &node.payload);
        let table = if ordinary_frame.is_none() {
            table_for(*node_id, &node.payload)
        } else {
            None
        };

        let frame = if let Some(frame) = ordinary_frame {
            ordinary_frame_ids.insert(frame.frame_id);
            frame
        } else if let Some(table) = table {
            table.validate()?;
            if !graph.stories.contains_key(&table.story_id) {
                return Err(IdmlSemanticError::MissingStory {
                    node_id: *node_id,
                    story_id: table.story_id,
                });
            }
            if !has_preserved_feature(plan, node_id.into_canonical(), "table.structure") {
                return Err(IdmlSemanticError::UnplannedTableStructure { node_id: *node_id });
            }
            if !has_reported_feature_loss(plan, node_id.into_canonical(), "table.style") {
                return Err(IdmlSemanticError::UnreportedTableStyleDowngrade { node_id: *node_id });
            }
            if tables
                .insert(table.story_id, (*node_id, table.clone()))
                .is_some()
            {
                return Err(IdmlSemanticError::DuplicateTableStory {
                    story_id: table.story_id,
                });
            }

            StoryFrame {
                story_id: table.story_id,
                frame_id: *node_id,
                ordinal: 0,
                previous: None,
                next: None,
            }
        } else {
            if !has_reported_unsupported_omission(plan, node_id.into_canonical()) {
                return Err(IdmlSemanticError::UnplannedNodeOmission { node_id: *node_id });
            }
            continue;
        };

        if frame.frame_id != *node_id {
            return Err(IdmlSemanticError::FrameIdMismatch {
                node_id: *node_id,
                frame_id: frame.frame_id,
            });
        }
        if !graph.stories.contains_key(&frame.story_id) {
            return Err(IdmlSemanticError::MissingStory {
                node_id: *node_id,
                story_id: frame.story_id,
            });
        }

        let page_id = graph
            .pages
            .iter()
            .find_map(|(page_id, page)| {
                (page.id.into_canonical() == node.header.parent_id).then_some(*page_id)
            })
            .ok_or(IdmlSemanticError::FrameParentIsNotPage { node_id: *node_id })?;

        if node.header.bounds.width.get() <= 0 || node.header.bounds.height.get() <= 0 {
            return Err(IdmlSemanticError::InvalidFrameBounds { node_id: *node_id });
        }
        let right = node
            .header
            .bounds
            .right()
            .ok_or(IdmlSemanticError::InvalidFrameBounds { node_id: *node_id })?;
        let bottom = node
            .header
            .bounds
            .bottom()
            .ok_or(IdmlSemanticError::InvalidFrameBounds { node_id: *node_id })?;

        frames.push(FrameProjection {
            page_id,
            frame,
            x: node.header.bounds.x,
            y: node.header.bounds.y,
            right,
            bottom,
        });
    }

    for projection in &frames {
        if ordinary_frame_ids.contains(&projection.frame.frame_id)
            && tables.contains_key(&projection.frame.story_id)
        {
            return Err(IdmlSemanticError::TableStoryAlsoLinked {
                story_id: projection.frame.story_id,
            });
        }
    }

    let flow_frames = frames
        .iter()
        .filter(|item| ordinary_frame_ids.contains(&item.frame.frame_id))
        .map(|item| item.frame.clone())
        .collect::<Vec<_>>();
    let flow_errors = validate_story_frames(&flow_frames);
    if !flow_errors.is_empty() {
        return Err(IdmlSemanticError::InvalidStoryFlow {
            errors: flow_errors,
        });
    }

    for story in graph.stories.values() {
        validate_story_downgrade(plan, story)?;
        let path = story_path(story.id);
        let xml = if let Some((node_id, table)) = tables.get(&story.id) {
            story_xml_with_table(story, *node_id, table, profile)?
        } else {
            story_xml(story, profile)?
        };
        builder.add_part(path, IdmlPartKind::Story, xml)?;
    }

    for page_id in &graph.document.pages {
        let page = graph
            .pages
            .get(page_id)
            .ok_or(IdmlSemanticError::MissingPage { page_id: *page_id })?;
        let page_frames = frames
            .iter()
            .filter(|item| item.page_id == *page_id)
            .collect::<Vec<_>>();
        let path = spread_path(*page_id);
        let xml = spread_xml(page, &page_frames, profile);
        builder.add_part(path, IdmlPartKind::Spread, xml)?;
    }

    let designmap = designmap_xml(graph, profile);
    builder.add_part("designmap.xml", IdmlPartKind::DesignMap, designmap)?;

    builder.finish().map_err(Into::into)
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

fn validate_story_downgrade(plan: &ExportPlan, story: &Story) -> Result<(), IdmlSemanticError> {
    let has_rich_semantics = !story.paragraphs.is_empty()
        || !story.runs.is_empty()
        || !story.fields.is_empty()
        || !story.hyperlinks.is_empty();

    if has_rich_semantics && !has_reported_unsupported_omission(plan, story.id.into_canonical()) {
        return Err(IdmlSemanticError::UnreportedRichStoryDowngrade { story_id: story.id });
    }

    Ok(())
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

fn designmap_xml<
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
    profile: &IdmlWireProfile,
) -> String {
    let mut xml = String::new();
    writeln!(xml, "<?xml version=\"1.0\" encoding=\"utf-8\"?>").unwrap();
    writeln!(
        xml,
        "<?aid style=\"50\" type=\"document\" readerVersion=\"{}\" featureSet=\"{}\"?>",
        profile.aid_reader_version, profile.aid_feature_set
    )
    .unwrap();
    writeln!(
        xml,
        "<Document xmlns:idPkg=\"{}\" DOMVersion=\"{}\" Self=\"{}\">",
        IDML_PACKAGING_NAMESPACE,
        profile.dom_version,
        idml_self("ud", graph.document.id.into_canonical())
    )
    .unwrap();

    for page_id in &graph.document.pages {
        writeln!(xml, "  <idPkg:Spread src=\"{}\"/>", spread_path(*page_id)).unwrap();
    }
    for story_id in graph.stories.keys() {
        writeln!(xml, "  <idPkg:Story src=\"{}\"/>", story_path(*story_id)).unwrap();
    }

    xml.push_str("</Document>\n");
    xml
}

fn story_xml(story: &Story, profile: &IdmlWireProfile) -> Result<String, IdmlSemanticError> {
    let escaped = escape_story_text(story)?;
    let mut xml = String::new();
    writeln!(xml, "<?xml version=\"1.0\" encoding=\"utf-8\"?>").unwrap();
    writeln!(
        xml,
        "<idPkg:Story xmlns:idPkg=\"{}\" DOMVersion=\"{}\">",
        IDML_PACKAGING_NAMESPACE, profile.dom_version
    )
    .unwrap();
    writeln!(
        xml,
        "  <Story Self=\"{}\">",
        idml_self("us", story.id.into_canonical())
    )
    .unwrap();
    xml.push_str(
        "    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\">\n",
    );
    xml.push_str(
        "      <CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\">\n",
    );
    writeln!(xml, "        <Content>{escaped}</Content>").unwrap();
    xml.push_str("      </CharacterStyleRange>\n");
    xml.push_str("    </ParagraphStyleRange>\n");
    xml.push_str("  </Story>\n</idPkg:Story>\n");
    Ok(xml)
}

fn story_xml_with_table(
    story: &Story,
    table_owner: NodeId,
    table: &IdmlSimpleTable,
    profile: &IdmlWireProfile,
) -> Result<String, IdmlSemanticError> {
    if story.id != table.story_id {
        return Err(IdmlSemanticError::MissingStory {
            node_id: table_owner,
            story_id: table.story_id,
        });
    }

    let mut xml = String::new();
    writeln!(xml, "<?xml version=\"1.0\" encoding=\"utf-8\"?>").unwrap();
    writeln!(
        xml,
        "<idPkg:Story xmlns:idPkg=\"{}\" DOMVersion=\"{}\">",
        IDML_PACKAGING_NAMESPACE, profile.dom_version
    )
    .unwrap();
    writeln!(
        xml,
        "  <Story Self=\"{}\">",
        idml_self("us", story.id.into_canonical())
    )
    .unwrap();
    xml.push_str(&table.write_story_body(table_owner.into_canonical())?);
    xml.push_str("  </Story>\n</idPkg:Story>\n");
    Ok(xml)
}

fn spread_xml(
    page: &pub_model::Page,
    frames: &[&FrameProjection],
    profile: &IdmlWireProfile,
) -> String {
    let width = format_emu_points(page.size.width);
    let height = format_emu_points(page.size.height);
    let tx = format_ratio(
        -i128::from(page.size.width.get()),
        i128::from(EMU_PER_POINT),
        15,
    );
    let ty = format_ratio(
        -i128::from(page.size.height.get()),
        i128::from(EMU_PER_POINT) * 2,
        15,
    );

    let mut xml = String::new();
    writeln!(xml, "<?xml version=\"1.0\" encoding=\"utf-8\"?>").unwrap();
    writeln!(
        xml,
        "<idPkg:Spread xmlns:idPkg=\"{}\" DOMVersion=\"{}\">",
        IDML_PACKAGING_NAMESPACE, profile.dom_version
    )
    .unwrap();
    writeln!(
        xml,
        "  <Spread Self=\"{}\" PageCount=\"1\">",
        idml_self("usp", page.id.into_canonical())
    )
    .unwrap();
    writeln!(
        xml,
        "    <Page Self=\"{}\" GeometricBounds=\"0 0 {height} {width}\" ItemTransform=\"1 0 0 1 {tx} {ty}\"/>",
        idml_self("up", page.id.into_canonical())
    )
    .unwrap();

    for projection in frames {
        let frame = &projection.frame;
        let previous = frame
            .previous
            .map(|id| idml_self("uf", id.into_canonical()))
            .unwrap_or_else(|| "n".into());
        let next = frame
            .next
            .map(|id| idml_self("uf", id.into_canonical()))
            .unwrap_or_else(|| "n".into());

        let x = format_emu_points(projection.x);
        let y = format_emu_points(projection.y);
        let right = format_emu_points(projection.right);
        let bottom = format_emu_points(projection.bottom);

        writeln!(
            xml,
            "    <TextFrame Self=\"{}\" ParentStory=\"{}\" PreviousTextFrame=\"{}\" NextTextFrame=\"{}\" ContentType=\"TextType\" ItemTransform=\"1 0 0 1 {tx} {ty}\">",
            idml_self("uf", frame.frame_id.into_canonical()),
            idml_self("us", frame.story_id.into_canonical()),
            previous,
            next,
        )
        .unwrap();
        xml.push_str("      <Properties>\n");
        xml.push_str("        <PathGeometry>\n");
        xml.push_str("          <GeometryPathType PathOpen=\"false\">\n");
        xml.push_str("            <PathPointArray>\n");
        write_path_point(&mut xml, &x, &y);
        write_path_point(&mut xml, &x, &bottom);
        write_path_point(&mut xml, &right, &bottom);
        write_path_point(&mut xml, &right, &y);
        xml.push_str("            </PathPointArray>\n");
        xml.push_str("          </GeometryPathType>\n");
        xml.push_str("        </PathGeometry>\n");
        xml.push_str("      </Properties>\n");
        xml.push_str("    </TextFrame>\n");
    }

    xml.push_str("  </Spread>\n</idPkg:Spread>\n");
    xml
}

fn write_path_point(xml: &mut String, x: &str, y: &str) {
    writeln!(
        xml,
        "              <PathPointType Anchor=\"{x} {y}\" LeftDirection=\"{x} {y}\" RightDirection=\"{x} {y}\"/>"
    )
    .unwrap();
}

fn story_path(story_id: StoryId) -> String {
    let self_id = idml_self("us", story_id.into_canonical());
    format!("Stories/Story_{self_id}.xml")
}

fn spread_path(page_id: PageId) -> String {
    let self_id = idml_self("usp", page_id.into_canonical());
    format!("Spreads/Spread_{self_id}.xml")
}

fn idml_self(prefix: &str, id: CanonicalId) -> String {
    let mut value = String::with_capacity(prefix.len() + 32);
    value.push_str(prefix);
    for byte in id.into_bytes() {
        write!(value, "{byte:02x}").unwrap();
    }
    value
}

fn format_emu_points(value: LengthEmu) -> String {
    format_ratio(i128::from(value.get()), i128::from(EMU_PER_POINT), 15)
}

fn format_ratio(numerator: i128, denominator: i128, precision: usize) -> String {
    debug_assert!(denominator > 0);
    if numerator == 0 {
        return "0".into();
    }

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
    for _ in 0..precision {
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

fn escape_story_text(story: &Story) -> Result<String, IdmlSemanticError> {
    let mut escaped = String::with_capacity(story.text.len());

    for character in story.text.chars() {
        let scalar = u32::from(character);
        if !is_xml_10_scalar(scalar) {
            return Err(IdmlSemanticError::InvalidXmlCharacter {
                story_id: story.id,
                scalar,
            });
        }

        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(character),
        }
    }

    Ok(escaped)
}

fn is_xml_10_scalar(value: u32) -> bool {
    matches!(
        value,
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_export::{
        CapabilityLevel, SemanticFeatureRequest, TargetCapabilityManifest, TargetProfile,
        plan_export,
    };
    use pub_model::{
        Affine2D, Document, DocumentId, Node, NodeHeader, NodeKind, Page, RectEmu, Sha256Digest,
        Size2D, SourceDescriptor,
    };
    use std::collections::{BTreeMap, BTreeSet};

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

    fn source_hash() -> Sha256Digest {
        Sha256Digest::from_bytes([0x55; 32])
    }

    fn plan(extra_requests: Vec<SemanticFeatureRequest>) -> ExportPlan {
        let target = TargetProfile {
            format: "idml".into(),
            adapter_version: "idml-v0.1".into(),
            profile: "bounded-editable".into(),
            schema_fence: Some(IDML_SCHEMA_FENCE_LEGACY_DOM_7.into()),
        };
        let mut features = BTreeMap::new();
        features.insert("page.geometry".into(), CapabilityLevel::Preserved);
        features.insert("story.text".into(), CapabilityLevel::Preserved);
        features.insert("story.linked_frames".into(), CapabilityLevel::Preserved);
        let manifest = TargetCapabilityManifest { target, features };

        let mut requests = vec![
            SemanticFeatureRequest {
                feature: "page.geometry".into(),
                origin: None,
                property_path: None,
                require_preserved: true,
            },
            SemanticFeatureRequest {
                feature: "story.text".into(),
                origin: None,
                property_path: None,
                require_preserved: true,
            },
            SemanticFeatureRequest {
                feature: "story.linked_frames".into(),
                origin: None,
                property_path: None,
                require_preserved: true,
            },
        ];
        requests.extend(extra_requests);
        plan_export(&manifest, requests)
    }

    fn graph_with_frames(frames: Vec<(NodeId, StoryFrame<StoryId, NodeId>, RectEmu)>) -> TestGraph {
        let page = page_id(10);
        let mut stories = BTreeMap::new();
        let story_ids = frames
            .iter()
            .map(|(_, frame, _)| frame.story_id)
            .collect::<BTreeSet<_>>();

        for story_id in story_ids {
            stories.insert(
                story_id,
                Story {
                    id: story_id,
                    text: "Hello & <World>!".into(),
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs: Vec::new(),
                },
            );
        }

        let mut nodes = BTreeMap::new();
        for (node_id, frame, bounds) in frames {
            nodes.insert(
                node_id,
                Node {
                    kind: NodeKind::Shape,
                    header: NodeHeader {
                        id: node_id,
                        parent_id: page.into_canonical(),
                        bounds,
                        transform: Affine2D::identity(),
                        source_refs: Vec::new(),
                        extensions: Vec::new(),
                    },
                    payload: Payload { frame: Some(frame) },
                },
            );
        }

        let mut pages = BTreeMap::new();
        pages.insert(
            page,
            Page {
                id: page,
                size: Size2D::new(
                    LengthEmu::new(612 * EMU_PER_POINT),
                    LengthEmu::new(792 * EMU_PER_POINT),
                ),
                bleed: None,
                margins: None,
                children: Vec::new(),
                extensions: Vec::new(),
            },
        );

        TestGraph {
            cdm_version: "0.1".into(),
            resolver_version: "test-resolver".into(),
            source: SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                adapter_version: "test".into(),
                source_hash: source_hash(),
            },
            document: Document {
                id: DocumentId::from_canonical(id(1)),
                format_origin: "pub".into(),
                source_hash: source_hash(),
                pages: vec![page],
                resources: Vec::new(),
                styles: Vec::new(),
            },
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

    fn cookbook_bounds() -> RectEmu {
        RectEmu::new(
            LengthEmu::new(36 * EMU_PER_POINT),
            LengthEmu::new(36 * EMU_PER_POINT),
            LengthEmu::new((172 - 36) * EMU_PER_POINT),
            LengthEmu::new((186 - 36) * EMU_PER_POINT),
        )
    }

    #[test]
    fn maps_cookbook_geometry_and_live_story_without_float_math() {
        let frame = StoryFrame {
            story_id: story_id(20),
            frame_id: node_id(30),
            ordinal: 0,
            previous: None,
            next: None,
        };
        let graph = graph_with_frames(vec![(node_id(30), frame, cookbook_bounds())]);

        let package = project_resolved_graph_to_idml(
            &plan(Vec::new()),
            &graph,
            &IdmlWireProfile::legacy_dom_7(),
            |_, payload| payload.frame.clone(),
        )
        .expect("bounded semantic projection should succeed");

        let spread = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Spread)
            .expect("spread part");
        assert!(spread.content.contains("GeometricBounds=\"0 0 792 612\""));
        assert!(
            spread
                .content
                .contains("ItemTransform=\"1 0 0 1 -612 -396\"")
        );
        assert!(spread.content.contains("Anchor=\"36 36\""));
        assert!(spread.content.contains("Anchor=\"172 186\""));
        assert!(
            spread
                .content
                .contains("<GeometryPathType PathOpen=\"false\">")
        );
        assert!(spread.content.contains("<PathPointType Anchor=\"36 36\""));
        assert!(!spread.content.contains("<GeometryPath PathOpen="));
        assert!(!spread.content.contains("<PathPoint Anchor="));
        assert!(spread.content.contains("PreviousTextFrame=\"n\""));
        assert!(spread.content.contains("NextTextFrame=\"n\""));

        let story = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Story)
            .expect("story part");
        assert!(story.content.contains(
            "<ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\">"
        ));
        assert!(story.content.contains(
            "<CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\">"
        ));
        assert!(
            story
                .content
                .contains("<Content>Hello &amp; &lt;World&gt;!</Content>")
        );
        assert!(
            story.content.find("<ParagraphStyleRange").unwrap()
                < story.content.find("<CharacterStyleRange").unwrap()
        );
        assert!(
            story.content.find("<CharacterStyleRange").unwrap()
                < story.content.find("<Content>").unwrap()
        );
    }

    #[test]
    fn explicit_flow_links_become_idml_frame_links() {
        let story = story_id(21);
        let first_id = node_id(31);
        let second_id = node_id(32);
        let first = StoryFrame {
            story_id: story,
            frame_id: first_id,
            ordinal: 0,
            previous: None,
            next: Some(second_id),
        };
        let second = StoryFrame {
            story_id: story,
            frame_id: second_id,
            ordinal: 1,
            previous: Some(first_id),
            next: None,
        };
        let graph = graph_with_frames(vec![
            (first_id, first, cookbook_bounds()),
            (
                second_id,
                second,
                RectEmu::new(
                    LengthEmu::new(200 * EMU_PER_POINT),
                    LengthEmu::new(36 * EMU_PER_POINT),
                    LengthEmu::new(100 * EMU_PER_POINT),
                    LengthEmu::new(150 * EMU_PER_POINT),
                ),
            ),
        ]);

        let package = project_resolved_graph_to_idml(
            &plan(Vec::new()),
            &graph,
            &IdmlWireProfile::legacy_dom_7(),
            |_, payload| payload.frame.clone(),
        )
        .expect("linked frames should project");

        let spread = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Spread)
            .expect("spread part");
        let second_self = idml_self("uf", second_id.into_canonical());
        let first_self = idml_self("uf", first_id.into_canonical());
        assert!(
            spread
                .content
                .contains(&format!("NextTextFrame=\"{second_self}\""))
        );
        assert!(
            spread
                .content
                .contains(&format!("PreviousTextFrame=\"{first_self}\""))
        );
    }

    #[test]
    fn unplanned_node_omission_fails_closed() {
        let mut graph = graph_with_frames(Vec::new());
        let page = graph.document.pages[0];
        let orphan = node_id(40);
        graph.nodes.insert(
            orphan,
            Node {
                kind: NodeKind::Shape,
                header: NodeHeader {
                    id: orphan,
                    parent_id: page.into_canonical(),
                    bounds: cookbook_bounds(),
                    transform: Affine2D::identity(),
                    source_refs: Vec::new(),
                    extensions: Vec::new(),
                },
                payload: Payload { frame: None },
            },
        );

        let result = project_resolved_graph_to_idml(
            &plan(Vec::new()),
            &graph,
            &IdmlWireProfile::legacy_dom_7(),
            |_, payload| payload.frame.clone(),
        );

        assert!(matches!(
            result,
            Err(IdmlSemanticError::UnplannedNodeOmission { node_id }) if node_id == orphan
        ));
    }

    #[test]
    fn explicit_unsupported_loss_allows_bounded_omission() {
        let mut graph = graph_with_frames(Vec::new());
        let page = graph.document.pages[0];
        let orphan = node_id(41);
        graph.nodes.insert(
            orphan,
            Node {
                kind: NodeKind::Shape,
                header: NodeHeader {
                    id: orphan,
                    parent_id: page.into_canonical(),
                    bounds: cookbook_bounds(),
                    transform: Affine2D::identity(),
                    source_refs: Vec::new(),
                    extensions: Vec::new(),
                },
                payload: Payload { frame: None },
            },
        );

        let request = SemanticFeatureRequest {
            feature: "shape.generic".into(),
            origin: Some(orphan.into_canonical()),
            property_path: None,
            require_preserved: false,
        };
        let package = project_resolved_graph_to_idml(
            &plan(vec![request]),
            &graph,
            &IdmlWireProfile::legacy_dom_7(),
            |_, payload| payload.frame.clone(),
        )
        .expect("reported unsupported node may be omitted");

        assert!(
            package
                .parts
                .iter()
                .any(|part| part.kind == IdmlPartKind::Spread)
        );
    }
}

mod authored_rectangle_overlay_v1 {
    use crate::{IDML_PACKAGING_NAMESPACE, IdmlPackage, IdmlPart, IdmlPartContent, IdmlPartKind};
    use pub_export::{
        AUTHORED_SHAPE_FILL_FEATURE, AUTHORED_SHAPE_GEOMETRY_FEATURE,
        AUTHORED_SHAPE_STROKE_FEATURE, AUTHORED_SHAPE_Z_ORDER_FEATURE, AuthoredRectangleExportV1,
        CapabilityLevel, ExportPlan, ExportSrgb8V1,
    };
    use pub_model::{CanonicalId, EMU_PER_POINT, LengthEmu, NodeId, PageId};
    use std::collections::BTreeSet;
    use std::fmt;
    use std::fmt::Write as _;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum IdmlAuthoredShapeError {
        NonIdmlPackage,
        DuplicateRectangle {
            node_id: NodeId,
        },
        InvalidPageSize {
            page_id: PageId,
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
        MissingSpread {
            page_id: PageId,
        },
        BinarySpread {
            page_id: PageId,
        },
        InvalidSpreadXml {
            page_id: PageId,
        },
        MissingDesignMap,
        BinaryDesignMap,
        UnexpectedDesignMapMarkup,
        DuplicateGraphicResource,
    }

    impl fmt::Display for IdmlAuthoredShapeError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::NonIdmlPackage => f.write_str("authored rectangle projection requires IDML"),
                Self::DuplicateRectangle { node_id } => {
                    write!(f, "duplicate authored rectangle {node_id}")
                }
                Self::InvalidPageSize { page_id } => {
                    write!(f, "authored rectangle page {page_id} has invalid size")
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
                Self::MissingSpread { page_id } => {
                    write!(f, "IDML spread missing for page {page_id}")
                }
                Self::BinarySpread { page_id } => {
                    write!(f, "IDML spread for page {page_id} is binary")
                }
                Self::InvalidSpreadXml { page_id } => write!(
                    f,
                    "IDML spread for page {page_id} is not overlay-compatible"
                ),
                Self::MissingDesignMap => f.write_str("IDML designmap.xml missing"),
                Self::BinaryDesignMap => f.write_str("IDML designmap.xml is binary"),
                Self::UnexpectedDesignMapMarkup => {
                    f.write_str("IDML designmap Document root is not overlay-compatible")
                }
                Self::DuplicateGraphicResource => f.write_str(
                    "IDML already carries Resources/Graphic.xml outside authored-shape overlay",
                ),
            }
        }
    }

    impl std::error::Error for IdmlAuthoredShapeError {}

    pub fn add_authored_rectangles_to_idml(
        plan: &ExportPlan,
        package: &mut IdmlPackage,
        rectangles: &[AuthoredRectangleExportV1],
    ) -> Result<(), IdmlAuthoredShapeError> {
        if plan.target.format != "idml" || package.target.format != "idml" {
            return Err(IdmlAuthoredShapeError::NonIdmlPackage);
        }

        let mut ordered = rectangles.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|item| (item.page_id, item.node_id));
        let mut seen = BTreeSet::new();
        let mut colors = BTreeSet::new();

        for item in &ordered {
            if !seen.insert(item.node_id) {
                return Err(IdmlAuthoredShapeError::DuplicateRectangle {
                    node_id: item.node_id,
                });
            }
            validate(plan, item)?;
            if item.fill.visible {
                colors.insert(item.fill.color);
            }
            if item.stroke.visible {
                colors.insert(item.stroke.color);
            }
        }

        if !colors.is_empty() {
            add_graphic_resource(package, &colors)?;
        }

        for item in ordered {
            let spread_path = spread_path(item.page_id);
            let spread = package
                .parts
                .iter_mut()
                .find(|part| part.kind == IdmlPartKind::Spread && part.path == spread_path)
                .ok_or(IdmlAuthoredShapeError::MissingSpread {
                    page_id: item.page_id,
                })?;
            let IdmlPartContent::Text(xml) = &mut spread.content else {
                return Err(IdmlAuthoredShapeError::BinarySpread {
                    page_id: item.page_id,
                });
            };
            let marker = "  </Spread>";
            let Some(insert_at) = xml.rfind(marker) else {
                return Err(IdmlAuthoredShapeError::InvalidSpreadXml {
                    page_id: item.page_id,
                });
            };
            xml.insert_str(insert_at, &rectangle_xml(item)?);
        }
        Ok(())
    }

    fn validate(
        plan: &ExportPlan,
        item: &AuthoredRectangleExportV1,
    ) -> Result<(), IdmlAuthoredShapeError> {
        if !item.page_size.is_positive() {
            return Err(IdmlAuthoredShapeError::InvalidPageSize {
                page_id: item.page_id,
            });
        }
        if item.bounds.width.get() <= 0
            || item.bounds.height.get() <= 0
            || item.bounds.right().is_none()
            || item.bounds.bottom().is_none()
        {
            return Err(IdmlAuthoredShapeError::InvalidBounds {
                node_id: item.node_id,
            });
        }
        if item.stroke.width_emu.get() <= 0 {
            return Err(IdmlAuthoredShapeError::InvalidStroke {
                node_id: item.node_id,
            });
        }
        for feature in [
            AUTHORED_SHAPE_GEOMETRY_FEATURE,
            AUTHORED_SHAPE_FILL_FEATURE,
            AUTHORED_SHAPE_STROKE_FEATURE,
        ] {
            if !has_preserved_feature(plan, item.node_id.into_canonical(), feature) {
                return Err(IdmlAuthoredShapeError::MissingPreservedFeature {
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
            return Err(IdmlAuthoredShapeError::MissingZOrderLoss {
                node_id: item.node_id,
            });
        }
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

    fn add_graphic_resource(
        package: &mut IdmlPackage,
        colors: &BTreeSet<ExportSrgb8V1>,
    ) -> Result<(), IdmlAuthoredShapeError> {
        if package
            .parts
            .iter()
            .any(|part| part.path == "Resources/Graphic.xml")
        {
            return Err(IdmlAuthoredShapeError::DuplicateGraphicResource);
        }
        let designmap = package
            .parts
            .iter_mut()
            .find(|part| part.kind == IdmlPartKind::DesignMap && part.path == "designmap.xml")
            .ok_or(IdmlAuthoredShapeError::MissingDesignMap)?;
        let IdmlPartContent::Text(xml) = &mut designmap.content else {
            return Err(IdmlAuthoredShapeError::BinaryDesignMap);
        };
        if xml.contains("<idPkg:Graphic") {
            return Err(IdmlAuthoredShapeError::DuplicateGraphicResource);
        }
        let document_start = xml
            .find("<Document")
            .ok_or(IdmlAuthoredShapeError::UnexpectedDesignMapMarkup)?;
        let open_end = xml[document_start..]
            .find('>')
            .map(|relative| document_start + relative)
            .ok_or(IdmlAuthoredShapeError::UnexpectedDesignMapMarkup)?;
        if xml[..=open_end].ends_with("/>") {
            return Err(IdmlAuthoredShapeError::UnexpectedDesignMapMarkup);
        }
        xml.insert_str(
            open_end + 1,
            "\n  <idPkg:Graphic src=\"Resources/Graphic.xml\"/>",
        );

        let mut graphic = String::new();
        writeln!(graphic, "<?xml version=\"1.0\" encoding=\"utf-8\"?>").unwrap();
        writeln!(
            graphic,
            "<idPkg:Graphic xmlns:idPkg=\"{}\" DOMVersion=\"7.0\">",
            IDML_PACKAGING_NAMESPACE
        )
        .unwrap();
        for color in colors {
            let id = color_id(*color);
            writeln!(
                graphic,
                "  <Color Self=\"{id}\" Model=\"Process\" Space=\"RGB\" ColorValue=\"{} {} {}\" ColorOverride=\"Normal\" Name=\"{}\" ColorEditable=\"true\" ColorRemovable=\"true\" Visible=\"true\"/>",
                color.r,
                color.g,
                color.b,
                color_name(*color),
            )
            .unwrap();
        }
        graphic.push_str("</idPkg:Graphic>\n");
        package.parts.push(IdmlPart {
            path: "Resources/Graphic.xml".into(),
            kind: IdmlPartKind::Resource,
            content: IdmlPartContent::Text(graphic),
        });
        package.parts.sort();
        Ok(())
    }

    fn rectangle_xml(item: &AuthoredRectangleExportV1) -> Result<String, IdmlAuthoredShapeError> {
        let right = item
            .bounds
            .right()
            .ok_or(IdmlAuthoredShapeError::InvalidBounds {
                node_id: item.node_id,
            })?;
        let bottom = item
            .bounds
            .bottom()
            .ok_or(IdmlAuthoredShapeError::InvalidBounds {
                node_id: item.node_id,
            })?;

        let x = format_emu_points(item.bounds.x);
        let y = format_emu_points(item.bounds.y);
        let right = format_emu_points(right);
        let bottom = format_emu_points(bottom);
        let tx = format_ratio(
            -i128::from(item.page_size.width.get()),
            i128::from(EMU_PER_POINT),
            15,
        );
        let ty = format_ratio(
            -i128::from(item.page_size.height.get()),
            i128::from(EMU_PER_POINT) * 2,
            15,
        );
        let fill = if item.fill.visible {
            color_id(item.fill.color)
        } else {
            "Swatch/None".into()
        };
        let stroke = if item.stroke.visible {
            color_id(item.stroke.color)
        } else {
            "Swatch/None".into()
        };
        let stroke_weight = if item.stroke.visible {
            format_emu_points(item.stroke.width_emu)
        } else {
            "0".into()
        };

        let mut xml = String::new();
        writeln!(
            xml,
            "    <Rectangle Self=\"{}\" ContentType=\"GraphicType\" AppliedObjectStyle=\"ObjectStyle/$ID/[None]\" Visible=\"true\" ItemTransform=\"1 0 0 1 {tx} {ty}\" FillColor=\"{fill}\" StrokeColor=\"{stroke}\" StrokeWeight=\"{stroke_weight}\">",
            idml_self("uar", item.node_id.into_canonical()),
        )
        .unwrap();
        xml.push_str("      <Properties>\n");
        xml.push_str("        <PathGeometry>\n");
        xml.push_str("          <GeometryPathType PathOpen=\"false\">\n");
        xml.push_str("            <PathPointArray>\n");
        write_path_point(&mut xml, &x, &y);
        write_path_point(&mut xml, &x, &bottom);
        write_path_point(&mut xml, &right, &bottom);
        write_path_point(&mut xml, &right, &y);
        xml.push_str("            </PathPointArray>\n");
        xml.push_str("          </GeometryPathType>\n");
        xml.push_str("        </PathGeometry>\n");
        xml.push_str("      </Properties>\n");
        xml.push_str("    </Rectangle>\n");
        Ok(xml)
    }

    fn color_name(color: ExportSrgb8V1) -> String {
        format!("Chaptera-{:02X}{:02X}{:02X}", color.r, color.g, color.b)
    }

    fn color_id(color: ExportSrgb8V1) -> String {
        format!("Color/{}", color_name(color))
    }

    fn write_path_point(xml: &mut String, x: &str, y: &str) {
        writeln!(
            xml,
            "              <PathPointType Anchor=\"{x} {y}\" LeftDirection=\"{x} {y}\" RightDirection=\"{x} {y}\"/>"
        )
        .unwrap();
    }

    fn spread_path(page_id: PageId) -> String {
        format!(
            "Spreads/Spread_{}.xml",
            idml_self("usp", page_id.into_canonical())
        )
    }

    fn idml_self(prefix: &str, id: CanonicalId) -> String {
        let mut value = String::with_capacity(prefix.len() + 32);
        value.push_str(prefix);
        for byte in id.into_bytes() {
            write!(value, "{byte:02x}").unwrap();
        }
        value
    }

    fn format_emu_points(value: LengthEmu) -> String {
        format_ratio(i128::from(value.get()), i128::from(EMU_PER_POINT), 15)
    }

    fn format_ratio(numerator: i128, denominator: i128, precision: usize) -> String {
        debug_assert!(denominator > 0);
        if numerator == 0 {
            return "0".into();
        }
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
        for _ in 0..precision {
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
        use crate::{IDML_ADAPTER_VERSION_V0_1, IdmlPackageBuilder};
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
                    format: "idml".into(),
                    adapter_version: IDML_ADAPTER_VERSION_V0_1.into(),
                    profile: "bounded-editable".into(),
                    schema_fence: Some("legacy-spec-8.02/dom-7.0".into()),
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

        fn package(item: &AuthoredRectangleExportV1, plan: &ExportPlan) -> IdmlPackage {
            let mut builder = IdmlPackageBuilder::from_export_plan(plan).unwrap();
            builder
                .add_part(
                    "designmap.xml",
                    IdmlPartKind::DesignMap,
                    format!(
                        "<Document xmlns:idPkg=\"{}\" DOMVersion=\"7.0\">\n</Document>\n",
                        IDML_PACKAGING_NAMESPACE
                    ),
                )
                .unwrap();
            builder
                .add_part(
                    spread_path(item.page_id),
                    IdmlPartKind::Spread,
                    format!(
                        "<idPkg:Spread xmlns:idPkg=\"{}\"><Spread Self=\"x\">\n  </Spread></idPkg:Spread>\n",
                        IDML_PACKAGING_NAMESPACE
                    ),
                )
                .unwrap();
            builder.finish().unwrap()
        }

        #[test]
        fn writes_rgb_resources_rectangle_geometry_and_explicit_z_order_loss() {
            let item = rectangle();
            let plan = plan(&item);
            assert!(plan.losses.iter().any(|loss| {
                loss.origin == Some(item.node_id.into_canonical())
                    && loss.feature == AUTHORED_SHAPE_Z_ORDER_FEATURE
            }));
            let mut package = package(&item, &plan);
            add_authored_rectangles_to_idml(&plan, &mut package, std::slice::from_ref(&item))
                .expect("authored rectangle");

            let designmap = package
                .parts
                .iter()
                .find(|part| part.path == "designmap.xml")
                .and_then(|part| part.content.as_text())
                .unwrap();
            assert!(designmap.contains("<idPkg:Graphic src=\"Resources/Graphic.xml\"/>"));

            let graphic = package
                .parts
                .iter()
                .find(|part| part.path == "Resources/Graphic.xml")
                .and_then(|part| part.content.as_text())
                .unwrap();
            assert!(graphic.contains("Space=\"RGB\" ColorValue=\"18 52 86\""));
            assert!(graphic.contains("Space=\"RGB\" ColorValue=\"170 187 204\""));

            let spread = package
                .parts
                .iter()
                .find(|part| part.kind == IdmlPartKind::Spread)
                .and_then(|part| part.content.as_text())
                .unwrap();
            assert!(spread.contains("<Rectangle Self=\"uar"));
            assert!(spread.contains("FillColor=\"Color/Chaptera-123456\""));
            assert!(spread.contains("StrokeColor=\"Color/Chaptera-AABBCC\""));
            assert!(spread.contains("StrokeWeight=\"1\""));
            assert!(spread.contains("Anchor=\"100 200\""));
            assert!(spread.contains("Anchor=\"300 300\""));
        }

        #[test]
        fn invisible_paint_uses_none_without_fake_color_resource() {
            let mut item = rectangle();
            item.fill.visible = false;
            item.stroke.visible = false;
            let plan = plan(&item);
            let mut package = package(&item, &plan);
            add_authored_rectangles_to_idml(&plan, &mut package, std::slice::from_ref(&item))
                .expect("authored rectangle");
            assert!(
                package
                    .parts
                    .iter()
                    .all(|part| part.path != "Resources/Graphic.xml")
            );
            let spread = package
                .parts
                .iter()
                .find(|part| part.kind == IdmlPartKind::Spread)
                .and_then(|part| part.content.as_text())
                .unwrap();
            assert!(spread.contains("FillColor=\"Swatch/None\""));
            assert!(spread.contains("StrokeColor=\"Swatch/None\""));
            assert!(spread.contains("StrokeWeight=\"0\""));
        }
    }
}

pub use authored_rectangle_overlay_v1::{IdmlAuthoredShapeError, add_authored_rectangles_to_idml};
