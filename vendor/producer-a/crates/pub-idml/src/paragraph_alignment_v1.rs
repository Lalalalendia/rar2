use crate::{IdmlPackage, IdmlPartContent, IdmlPartKind};
use pub_export::{EffectiveParagraphAlignmentExportV1, ParagraphAlignmentV1};
use pub_model::{CanonicalId, ParagraphId, StoryId, TextRange};
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdmlParagraphAlignmentPlacementV1 {
    pub story_id: StoryId,
    pub story_text: String,
    pub paragraphs: Vec<EffectiveParagraphAlignmentExportV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdmlParagraphAlignmentErrorV1 {
    NonIdmlPackage,
    DuplicateStory { story_id: StoryId },
    MissingStoryPart { story_id: StoryId },
    BinaryStoryPart { story_id: StoryId },
    UnexpectedStoryMarkup { story_id: StoryId },
    InvalidParagraphTopology { story_id: StoryId },
    UnsupportedAlignment {
        paragraph_id: ParagraphId,
        alignment: ParagraphAlignmentV1,
    },
    InvalidXmlCharacter { story_id: StoryId, scalar: u32 },
}

impl fmt::Display for IdmlParagraphAlignmentErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonIdmlPackage => formatter.write_str(
                "paragraph-scoped alignment materialization requires an IDML package",
            ),
            Self::DuplicateStory { story_id } => write!(
                formatter,
                "duplicate paragraph-alignment placement for Story {}",
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
                "IDML Story {} is outside the bounded unstyled single-range source shape",
                story_id.as_canonical()
            ),
            Self::InvalidParagraphTopology { story_id } => write!(
                formatter,
                "Story {} paragraph alignment does not cover one canonical scalar topology",
                story_id.as_canonical()
            ),
            Self::UnsupportedAlignment {
                paragraph_id,
                alignment,
            } => write!(
                formatter,
                "Paragraph {} alignment {alignment:?} is not materializable by the bounded IDML V1 seam",
                paragraph_id.as_canonical()
            ),
            Self::InvalidXmlCharacter { story_id, scalar } => write!(
                formatter,
                "Story {} contains invalid XML 1.0 scalar U+{scalar:04X}",
                story_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for IdmlParagraphAlignmentErrorV1 {}

pub fn add_effective_paragraph_alignment_to_idml_v1(
    package: &mut IdmlPackage,
    placements: &[IdmlParagraphAlignmentPlacementV1],
) -> Result<(), IdmlParagraphAlignmentErrorV1> {
    if package.target.format != "idml" {
        return Err(IdmlParagraphAlignmentErrorV1::NonIdmlPackage);
    }

    let mut ordered = placements.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|placement| placement.story_id);
    let mut seen = BTreeSet::new();

    for placement in ordered {
        if !seen.insert(placement.story_id) {
            return Err(IdmlParagraphAlignmentErrorV1::DuplicateStory {
                story_id: placement.story_id,
            });
        }
        let paragraphs = canonical_paragraphs(placement)?;
        let path = story_path(placement.story_id);
        let part = package
            .parts
            .iter_mut()
            .find(|part| part.kind == IdmlPartKind::Story && part.path == path)
            .ok_or(IdmlParagraphAlignmentErrorV1::MissingStoryPart {
                story_id: placement.story_id,
            })?;
        let IdmlPartContent::Text(xml) = &mut part.content else {
            return Err(IdmlParagraphAlignmentErrorV1::BinaryStoryPart {
                story_id: placement.story_id,
            });
        };

        if xml.matches("<ParagraphStyleRange ").count() != 1
            || xml.matches("<CharacterStyleRange ").count() != 1
            || xml.matches("<Content>").count() != 1
            || xml.contains("FontStyle=")
            || xml.contains("<AppliedFont ")
        {
            return Err(IdmlParagraphAlignmentErrorV1::UnexpectedStoryMarkup {
                story_id: placement.story_id,
            });
        }

        let story_open = format!(
            "  <Story Self=\"{}\">\n",
            idml_self("us", placement.story_id.into_canonical())
        );
        let start = xml
            .find(&story_open)
            .map(|index| index + story_open.len())
            .ok_or(IdmlParagraphAlignmentErrorV1::UnexpectedStoryMarkup {
                story_id: placement.story_id,
            })?;
        let close = "  </Story>\n";
        let end = xml
            .find(close)
            .ok_or(IdmlParagraphAlignmentErrorV1::UnexpectedStoryMarkup {
                story_id: placement.story_id,
            })?;

        let mut body = String::new();
        for paragraph in paragraphs {
            let justification = match paragraph.item.alignment {
                None => None,
                Some(ParagraphAlignmentV1::Left) => Some("LeftAlign"),
                Some(ParagraphAlignmentV1::Center) => Some("CenterAlign"),
                Some(ParagraphAlignmentV1::Right) => Some("RightAlign"),
                Some(ParagraphAlignmentV1::InterWord | ParagraphAlignmentV1::Distribute) => {
                    return Err(IdmlParagraphAlignmentErrorV1::UnsupportedAlignment {
                        paragraph_id: paragraph.item.paragraph_id,
                        alignment: paragraph.item.alignment.expect("matched Some"),
                    });
                }
            };

            body.push_str(
                "    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\"",
            );
            if let Some(justification) = justification {
                write!(body, " Justification=\"{justification}\"").unwrap();
            }
            body.push_str(">\n");
            body.push_str(
                "      <CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\">\n",
            );
            writeln!(body, "        <Content>{}</Content>", paragraph.escaped_text).unwrap();
            body.push_str("      </CharacterStyleRange>\n");
            body.push_str("    </ParagraphStyleRange>\n");
        }

        xml.replace_range(start..end, &body);
    }

    Ok(())
}

struct CanonicalParagraph<'a> {
    item: &'a EffectiveParagraphAlignmentExportV1,
    escaped_text: String,
}

fn canonical_paragraphs<'a>(
    placement: &'a IdmlParagraphAlignmentPlacementV1,
) -> Result<Vec<CanonicalParagraph<'a>>, IdmlParagraphAlignmentErrorV1> {
    let mut items = placement.paragraphs.iter().collect::<Vec<_>>();
    items.sort_by_key(|item| (item.range.start, item.range.end, item.paragraph_id));

    let scalar_len = u64::try_from(placement.story_text.chars().count()).map_err(|_| {
        IdmlParagraphAlignmentErrorV1::InvalidParagraphTopology {
            story_id: placement.story_id,
        }
    })?;
    if items.is_empty() {
        return Err(IdmlParagraphAlignmentErrorV1::InvalidParagraphTopology {
            story_id: placement.story_id,
        });
    }

    let mut cursor = 0_u64;
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        if item.story_id != placement.story_id
            || item.range.start != cursor
            || item.range.end < item.range.start
            || item.range.end > scalar_len
        {
            return Err(IdmlParagraphAlignmentErrorV1::InvalidParagraphTopology {
                story_id: placement.story_id,
            });
        }
        let raw = scalar_slice(&placement.story_text, item.range).ok_or(
            IdmlParagraphAlignmentErrorV1::InvalidParagraphTopology {
                story_id: placement.story_id,
            },
        )?;
        let interior = raw.strip_suffix('\r').unwrap_or(raw.as_str());
        if interior.contains('\r') {
            return Err(IdmlParagraphAlignmentErrorV1::InvalidParagraphTopology {
                story_id: placement.story_id,
            });
        }
        result.push(CanonicalParagraph {
            item,
            escaped_text: escape_xml_text(placement.story_id, &raw)?,
        });
        cursor = item.range.end;
    }
    if cursor != scalar_len {
        return Err(IdmlParagraphAlignmentErrorV1::InvalidParagraphTopology {
            story_id: placement.story_id,
        });
    }
    Ok(result)
}

fn scalar_slice(text: &str, range: TextRange) -> Option<String> {
    let start = usize::try_from(range.start).ok()?;
    let len = usize::try_from(range.len()).ok()?;
    let chars = text.chars().skip(start).take(len).collect::<String>();
    (chars.chars().count() == len).then_some(chars)
}

fn escape_xml_text(
    story_id: StoryId,
    input: &str,
) -> Result<String, IdmlParagraphAlignmentErrorV1> {
    let mut output = String::new();
    for character in input.chars() {
        let scalar = u32::from(character);
        if !matches!(
            scalar,
            0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
        ) {
            return Err(IdmlParagraphAlignmentErrorV1::InvalidXmlCharacter { story_id, scalar });
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
    use crate::{IDML_ADAPTER_VERSION_V0_1, IDML_SCHEMA_FENCE_LEGACY_DOM_7, IdmlPart};
    use pub_export::TargetProfile;

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn story(byte: u8) -> StoryId {
        StoryId::from_canonical(canonical(byte))
    }

    fn paragraph(byte: u8) -> ParagraphId {
        ParagraphId::from_canonical(canonical(byte))
    }

    fn package(story_id: StoryId) -> IdmlPackage {
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<idPkg:Story xmlns:idPkg=\"http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging\" DOMVersion=\"7.0\">\n  <Story Self=\"{}\">\n    <ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/[No paragraph style]\">\n      <CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\">\n        <Content>legacy carrier</Content>\n      </CharacterStyleRange>\n    </ParagraphStyleRange>\n  </Story>\n</idPkg:Story>\n",
            idml_self("us", story_id.into_canonical())
        );
        IdmlPackage {
            target: TargetProfile {
                format: "idml".into(),
                adapter_version: IDML_ADAPTER_VERSION_V0_1.into(),
                profile: "bounded-editable".into(),
                schema_fence: Some(IDML_SCHEMA_FENCE_LEGACY_DOM_7.into()),
            },
            conversion_fence: None,
            parts: vec![IdmlPart {
                path: story_path(story_id),
                kind: IdmlPartKind::Story,
                content: IdmlPartContent::Text(xml),
            }],
        }
    }

    #[test]
    fn canonical_ranges_split_story_and_keep_terminal_cr_owned() {
        let story_id = story(1);
        let first = paragraph(2);
        let second = paragraph(3);
        let mut package = package(story_id);
        let placement = IdmlParagraphAlignmentPlacementV1 {
            story_id,
            story_text: "alpha\rbeta\r".into(),
            paragraphs: vec![
                EffectiveParagraphAlignmentExportV1 {
                    paragraph_id: first,
                    story_id,
                    range: TextRange::new(0, 6).unwrap(),
                    alignment: Some(ParagraphAlignmentV1::Left),
                },
                EffectiveParagraphAlignmentExportV1 {
                    paragraph_id: second,
                    story_id,
                    range: TextRange::new(6, 11).unwrap(),
                    alignment: Some(ParagraphAlignmentV1::Right),
                },
            ],
        };

        add_effective_paragraph_alignment_to_idml_v1(
            &mut package,
            std::slice::from_ref(&placement),
        )
        .expect("paragraph-scoped IDML materialization");

        let xml = package.parts[0].content.as_text().unwrap();
        assert!(!xml.contains("legacy carrier"));
        assert_eq!(xml.matches("<ParagraphStyleRange ").count(), 2);
        assert_eq!(xml.matches("<CharacterStyleRange ").count(), 2);
        assert!(xml.contains("Justification=\"LeftAlign\""));
        assert!(xml.contains("Justification=\"RightAlign\""));
        assert!(xml.contains("<Content>alpha\r</Content>"));
        assert!(xml.contains("<Content>beta\r</Content>"));
        assert!(!xml.contains("<Content></Content>"));
    }

    #[test]
    fn unsupported_alignment_fails_closed_without_coercion() {
        let story_id = story(4);
        let paragraph_id = paragraph(5);
        let mut package = package(story_id);
        let placement = IdmlParagraphAlignmentPlacementV1 {
            story_id,
            story_text: "alpha".into(),
            paragraphs: vec![EffectiveParagraphAlignmentExportV1 {
                paragraph_id,
                story_id,
                range: TextRange::new(0, 5).unwrap(),
                alignment: Some(ParagraphAlignmentV1::InterWord),
            }],
        };

        assert!(matches!(
            add_effective_paragraph_alignment_to_idml_v1(
                &mut package,
                std::slice::from_ref(&placement),
            ),
            Err(IdmlParagraphAlignmentErrorV1::UnsupportedAlignment {
                paragraph_id: found,
                alignment: ParagraphAlignmentV1::InterWord,
            }) if found == paragraph_id
        ));
    }

    #[test]
    fn non_contiguous_ranges_fail_closed() {
        let story_id = story(6);
        let mut package = package(story_id);
        let placement = IdmlParagraphAlignmentPlacementV1 {
            story_id,
            story_text: "alpha beta".into(),
            paragraphs: vec![
                EffectiveParagraphAlignmentExportV1 {
                    paragraph_id: paragraph(7),
                    story_id,
                    range: TextRange::new(0, 5).unwrap(),
                    alignment: Some(ParagraphAlignmentV1::Center),
                },
                EffectiveParagraphAlignmentExportV1 {
                    paragraph_id: paragraph(8),
                    story_id,
                    range: TextRange::new(6, 10).unwrap(),
                    alignment: Some(ParagraphAlignmentV1::Right),
                },
            ],
        };

        assert!(matches!(
            add_effective_paragraph_alignment_to_idml_v1(
                &mut package,
                std::slice::from_ref(&placement),
            ),
            Err(IdmlParagraphAlignmentErrorV1::InvalidParagraphTopology { .. })
        ));
    }
}
