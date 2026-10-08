use crate::{
    AuthoredParagraphAlignmentValueV1, EditorError, EditorSession, EffectiveParagraphAlignmentV1,
    EffectiveParagraphAlignmentValueV1, ImportedParagraphProjectionErrorV1, ImportedParagraphV1,
    ParagraphAlignmentAuthorityV1,
};
use pub_model::{ParagraphId, StoryId, TextRange};
use pub_reader::{PubParagraphAlignment, PubParagraphAlignmentRun};
use std::{collections::BTreeMap, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportedParagraphAlignmentValueV1 {
    Center,
    Right,
    InterWord,
    Distribute,
}

impl From<PubParagraphAlignment> for ImportedParagraphAlignmentValueV1 {
    fn from(value: PubParagraphAlignment) -> Self {
        match value {
            PubParagraphAlignment::Center => Self::Center,
            PubParagraphAlignment::Right => Self::Right,
            PubParagraphAlignment::InterWord => Self::InterWord,
            PubParagraphAlignment::Distribute => Self::Distribute,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedParagraphBaseAlignmentV1 {
    pub paragraph_id: ParagraphId,
    pub story_id: StoryId,
    pub range: TextRange,
    pub alignment: ImportedParagraphAlignmentValueV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedParagraphBaseAlignmentErrorV1 {
    ParagraphProjection(ImportedParagraphProjectionErrorV1),
}

impl fmt::Display for ImportedParagraphBaseAlignmentErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParagraphProjection(error) => {
                write!(formatter, "imported paragraph projection failed: {error}")
            }
        }
    }
}

impl std::error::Error for ImportedParagraphBaseAlignmentErrorV1 {}

impl From<ImportedParagraphProjectionErrorV1> for ImportedParagraphBaseAlignmentErrorV1 {
    fn from(error: ImportedParagraphProjectionErrorV1) -> Self {
        Self::ParagraphProjection(error)
    }
}

fn effective_paragraph_alignment_rows_v1(
    paragraphs: impl IntoIterator<Item = ImportedParagraphV1>,
    base_by_id: &BTreeMap<ParagraphId, ImportedParagraphAlignmentValueV1>,
    overrides: &BTreeMap<ParagraphId, AuthoredParagraphAlignmentValueV1>,
) -> Vec<EffectiveParagraphAlignmentV1> {
    let mut result = paragraphs
        .into_iter()
        .map(|paragraph| {
            let imported_base = base_by_id.get(&paragraph.paragraph_id).copied();
            let authored_override = overrides.get(&paragraph.paragraph_id).copied();
            let (effective, authority) = if let Some(value) = authored_override {
                (
                    Some(EffectiveParagraphAlignmentValueV1::from(value)),
                    Some(ParagraphAlignmentAuthorityV1::ChapteraOverride),
                )
            } else if let Some(value) = imported_base {
                (
                    Some(EffectiveParagraphAlignmentValueV1::from(value)),
                    Some(ParagraphAlignmentAuthorityV1::ImportedBase),
                )
            } else {
                (None, None)
            };
            EffectiveParagraphAlignmentV1 {
                paragraph_id: paragraph.paragraph_id,
                story_id: paragraph.story_id,
                range: paragraph.range,
                imported_base,
                authored_override,
                effective,
                authority,
            }
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|item| (item.story_id, item.range.start, item.paragraph_id));
    result
}

impl EditorSession {
    /// Resolves the complete current alignment authority for one Story in one
    /// bounded batch. This is the layout-oriented companion to the existing
    /// single-ParagraphId oracle and folds authored override history only once.
    pub fn effective_story_paragraph_alignments_v1(
        &self,
        story_id: StoryId,
    ) -> Result<Vec<EffectiveParagraphAlignmentV1>, EditorError> {
        self.validate_source_identity()?;
        let paragraphs = self
            .imported_paragraphs_v1()
            .map_err(|_| EditorError::ParagraphAlignmentProjectionUnavailable)?
            .into_iter()
            .filter(|paragraph| paragraph.story_id == story_id)
            .collect::<Vec<_>>();
        let base_by_id = self
            .imported_paragraph_base_alignments_v1()
            .map_err(|_| EditorError::ParagraphAlignmentProjectionUnavailable)?
            .into_iter()
            .filter(|item| item.story_id == story_id)
            .map(|item| (item.paragraph_id, item.alignment))
            .collect::<BTreeMap<_, _>>();
        let overrides = self.current_paragraph_alignment_overrides_v1()?;

        Ok(effective_paragraph_alignment_rows_v1(
            paragraphs,
            &base_by_id,
            &overrides,
        ))
    }

    /// Returns only imported paragraph base alignments that are fully and
    /// unambiguously covered by current source-authority alignment runs.
    ///
    /// Missing source evidence is not interpreted as Left/default. Empty
    /// paragraphs are not assigned a base value from text-run evidence.
    pub fn imported_paragraph_base_alignments_v1(
        &self,
    ) -> Result<Vec<ImportedParagraphBaseAlignmentV1>, ImportedParagraphBaseAlignmentErrorV1> {
        let mut result = Vec::new();

        for paragraph in self.imported_paragraphs_v1()? {
            let runs = self
                .source_paragraph_alignments
                .iter()
                .filter(|run| run.story_id == paragraph.story_id)
                .collect::<Vec<_>>();
            let Some(alignment) = resolve_paragraph_alignment_v1(paragraph.range, &runs) else {
                continue;
            };
            result.push(ImportedParagraphBaseAlignmentV1 {
                paragraph_id: paragraph.paragraph_id,
                story_id: paragraph.story_id,
                range: paragraph.range,
                alignment,
            });
        }

        result.sort_by_key(|item| (item.story_id, item.range.start, item.paragraph_id));
        Ok(result)
    }

    pub fn imported_paragraph_base_alignment_v1(
        &self,
        paragraph_id: ParagraphId,
    ) -> Result<Option<ImportedParagraphBaseAlignmentV1>, ImportedParagraphBaseAlignmentErrorV1>
    {
        Ok(self
            .imported_paragraph_base_alignments_v1()?
            .into_iter()
            .find(|item| item.paragraph_id == paragraph_id))
    }
}

fn resolve_paragraph_alignment_v1(
    paragraph: TextRange,
    runs: &[&PubParagraphAlignmentRun],
) -> Option<ImportedParagraphAlignmentValueV1> {
    if paragraph.is_empty() {
        return None;
    }

    let mut intervals = Vec::new();
    let mut alignment = None;

    for run in runs {
        let run_start = u64::from(run.story_scalar_start);
        let run_end = u64::from(run.story_scalar_end);
        if run_start >= run_end || run_end <= paragraph.start || run_start >= paragraph.end {
            continue;
        }

        let value = ImportedParagraphAlignmentValueV1::from(run.alignment);
        if alignment.is_some_and(|current| current != value) {
            return None;
        }
        alignment = Some(value);

        intervals.push((run_start.max(paragraph.start), run_end.min(paragraph.end)));
    }

    let alignment = alignment?;
    intervals.sort_unstable();

    let mut covered_end = paragraph.start;
    for (start, end) in intervals {
        if start > covered_end {
            return None;
        }
        if end > covered_end {
            covered_end = end;
        }
    }

    (covered_end >= paragraph.end).then_some(alignment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::{
        AuthorityClass, ByteRange, ReadConfidence, Sha256Digest, SourceRef, SourceRole,
    };

    fn source_ref() -> SourceRef {
        SourceRef {
            format: "pub".to_owned(),
            adapter_version: "test".to_owned(),
            source_hash: Sha256Digest::from_bytes([0x11; 32]),
            carrier: "/Quill/QuillSub/CONTENTS".to_owned(),
            object_key: Some("story/test".to_owned()),
            path: Some("FDPP/ParagraphAlignment".to_owned()),
            byte_range: Some(ByteRange::new(0, 1)),
            role: SourceRole::Semantic,
            authority: AuthorityClass::Authoritative,
            confidence: Some(ReadConfidence::Exact),
        }
    }

    fn story_id() -> StoryId {
        serde_json::from_str("\"10000000-0000-4000-8000-000000000001\"").expect("canonical StoryId")
    }

    fn run(start: u32, end: u32, alignment: PubParagraphAlignment) -> PubParagraphAlignmentRun {
        PubParagraphAlignmentRun {
            story_id: story_id(),
            story_utf16_start: start,
            story_utf16_end: end,
            story_scalar_start: start,
            story_scalar_end: end,
            alignment,
            source_value: 0,
            source_ref: source_ref(),
        }
    }

    fn paragraph_id(suffix: u8) -> ParagraphId {
        serde_json::from_str(&format!(
            "\"20000000-0000-4000-8000-0000000000{suffix:02x}\""
        ))
        .expect("canonical ParagraphId")
    }

    #[test]
    fn effective_batch_preserves_base_override_and_empty_authority() {
        let base_paragraph = ImportedParagraphV1 {
            paragraph_id: paragraph_id(1),
            story_id: story_id(),
            range: TextRange::new(0, 4).unwrap(),
        };
        let override_paragraph = ImportedParagraphV1 {
            paragraph_id: paragraph_id(2),
            story_id: story_id(),
            range: TextRange::new(4, 8).unwrap(),
        };
        let empty_paragraph = ImportedParagraphV1 {
            paragraph_id: paragraph_id(3),
            story_id: story_id(),
            range: TextRange::new(8, 8).unwrap(),
        };
        let base_by_id = BTreeMap::from([
            (
                base_paragraph.paragraph_id,
                ImportedParagraphAlignmentValueV1::Center,
            ),
            (
                override_paragraph.paragraph_id,
                ImportedParagraphAlignmentValueV1::InterWord,
            ),
        ]);
        let overrides = BTreeMap::from([(
            override_paragraph.paragraph_id,
            AuthoredParagraphAlignmentValueV1::Right,
        )]);

        let rows = effective_paragraph_alignment_rows_v1(
            [
                empty_paragraph.clone(),
                override_paragraph.clone(),
                base_paragraph.clone(),
            ],
            &base_by_id,
            &overrides,
        );

        assert_eq!(
            rows.iter().map(|row| row.paragraph_id).collect::<Vec<_>>(),
            vec![
                base_paragraph.paragraph_id,
                override_paragraph.paragraph_id,
                empty_paragraph.paragraph_id,
            ]
        );
        assert_eq!(
            (
                rows[0].imported_base,
                rows[0].authored_override,
                rows[0].effective,
                rows[0].authority,
            ),
            (
                Some(ImportedParagraphAlignmentValueV1::Center),
                None,
                Some(EffectiveParagraphAlignmentValueV1::Center),
                Some(ParagraphAlignmentAuthorityV1::ImportedBase),
            )
        );
        assert_eq!(
            (
                rows[1].imported_base,
                rows[1].authored_override,
                rows[1].effective,
                rows[1].authority,
            ),
            (
                Some(ImportedParagraphAlignmentValueV1::InterWord),
                Some(AuthoredParagraphAlignmentValueV1::Right),
                Some(EffectiveParagraphAlignmentValueV1::Right),
                Some(ParagraphAlignmentAuthorityV1::ChapteraOverride),
            )
        );
        assert_eq!(
            (
                rows[2].imported_base,
                rows[2].authored_override,
                rows[2].effective,
                rows[2].authority,
            ),
            (None, None, None, None)
        );
    }

    #[test]
    #[ignore = "requires the pinned public Carlton March PUB path"]
    fn real_carlton_imported_paragraph_base_alignment_uses_current_paragraph_ids() {
        let path = std::env::var_os("CHAPTERA_CARLTON_PUB")
            .map(std::path::PathBuf::from)
            .expect("CHAPTERA_CARLTON_PUB");
        let bytes = std::fs::read(path).expect("read pinned Carlton March PUB");
        let source_hash = "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3"
            .parse::<Sha256Digest>()
            .expect("pinned Carlton source hash");
        let session = crate::open_mature_0x2c_editor(&bytes, source_hash)
            .expect("open Carlton EditorSession");

        let paragraphs = session
            .imported_paragraphs_v1()
            .expect("project Carlton imported ParagraphIds");
        let paragraph_ids = paragraphs
            .iter()
            .map(|paragraph| paragraph.paragraph_id)
            .collect::<std::collections::BTreeSet<_>>();
        let base = session
            .imported_paragraph_base_alignments_v1()
            .expect("bind Carlton imported paragraph base alignment");

        assert!(
            !paragraph_ids.is_empty(),
            "Carlton must expose imported ParagraphIds"
        );
        assert!(
            !base.is_empty(),
            "Carlton's grounded FDPP alignment runs must bind to at least one current imported ParagraphId"
        );
        assert!(
            base.iter()
                .all(|item| paragraph_ids.contains(&item.paragraph_id)),
            "every imported base alignment must be keyed by a current imported ParagraphId"
        );
        assert!(
            base.iter()
                .any(|item| item.alignment == ImportedParagraphAlignmentValueV1::Right),
            "Carlton Reception authority must retain at least one grounded Right paragraph base"
        );
    }

    #[test]
    fn exact_uniform_coverage_produces_base_alignment() {
        let center = run(2, 8, PubParagraphAlignment::Center);
        assert_eq!(
            resolve_paragraph_alignment_v1(TextRange::new(2, 8).unwrap(), &[&center]),
            Some(ImportedParagraphAlignmentValueV1::Center)
        );
    }

    #[test]
    fn contiguous_same_value_runs_may_jointly_cover_one_paragraph() {
        let first = run(2, 5, PubParagraphAlignment::Center);
        let second = run(5, 8, PubParagraphAlignment::Center);
        assert_eq!(
            resolve_paragraph_alignment_v1(TextRange::new(2, 8).unwrap(), &[&second, &first]),
            Some(ImportedParagraphAlignmentValueV1::Center)
        );
    }

    #[test]
    fn gaps_and_mixed_values_fail_closed() {
        let first = run(2, 4, PubParagraphAlignment::Center);
        let gap = run(5, 8, PubParagraphAlignment::Center);
        assert_eq!(
            resolve_paragraph_alignment_v1(TextRange::new(2, 8).unwrap(), &[&first, &gap]),
            None
        );

        let right = run(4, 8, PubParagraphAlignment::Right);
        assert_eq!(
            resolve_paragraph_alignment_v1(TextRange::new(2, 8).unwrap(), &[&first, &right]),
            None
        );
    }

    #[test]
    fn crossing_run_is_safe_when_its_value_is_uniform_over_the_paragraph() {
        let center = run(0, 10, PubParagraphAlignment::Center);
        assert_eq!(
            resolve_paragraph_alignment_v1(TextRange::new(2, 8).unwrap(), &[&center]),
            Some(ImportedParagraphAlignmentValueV1::Center)
        );
    }

    #[test]
    fn unsupported_but_known_source_values_remain_typed_base_semantics() {
        let interword = run(2, 8, PubParagraphAlignment::InterWord);
        assert_eq!(
            resolve_paragraph_alignment_v1(TextRange::new(2, 8).unwrap(), &[&interword]),
            Some(ImportedParagraphAlignmentValueV1::InterWord)
        );

        let distribute = run(2, 8, PubParagraphAlignment::Distribute);
        assert_eq!(
            resolve_paragraph_alignment_v1(TextRange::new(2, 8).unwrap(), &[&distribute]),
            Some(ImportedParagraphAlignmentValueV1::Distribute)
        );
    }

    #[test]
    fn empty_paragraph_has_no_run_derived_base_alignment() {
        let center = run(0, 10, PubParagraphAlignment::Center);
        assert_eq!(
            resolve_paragraph_alignment_v1(TextRange::new(4, 4).unwrap(), &[&center]),
            None
        );
    }
}
