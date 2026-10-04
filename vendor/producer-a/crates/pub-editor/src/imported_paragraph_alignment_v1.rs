use crate::{EditorSession, ImportedParagraphProjectionErrorV1, ImportedParagraphV1};
use pub_model::{ParagraphId, StoryId, TextRange};
use pub_reader::{PubParagraphAlignment, PubParagraphAlignmentRun};
use std::fmt;

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

impl EditorSession {
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
    ) -> Result<Option<ImportedParagraphBaseAlignmentV1>, ImportedParagraphBaseAlignmentErrorV1> {
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
    use pub_core::{RawSpan, StreamPath};
    use pub_model::{
        AuthorityClass, ByteRange, ReadConfidence, Sha256Digest, SourceDescriptor, SourceRef,
        SourceRole,
    };

    fn source_ref() -> SourceRef {
        SourceRef {
            source: SourceDescriptor {
                format: "pub".to_owned(),
                format_version: Some("0x2c".to_owned()),
                adapter_version: "test".to_owned(),
                source_hash: Sha256Digest::from_bytes([0x11; 32]),
            },
            stream: StreamPath::new("/Quill/QuillSub/CONTENTS").expect("stream path"),
            range: ByteRange { start: 0, end: 1 },
            object_key: Some("story/test".to_owned()),
            property_path: Some("FDPP/ParagraphAlignment".to_owned()),
            role: SourceRole::Semantic,
            authority: AuthorityClass::Authoritative,
            confidence: ReadConfidence::Exact,
        }
    }

    fn story_id() -> StoryId {
        serde_json::from_str(""10000000-0000-4000-8000-000000000001"")
            .expect("canonical StoryId")
    }

    fn run(
        start: u32,
        end: u32,
        alignment: PubParagraphAlignment,
    ) -> PubParagraphAlignmentRun {
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
            resolve_paragraph_alignment_v1(
                TextRange::new(2, 8).unwrap(),
                &[&second, &first]
            ),
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
