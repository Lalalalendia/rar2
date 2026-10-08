use crate::{EditorSession, ImportedParagraphProjectionErrorV1};
use pub_model::{ParagraphId, StoryId, TextRange};
use pub_reader::{PubParagraphFlowConstraint, PubParagraphFlowRun};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportedParagraphFlowConstraintV1 {
    StartInNextTextBox,
    KeepLinesTogether,
    KeepWithNext,
    WidowControl,
}

impl From<PubParagraphFlowConstraint> for ImportedParagraphFlowConstraintV1 {
    fn from(value: PubParagraphFlowConstraint) -> Self {
        match value {
            PubParagraphFlowConstraint::StartInNextTextBox => Self::StartInNextTextBox,
            PubParagraphFlowConstraint::KeepLinesTogether => Self::KeepLinesTogether,
            PubParagraphFlowConstraint::KeepWithNext => Self::KeepWithNext,
            PubParagraphFlowConstraint::WidowControl => Self::WidowControl,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedParagraphFlowConstraintBindingV1 {
    pub paragraph_id: ParagraphId,
    pub story_id: StoryId,
    pub range: TextRange,
    pub constraint: ImportedParagraphFlowConstraintV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedParagraphFlowErrorV1 {
    ParagraphProjection(ImportedParagraphProjectionErrorV1),
}

impl fmt::Display for ImportedParagraphFlowErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParagraphProjection(error) => {
                write!(formatter, "imported paragraph projection failed: {error}")
            }
        }
    }
}

impl std::error::Error for ImportedParagraphFlowErrorV1 {}

impl From<ImportedParagraphProjectionErrorV1> for ImportedParagraphFlowErrorV1 {
    fn from(error: ImportedParagraphProjectionErrorV1) -> Self {
        Self::ParagraphProjection(error)
    }
}

impl EditorSession {
    /// Binds only explicit, native-grounded paragraph-flow ON records to the
    /// current immutable-source ParagraphIds.
    ///
    /// Absence is intentionally not projected as false: Publisher style/default
    /// inheritance and explicit clear serialization remain outside the bounded
    /// carrier slice.
    pub fn imported_paragraph_flow_constraints_v1(
        &self,
    ) -> Result<Vec<ImportedParagraphFlowConstraintBindingV1>, ImportedParagraphFlowErrorV1> {
        let mut result = Vec::new();

        for paragraph in self.imported_paragraphs_v1()? {
            let runs = self
                .source_paragraph_flow_runs
                .iter()
                .filter(|run| run.story_id == paragraph.story_id)
                .collect::<Vec<_>>();
            for constraint in resolve_paragraph_flow_constraints_v1(paragraph.range, &runs) {
                result.push(ImportedParagraphFlowConstraintBindingV1 {
                    paragraph_id: paragraph.paragraph_id,
                    story_id: paragraph.story_id,
                    range: paragraph.range,
                    constraint,
                });
            }
        }

        result.sort_by_key(|item| {
            (
                item.story_id,
                item.range.start,
                item.paragraph_id,
                item.constraint,
            )
        });
        Ok(result)
    }
}

fn resolve_paragraph_flow_constraints_v1(
    paragraph: TextRange,
    runs: &[&PubParagraphFlowRun],
) -> Vec<ImportedParagraphFlowConstraintV1> {
    if paragraph.is_empty() {
        return Vec::new();
    }

    let mut constraints = BTreeSet::new();
    for run in runs {
        let run_start = u64::from(run.story_scalar_start);
        let run_end = u64::from(run.story_scalar_end);
        if run_start <= paragraph.start && run_end >= paragraph.end && run_start < run_end {
            constraints.insert(ImportedParagraphFlowConstraintV1::from(run.constraint));
        }
    }
    constraints.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::{
        AuthorityClass, ByteRange, ReadConfidence, Sha256Digest, SourceRef, SourceRole,
    };

    fn story_id() -> StoryId {
        serde_json::from_str("\"10000000-0000-4000-8000-000000000001\"").expect("canonical StoryId")
    }

    fn source_ref() -> SourceRef {
        SourceRef {
            format: "pub".to_owned(),
            adapter_version: "test".to_owned(),
            source_hash: Sha256Digest::from_bytes([0x11; 32]),
            carrier: "/Quill/QuillSub/CONTENTS".to_owned(),
            object_key: Some("story/test".to_owned()),
            path: Some("FDPP/ParagraphFlow".to_owned()),
            byte_range: Some(ByteRange::new(0, 6)),
            role: SourceRole::Semantic,
            authority: AuthorityClass::Authoritative,
            confidence: Some(ReadConfidence::Exact),
        }
    }

    fn run(start: u32, end: u32, constraint: PubParagraphFlowConstraint) -> PubParagraphFlowRun {
        PubParagraphFlowRun {
            story_id: story_id(),
            story_utf16_start: start,
            story_utf16_end: end,
            story_scalar_start: start,
            story_scalar_end: end,
            constraint,
            source_value: 4,
            source_ref: source_ref(),
        }
    }

    #[test]
    fn exact_cover_binds_explicit_on_constraint() {
        let keep = run(2, 8, PubParagraphFlowConstraint::KeepWithNext);
        assert_eq!(
            resolve_paragraph_flow_constraints_v1(TextRange::new(2, 8).unwrap(), &[&keep]),
            vec![ImportedParagraphFlowConstraintV1::KeepWithNext]
        );
    }

    #[test]
    fn partial_overlap_does_not_invent_paragraph_policy() {
        let keep = run(3, 8, PubParagraphFlowConstraint::KeepWithNext);
        assert!(
            resolve_paragraph_flow_constraints_v1(TextRange::new(2, 8).unwrap(), &[&keep])
                .is_empty()
        );
    }

    #[test]
    fn duplicate_source_observations_deduplicate_by_constraint() {
        let first = run(2, 8, PubParagraphFlowConstraint::WidowControl);
        let second = run(2, 8, PubParagraphFlowConstraint::WidowControl);
        assert_eq!(
            resolve_paragraph_flow_constraints_v1(
                TextRange::new(2, 8).unwrap(),
                &[&first, &second]
            ),
            vec![ImportedParagraphFlowConstraintV1::WidowControl]
        );
    }

    #[test]
    fn multiple_explicit_constraints_are_stably_ordered() {
        let widow = run(2, 8, PubParagraphFlowConstraint::WidowControl);
        let start = run(2, 8, PubParagraphFlowConstraint::StartInNextTextBox);
        assert_eq!(
            resolve_paragraph_flow_constraints_v1(TextRange::new(2, 8).unwrap(), &[&widow, &start]),
            vec![
                ImportedParagraphFlowConstraintV1::StartInNextTextBox,
                ImportedParagraphFlowConstraintV1::WidowControl,
            ]
        );
    }
}
