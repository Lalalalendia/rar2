use crate::{ODG_CONTENT_PATH, OdgPackage, OdgPartKind};
use pub_export::{EffectiveParagraphAlignmentExportV1, ParagraphAlignmentV1};
use pub_model::{CanonicalId, NodeId, ParagraphId, StoryId, TextRange};
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OdgParagraphAlignmentPlacementV1 {
    pub story_id: StoryId,
    pub story_text: String,
    pub frame_ids: Vec<NodeId>,
    pub paragraphs: Vec<EffectiveParagraphAlignmentExportV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OdgParagraphAlignmentErrorV1 {
    NonOdgPackage,
    DuplicateStory { story_id: StoryId },
    DuplicateFrame { node_id: NodeId },
    EmptyFrames { story_id: StoryId },
    MissingContent,
    InvalidContentUtf8,
    MissingFrame { node_id: NodeId },
    MissingTextBox { node_id: NodeId },
    InvalidParagraphTopology { story_id: StoryId },
    UnsupportedAlignment {
        paragraph_id: ParagraphId,
        alignment: ParagraphAlignmentV1,
    },
    InvalidXmlCharacter { story_id: StoryId, scalar: u32 },
}

impl fmt::Display for OdgParagraphAlignmentErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonOdgPackage => formatter.write_str(
                "paragraph-scoped alignment materialization requires an ODG package",
            ),
            Self::DuplicateStory { story_id } => write!(
                formatter,
                "duplicate paragraph-alignment placement for Story {}",
                story_id.as_canonical()
            ),
            Self::DuplicateFrame { node_id } => write!(
                formatter,
                "duplicate ODG paragraph-alignment frame {}",
                node_id.as_canonical()
            ),
            Self::EmptyFrames { story_id } => write!(
                formatter,
                "Story {} has no ODG root frame for paragraph alignment",
                story_id.as_canonical()
            ),
            Self::MissingContent => formatter.write_str("ODG package is missing content.xml"),
            Self::InvalidContentUtf8 => formatter.write_str("ODG content.xml is not UTF-8"),
            Self::MissingFrame { node_id } => write!(
                formatter,
                "ODG content.xml is missing text frame {}",
                node_id.as_canonical()
            ),
            Self::MissingTextBox { node_id } => write!(
                formatter,
                "ODG frame {} has no bounded root text-box carrier",
                node_id.as_canonical()
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
                "Paragraph {} alignment {alignment:?} is not materializable by the bounded ODG V1 seam",
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

impl std::error::Error for OdgParagraphAlignmentErrorV1 {}

pub fn add_effective_paragraph_alignment_to_odg_v1(
    package: &mut OdgPackage,
    placements: &[OdgParagraphAlignmentPlacementV1],
) -> Result<(), OdgParagraphAlignmentErrorV1> {
    if package.target.format != "odg" {
        return Err(OdgParagraphAlignmentErrorV1::NonOdgPackage);
    }

    let content_index = package
        .parts
        .iter()
        .position(|part| part.kind == OdgPartKind::Content && part.path == ODG_CONTENT_PATH)
        .ok_or(OdgParagraphAlignmentErrorV1::MissingContent)?;
    let mut xml = String::from_utf8(package.parts[content_index].content.clone())
        .map_err(|_| OdgParagraphAlignmentErrorV1::InvalidContentUtf8)?;

    let mut ordered = placements.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|placement| placement.story_id);

    let mut seen_stories = BTreeSet::new();
    let mut seen_frames = BTreeSet::new();
    let mut automatic_styles = String::new();

    for placement in ordered {
        if !seen_stories.insert(placement.story_id) {
            return Err(OdgParagraphAlignmentErrorV1::DuplicateStory {
                story_id: placement.story_id,
            });
        }
        if placement.frame_ids.is_empty() {
            return Err(OdgParagraphAlignmentErrorV1::EmptyFrames {
                story_id: placement.story_id,
            });
        }

        let paragraphs = canonical_paragraphs(placement)?;
        for paragraph in &paragraphs {
            let Some(alignment) = paragraph.item.alignment else {
                continue;
            };
            let value = match alignment {
                ParagraphAlignmentV1::Left => "left",
                ParagraphAlignmentV1::Center => "center",
                ParagraphAlignmentV1::Right => "right",
                ParagraphAlignmentV1::InterWord | ParagraphAlignmentV1::Distribute => {
                    return Err(OdgParagraphAlignmentErrorV1::UnsupportedAlignment {
                        paragraph_id: paragraph.item.paragraph_id,
                        alignment,
                    });
                }
            };
            let style_name = paragraph_style_name(paragraph.item.paragraph_id);
            writeln!(
                automatic_styles,
                "    <style:style style:name=\"{style_name}\" style:family=\"paragraph\">"
            )
            .unwrap();
            writeln!(
                automatic_styles,
                "      <style:paragraph-properties fo:text-align=\"{value}\"/>"
            )
            .unwrap();
            automatic_styles.push_str("    </style:style>\n");
        }

        let mut frame_ids = placement.frame_ids.clone();
        frame_ids.sort_unstable();
        frame_ids.dedup();
        for frame_id in frame_ids {
            if !seen_frames.insert(frame_id) {
                return Err(OdgParagraphAlignmentErrorV1::DuplicateFrame { node_id: frame_id });
            }
            replace_root_text_box(
                &mut xml,
                frame_id,
                placement.story_id,
                &paragraphs,
            )?;
        }
    }

    add_automatic_styles(&mut xml, &automatic_styles)?;
    package.parts[content_index].content = xml.into_bytes();
    Ok(())
}

struct CanonicalParagraph<'a> {
    item: &'a EffectiveParagraphAlignmentExportV1,
    visible_text: String,
}

fn canonical_paragraphs<'a>(
    placement: &'a OdgParagraphAlignmentPlacementV1,
) -> Result<Vec<CanonicalParagraph<'a>>, OdgParagraphAlignmentErrorV1> {
    let mut items = placement.paragraphs.iter().collect::<Vec<_>>();
    items.sort_by_key(|item| (item.range.start, item.range.end, item.paragraph_id));

    let scalar_len = u64::try_from(placement.story_text.chars().count()).map_err(|_| {
        OdgParagraphAlignmentErrorV1::InvalidParagraphTopology {
            story_id: placement.story_id,
        }
    })?;
    if items.is_empty() {
        return Err(OdgParagraphAlignmentErrorV1::InvalidParagraphTopology {
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
            return Err(OdgParagraphAlignmentErrorV1::InvalidParagraphTopology {
                story_id: placement.story_id,
            });
        }

        let raw = scalar_slice(&placement.story_text, item.range).ok_or(
            OdgParagraphAlignmentErrorV1::InvalidParagraphTopology {
                story_id: placement.story_id,
            },
        )?;
        let visible = if let Some(without_cr) = raw.strip_suffix('\r') {
            if without_cr.contains('\r') {
                return Err(OdgParagraphAlignmentErrorV1::InvalidParagraphTopology {
                    story_id: placement.story_id,
                });
            }
            without_cr
        } else {
            if raw.contains('\r') {
                return Err(OdgParagraphAlignmentErrorV1::InvalidParagraphTopology {
                    story_id: placement.story_id,
                });
            }
            raw.as_str()
        };

        result.push(CanonicalParagraph {
            item,
            visible_text: escape_odf_text(placement.story_id, visible)?,
        });
        cursor = item.range.end;
    }

    if cursor != scalar_len {
        return Err(OdgParagraphAlignmentErrorV1::InvalidParagraphTopology {
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

fn replace_root_text_box(
    xml: &mut String,
    frame_id: NodeId,
    story_id: StoryId,
    paragraphs: &[CanonicalParagraph<'_>],
) -> Result<(), OdgParagraphAlignmentErrorV1> {
    let frame_marker = format!("<draw:frame draw:name=\"{}\"", frame_name(frame_id));
    let frame_start = xml
        .find(&frame_marker)
        .ok_or(OdgParagraphAlignmentErrorV1::MissingFrame { node_id: frame_id })?;
    let frame_close_marker = "        </draw:frame>";
    let relative_frame_end = xml[frame_start..]
        .find(frame_close_marker)
        .ok_or(OdgParagraphAlignmentErrorV1::MissingFrame { node_id: frame_id })?;
    let frame_end = frame_start + relative_frame_end + frame_close_marker.len();

    let block = &xml[frame_start..frame_end];
    let open = "          <draw:text-box>\n";
    let close = "          </draw:text-box>";
    let relative_open = block
        .find(open)
        .ok_or(OdgParagraphAlignmentErrorV1::MissingTextBox { node_id: frame_id })?;
    let content_start = frame_start + relative_open + open.len();
    let relative_close = block[relative_open + open.len()..]
        .find(close)
        .ok_or(OdgParagraphAlignmentErrorV1::MissingTextBox { node_id: frame_id })?;
    let content_end = content_start + relative_close;

    let mut replacement = String::new();
    for paragraph in paragraphs {
        replacement.push_str("            <text:p");
        if let Some(alignment) = paragraph.item.alignment {
            match alignment {
                ParagraphAlignmentV1::Left
                | ParagraphAlignmentV1::Center
                | ParagraphAlignmentV1::Right => {
                    write!(
                        replacement,
                        " text:style-name=\"{}\"",
                        paragraph_style_name(paragraph.item.paragraph_id)
                    )
                    .unwrap();
                }
                ParagraphAlignmentV1::InterWord | ParagraphAlignmentV1::Distribute => {
                    return Err(OdgParagraphAlignmentErrorV1::UnsupportedAlignment {
                        paragraph_id: paragraph.item.paragraph_id,
                        alignment,
                    });
                }
            }
        }
        replacement.push('>');
        replacement.push_str(&paragraph.visible_text);
        replacement.push_str("</text:p>\n");
    }

    let _ = story_id;
    xml.replace_range(content_start..content_end, &replacement);
    Ok(())
}

fn add_automatic_styles(
    xml: &mut String,
    style_xml: &str,
) -> Result<(), OdgParagraphAlignmentErrorV1> {
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
        return Err(OdgParagraphAlignmentErrorV1::MissingContent);
    }
    let close = xml
        .find(close_marker)
        .ok_or(OdgParagraphAlignmentErrorV1::MissingContent)?;
    xml.insert_str(close, style_xml);
    Ok(())
}

fn escape_odf_text(
    story_id: StoryId,
    input: &str,
) -> Result<String, OdgParagraphAlignmentErrorV1> {
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
        if !matches!(
            scalar,
            0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
        ) {
            return Err(OdgParagraphAlignmentErrorV1::InvalidXmlCharacter { story_id, scalar });
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

fn paragraph_style_name(paragraph_id: ParagraphId) -> String {
    stable_name("PubParaP", paragraph_id.into_canonical())
}

fn frame_name(frame_id: NodeId) -> String {
    stable_name("Frame", frame_id.into_canonical())
}

fn stable_name(prefix: &str, id: CanonicalId) -> String {
    let mut value = String::with_capacity(prefix.len() + 33);
    value.push_str(prefix);
    for byte in id.into_bytes() {
        write!(value, "{byte:02x}").unwrap();
    }
    value
}
