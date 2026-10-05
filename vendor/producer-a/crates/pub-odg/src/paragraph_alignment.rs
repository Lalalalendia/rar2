use crate::{ODG_CONTENT_PATH, OdgPackage, OdgPartKind};
use pub_export::{
    ExportPlan, FullStoryParagraphAlignmentV1, ParagraphAlignmentV1, ParagraphScopedAlignmentV1,
    ParagraphScopedAlignmentValueV1, STORY_PARAGRAPH_ALIGNMENT_FEATURE,
};
use pub_model::{CanonicalId, NodeId, StoryId};
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OdgFullStoryParagraphAlignmentPlacement {
    pub alignment: FullStoryParagraphAlignmentV1,
    pub frame_ids: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OdgParagraphScopedAlignmentPlacement {
    pub story_id: StoryId,
    pub paragraphs: Vec<ParagraphScopedAlignmentV1>,
    pub frame_ids: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OdgParagraphAlignmentError {
    NonOdgPackage,
    DuplicateStory { story_id: StoryId },
    DuplicateFrame { node_id: NodeId },
    EmptyFrames { story_id: StoryId },
    MissingPlannedFeature { story_id: StoryId },
    MissingContent,
    InvalidContentUtf8,
    InvalidAutomaticStyles,
    MissingFrame { node_id: NodeId },
    MissingTextCarrier { node_id: NodeId },
    ExistingParagraphStyle { node_id: NodeId },
    EmptyParagraphs { story_id: StoryId },
    ParagraphStoryMismatch {
        story_id: StoryId,
        paragraph_id: pub_model::ParagraphId,
        paragraph_story_id: StoryId,
    },
    DuplicateParagraph {
        paragraph_id: pub_model::ParagraphId,
    },
    NonMonotonicParagraphRanges {
        story_id: StoryId,
    },
    ParagraphCarrierCountMismatch {
        node_id: NodeId,
        expected: usize,
        found: usize,
    },
    UnexpectedExtraParagraphCarrier {
        node_id: NodeId,
    },
}

impl fmt::Display for OdgParagraphAlignmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonOdgPackage => {
                formatter.write_str("full-Story paragraph alignment requires an ODG package")
            }
            Self::DuplicateStory { story_id } => write!(
                formatter,
                "duplicate full-Story paragraph alignment for {}",
                story_id.as_canonical()
            ),
            Self::DuplicateFrame { node_id } => write!(
                formatter,
                "duplicate ODG paragraph-alignment frame {}",
                node_id.as_canonical()
            ),
            Self::EmptyFrames { story_id } => write!(
                formatter,
                "Story {} has no ODG text frames for paragraph alignment",
                story_id.as_canonical()
            ),
            Self::MissingPlannedFeature { story_id } => write!(
                formatter,
                "Story {} has no planned paragraph-alignment feature",
                story_id.as_canonical()
            ),
            Self::MissingContent => formatter.write_str("ODG package is missing content.xml"),
            Self::InvalidContentUtf8 => formatter.write_str("ODG content.xml is not UTF-8"),
            Self::InvalidAutomaticStyles => {
                formatter.write_str("ODG content.xml has an unsupported automatic-styles shape")
            }
            Self::MissingFrame { node_id } => write!(
                formatter,
                "ODG content.xml is missing text frame {}",
                node_id.as_canonical()
            ),
            Self::MissingTextCarrier { node_id } => write!(
                formatter,
                "ODG text frame {} carries no bounded text paragraphs",
                node_id.as_canonical()
            ),
            Self::ExistingParagraphStyle { node_id } => write!(
                formatter,
                "ODG text frame {} already contains paragraph style references",
                node_id.as_canonical()
            ),
            Self::EmptyParagraphs { story_id } => write!(
                formatter,
                "Story {} has no canonical ParagraphId alignment input",
                story_id.as_canonical()
            ),
            Self::ParagraphStoryMismatch {
                story_id,
                paragraph_id,
                paragraph_story_id,
            } => write!(
                formatter,
                "Paragraph {} belongs to Story {}, expected {}",
                paragraph_id.as_canonical(),
                paragraph_story_id.as_canonical(),
                story_id.as_canonical()
            ),
            Self::DuplicateParagraph { paragraph_id } => write!(
                formatter,
                "duplicate paragraph-scoped alignment for {}",
                paragraph_id.as_canonical()
            ),
            Self::NonMonotonicParagraphRanges { story_id } => write!(
                formatter,
                "Story {} paragraph ranges are not strictly ordered and non-overlapping",
                story_id.as_canonical()
            ),
            Self::ParagraphCarrierCountMismatch {
                node_id,
                expected,
                found,
            } => write!(
                formatter,
                "ODG text frame {} has {found} physical paragraph carriers, expected {expected} canonical paragraphs or one trailing empty legacy carrier",
                node_id.as_canonical()
            ),
            Self::UnexpectedExtraParagraphCarrier { node_id } => write!(
                formatter,
                "ODG text frame {} has an extra non-empty paragraph carrier",
                node_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for OdgParagraphAlignmentError {}

pub fn add_full_story_paragraph_alignment_to_odg(
    plan: &ExportPlan,
    package: &mut OdgPackage,
    placements: &[OdgFullStoryParagraphAlignmentPlacement],
) -> Result<(), OdgParagraphAlignmentError> {
    if package.target.format != "odg" || plan.target.format != "odg" {
        return Err(OdgParagraphAlignmentError::NonOdgPackage);
    }

    let content_index = package
        .parts
        .iter()
        .position(|part| part.kind == OdgPartKind::Content && part.path == ODG_CONTENT_PATH)
        .ok_or(OdgParagraphAlignmentError::MissingContent)?;
    let mut xml = String::from_utf8(package.parts[content_index].content.clone())
        .map_err(|_| OdgParagraphAlignmentError::InvalidContentUtf8)?;

    let mut ordered = placements.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|placement| placement.alignment.story_id);

    let mut seen_stories = BTreeSet::new();
    let mut seen_frames = BTreeSet::new();
    let mut style_xml = String::new();

    for placement in ordered {
        let item = &placement.alignment;
        if !seen_stories.insert(item.story_id) {
            return Err(OdgParagraphAlignmentError::DuplicateStory {
                story_id: item.story_id,
            });
        }
        if !has_planned_feature(plan, item.story_id.into_canonical()) {
            return Err(OdgParagraphAlignmentError::MissingPlannedFeature {
                story_id: item.story_id,
            });
        }
        if placement.frame_ids.is_empty() {
            return Err(OdgParagraphAlignmentError::EmptyFrames {
                story_id: item.story_id,
            });
        }

        let style_name = style_name(item.story_id);
        let value = match item.alignment {
            ParagraphAlignmentV1::Center => "center",
            ParagraphAlignmentV1::Right => "right",
        };
        writeln!(
            style_xml,
            "    <style:style style:name=\"{style_name}\" style:family=\"paragraph\">"
        )
        .unwrap();
        writeln!(
            style_xml,
            "      <style:paragraph-properties fo:text-align=\"{value}\"/>"
        )
        .unwrap();
        style_xml.push_str("    </style:style>\n");

        let mut frame_ids = placement.frame_ids.clone();
        frame_ids.sort_unstable();
        for frame_id in frame_ids {
            if !seen_frames.insert(frame_id) {
                return Err(OdgParagraphAlignmentError::DuplicateFrame { node_id: frame_id });
            }
            apply_style_to_frame(&mut xml, frame_id, &style_name)?;
        }
    }

    add_automatic_styles(&mut xml, &style_xml)?;
    package.parts[content_index].content = xml.into_bytes();
    Ok(())
}

pub fn add_paragraph_scoped_alignment_to_odg(
    plan: &ExportPlan,
    package: &mut OdgPackage,
    placements: &[OdgParagraphScopedAlignmentPlacement],
) -> Result<(), OdgParagraphAlignmentError> {
    if package.target.format != "odg" || plan.target.format != "odg" {
        return Err(OdgParagraphAlignmentError::NonOdgPackage);
    }

    let content_index = package
        .parts
        .iter()
        .position(|part| part.kind == OdgPartKind::Content && part.path == ODG_CONTENT_PATH)
        .ok_or(OdgParagraphAlignmentError::MissingContent)?;
    let mut xml = String::from_utf8(package.parts[content_index].content.clone())
        .map_err(|_| OdgParagraphAlignmentError::InvalidContentUtf8)?;

    let mut ordered = placements.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|placement| placement.story_id);

    let mut seen_stories = BTreeSet::new();
    let mut seen_frames = BTreeSet::new();
    let mut seen_paragraphs = BTreeSet::new();
    let mut style_xml = String::new();

    for placement in ordered {
        if !seen_stories.insert(placement.story_id) {
            return Err(OdgParagraphAlignmentError::DuplicateStory {
                story_id: placement.story_id,
            });
        }
        if !has_planned_feature(plan, placement.story_id.into_canonical()) {
            return Err(OdgParagraphAlignmentError::MissingPlannedFeature {
                story_id: placement.story_id,
            });
        }
        if placement.frame_ids.is_empty() {
            return Err(OdgParagraphAlignmentError::EmptyFrames {
                story_id: placement.story_id,
            });
        }
        if placement.paragraphs.is_empty() {
            return Err(OdgParagraphAlignmentError::EmptyParagraphs {
                story_id: placement.story_id,
            });
        }

        let mut paragraphs = placement.paragraphs.clone();
        paragraphs.sort_by_key(|paragraph| {
            (
                paragraph.range.start,
                paragraph.range.end,
                paragraph.paragraph_id,
            )
        });

        let mut previous_end = None;
        for paragraph in &paragraphs {
            if paragraph.story_id != placement.story_id {
                return Err(OdgParagraphAlignmentError::ParagraphStoryMismatch {
                    story_id: placement.story_id,
                    paragraph_id: paragraph.paragraph_id,
                    paragraph_story_id: paragraph.story_id,
                });
            }
            if !seen_paragraphs.insert(paragraph.paragraph_id) {
                return Err(OdgParagraphAlignmentError::DuplicateParagraph {
                    paragraph_id: paragraph.paragraph_id,
                });
            }
            let contiguous = match previous_end {
                None => paragraph.range.start == 0,
                Some(end) => paragraph.range.start == end,
            };
            if !contiguous || paragraph.range.end < paragraph.range.start {
                return Err(OdgParagraphAlignmentError::NonMonotonicParagraphRanges {
                    story_id: placement.story_id,
                });
            }
            previous_end = Some(paragraph.range.end);

            let style_name = paragraph_style_name(paragraph.paragraph_id);
            let value = match paragraph.alignment {
                ParagraphScopedAlignmentValueV1::Left => "left",
                ParagraphScopedAlignmentValueV1::Center => "center",
                ParagraphScopedAlignmentValueV1::Right => "right",
            };
            writeln!(
                style_xml,
                "    <style:style style:name=\"{style_name}\" style:family=\"paragraph\">"
            )
            .unwrap();
            writeln!(
                style_xml,
                "      <style:paragraph-properties fo:text-align=\"{value}\"/>"
            )
            .unwrap();
            style_xml.push_str("    </style:style>\n");
        }

        let mut frame_ids = placement.frame_ids.clone();
        frame_ids.sort_unstable();
        frame_ids.dedup();
        for frame_id in frame_ids {
            if !seen_frames.insert(frame_id) {
                return Err(OdgParagraphAlignmentError::DuplicateFrame { node_id: frame_id });
            }
            apply_scoped_styles_to_frame(&mut xml, frame_id, &paragraphs)?;
        }
    }

    add_automatic_styles(&mut xml, &style_xml)?;
    package.parts[content_index].content = xml.into_bytes();
    Ok(())
}

fn apply_scoped_styles_to_frame(
    xml: &mut String,
    frame_id: NodeId,
    paragraphs: &[ParagraphScopedAlignmentV1],
) -> Result<(), OdgParagraphAlignmentError> {
    let marker = format!("<draw:frame draw:name=\"{}\"", frame_name(frame_id));
    let frame_start = xml
        .find(&marker)
        .ok_or(OdgParagraphAlignmentError::MissingFrame { node_id: frame_id })?;
    let close_marker = "        </draw:frame>";
    let relative_close = xml[frame_start..]
        .find(close_marker)
        .ok_or(OdgParagraphAlignmentError::MissingFrame { node_id: frame_id })?;
    let frame_end = frame_start + relative_close + close_marker.len();

    let mut block = xml[frame_start..frame_end].to_owned();
    if block.contains("<text:p text:style-name=") {
        return Err(OdgParagraphAlignmentError::ExistingParagraphStyle { node_id: frame_id });
    }

    let carriers = paragraph_carriers(&block, frame_id)?;
    if carriers.len() < paragraphs.len() || carriers.len() > paragraphs.len() + 1 {
        return Err(OdgParagraphAlignmentError::ParagraphCarrierCountMismatch {
            node_id: frame_id,
            expected: paragraphs.len(),
            found: carriers.len(),
        });
    }
    if carriers.len() == paragraphs.len() + 1 && !carriers.last().is_some_and(|item| item.2) {
        return Err(OdgParagraphAlignmentError::UnexpectedExtraParagraphCarrier {
            node_id: frame_id,
        });
    }

    for (index, paragraph) in paragraphs.iter().enumerate().rev() {
        let (open_start, _, _) = carriers[index];
        let open_end = open_start + "<text:p>".len();
        let replacement = format!(
            "<text:p text:style-name=\"{}\">",
            paragraph_style_name(paragraph.paragraph_id)
        );
        block.replace_range(open_start..open_end, &replacement);
    }

    xml.replace_range(frame_start..frame_end, &block);
    Ok(())
}

fn paragraph_carriers(
    block: &str,
    frame_id: NodeId,
) -> Result<Vec<(usize, usize, bool)>, OdgParagraphAlignmentError> {
    let mut carriers = Vec::new();
    let mut cursor = 0;
    let open_marker = "<text:p>";
    let close_marker = "</text:p>";

    while let Some(relative_open) = block[cursor..].find(open_marker) {
        let open_start = cursor + relative_open;
        let content_start = open_start + open_marker.len();
        let relative_close = block[content_start..]
            .find(close_marker)
            .ok_or(OdgParagraphAlignmentError::MissingTextCarrier { node_id: frame_id })?;
        let content_end = content_start + relative_close;
        let carrier_end = content_end + close_marker.len();
        carriers.push((
            open_start,
            carrier_end,
            block[content_start..content_end].is_empty(),
        ));
        cursor = carrier_end;
    }

    if carriers.is_empty() {
        return Err(OdgParagraphAlignmentError::MissingTextCarrier { node_id: frame_id });
    }
    Ok(carriers)
}

fn paragraph_style_name(paragraph_id: pub_model::ParagraphId) -> String {
    stable_name("PubParagraphP", paragraph_id.into_canonical())
}

fn add_automatic_styles(
    xml: &mut String,
    style_xml: &str,
) -> Result<(), OdgParagraphAlignmentError> {
    if style_xml.is_empty() {
        return Ok(());
    }

    let empty_marker = "  <office:automatic-styles/>\n";
    if xml.matches(empty_marker).count() == 1 {
        let replacement =
            format!("  <office:automatic-styles>\n{style_xml}  </office:automatic-styles>\n");
        *xml = xml.replacen(empty_marker, &replacement, 1);
        return Ok(());
    }

    let open_marker = "  <office:automatic-styles>\n";
    let close_marker = "  </office:automatic-styles>\n";
    if xml.matches(open_marker).count() != 1 || xml.matches(close_marker).count() != 1 {
        return Err(OdgParagraphAlignmentError::InvalidAutomaticStyles);
    }
    let close = xml
        .find(close_marker)
        .ok_or(OdgParagraphAlignmentError::InvalidAutomaticStyles)?;
    xml.insert_str(close, style_xml);
    Ok(())
}

fn has_planned_feature(plan: &ExportPlan, origin: CanonicalId) -> bool {
    plan.features.iter().any(|planned| {
        planned.request.origin == Some(origin)
            && planned.request.feature == STORY_PARAGRAPH_ALIGNMENT_FEATURE
    })
}

fn apply_style_to_frame(
    xml: &mut String,
    frame_id: NodeId,
    style_name: &str,
) -> Result<(), OdgParagraphAlignmentError> {
    let marker = format!("<draw:frame draw:name=\"{}\"", frame_name(frame_id));
    let frame_start = xml
        .find(&marker)
        .ok_or(OdgParagraphAlignmentError::MissingFrame { node_id: frame_id })?;
    let close_marker = "        </draw:frame>";
    let relative_close = xml[frame_start..]
        .find(close_marker)
        .ok_or(OdgParagraphAlignmentError::MissingFrame { node_id: frame_id })?;
    let frame_end = frame_start + relative_close + close_marker.len();

    let mut block = xml[frame_start..frame_end].to_owned();
    if block.contains("<text:p text:style-name=") {
        return Err(OdgParagraphAlignmentError::ExistingParagraphStyle { node_id: frame_id });
    }
    if !block.contains("<text:p>") {
        return Err(OdgParagraphAlignmentError::MissingTextCarrier { node_id: frame_id });
    }

    block = block.replace(
        "<text:p>",
        &format!("<text:p text:style-name=\"{style_name}\">"),
    );
    xml.replace_range(frame_start..frame_end, &block);
    Ok(())
}

fn style_name(story_id: StoryId) -> String {
    stable_name("PubStoryP", story_id.into_canonical())
}

fn frame_name(frame_id: NodeId) -> String {
    stable_name("Frame", frame_id.into_canonical())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ODG_ADAPTER_VERSION_V0_1, ODG_SCHEMA_FENCE_ODF_1_4, OdgPart, OdgPartKind};
    use pub_export::{
        SemanticFeatureRequest, TargetCapabilityManifest, TargetProfile, plan_export,
    };
    use pub_model::CanonicalId;
    use std::collections::BTreeMap;

    fn id(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn story(byte: u8) -> StoryId {
        StoryId::from_canonical(id(byte))
    }

    fn frame(byte: u8) -> NodeId {
        NodeId::from_canonical(id(byte))
    }

    fn plan(story_id: StoryId) -> ExportPlan {
        let target = TargetProfile {
            format: "odg".into(),
            adapter_version: ODG_ADAPTER_VERSION_V0_1.into(),
            profile: "bounded-editable".into(),
            schema_fence: Some(ODG_SCHEMA_FENCE_ODF_1_4.into()),
        };
        plan_export(
            &TargetCapabilityManifest {
                target,
                features: BTreeMap::new(),
            },
            vec![SemanticFeatureRequest {
                feature: STORY_PARAGRAPH_ALIGNMENT_FEATURE.into(),
                origin: Some(story_id.into_canonical()),
                property_path: Some("story.paragraph_alignment".into()),
                require_preserved: false,
            }],
        )
    }

    fn package(export_plan: &ExportPlan, frame_id: NodeId) -> OdgPackage {
        let content = format!(
            "<?xml version=\"1.0\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\">\n  <office:automatic-styles/>\n<office:body><office:drawing><draw:page><draw:frame draw:name=\"{}\"><draw:text-box><text:p>Hello</text:p></draw:text-box>\n        </draw:frame></draw:page></office:drawing></office:body></office:document-content>",
            frame_name(frame_id)
        );
        OdgPackage {
            target: export_plan.target.clone(),
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
    fn writes_center_paragraph_style_without_upgrading_loss_state() {
        let story_id = story(1);
        let frame_id = frame(2);
        let export_plan = plan(story_id);
        let mut package = package(&export_plan, frame_id);
        let placement = OdgFullStoryParagraphAlignmentPlacement {
            alignment: FullStoryParagraphAlignmentV1 {
                story_id,
                alignment: ParagraphAlignmentV1::Center,
            },
            frame_ids: vec![frame_id],
        };

        add_full_story_paragraph_alignment_to_odg(
            &export_plan,
            &mut package,
            std::slice::from_ref(&placement),
        )
        .expect("bounded paragraph alignment");

        let xml = std::str::from_utf8(&package.parts[0].content).expect("content XML");
        assert!(xml.contains("style:family=\"paragraph\""));
        assert!(xml.contains("fo:text-align=\"center\""));
        assert!(xml.contains("<text:p text:style-name=\"PubStoryP_"));
        assert!(export_plan.losses.iter().any(|loss| {
            loss.origin == Some(story_id.into_canonical())
                && loss.feature == STORY_PARAGRAPH_ALIGNMENT_FEATURE
        }));
    }

    #[test]
    fn appends_paragraph_style_after_existing_automatic_text_style() {
        let story_id = story(3);
        let frame_id = frame(4);
        let export_plan = plan(story_id);
        let mut package = package(&export_plan, frame_id);
        let xml = String::from_utf8(package.parts[0].content.clone()).unwrap();
        let xml = xml.replace(
            "  <office:automatic-styles/>\n",
            "  <office:automatic-styles>\n    <style:style style:name=\"Existing\" style:family=\"text\"/>\n  </office:automatic-styles>\n",
        );
        package.parts[0].content = xml.into_bytes();
        let placement = OdgFullStoryParagraphAlignmentPlacement {
            alignment: FullStoryParagraphAlignmentV1 {
                story_id,
                alignment: ParagraphAlignmentV1::Right,
            },
            frame_ids: vec![frame_id],
        };

        add_full_story_paragraph_alignment_to_odg(
            &export_plan,
            &mut package,
            std::slice::from_ref(&placement),
        )
        .expect("bounded paragraph alignment");

        let xml = std::str::from_utf8(&package.parts[0].content).expect("content XML");
        assert!(xml.contains("style:name=\"Existing\""));
        assert!(xml.contains("fo:text-align=\"right\""));
    }
}
