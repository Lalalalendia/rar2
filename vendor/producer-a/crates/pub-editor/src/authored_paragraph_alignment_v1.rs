use crate::{EditOperation, ImportedParagraphAlignmentValueV1};
use pub_model::{ParagraphId, StoryId, TextRange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoredParagraphAlignmentValueV1 {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveParagraphAlignmentValueV1 {
    Left,
    Center,
    Right,
    InterWord,
    Distribute,
}

impl From<AuthoredParagraphAlignmentValueV1> for EffectiveParagraphAlignmentValueV1 {
    fn from(value: AuthoredParagraphAlignmentValueV1) -> Self {
        match value {
            AuthoredParagraphAlignmentValueV1::Left => Self::Left,
            AuthoredParagraphAlignmentValueV1::Center => Self::Center,
            AuthoredParagraphAlignmentValueV1::Right => Self::Right,
        }
    }
}

impl From<ImportedParagraphAlignmentValueV1> for EffectiveParagraphAlignmentValueV1 {
    fn from(value: ImportedParagraphAlignmentValueV1) -> Self {
        match value {
            ImportedParagraphAlignmentValueV1::Center => Self::Center,
            ImportedParagraphAlignmentValueV1::Right => Self::Right,
            ImportedParagraphAlignmentValueV1::InterWord => Self::InterWord,
            ImportedParagraphAlignmentValueV1::Distribute => Self::Distribute,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphAlignmentAuthorityV1 {
    ImportedBase,
    ChapteraOverride,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveParagraphAlignmentV1 {
    pub paragraph_id: ParagraphId,
    pub story_id: StoryId,
    pub range: TextRange,
    pub imported_base: Option<ImportedParagraphAlignmentValueV1>,
    pub authored_override: Option<AuthoredParagraphAlignmentValueV1>,
    pub effective: Option<EffectiveParagraphAlignmentValueV1>,
    pub authority: Option<ParagraphAlignmentAuthorityV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParagraphAlignmentOverrideSnapshotV1 {
    pub paragraph_id: ParagraphId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<AuthoredParagraphAlignmentValueV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParagraphAlignmentTransitionErrorV1 {
    NonCanonicalTargets,
    SnapshotShapeMismatch,
    Stale {
        paragraph_id: ParagraphId,
        expected: Option<AuthoredParagraphAlignmentValueV1>,
        found: Option<AuthoredParagraphAlignmentValueV1>,
    },
}

impl fmt::Display for ParagraphAlignmentTransitionErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonCanonicalTargets => {
                formatter.write_str("paragraph alignment targets are not canonical sorted unique ids")
            }
            Self::SnapshotShapeMismatch => formatter.write_str(
                "paragraph alignment before/after snapshots do not cover the same targets",
            ),
            Self::Stale {
                paragraph_id,
                expected,
                found,
            } => write!(
                formatter,
                "paragraph alignment override for {} is stale: expected {expected:?}, found {found:?}",
                paragraph_id.as_canonical()
            ),
        }
    }
}

impl std::error::Error for ParagraphAlignmentTransitionErrorV1 {}

pub(crate) fn normalized_paragraph_alignment_override_v1(
    imported_base: Option<ImportedParagraphAlignmentValueV1>,
    requested: AuthoredParagraphAlignmentValueV1,
) -> Option<AuthoredParagraphAlignmentValueV1> {
    match (imported_base, requested) {
        (
            Some(ImportedParagraphAlignmentValueV1::Center),
            AuthoredParagraphAlignmentValueV1::Center,
        )
        | (
            Some(ImportedParagraphAlignmentValueV1::Right),
            AuthoredParagraphAlignmentValueV1::Right,
        ) => None,
        _ => Some(requested),
    }
}

pub(crate) fn paragraph_alignment_override_snapshots_v1(
    overrides: &BTreeMap<ParagraphId, AuthoredParagraphAlignmentValueV1>,
    paragraph_ids: &[ParagraphId],
) -> Vec<ParagraphAlignmentOverrideSnapshotV1> {
    paragraph_ids
        .iter()
        .map(|paragraph_id| ParagraphAlignmentOverrideSnapshotV1 {
            paragraph_id: *paragraph_id,
            value: overrides.get(paragraph_id).copied(),
        })
        .collect()
}

pub(crate) fn apply_paragraph_alignment_override_transition_v1(
    overrides: &mut BTreeMap<ParagraphId, AuthoredParagraphAlignmentValueV1>,
    expected: &[ParagraphAlignmentOverrideSnapshotV1],
    replacement: &[ParagraphAlignmentOverrideSnapshotV1],
) -> Result<(), ParagraphAlignmentTransitionErrorV1> {
    if expected.len() != replacement.len()
        || expected
            .iter()
            .zip(replacement)
            .any(|(before, after)| before.paragraph_id != after.paragraph_id)
    {
        return Err(ParagraphAlignmentTransitionErrorV1::SnapshotShapeMismatch);
    }

    let mut previous = None;
    for snapshot in expected {
        if previous.is_some_and(|value| value >= snapshot.paragraph_id) {
            return Err(ParagraphAlignmentTransitionErrorV1::NonCanonicalTargets);
        }
        previous = Some(snapshot.paragraph_id);

        let found = overrides.get(&snapshot.paragraph_id).copied();
        if found != snapshot.value {
            return Err(ParagraphAlignmentTransitionErrorV1::Stale {
                paragraph_id: snapshot.paragraph_id,
                expected: snapshot.value,
                found,
            });
        }
    }

    for snapshot in replacement {
        match snapshot.value {
            Some(value) => {
                overrides.insert(snapshot.paragraph_id, value);
            }
            None => {
                overrides.remove(&snapshot.paragraph_id);
            }
        }
    }
    Ok(())
}


pub(crate) fn paragraph_alignment_operation_snapshots_v1(
    operation: &EditOperation,
) -> Option<(
    &[ParagraphAlignmentOverrideSnapshotV1],
    &[ParagraphAlignmentOverrideSnapshotV1],
)> {
    match operation {
        EditOperation::SetParagraphAlignmentOverride { before, after, .. }
        | EditOperation::ClearParagraphAlignmentOverride { before, after, .. } => {
            Some((before, after))
        }
        _ => None,
    }
}

pub(crate) fn paragraph_alignment_override_state_from_history_v1(
    operations: &[EditOperation],
) -> Result<
    BTreeMap<ParagraphId, AuthoredParagraphAlignmentValueV1>,
    ParagraphAlignmentTransitionErrorV1,
> {
    let mut overrides = BTreeMap::new();
    for operation in operations {
        let Some((before, after)) = paragraph_alignment_operation_snapshots_v1(operation) else {
            continue;
        };
        apply_paragraph_alignment_override_transition_v1(&mut overrides, before, after)?;
    }
    Ok(overrides)
}

pub(crate) fn validate_paragraph_alignment_operation_against_history_v1(
    history_before: &[EditOperation],
    operation: &EditOperation,
) -> Result<(), ParagraphAlignmentTransitionErrorV1> {
    let mut overrides = paragraph_alignment_override_state_from_history_v1(history_before)?;
    let Some((before, after)) = paragraph_alignment_operation_snapshots_v1(operation) else {
        return Err(ParagraphAlignmentTransitionErrorV1::SnapshotShapeMismatch);
    };
    apply_paragraph_alignment_override_transition_v1(&mut overrides, before, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn paragraph(raw: u128) -> ParagraphId {
        ParagraphId::from_canonical(Uuid::from_u128(raw))
    }

    #[test]
    fn override_equal_to_known_base_normalizes_to_inherit() {
        assert_eq!(
            normalized_paragraph_alignment_override_v1(
                Some(ImportedParagraphAlignmentValueV1::Center),
                AuthoredParagraphAlignmentValueV1::Center,
            ),
            None
        );
        assert_eq!(
            normalized_paragraph_alignment_override_v1(
                Some(ImportedParagraphAlignmentValueV1::Right),
                AuthoredParagraphAlignmentValueV1::Right,
            ),
            None
        );
        assert_eq!(
            normalized_paragraph_alignment_override_v1(
                Some(ImportedParagraphAlignmentValueV1::InterWord),
                AuthoredParagraphAlignmentValueV1::Center,
            ),
            Some(AuthoredParagraphAlignmentValueV1::Center)
        );
    }

    #[test]
    fn transition_is_exact_atomic_and_reversible() {
        let first = paragraph(1);
        let second = paragraph(2);
        let mut state = BTreeMap::from([(
            first,
            AuthoredParagraphAlignmentValueV1::Left,
        )]);
        let before = paragraph_alignment_override_snapshots_v1(&state, &[first, second]);
        let after = vec![
            ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: Some(AuthoredParagraphAlignmentValueV1::Right),
            },
            ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: second,
                value: Some(AuthoredParagraphAlignmentValueV1::Right),
            },
        ];

        apply_paragraph_alignment_override_transition_v1(&mut state, &before, &after)
            .expect("forward");
        assert_eq!(
            state,
            BTreeMap::from([
                (first, AuthoredParagraphAlignmentValueV1::Right),
                (second, AuthoredParagraphAlignmentValueV1::Right),
            ])
        );

        apply_paragraph_alignment_override_transition_v1(&mut state, &after, &before)
            .expect("inverse");
        assert_eq!(
            state,
            BTreeMap::from([(first, AuthoredParagraphAlignmentValueV1::Left)])
        );
    }

    #[test]
    fn stale_transition_fails_without_partial_mutation() {
        let first = paragraph(1);
        let second = paragraph(2);
        let mut state = BTreeMap::from([(
            first,
            AuthoredParagraphAlignmentValueV1::Center,
        )]);
        let before = vec![
            ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: Some(AuthoredParagraphAlignmentValueV1::Left),
            },
            ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: second,
                value: None,
            },
        ];
        let after = vec![
            ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: Some(AuthoredParagraphAlignmentValueV1::Right),
            },
            ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: second,
                value: Some(AuthoredParagraphAlignmentValueV1::Right),
            },
        ];
        let original = state.clone();

        assert!(matches!(
            apply_paragraph_alignment_override_transition_v1(&mut state, &before, &after),
            Err(ParagraphAlignmentTransitionErrorV1::Stale { paragraph_id, .. })
                if paragraph_id == first
        ));
        assert_eq!(state, original);
    }

    #[test]
    fn history_replay_is_the_single_override_state_authority() {
        let first = paragraph(1);
        let second = paragraph(2);
        let set = EditOperation::SetParagraphAlignmentOverride {
            paragraph_ids: vec![first, second],
            value: AuthoredParagraphAlignmentValueV1::Right,
            before: vec![
                ParagraphAlignmentOverrideSnapshotV1 {
                    paragraph_id: first,
                    value: None,
                },
                ParagraphAlignmentOverrideSnapshotV1 {
                    paragraph_id: second,
                    value: None,
                },
            ],
            after: vec![
                ParagraphAlignmentOverrideSnapshotV1 {
                    paragraph_id: first,
                    value: Some(AuthoredParagraphAlignmentValueV1::Right),
                },
                ParagraphAlignmentOverrideSnapshotV1 {
                    paragraph_id: second,
                    value: Some(AuthoredParagraphAlignmentValueV1::Right),
                },
            ],
        };
        let clear_first = EditOperation::ClearParagraphAlignmentOverride {
            paragraph_ids: vec![first],
            before: vec![ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: Some(AuthoredParagraphAlignmentValueV1::Right),
            }],
            after: vec![ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: None,
            }],
        };

        let state =
            paragraph_alignment_override_state_from_history_v1(&[set, clear_first]).expect("history replay");
        assert_eq!(
            state,
            BTreeMap::from([(second, AuthoredParagraphAlignmentValueV1::Right)])
        );
    }

    #[test]
    fn malformed_history_is_rejected_instead_of_becoming_effective_state() {
        let first = paragraph(1);
        let valid = EditOperation::SetParagraphAlignmentOverride {
            paragraph_ids: vec![first],
            value: AuthoredParagraphAlignmentValueV1::Center,
            before: vec![ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: None,
            }],
            after: vec![ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: Some(AuthoredParagraphAlignmentValueV1::Center),
            }],
        };
        let stale = EditOperation::SetParagraphAlignmentOverride {
            paragraph_ids: vec![first],
            value: AuthoredParagraphAlignmentValueV1::Right,
            before: vec![ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: None,
            }],
            after: vec![ParagraphAlignmentOverrideSnapshotV1 {
                paragraph_id: first,
                value: Some(AuthoredParagraphAlignmentValueV1::Right),
            }],
        };

        assert!(matches!(
            paragraph_alignment_override_state_from_history_v1(&[valid, stale]),
            Err(ParagraphAlignmentTransitionErrorV1::Stale {
                paragraph_id,
                expected: None,
                found: Some(AuthoredParagraphAlignmentValueV1::Center),
            }) if paragraph_id == first
        ));
    }
}
