use crate::{ODG_CONTENT_PATH, OdgPackage, OdgPartKind};
use pub_export::{
    ExportPlan, FullStoryTypographyV1, STORY_FONT_FAMILY_FEATURE, STORY_FONT_SIZE_FEATURE,
};
use pub_model::{CanonicalId, EMU_PER_POINT, LengthEmu, NodeId, StoryId};
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OdgFullStoryTypographyPlacement {
    pub typography: FullStoryTypographyV1,
    pub frame_ids: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OdgTypographyError {
    NonOdgPackage,
    DuplicateStory { story_id: StoryId },
    DuplicateFrame { node_id: NodeId },
    EmptyFrames { story_id: StoryId },
    InvalidFontFamily { story_id: StoryId },
    InvalidFontSize { story_id: StoryId },
    MissingPlannedFeature { story_id: StoryId, feature: &'static str },
    MissingContent,
    InvalidContentUtf8,
    MissingAutomaticStyles,
    MissingFrame { node_id: NodeId },
    MissingTextCarrier { node_id: NodeId },
    ExistingTextStyle { node_id: NodeId },
    InvalidXmlCharacter { story_id: StoryId, scalar: u32 },
}

impl fmt::Display for OdgTypographyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonOdgPackage => {
                formatter.write_str("full-Story typography projection requires an ODG package")
            }
            Self::DuplicateStory { story_id } => write!(
                formatter,
                "duplicate full-Story typography for {}",
                story_id.as_canonical()
            ),
            Self::DuplicateFrame { node_id } => write!(
                formatter,
                "ODG typography frame {} is assigned more than once",
                node_id.as_canonical()
            ),
            Self::EmptyFrames { story_id } => write!(
                formatter,
                "Story {} has no ODG text carrier frame",
                story_id.as_canonical()
            ),
            Self::InvalidFontFamily { story_id } => write!(
                formatter,
                "Story {} has an invalid bounded font family",
                story_id.as_canonical()
            ),
            Self::InvalidFontSize { story_id } => write!(
                formatter,
                "Story {} has a non-positive bounded font size",
                story_id.as_canonical()
            ),
            Self::MissingPlannedFeature { story_id, feature } => write!(
                formatter,
                "Story {} has no planned {feature} feature",
                story_id.as_canonical()
            ),
            Self::MissingContent => formatter.write_str("ODG package is missing content.xml"),
            Self::InvalidContentUtf8 => formatter.write_str("ODG content.xml is not UTF-8"),
            Self::MissingAutomaticStyles => formatter.write_str(
                "ODG content.xml is outside the bounded empty automatic-styles wire shape",
            ),
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
            Self::ExistingTextStyle { node_id } => write!(
                formatter,
                "ODG text frame {} already contains styled spans",
                node_id.as_canonical()
            ),
            Self::InvalidXmlCharacter { story_id, scalar } => write!(
                formatter,
                "Story {} font family contains invalid XML scalar U+{scalar:04X}",
                story_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for OdgTypographyError {}

pub fn add_full_story_typography_to_odg(
    plan: &ExportPlan,
    package: &mut OdgPackage,
    placements: &[OdgFullStoryTypographyPlacement],
) -> Result<(), OdgTypographyError> {
    if package.target.format != "odg" || plan.target.format != "odg" {
        return Err(OdgTypographyError::NonOdgPackage);
    }

    let content_index = package
        .parts
        .iter()
        .position(|part| part.kind == OdgPartKind::Content && part.path == ODG_CONTENT_PATH)
        .ok_or(OdgTypographyError::MissingContent)?;
    let mut xml = String::from_utf8(package.parts[content_index].content.clone())
        .map_err(|_| OdgTypographyError::InvalidContentUtf8)?;

    let automatic_marker = "  <office:automatic-styles/>\n";
    if xml.matches(automatic_marker).count() != 1 {
        return Err(OdgTypographyError::MissingAutomaticStyles);
    }

    let mut ordered = placements.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|placement| placement.typography.story_id);

    let mut seen_stories = BTreeSet::new();
    let mut seen_frames = BTreeSet::new();
    let mut style_xml = String::new();

    for placement in ordered {
        let item = &placement.typography;
        if !seen_stories.insert(item.story_id) {
            return Err(OdgTypographyError::DuplicateStory {
                story_id: item.story_id,
            });
        }
        validate_plan_and_value(plan, item)?;
        if placement.frame_ids.is_empty() {
            return Err(OdgTypographyError::EmptyFrames {
                story_id: item.story_id,
            });
        }

        let style_name = style_name(item.story_id);
        let font_family = escape_xml_attr(item.story_id, &item.font_family)?;
        let point_size = format_emu_points(item.font_size_emu);
        writeln!(
            style_xml,
            "    <style:style style:name=\"{style_name}\" style:family=\"text\">"
        )
        .unwrap();
        writeln!(
            style_xml,
            "      <style:text-properties fo:font-family=\"{font_family}\" fo:font-size=\"{point_size}pt\"/>"
        )
        .unwrap();
        style_xml.push_str("    </style:style>\n");

        let mut frame_ids = placement.frame_ids.clone();
        frame_ids.sort_unstable();
        for frame_id in frame_ids {
            if !seen_frames.insert(frame_id) {
                return Err(OdgTypographyError::DuplicateFrame { node_id: frame_id });
            }
            apply_style_to_frame(&mut xml, frame_id, &style_name)?;
        }
    }

    let replacement = format!(
        "  <office:automatic-styles>\n{style_xml}  </office:automatic-styles>\n"
    );
    xml = xml.replacen(automatic_marker, &replacement, 1);
    package.parts[content_index].content = xml.into_bytes();
    Ok(())
}

fn validate_plan_and_value(
    plan: &ExportPlan,
    item: &FullStoryTypographyV1,
) -> Result<(), OdgTypographyError> {
    if item.font_family.trim().is_empty() {
        return Err(OdgTypographyError::InvalidFontFamily {
            story_id: item.story_id,
        });
    }
    if item.font_size_emu.get() <= 0 {
        return Err(OdgTypographyError::InvalidFontSize {
            story_id: item.story_id,
        });
    }
    for feature in [STORY_FONT_FAMILY_FEATURE, STORY_FONT_SIZE_FEATURE] {
        if !has_planned_feature(plan, item.story_id.into_canonical(), feature) {
            return Err(OdgTypographyError::MissingPlannedFeature {
                story_id: item.story_id,
                feature,
            });
        }
    }
    Ok(())
}

fn has_planned_feature(plan: &ExportPlan, origin: CanonicalId, feature: &str) -> bool {
    plan.features.iter().any(|planned| {
        planned.request.origin == Some(origin) && planned.request.feature == feature
    })
}

fn apply_style_to_frame(
    xml: &mut String,
    frame_id: NodeId,
    style_name: &str,
) -> Result<(), OdgTypographyError> {
    let marker = format!("<draw:frame draw:name=\"{}\"", frame_name(frame_id));
    let frame_start = xml
        .find(&marker)
        .ok_or(OdgTypographyError::MissingFrame { node_id: frame_id })?;
    let close_marker = "        </draw:frame>";
    let relative_close = xml[frame_start..]
        .find(close_marker)
        .ok_or(OdgTypographyError::MissingFrame { node_id: frame_id })?;
    let frame_end = frame_start + relative_close + close_marker.len();

    let mut block = xml[frame_start..frame_end].to_owned();
    if block.contains("<text:span ") {
        return Err(OdgTypographyError::ExistingTextStyle { node_id: frame_id });
    }
    if !block.contains("<text:p>") {
        return Err(OdgTypographyError::MissingTextCarrier { node_id: frame_id });
    }

    block = block.replace(
        "<text:p>",
        &format!("<text:p><text:span text:style-name=\"{style_name}\">"),
    );
    block = block.replace("</text:p>", "</text:span></text:p>");
    xml.replace_range(frame_start..frame_end, &block);
    Ok(())
}

fn style_name(story_id: StoryId) -> String {
    stable_name("PubStoryT", story_id.into_canonical())
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

fn escape_xml_attr(
    story_id: StoryId,
    input: &str,
) -> Result<String, OdgTypographyError> {
    let mut output = String::new();
    for character in input.chars() {
        let scalar = u32::from(character);
        if !is_xml_10_scalar(scalar) {
            return Err(OdgTypographyError::InvalidXmlCharacter { story_id, scalar });
        }
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(character),
        }
    }
    Ok(output)
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
    use crate::{ODG_ADAPTER_VERSION_V0_1, ODG_SCHEMA_FENCE_ODF_1_4, OdgPart};
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
            vec![
                SemanticFeatureRequest {
                    feature: STORY_FONT_FAMILY_FEATURE.into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.typography.font_family".into()),
                    require_preserved: false,
                },
                SemanticFeatureRequest {
                    feature: STORY_FONT_SIZE_FEATURE.into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.typography.font_size".into()),
                    require_preserved: false,
                },
            ],
        )
    }

    #[test]
    fn writes_automatic_text_style_and_span_without_upgrading_loss_state() {
        let story_id = story(1);
        let frame_id = frame(2);
        let export_plan = plan(story_id);
        let content = format!(
            "<?xml version=\"1.0\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\"><office:automatic-styles/>\n<office:body><office:drawing><draw:page><draw:frame draw:name=\"{}\"><draw:text-box><text:p>Hello</text:p></draw:text-box>\n        </draw:frame></draw:page></office:drawing></office:body></office:document-content>",
            frame_name(frame_id)
        );
        let mut package = OdgPackage {
            target: export_plan.target.clone(),
            conversion_fence: None,
            parts: vec![OdgPart {
                path: ODG_CONTENT_PATH.into(),
                kind: OdgPartKind::Content,
                media_type: "text/xml".into(),
                content: content.into_bytes(),
            }],
        };
        let placement = OdgFullStoryTypographyPlacement {
            typography: FullStoryTypographyV1 {
                story_id,
                font_family: "A&B Sans".into(),
                font_size_emu: LengthEmu::new(12 * EMU_PER_POINT),
            },
            frame_ids: vec![frame_id],
        };

        add_full_story_typography_to_odg(
            &export_plan,
            &mut package,
            std::slice::from_ref(&placement),
        )
        .expect("bounded typography");

        let xml = std::str::from_utf8(&package.parts[0].content).unwrap();
        assert!(xml.contains("style:family=\"text\""));
        assert!(xml.contains("fo:font-family=\"A&amp;B Sans\""));
        assert!(xml.contains("fo:font-size=\"12pt\""));
        assert!(xml.contains("<text:span text:style-name=\"PubStoryT_"));
        assert!(xml.contains(">Hello</text:span></text:p>"));
        assert!(export_plan.losses.iter().any(|loss| {
            loss.origin == Some(story_id.into_canonical())
                && loss.feature == STORY_FONT_SIZE_FEATURE
        }));
    }

    #[test]
    fn duplicate_story_is_rejected() {
        let story_id = story(3);
        let frame_id = frame(4);
        let export_plan = plan(story_id);
        let content = format!(
            "<office:document-content><office:automatic-styles/>\n<draw:frame draw:name=\"{}\"><text:p>Hello</text:p>        </draw:frame></office:document-content>",
            frame_name(frame_id)
        );
        let mut package = OdgPackage {
            target: export_plan.target.clone(),
            conversion_fence: None,
            parts: vec![OdgPart {
                path: ODG_CONTENT_PATH.into(),
                kind: OdgPartKind::Content,
                media_type: "text/xml".into(),
                content: content.into_bytes(),
            }],
        };
        let placement = OdgFullStoryTypographyPlacement {
            typography: FullStoryTypographyV1 {
                story_id,
                font_family: "Arial".into(),
                font_size_emu: LengthEmu::new(10 * EMU_PER_POINT),
            },
            frame_ids: vec![frame_id],
        };

        assert!(matches!(
            add_full_story_typography_to_odg(
                &export_plan,
                &mut package,
                &[placement.clone(), placement],
            ),
            Err(OdgTypographyError::DuplicateStory { .. })
        ));
    }
}
