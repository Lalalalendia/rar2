use crate::{PUB_ADAPTER_ID, PubSourceGraph};
use pub_model::{
    Paragraph, ParagraphId, SourceDerivedIdInput, StoryId, TextRange, derive_source_canonical_id,
};
use std::fmt;

const PARAGRAPH_SEMANTIC_ROLE: &str = "cdm.paragraph";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PubParagraphMaterializationError {
    Identity {
        story_id: StoryId,
        ordinal: usize,
        message: String,
    },
    Range {
        story_id: StoryId,
        ordinal: usize,
        start: u64,
        end: u64,
    },
    DuplicateIdentity {
        paragraph_id: ParagraphId,
    },
}

impl fmt::Display for PubParagraphMaterializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity {
                story_id,
                ordinal,
                message,
            } => write!(
                formatter,
                "cannot derive paragraph identity for Story {} paragraph {ordinal}: {message}",
                story_id.as_canonical()
            ),
            Self::Range {
                story_id,
                ordinal,
                start,
                end,
            } => write!(
                formatter,
                "invalid paragraph range for Story {} paragraph {ordinal}: {start}..{end}",
                story_id.as_canonical()
            ),
            Self::DuplicateIdentity { paragraph_id } => write!(
                formatter,
                "duplicate paragraph identity {}",
                paragraph_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for PubParagraphMaterializationError {}

pub(crate) fn materialize_imported_story_paragraphs_v1(
    graph: &mut PubSourceGraph,
) -> Result<(), PubParagraphMaterializationError> {
    let source_hash = graph.source.source_hash;
    let story_ids = graph.stories.keys().copied().collect::<Vec<_>>();

    for story_id in story_ids {
        let text = graph
            .stories
            .get(&story_id)
            .expect("StoryId came from graph registry")
            .text
            .clone();
        let ranges = canonical_paragraph_ranges(&text);

        let mut paragraph_ids = Vec::with_capacity(ranges.len());
        for (ordinal, (start, end)) in ranges.into_iter().enumerate() {
            let source_object_key = format!(
                "story/{}/paragraph/{ordinal}",
                story_id.as_canonical()
            );
            let canonical = derive_source_canonical_id(SourceDerivedIdInput {
                source_hash: &source_hash,
                adapter_id: PUB_ADAPTER_ID,
                source_object_key: &source_object_key,
                semantic_role: PARAGRAPH_SEMANTIC_ROLE,
            })
            .map_err(|error| PubParagraphMaterializationError::Identity {
                story_id,
                ordinal,
                message: format!("{error:?}"),
            })?;
            let paragraph_id = ParagraphId::from_canonical(canonical);
            let range = TextRange::new(start, end).map_err(|_| {
                PubParagraphMaterializationError::Range {
                    story_id,
                    ordinal,
                    start,
                    end,
                }
            })?;

            if graph
                .paragraphs
                .insert(
                    paragraph_id,
                    Paragraph {
                        id: paragraph_id,
                        story_id,
                        range,
                        style_ref: None,
                        properties: (),
                    },
                )
                .is_some()
            {
                return Err(PubParagraphMaterializationError::DuplicateIdentity {
                    paragraph_id,
                });
            }
            paragraph_ids.push(paragraph_id);
        }

        graph
            .stories
            .get_mut(&story_id)
            .expect("StoryId came from graph registry")
            .paragraphs = paragraph_ids;
    }

    Ok(())
}

fn canonical_paragraph_ranges(text: &str) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut start = 0_u64;
    let mut scalar_index = 0_u64;

    for character in text.chars() {
        scalar_index += 1;
        if character == '\r' {
            ranges.push((start, scalar_index));
            start = scalar_index;
        }
    }

    ranges.push((start, scalar_index));
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_paragraph_without_terminal_cr_is_materialized() {
        assert_eq!(
            canonical_paragraph_ranges("alpha\rbeta"),
            vec![(0, 6), (6, 10)]
        );
    }

    #[test]
    fn trailing_cr_materializes_empty_final_paragraph() {
        assert_eq!(canonical_paragraph_ranges("alpha\r"), vec![(0, 6), (6, 6)]);
    }

    #[test]
    fn consecutive_cr_materializes_explicit_empty_middle_paragraph() {
        assert_eq!(
            canonical_paragraph_ranges("a\r\rb"),
            vec![(0, 2), (2, 3), (3, 4)]
        );
    }

    #[test]
    fn empty_story_has_one_empty_final_paragraph() {
        assert_eq!(canonical_paragraph_ranges(""), vec![(0, 0)]);
    }

    #[test]
    fn scalar_ranges_do_not_count_utf16_code_units() {
        assert_eq!(canonical_paragraph_ranges("😀\rx"), vec![(0, 2), (2, 3)]);
    }
}
