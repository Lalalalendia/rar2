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
pub struct IdmlParagraphScopedAlignmentPlacement {
    pub story_id: StoryId,
    pub story_text: String,
    pub paragraphs: Vec<ParagraphScopedAlignmentV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdmlParagraphAlignmentError {
    NonIdmlPackage,
    DuplicateStory { story_id: StoryId },
    MissingPlannedFeature { story_id: StoryId },
    MissingStoryPart { story_id: StoryId },
    BinaryStoryPart { story_id: StoryId },
    UnexpectedStoryMarkup { story_id: StoryId },
    EmptyParagraphs { story_id: StoryId },
    ParagraphStoryMismatch {
        story_id: StoryId,
        paragraph_id: pub_model::ParagraphId,
        paragraph_story_id: StoryId,
    },
    DuplicateParagraph {
        paragraph_id: pub_model::ParagraphId,
    },
    NonContiguousParagraphRanges {
        story_id: StoryId,
    },
    ParagraphCoverageMismatch {
        story_id: StoryId,
        expected_end: u64,
        found_end: u64,
    },
    MissingParagraphTerminator {
        story_id: StoryId,
        paragraph_id: pub_model::ParagraphId,
    },
    EmbeddedParagraphTerminator {
        story_id: StoryId,
        paragraph_id: pub_model::ParagraphId,
    },
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
            Self::NonContiguousParagraphRanges { story_id } => write!(
                formatter,
                "Story {} paragraph ranges are not contiguous from scalar zero",
                story_id.as_canonical()
            ),
            Self::ParagraphCoverageMismatch {
                story_id,
                expected_end,
                found_end,
            } => write!(
                formatter,
                "Story {} paragraph coverage ends at {found_end}, expected {expected_end}",
                story_id.as_canonical()
            ),
            Self::MissingParagraphTerminator {
                story_id,
                paragraph_id,
            } => write!(
                formatter,
                "non-final Paragraph {} in Story {} does not own terminal U+000D",
                paragraph_id.as_canonical(),
                story_id.as_canonical()
            ),
            Self::EmbeddedParagraphTerminator {
                story_id,
                paragraph_id,
            } => write!(
                formatter,
                "Paragraph {} in Story {} contains embedded U+000D outside its terminal boundary",
                paragraph_id.as_canonical(),
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

pub fn add_paragraph_scoped_alignment_to_idml(
    plan: &ExportPlan,
    package: &mut IdmlPackage,
    placements: &[IdmlParagraphScopedAlignmentPlacement],
) -> Result<(), IdmlParagraphAlignmentError> {
    if package.target.format != "idml" || plan.target.format != "idml" {
        return Err(IdmlParagraphAlignmentError::NonIdmlPackage);
    }

    let mut ordered = placements.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|placement| placement.story_id);
    let mut seen_stories = BTreeSet::new();
    let mut seen_paragraphs = BTreeSet::new();

    for placement in ordered {
        if !seen_stories.insert(placement.story_id) {
            return Err(IdmlParagraphAlignmentError::DuplicateStory {
                story_id: placement.story_id,
            });
        }
        if !has_planned_feature(plan, placement.story_id.into_canonical()) {
            return Err(IdmlParagraphAlignmentError::MissingPlannedFeature {
                story_id: placement.story_id,
            });
        }
        if placement.paragraphs.is_empty() {
            return Err(IdmlParagraphAlignmentError::EmptyParagraphs {
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

        let scalar_len = u64::try_from(placement.story_text.chars().count()).unwrap();
        let mut previous_end = 0_u64;
        for paragraph in &paragraphs {
            if paragraph.story_id != placement.story_id {
                return Err(IdmlParagraphAlignmentError::ParagraphStoryMismatch {
                    story_id: placement.story_id,
                    paragraph_id: paragraph.paragraph_id,
                    paragraph_story_id: paragraph.story_id,
                });
            }
            if !seen_paragraphs.insert(paragraph.paragraph_id) {
                return Err(IdmlParagraphAlignmentError::DuplicateParagraph {
                    paragraph_id: paragraph.paragraph_id,
                });
            }
            if paragraph.range.start != previous_end || paragraph.range.end < paragraph.range.start {
                return Err(IdmlParagraphAlignmentError::NonContiguousParagraphRanges {
                    story_id: placement.story_id,
                });
            }
            previous_end = paragraph.range.end;
        }
        if previous_end != scalar_len {
            return Err(IdmlParagraphAlignmentError::ParagraphCoverageMismatch {
                story_id: placement.story_id,
                expected_end: scalar_len,
                found_end: previous_end,
            });
        }

        let path = story_path(placement.story_id);
        let part = package
            .parts
            .iter_mut()
            .find(|part| part.kind == IdmlPartKind::Story && part.path == path)
            .ok_or(IdmlParagraphAlignmentError::MissingStoryPart {
                story_id: placement.story_id,
            })?;
        let IdmlPartContent::Text(xml) = &mut part.content else {
            return Err(IdmlParagraphAlignmentError::BinaryStoryPart {
                story_id: placement.story_id,
            });
        };

        let paragraph_open =
            "    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\">\n";
        let paragraph_close = "    </ParagraphStyleRange>\n";
        if xml.matches(paragraph_open).count() != 1 || xml.matches(paragraph_close).count() != 1 {
            return Err(IdmlParagraphAlignmentError::UnexpectedStoryMarkup {
                story_id: placement.story_id,
            });
        }
        let paragraph_start = xml.find(paragraph_open).unwrap();
        let relative_close = xml[paragraph_start..].find(paragraph_close).unwrap();
        let paragraph_end = paragraph_start + relative_close + paragraph_close.len();
        let block = &xml[paragraph_start..paragraph_end];

        let content_open = "<Content>";
        let content_close = "</Content>";
        if block.matches(content_open).count() != 1 || block.matches(content_close).count() != 1 {
            return Err(IdmlParagraphAlignmentError::UnexpectedStoryMarkup {
                story_id: placement.story_id,
            });
        }
        let content_start = block.find(content_open).unwrap();
        let content_end = block.find(content_close).unwrap();
        if content_end < content_start {
            return Err(IdmlParagraphAlignmentError::UnexpectedStoryMarkup {
                story_id: placement.story_id,
            });
        }

        let char_prefix_start = paragraph_open.len();
        let char_prefix_end = content_start;
        let char_prefix = &block[char_prefix_start..char_prefix_end];
        let char_suffix_start = content_end + content_close.len();
        let char_suffix_end = block.len() - paragraph_close.len();
        let char_suffix = &block[char_suffix_start..char_suffix_end];

        let mut replacement = String::new();
        for (index, paragraph) in paragraphs.iter().enumerate() {
            let raw = scalar_slice(
                &placement.story_text,
                paragraph.range.start,
                paragraph.range.end,
            );
            let is_last = index + 1 == paragraphs.len();
            let content = paragraph_content_from_canonical_range(
                placement.story_id,
                paragraph.paragraph_id,
                raw,
                is_last,
            )?;
            let escaped = escape_xml_content(content);
            let justification = match paragraph.alignment {
                ParagraphScopedAlignmentValueV1::Left => "LeftAlign",
                ParagraphScopedAlignmentValueV1::Center => "CenterAlign",
                ParagraphScopedAlignmentValueV1::Right => "RightAlign",
            };
            writeln!(
                replacement,
                "    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\" Justification=\"{justification}\">"
            )
            .unwrap();
            replacement.push_str(char_prefix);
            replacement.push_str(content_open);
            replacement.push_str(&escaped);
            replacement.push_str(content_close);
            replacement.push_str(char_suffix);
            replacement.push_str(paragraph_close);
        }

        xml.replace_range(paragraph_start..paragraph_end, &replacement);
    }

    Ok(())
}

fn scalar_slice(input: &str, start: u64, end: u64) -> String {
    input
        .chars()
        .skip(usize::try_from(start).unwrap())
        .take(usize::try_from(end - start).unwrap())
        .collect()
}

fn paragraph_content_from_canonical_range(
    story_id: StoryId,
    paragraph_id: pub_model::ParagraphId,
    mut raw: String,
    is_last: bool,
) -> Result<String, IdmlParagraphAlignmentError> {
    if !is_last && !raw.ends_with('\r') {
        return Err(IdmlParagraphAlignmentError::MissingParagraphTerminator {
            story_id,
            paragraph_id,
        });
    }
    if raw.ends_with('\r') {
        raw.pop();
    }
    if raw.contains('\r') {
        return Err(IdmlParagraphAlignmentError::EmbeddedParagraphTerminator {
            story_id,
            paragraph_id,
        });
    }
    Ok(raw)
}

fn escape_xml_content(input: String) -> String {
    let mut escaped = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(character),
        }
    }
    escaped
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
        ParagraphScopedAlignmentV1, ParagraphScopedAlignmentValueV1, SemanticFeatureRequest,
        TargetCapabilityManifest, TargetProfile, plan_export,
    };
    use pub_model::{CanonicalId, ParagraphId, TextRange};
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

    fn package_with_story_text(
        export_plan: &ExportPlan,
        story_id: StoryId,
        escaped_story_text: &str,
    ) -> IdmlPackage {
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
                    "<idPkg:Story><Story Self=\"{}\">\n    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\">\n      <CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\" FontStyle=\"Regular\" PointSize=\"12\">\n        <Properties>\n          <AppliedFont type=\"string\">Montserrat</AppliedFont>\n        </Properties>\n        <Content>{escaped_story_text}</Content>\n      </CharacterStyleRange>\n    </ParagraphStyleRange>\n  </Story></idPkg:Story>\n",
                    idml_self("us", story_id.into_canonical())
                ),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    fn scoped(
        paragraph_byte: u8,
        story_id: StoryId,
        start: u64,
        end: u64,
        alignment: ParagraphScopedAlignmentValueV1,
    ) -> ParagraphScopedAlignmentV1 {
        ParagraphScopedAlignmentV1 {
            story_id,
            paragraph_id: ParagraphId::from_canonical(id(paragraph_byte)),
            range: TextRange::new(start, end).expect("canonical paragraph range"),
            alignment,
        }
    }

    #[test]
    fn writes_scoped_left_center_right_as_sibling_paragraph_ranges() {
        let story_id = story(10);
        let export_plan = plan(story_id);
        let story_text = "One &\rTwo<\rThree";
        let mut package = package_with_story_text(&export_plan, story_id, "One &amp;\rTwo&lt;\rThree");
        let placement = IdmlParagraphScopedAlignmentPlacement {
            story_id,
            story_text: story_text.into(),
            paragraphs: vec![
                scoped(
                    12,
                    story_id,
                    6,
                    11,
                    ParagraphScopedAlignmentValueV1::Center,
                ),
                scoped(
                    11,
                    story_id,
                    0,
                    6,
                    ParagraphScopedAlignmentValueV1::Left,
                ),
                scoped(
                    13,
                    story_id,
                    11,
                    16,
                    ParagraphScopedAlignmentValueV1::Right,
                ),
            ],
        };

        add_paragraph_scoped_alignment_to_idml(
            &export_plan,
            &mut package,
            std::slice::from_ref(&placement),
        )
        .expect("paragraph-scoped IDML alignment");

        let xml = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Story)
            .and_then(|part| part.content.as_text())
            .expect("story XML");
        assert_eq!(xml.matches("<ParagraphStyleRange ").count(), 3);
        assert_eq!(xml.matches("Justification=\"LeftAlign\"").count(), 1);
        assert_eq!(xml.matches("Justification=\"CenterAlign\"").count(), 1);
        assert_eq!(xml.matches("Justification=\"RightAlign\"").count(), 1);
        assert!(xml.contains("<Content>One &amp;</Content>"));
        assert!(xml.contains("<Content>Two&lt;</Content>"));
        assert!(xml.contains("<Content>Three</Content>"));
        assert!(!xml.contains('\r'));
        assert_eq!(xml.matches("PointSize=\"12\"").count(), 3);
        assert_eq!(
            xml.matches("<AppliedFont type=\"string\">Montserrat</AppliedFont>")
                .count(),
            3
        );
    }

    #[test]
    fn provenance_terminal_cr_becomes_one_final_paragraph_range_not_br() {
        let story_id = story(20);
        let export_plan = plan(story_id);
        let mut package = package_with_story_text(&export_plan, story_id, "Alpha\r");
        let placement = IdmlParagraphScopedAlignmentPlacement {
            story_id,
            story_text: "Alpha\r".into(),
            paragraphs: vec![scoped(
                21,
                story_id,
                0,
                6,
                ParagraphScopedAlignmentValueV1::Right,
            )],
        };

        add_paragraph_scoped_alignment_to_idml(
            &export_plan,
            &mut package,
            std::slice::from_ref(&placement),
        )
        .expect("terminal CR is structural paragraph boundary");

        let xml = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Story)
            .and_then(|part| part.content.as_text())
            .expect("story XML");
        assert_eq!(xml.matches("<ParagraphStyleRange ").count(), 1);
        assert!(xml.contains("<Content>Alpha</Content>"));
        assert!(!xml.contains("<Br"));
        assert!(!xml.contains('\r'));
    }

    #[test]
    fn rejects_non_final_range_without_owned_terminal_cr() {
        let story_id = story(30);
        let export_plan = plan(story_id);
        let mut package = package_with_story_text(&export_plan, story_id, "OneTwo");
        let placement = IdmlParagraphScopedAlignmentPlacement {
            story_id,
            story_text: "OneTwo".into(),
            paragraphs: vec![
                scoped(
                    31,
                    story_id,
                    0,
                    3,
                    ParagraphScopedAlignmentValueV1::Left,
                ),
                scoped(
                    32,
                    story_id,
                    3,
                    6,
                    ParagraphScopedAlignmentValueV1::Right,
                ),
            ],
        };

        assert!(matches!(
            add_paragraph_scoped_alignment_to_idml(
                &export_plan,
                &mut package,
                std::slice::from_ref(&placement),
            ),
            Err(IdmlParagraphAlignmentError::MissingParagraphTerminator {
                story_id: found,
                ..
            }) if found == story_id
        ));
    }

    #[test]
    fn rejects_incomplete_canonical_story_coverage() {
        let story_id = story(40);
        let export_plan = plan(story_id);
        let mut package = package_with_story_text(&export_plan, story_id, "Alpha");
        let placement = IdmlParagraphScopedAlignmentPlacement {
            story_id,
            story_text: "Alpha".into(),
            paragraphs: vec![scoped(
                41,
                story_id,
                0,
                4,
                ParagraphScopedAlignmentValueV1::Center,
            )],
        };

        assert!(matches!(
            add_paragraph_scoped_alignment_to_idml(
                &export_plan,
                &mut package,
                std::slice::from_ref(&placement),
            ),
            Err(IdmlParagraphAlignmentError::ParagraphCoverageMismatch {
                story_id: found,
                expected_end: 5,
                found_end: 4,
            }) if found == story_id
        ));
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
