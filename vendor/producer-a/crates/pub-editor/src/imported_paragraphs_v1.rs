use crate::{EditorSession, story_state_id_v1};
use pub_model::{
    AuthorityClass, ParagraphId, ReadConfidence, Sha256Digest, SourceDerivedIdInput,
    SourceDescriptor, SourceRole, Story, StoryId, TextRange, derive_source_canonical_id,
};
use std::collections::BTreeSet;
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
    TerminalCrProvenanceUnknown {
        story_id: StoryId,
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
            Self::TerminalCrProvenanceUnknown { story_id } => write!(
                formatter,
                "cannot project terminal-CR paragraph topology for Story {} without exact mature-Quill provenance",
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
            let Some(source_state_id) = self.source_story_state_ids.get(story_id) else {
                continue;
            };
            let protected_terminal_cr =
                imported_mature_quill_terminal_cr_is_proven_v1(&self.graph.source, story);
            if story.text.ends_with('\r') && !protected_terminal_cr {
                return Err(
                    ImportedParagraphProjectionErrorV1::TerminalCrProvenanceUnknown {
                        story_id: *story_id,
                    },
                );
            }
            result.extend(project_imported_story_paragraphs_v1(
                self.source_hash,
                *story_id,
                source_state_id,
                &story.text,
                protected_terminal_cr,
            )?);
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

fn project_imported_story_paragraphs_v1(
    source_hash: Sha256Digest,
    story_id: StoryId,
    source_state_id: &str,
    current_text: &str,
    protected_terminal_cr: bool,
) -> Result<Vec<ImportedParagraphV1>, ImportedParagraphProjectionErrorV1> {
    if story_state_id_v1(story_id, current_text) != source_state_id {
        return Ok(Vec::new());
    }

    canonical_paragraph_ranges_v1(current_text, protected_terminal_cr)
        .into_iter()
        .enumerate()
        .map(|(ordinal, (start, end))| {
            let paragraph_id = derive_imported_paragraph_id_v1(source_hash, story_id, ordinal)?;
            let range = TextRange::new(start, end).map_err(|_| {
                ImportedParagraphProjectionErrorV1::Range {
                    story_id,
                    ordinal,
                    start,
                    end,
                }
            })?;
            Ok(ImportedParagraphV1 {
                paragraph_id,
                story_id,
                range,
            })
        })
        .collect()
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

// Uses the same persisted-source evidence contract as the current text-input
// StoryEditDomainV1 authority: mature 0x2C + exact authoritative Quill
// SYID/TEXT refs sharing one persisted Story object key. Never infer from a
// trailing U+000D alone.
fn imported_mature_quill_terminal_cr_is_proven_v1(
    source: &SourceDescriptor,
    story: &Story,
) -> bool {
    if !story.text.ends_with('\r') {
        return false;
    }

    if source.format != "pub"
        || source.format_version.as_deref() != Some("0x2c")
        || !source.adapter_version.starts_with("pub-rs/")
    {
        return false;
    }

    let exact_keys = |role: SourceRole, path: &str| {
        story
            .source_refs
            .iter()
            .filter(|reference| {
                reference.validate_primary_source(source).is_ok()
                    && reference.carrier == pub_reader::QUILL_STREAM_PATH
                    && reference.path.as_deref() == Some(path)
                    && reference.role == role
                    && reference.authority == AuthorityClass::Authoritative
                    && reference.confidence == Some(ReadConfidence::Exact)
                    && reference
                        .object_key
                        .as_deref()
                        .is_some_and(|key| key.starts_with("quill/syid/"))
            })
            .filter_map(|reference| reference.object_key.clone())
            .collect::<BTreeSet<_>>()
    };

    let syid = exact_keys(SourceRole::Relation, "SYID");
    let text = exact_keys(SourceRole::Semantic, "TEXT");
    syid.iter().any(|key| text.contains(key))
}

fn canonical_paragraph_ranges_v1(text: &str, protected_terminal_cr: bool) -> Vec<(u64, u64)> {
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

    if !protected_terminal_cr || start != scalar_index {
        ranges.push((start, scalar_index));
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::{CanonicalId, SourceRef};

    fn story_id() -> StoryId {
        StoryId::from_canonical(CanonicalId::from_bytes([0x42; 16]))
    }

    fn source_hash() -> Sha256Digest {
        Sha256Digest::from_bytes([0x24; 32])
    }

    fn source_descriptor() -> SourceDescriptor {
        SourceDescriptor {
            format: "pub".to_owned(),
            format_version: Some("0x2c".to_owned()),
            adapter_version: "pub-rs/test".to_owned(),
            source_hash: source_hash(),
        }
    }

    fn quill_ref(role: SourceRole, path: &str, object_key: &str) -> SourceRef {
        SourceRef {
            format: "pub".to_owned(),
            adapter_version: "pub-rs/test".to_owned(),
            source_hash: source_hash(),
            carrier: pub_reader::QUILL_STREAM_PATH.to_owned(),
            object_key: Some(object_key.to_owned()),
            path: Some(path.to_owned()),
            byte_range: None,
            role,
            authority: AuthorityClass::Authoritative,
            confidence: Some(ReadConfidence::Exact),
        }
    }

    fn source_story(text: &str, source_refs: Vec<SourceRef>) -> Story {
        Story {
            id: story_id(),
            text: text.to_owned(),
            paragraphs: Vec::new(),
            runs: Vec::new(),
            fields: Vec::new(),
            hyperlinks: Vec::new(),
            source_refs,
        }
    }

    #[test]
    fn terminal_cr_protection_requires_matching_exact_quill_story_refs() {
        let source = source_descriptor();
        let proven = source_story(
            "alpha\r",
            vec![
                quill_ref(SourceRole::Relation, "SYID", "quill/syid/7"),
                quill_ref(SourceRole::Semantic, "TEXT", "quill/syid/7"),
            ],
        );
        assert!(imported_mature_quill_terminal_cr_is_proven_v1(
            &source, &proven
        ));

        let trailing_cr_only = source_story("alpha\r", Vec::new());
        assert!(!imported_mature_quill_terminal_cr_is_proven_v1(
            &source,
            &trailing_cr_only
        ));

        let mismatched = source_story(
            "alpha\r",
            vec![
                quill_ref(SourceRole::Relation, "SYID", "quill/syid/7"),
                quill_ref(SourceRole::Semantic, "TEXT", "quill/syid/8"),
            ],
        );
        assert!(!imported_mature_quill_terminal_cr_is_proven_v1(
            &source,
            &mismatched
        ));
    }

    #[test]
    fn paragraph_boundaries_follow_canonical_cr_law() {
        assert_eq!(
            canonical_paragraph_ranges_v1("alpha\rbeta", false),
            vec![(0, 6), (6, 10)]
        );
        assert_eq!(
            canonical_paragraph_ranges_v1("alpha\r", false),
            vec![(0, 6), (6, 6)]
        );
        assert_eq!(
            canonical_paragraph_ranges_v1("a\r\rb", false),
            vec![(0, 2), (2, 3), (3, 4)]
        );
        assert_eq!(canonical_paragraph_ranges_v1("", false), vec![(0, 0)]);
    }

    #[test]
    fn proven_imported_terminal_cr_does_not_create_empty_final_paragraph() {
        assert_eq!(canonical_paragraph_ranges_v1("alpha\r", true), vec![(0, 6)]);
        assert_eq!(canonical_paragraph_ranges_v1("\r", true), vec![(0, 1)]);
        assert_eq!(
            canonical_paragraph_ranges_v1("a\r\rb\r", true),
            vec![(0, 2), (2, 3), (3, 5)]
        );
    }

    #[test]
    fn paragraph_ranges_use_unicode_scalar_coordinates() {
        assert_eq!(
            canonical_paragraph_ranges_v1("😀\rx", false),
            vec![(0, 2), (2, 3)]
        );
    }

    #[test]
    fn source_story_edit_invalidates_projection_and_exact_restore_recovers_ids() {
        let source = "alpha\rbeta";
        let source_state = story_state_id_v1(story_id(), source);
        let before = project_imported_story_paragraphs_v1(
            source_hash(),
            story_id(),
            &source_state,
            source,
            false,
        )
        .expect("source projection");
        assert_eq!(before.len(), 2);

        let edited = project_imported_story_paragraphs_v1(
            source_hash(),
            story_id(),
            &source_state,
            "alphx\rbeta",
            false,
        )
        .expect("edited projection");
        assert!(edited.is_empty());

        let restored = project_imported_story_paragraphs_v1(
            source_hash(),
            story_id(),
            &source_state,
            source,
            false,
        )
        .expect("restored projection");
        assert_eq!(restored, before);
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
