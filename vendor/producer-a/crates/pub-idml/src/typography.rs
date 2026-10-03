use crate::{IdmlPackage, IdmlPartContent, IdmlPartKind};
use pub_export::{
    ExportPlan, FullStoryTypographyV1, STORY_FONT_FAMILY_FEATURE, STORY_FONT_SIZE_FEATURE,
};
use pub_model::{CanonicalId, EMU_PER_POINT, LengthEmu, StoryId};
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdmlTypographyError {
    NonIdmlPackage,
    DuplicateStory { story_id: StoryId },
    InvalidFontFamily { story_id: StoryId },
    InvalidFontSize { story_id: StoryId },
    MissingPlannedFeature { story_id: StoryId, feature: &'static str },
    MissingStoryPart { story_id: StoryId },
    BinaryStoryPart { story_id: StoryId },
    UnexpectedStoryMarkup { story_id: StoryId },
    InvalidXmlCharacter { story_id: StoryId, scalar: u32 },
}

impl fmt::Display for IdmlTypographyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonIdmlPackage => {
                formatter.write_str("full-Story typography projection requires an IDML package")
            }
            Self::DuplicateStory { story_id } => write!(
                formatter,
                "duplicate full-Story typography for {}",
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
            Self::MissingStoryPart { story_id } => write!(
                formatter,
                "IDML package is missing Story part for {}",
                story_id.as_canonical()
            ),
            Self::BinaryStoryPart { story_id } => write!(
                formatter,
                "IDML Story part for {} is not UTF-8 text",
                story_id.as_canonical()
            ),
            Self::UnexpectedStoryMarkup { story_id } => write!(
                formatter,
                "IDML Story {} is outside the bounded single-range typography wire shape",
                story_id.as_canonical()
            ),
            Self::InvalidXmlCharacter { story_id, scalar } => write!(
                formatter,
                "Story {} font family contains invalid XML scalar U+{scalar:04X}",
                story_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for IdmlTypographyError {}

pub fn add_full_story_typography_to_idml(
    plan: &ExportPlan,
    package: &mut IdmlPackage,
    typography: &[FullStoryTypographyV1],
) -> Result<(), IdmlTypographyError> {
    if package.target.format != "idml" || plan.target.format != "idml" {
        return Err(IdmlTypographyError::NonIdmlPackage);
    }

    let mut ordered = typography.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|item| item.story_id);

    let mut seen = BTreeSet::new();
    for item in ordered {
        if !seen.insert(item.story_id) {
            return Err(IdmlTypographyError::DuplicateStory {
                story_id: item.story_id,
            });
        }
        validate_plan_and_value(plan, item)?;

        let path = story_path(item.story_id);
        let part = package
            .parts
            .iter_mut()
            .find(|part| part.kind == IdmlPartKind::Story && part.path == path)
            .ok_or(IdmlTypographyError::MissingStoryPart {
                story_id: item.story_id,
            })?;
        let IdmlPartContent::Text(xml) = &mut part.content else {
            return Err(IdmlTypographyError::BinaryStoryPart {
                story_id: item.story_id,
            });
        };

        let marker = "      <CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\">\n";
        if xml.matches(marker).count() != 1 {
            return Err(IdmlTypographyError::UnexpectedStoryMarkup {
                story_id: item.story_id,
            });
        }

        let font_family = escape_xml_text(item.story_id, &item.font_family)?;
        let point_size = format_emu_points(item.font_size_emu);
        let replacement = format!(
            "      <CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\" PointSize=\"{point_size}\">\n        <Properties>\n          <AppliedFont type=\"string\">{font_family}</AppliedFont>\n        </Properties>\n"
        );
        *xml = xml.replacen(marker, &replacement, 1);
    }

    Ok(())
}

fn validate_plan_and_value(
    plan: &ExportPlan,
    item: &FullStoryTypographyV1,
) -> Result<(), IdmlTypographyError> {
    if item.font_family.trim().is_empty() {
        return Err(IdmlTypographyError::InvalidFontFamily {
            story_id: item.story_id,
        });
    }
    if item.font_size_emu.get() <= 0 {
        return Err(IdmlTypographyError::InvalidFontSize {
            story_id: item.story_id,
        });
    }
    for feature in [STORY_FONT_FAMILY_FEATURE, STORY_FONT_SIZE_FEATURE] {
        if !has_planned_feature(plan, item.story_id.into_canonical(), feature) {
            return Err(IdmlTypographyError::MissingPlannedFeature {
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

fn story_path(story_id: StoryId) -> String {
    format!("Stories/Story_{}.xml", idml_self("us", story_id.into_canonical()))
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

fn escape_xml_text(
    story_id: StoryId,
    input: &str,
) -> Result<String, IdmlTypographyError> {
    let mut output = String::new();
    for character in input.chars() {
        let scalar = u32::from(character);
        if !is_xml_10_scalar(scalar) {
            return Err(IdmlTypographyError::InvalidXmlCharacter { story_id, scalar });
        }
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
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
    use crate::{IDML_ADAPTER_VERSION_V0_1, IdmlPackageBuilder};
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

    fn plan(story_id: StoryId) -> ExportPlan {
        let target = TargetProfile {
            format: "idml".into(),
            adapter_version: IDML_ADAPTER_VERSION_V0_1.into(),
            profile: "bounded-editable".into(),
            schema_fence: Some(crate::IDML_SCHEMA_FENCE_LEGACY_DOM_7.into()),
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

    fn package(export_plan: &ExportPlan, story_id: StoryId) -> IdmlPackage {
        let mut builder = IdmlPackageBuilder::from_export_plan(export_plan).expect("builder");
        builder
            .add_part("designmap.xml", IdmlPartKind::DesignMap, "<Document/>")
            .unwrap();
        builder
            .add_part(
                story_path(story_id),
                IdmlPartKind::Story,
                format!(
                    "<idPkg:Story><Story Self=\"{}\">\n    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\">\n      <CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\">\n        <Content>Hello</Content>\n      </CharacterStyleRange>\n    </ParagraphStyleRange>\n  </Story></idPkg:Story>\n",
                    idml_self("us", story_id.into_canonical())
                ),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    #[test]
    fn writes_local_applied_font_and_point_size_without_upgrading_loss_state() {
        let story_id = story(1);
        let export_plan = plan(story_id);
        let mut package = package(&export_plan, story_id);
        let typography = FullStoryTypographyV1 {
            story_id,
            font_family: "A&B Sans".into(),
            font_size_emu: LengthEmu::new(12 * EMU_PER_POINT),
        };

        add_full_story_typography_to_idml(
            &export_plan,
            &mut package,
            std::slice::from_ref(&typography),
        )
        .expect("bounded typography");

        let xml = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Story)
            .and_then(|part| part.content.as_text())
            .expect("story text");
        assert!(xml.contains("PointSize=\"12\""));
        assert!(xml.contains("<AppliedFont type=\"string\">A&amp;B Sans</AppliedFont>"));
        assert!(export_plan.losses.iter().any(|loss| {
            loss.origin == Some(story_id.into_canonical())
                && loss.feature == STORY_FONT_FAMILY_FEATURE
        }));
    }

    #[test]
    fn duplicate_story_is_rejected() {
        let story_id = story(2);
        let export_plan = plan(story_id);
        let mut package = package(&export_plan, story_id);
        let typography = FullStoryTypographyV1 {
            story_id,
            font_family: "Arial".into(),
            font_size_emu: LengthEmu::new(10 * EMU_PER_POINT),
        };

        assert!(matches!(
            add_full_story_typography_to_idml(
                &export_plan,
                &mut package,
                &[typography.clone(), typography],
            ),
            Err(IdmlTypographyError::DuplicateStory { .. })
        ));
    }
}
