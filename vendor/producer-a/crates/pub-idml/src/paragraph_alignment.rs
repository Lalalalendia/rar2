use crate::{IdmlPackage, IdmlPartContent, IdmlPartKind};
use pub_export::{
    ExportPlan, FullStoryParagraphAlignmentV1, ParagraphAlignmentV1, ParagraphScopedAlignmentV1,
    ParagraphScopedAlignmentValueV1, STORY_PARAGRAPH_ALIGNMENT_FEATURE,
};
use pub_model::{CanonicalId, StoryId};
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdmlParagraphScopedAlignmentPlacement {
    pub story_id: StoryId,
    pub story_text: String,
    pub paragraphs: Vec<ParagraphScopedAlignmentV1>,
}

pub enum IdmlParagraphAlignmentError {
    NonIdmlPackage,
    DuplicateStory { story_id: StoryId },
    MissingPlannedFeature { story_id: StoryId },
    MissingStoryPart { story_id: StoryId },
    BinaryStoryPart { story_id: StoryId },
    UnexpectedStoryMarkup { story_id: StoryId },
}

impl fmt::Display for IdmlParagraphAlignmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonIdmlPackage => {
                formatter.write_str("full-Story paragraph alignment requires an IDML package")
            }
            Self::DuplicateStory { story_id } => write!(
                formatter,
                "duplicate full-Story paragraph alignment for {}",
                story_id.as_canonical()
            ),
            Self::MissingPlannedFeature { story_id } => write!(
                formatter,
                "Story {} has no planned paragraph-alignment feature",
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
                "IDML Story {} is outside the bounded paragraph-alignment wire shape",
                story_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for IdmlParagraphAlignmentError {}

pub fn add_full_story_paragraph_alignment_to_idml(
    plan: &ExportPlan,
    package: &mut IdmlPackage,
    alignments: &[FullStoryParagraphAlignmentV1],
) -> Result<(), IdmlParagraphAlignmentError> {
    if package.target.format != "idml" || plan.target.format != "idml" {
        return Err(IdmlParagraphAlignmentError::NonIdmlPackage);
    }

    let mut ordered = alignments.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|item| item.story_id);

    let mut seen = BTreeSet::new();
    for item in ordered {
        if !seen.insert(item.story_id) {
            return Err(IdmlParagraphAlignmentError::DuplicateStory {
                story_id: item.story_id,
            });
        }
        if !has_planned_feature(plan, item.story_id.into_canonical()) {
            return Err(IdmlParagraphAlignmentError::MissingPlannedFeature {
                story_id: item.story_id,
            });
        }

        let path = story_path(item.story_id);
        let part = package
            .parts
            .iter_mut()
            .find(|part| part.kind == IdmlPartKind::Story && part.path == path)
            .ok_or(IdmlParagraphAlignmentError::MissingStoryPart {
                story_id: item.story_id,
            })?;
        let IdmlPartContent::Text(xml) = &mut part.content else {
            return Err(IdmlParagraphAlignmentError::BinaryStoryPart {
                story_id: item.story_id,
            });
        };

        let marker = "    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\">\n";
        if xml.matches(marker).count() != 1 {
            return Err(IdmlParagraphAlignmentError::UnexpectedStoryMarkup {
                story_id: item.story_id,
            });
        }
        let justification = match item.alignment {
            ParagraphAlignmentV1::Center => "CenterAlign",
            ParagraphAlignmentV1::Right => "RightAlign",
        };
        let replacement = format!(
            "    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\" Justification=\"{justification}\">\n"
        );
        *xml = xml.replacen(marker, &replacement, 1);
    }

    Ok(())
}

fn has_planned_feature(plan: &ExportPlan, origin: CanonicalId) -> bool {
    plan.features.iter().any(|planned| {
        planned.request.origin == Some(origin)
            && planned.request.feature == STORY_PARAGRAPH_ALIGNMENT_FEATURE
    })
}

fn story_path(story_id: StoryId) -> String {
    format!(
        "Stories/Story_{}.xml",
        idml_self("us", story_id.into_canonical())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IDML_ADAPTER_VERSION_V0_1, IDML_PACKAGING_NAMESPACE, IDML_SCHEMA_FENCE_LEGACY_DOM_7,
        IdmlPackageBuilder, IdmlPartKind,
    };
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
            schema_fence: Some(IDML_SCHEMA_FENCE_LEGACY_DOM_7.into()),
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

    fn package(export_plan: &ExportPlan, story_id: StoryId) -> IdmlPackage {
        let mut builder = IdmlPackageBuilder::from_export_plan(export_plan).expect("builder");
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
    fn writes_center_alignment_without_upgrading_loss_state() {
        let story_id = story(1);
        let export_plan = plan(story_id);
        let mut package = package(&export_plan, story_id);
        let alignment = FullStoryParagraphAlignmentV1 {
            story_id,
            alignment: ParagraphAlignmentV1::Center,
        };

        add_full_story_paragraph_alignment_to_idml(
            &export_plan,
            &mut package,
            std::slice::from_ref(&alignment),
        )
        .expect("bounded paragraph alignment");

        let xml = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Story)
            .and_then(|part| part.content.as_text())
            .expect("story text");
        assert!(xml.contains("Justification=\"CenterAlign\""));
        assert!(export_plan.losses.iter().any(|loss| {
            loss.origin == Some(story_id.into_canonical())
                && loss.feature == STORY_PARAGRAPH_ALIGNMENT_FEATURE
        }));
    }

    #[test]
    fn writes_right_alignment() {
        let story_id = story(2);
        let export_plan = plan(story_id);
        let mut package = package(&export_plan, story_id);
        let alignment = FullStoryParagraphAlignmentV1 {
            story_id,
            alignment: ParagraphAlignmentV1::Right,
        };

        add_full_story_paragraph_alignment_to_idml(
            &export_plan,
            &mut package,
            std::slice::from_ref(&alignment),
        )
        .expect("bounded paragraph alignment");

        let xml = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Story)
            .and_then(|part| part.content.as_text())
            .expect("story text");
        assert!(xml.contains("Justification=\"RightAlign\""));
    }
}
