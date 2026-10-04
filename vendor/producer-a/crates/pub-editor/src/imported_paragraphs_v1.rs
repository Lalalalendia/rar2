use crate::{EditorSession, story_state_id_v1};
use pub_model::{
    ParagraphId, Sha256Digest, SourceDerivedIdInput, StoryId, TextRange, derive_source_canonical_id,
};
use std::fmt;

const IMPORTED_PARAGRAPH_ROLE_V1: &str = "cdm.paragraph";
const IMPORTED_PARAGRAPH_ADAPTER_V1: &str = "pub-rs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedParagraphV1 {
    pub paragraph_id: ParagraphId,
    pub story_id: StoryId,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedParagraphProjectionErrorV1 {
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
}

impl fmt::Display for ImportedParagraphProjectionErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity {
                story_id,
                ordinal,
                message,
            } => write!(
                formatter,
                "cannot derive imported ParagraphId for Story {} paragraph {ordinal}: {message}",
                story_id.as_canonical()
            ),
            Self::Range {
                story_id,
                ordinal,
                start,
                end,
            } => write!(
                formatter,
                "invalid imported paragraph range for Story {} paragraph {ordinal}: {start}..{end}",
                story_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for ImportedParagraphProjectionErrorV1 {}

impl EditorSession {
    /// Projects immutable-source paragraph identity without changing the resolved
    /// graph's current plain-Story authoring contract.
    ///
    /// A source paragraph projection exists only while the Story still has its
    /// exact source text state. Any Story edit invalidates the projection; exact
    /// undo to source state makes it available again.
    pub fn imported_paragraphs_v1(
        &self,
    ) -> Result<Vec<ImportedParagraphV1>, ImportedParagraphProjectionErrorV1> {
        let mut result = Vec::new();

        for (story_id, story) in &self.graph.stories {
            let current_state = story_state_id_v1(*story_id, &story.text);
            if self.source_story_state_ids.get(story_id) != Some(&current_state) {
                continue;
            }

            for (ordinal, (start, end)) in canonical_paragraph_ranges_v1(&story.text)
                .into_iter()
                .enumerate()
            {
                let paragraph_id =
                    derive_imported_paragraph_id_v1(self.source_hash, *story_id, ordinal)?;
                let range = TextRange::new(start, end).map_err(|_| {
                    ImportedParagraphProjectionErrorV1::Range {
                        story_id: *story_id,
                        ordinal,
                        start,
                        end,
                    }
                })?;
                result.push(ImportedParagraphV1 {
                    paragraph_id,
                    story_id: *story_id,
                    range,
                });
            }
        }

        result.sort_by_key(|item| (item.story_id, item.range.start, item.paragraph_id));
        Ok(result)
    }

    pub fn imported_paragraph_v1(
        &self,
        paragraph_id: ParagraphId,
    ) -> Result<Option<ImportedParagraphV1>, ImportedParagraphProjectionErrorV1> {
        Ok(self
            .imported_paragraphs_v1()?
            .into_iter()
            .find(|item| item.paragraph_id == paragraph_id))
    }
}

fn derive_imported_paragraph_id_v1(
    source_hash: Sha256Digest,
    story_id: StoryId,
    ordinal: usize,
) -> Result<ParagraphId, ImportedParagraphProjectionErrorV1> {
    let source_object_key = format!("story/{}/paragraph/{ordinal}", story_id.as_canonical());
    let id = derive_source_canonical_id(SourceDerivedIdInput {
        source_hash: &source_hash,
        adapter_id: IMPORTED_PARAGRAPH_ADAPTER_V1,
        source_object_key: &source_object_key,
        semantic_role: IMPORTED_PARAGRAPH_ROLE_V1,
    })
    .map_err(|error| ImportedParagraphProjectionErrorV1::Identity {
        story_id,
        ordinal,
        message: format!("{error:?}"),
    })?;
    Ok(ParagraphId::from_canonical(id))
}

fn canonical_paragraph_ranges_v1(text: &str) -> Vec<(u64, u64)> {
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
    use pub_model::CanonicalId;

    fn story_id() -> StoryId {
        StoryId::from_canonical(CanonicalId::from_bytes([0x42; 16]))
    }

    fn source_hash() -> Sha256Digest {
        Sha256Digest::from_bytes([0x24; 32])
    }

    #[test]
    fn paragraph_boundaries_follow_canonical_cr_law() {
        assert_eq!(
            canonical_paragraph_ranges_v1("alpha\rbeta"),
            vec![(0, 6), (6, 10)]
        );
        assert_eq!(
            canonical_paragraph_ranges_v1("alpha\r"),
            vec![(0, 6), (6, 6)]
        );
        assert_eq!(
            canonical_paragraph_ranges_v1("a\r\rb"),
            vec![(0, 2), (2, 3), (3, 4)]
        );
        assert_eq!(canonical_paragraph_ranges_v1(""), vec![(0, 0)]);
    }

    #[test]
    fn paragraph_ranges_use_unicode_scalar_coordinates() {
        assert_eq!(canonical_paragraph_ranges_v1("😀\rx"), vec![(0, 2), (2, 3)]);
    }

    #[test]
    fn imported_paragraph_ids_are_deterministic_and_ordinal_distinct() {
        let first =
            derive_imported_paragraph_id_v1(source_hash(), story_id(), 0).expect("paragraph 0");
        let again =
            derive_imported_paragraph_id_v1(source_hash(), story_id(), 0).expect("paragraph 0");
        let second =
            derive_imported_paragraph_id_v1(source_hash(), story_id(), 1).expect("paragraph 1");

        assert_eq!(first, again);
        assert_ne!(first, second);
    }
}
