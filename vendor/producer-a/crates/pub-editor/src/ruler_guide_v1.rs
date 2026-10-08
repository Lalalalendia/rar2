use super::{
    EDITOR_PROJECT_VERSION_V0_24, EditOperation, EditorError, EditorSession, LengthEmu, PageId,
};
use pub_export::{PersistenceRequirement, SemanticFeatureRequest};
use pub_model::{RulerGuide, RulerGuideAxis};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const RULER_GUIDE_HISTORY_V1: &str = "chaptera.ruler-guide-history.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredRulerGuideV1 {
    pub guide_id: String,
    pub page_id: PageId,
    pub guide: RulerGuide<LengthEmu>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RulerGuideTransitionV1 {
    Add {
        after: AuthoredRulerGuideV1,
    },
    Move {
        before: AuthoredRulerGuideV1,
        after: AuthoredRulerGuideV1,
    },
    Delete {
        before: AuthoredRulerGuideV1,
    },
}

impl RulerGuideTransitionV1 {
    pub fn page_id(&self) -> PageId {
        match self {
            Self::Add { after } | Self::Move { after, .. } => after.page_id,
            Self::Delete { before } => before.page_id,
        }
    }

    pub fn guide_id(&self) -> &str {
        match self {
            Self::Add { after } | Self::Move { after, .. } => &after.guide_id,
            Self::Delete { before } => &before.guide_id,
        }
    }
}

fn unsupported(message: impl Into<String>) -> EditorError {
    EditorError::RulerGuideUnsupported {
        message: message.into(),
    }
}

fn validate_guide_id(guide_id: &str) -> Result<(), EditorError> {
    let parsed = Uuid::parse_str(guide_id)
        .map_err(|_| unsupported("guide identity is not a canonical UUID"))?;
    if parsed.to_string() != guide_id || parsed.get_version_num() != 7 {
        return Err(unsupported(
            "guide identity must be a lowercase canonical UUIDv7",
        ));
    }
    Ok(())
}

fn validate_transition_shape(transition: &RulerGuideTransitionV1) -> Result<(), EditorError> {
    validate_guide_id(transition.guide_id())?;
    if let RulerGuideTransitionV1::Move { before, after } = transition {
        if before.guide_id != after.guide_id
            || before.page_id != after.page_id
            || before.guide.axis != after.guide.axis
        {
            return Err(unsupported(
                "Move must preserve guide identity, PageId ownership, and axis",
            ));
        }
        if before.guide.position == after.guide.position {
            return Err(EditorError::RulerGuideNoChange {
                guide_id: before.guide_id.clone(),
            });
        }
    }
    Ok(())
}

fn apply_transition_forward(
    state: &mut BTreeMap<String, AuthoredRulerGuideV1>,
    transition: &RulerGuideTransitionV1,
) -> Result<(), EditorError> {
    validate_transition_shape(transition)?;
    match transition {
        RulerGuideTransitionV1::Add { after } => {
            if state.contains_key(&after.guide_id) {
                return Err(EditorError::StaleRulerGuide {
                    guide_id: after.guide_id.clone(),
                });
            }
            state.insert(after.guide_id.clone(), after.clone());
        }
        RulerGuideTransitionV1::Move { before, after } => {
            if state.get(&before.guide_id) != Some(before) {
                return Err(EditorError::StaleRulerGuide {
                    guide_id: before.guide_id.clone(),
                });
            }
            state.insert(after.guide_id.clone(), after.clone());
        }
        RulerGuideTransitionV1::Delete { before } => {
            if state.get(&before.guide_id) != Some(before) {
                return Err(EditorError::StaleRulerGuide {
                    guide_id: before.guide_id.clone(),
                });
            }
            state.remove(&before.guide_id);
        }
    }
    Ok(())
}

fn apply_transition_inverse(
    state: &mut BTreeMap<String, AuthoredRulerGuideV1>,
    transition: &RulerGuideTransitionV1,
) -> Result<(), EditorError> {
    validate_transition_shape(transition)?;
    match transition {
        RulerGuideTransitionV1::Add { after } => {
            if state.get(&after.guide_id) != Some(after) {
                return Err(EditorError::StaleRulerGuide {
                    guide_id: after.guide_id.clone(),
                });
            }
            state.remove(&after.guide_id);
        }
        RulerGuideTransitionV1::Move { before, after } => {
            if state.get(&after.guide_id) != Some(after) {
                return Err(EditorError::StaleRulerGuide {
                    guide_id: after.guide_id.clone(),
                });
            }
            state.insert(before.guide_id.clone(), before.clone());
        }
        RulerGuideTransitionV1::Delete { before } => {
            if state.contains_key(&before.guide_id) {
                return Err(EditorError::StaleRulerGuide {
                    guide_id: before.guide_id.clone(),
                });
            }
            state.insert(before.guide_id.clone(), before.clone());
        }
    }
    Ok(())
}

fn derive_authored_ruler_guides_v1(
    operations: &[EditOperation],
) -> Result<BTreeMap<String, AuthoredRulerGuideV1>, EditorError> {
    let mut state = BTreeMap::new();
    for operation in operations {
        if let EditOperation::RulerGuideV1 { transition } = operation {
            apply_transition_forward(&mut state, transition)?;
        }
    }
    Ok(state)
}

pub(super) fn legacy_operation_index_v1(
    schema_version: &str,
    operations: &[EditOperation],
) -> Option<usize> {
    if schema_version == EDITOR_PROJECT_VERSION_V0_24 {
        return None;
    }
    operations
        .iter()
        .position(|operation| matches!(operation, EditOperation::RulerGuideV1 { .. }))
}

pub(super) fn persistence_requirements_v1(
    transition: &RulerGuideTransitionV1,
) -> Vec<PersistenceRequirement> {
    vec![PersistenceRequirement {
        feature: "page.ruler_guide".into(),
        origin: Some(transition.page_id().into_canonical()),
        property_path: Some("page.ruler_guides".into()),
    }]
}

pub(super) fn carries_history_v1(operations: &[EditOperation]) -> bool {
    operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::RulerGuideV1 { .. }))
}

pub(super) fn editable_export_requests_v1(
    operations: &[EditOperation],
) -> Result<Vec<SemanticFeatureRequest>, EditorError> {
    Ok(derive_authored_ruler_guides_v1(operations)?
        .into_values()
        .map(|authored| SemanticFeatureRequest {
            feature: "page.ruler_guide".into(),
            origin: Some(authored.page_id.into_canonical()),
            property_path: Some("page.ruler_guides".into()),
            require_preserved: false,
        })
        .collect())
}

pub(super) fn validate_ruler_guide_undo_v1(
    history_without_operation: &[EditOperation],
    transition: &RulerGuideTransitionV1,
) -> Result<(), EditorError> {
    let expected_before = derive_authored_ruler_guides_v1(history_without_operation)?;
    let mut current = expected_before.clone();
    apply_transition_forward(&mut current, transition)?;
    apply_transition_inverse(&mut current, transition)?;
    if current != expected_before {
        return Err(unsupported(
            "ruler-guide inverse did not restore exact prior history state",
        ));
    }
    Ok(())
}

pub(super) fn validate_ruler_guide_redo_v1(
    history_before_operation: &[EditOperation],
    transition: &RulerGuideTransitionV1,
) -> Result<(), EditorError> {
    let mut current = derive_authored_ruler_guides_v1(history_before_operation)?;
    apply_transition_forward(&mut current, transition)
}

impl EditorSession {
    fn validate_ruler_guide_transition_v1(
        &self,
        transition: &RulerGuideTransitionV1,
    ) -> Result<(), EditorError> {
        validate_transition_shape(transition)?;
        let page_id = transition.page_id();
        if !self.graph.pages.contains_key(&page_id) {
            return Err(unsupported(format!(
                "page {} is not present in the current document",
                page_id.as_canonical()
            )));
        }
        Ok(())
    }

    pub fn current_authored_ruler_guides_v1(
        &self,
    ) -> Result<Vec<AuthoredRulerGuideV1>, EditorError> {
        self.validate_source_identity()?;
        Ok(derive_authored_ruler_guides_v1(&self.undo)?
            .into_values()
            .collect())
    }

    pub fn add_ruler_guide_v1(
        &mut self,
        page_id: PageId,
        axis: RulerGuideAxis,
        position: LengthEmu,
    ) -> Result<EditOperation, EditorError> {
        let transition = RulerGuideTransitionV1::Add {
            after: AuthoredRulerGuideV1 {
                guide_id: Uuid::now_v7().to_string(),
                page_id,
                guide: RulerGuide { axis, position },
            },
        };
        self.consume_canonical_ruler_guide_v1(transition)
    }

    pub fn move_ruler_guide_v1(
        &mut self,
        guide_id: &str,
        position: LengthEmu,
    ) -> Result<EditOperation, EditorError> {
        let current = derive_authored_ruler_guides_v1(&self.undo)?
            .remove(guide_id)
            .ok_or_else(|| EditorError::RulerGuideMissing {
                guide_id: guide_id.to_owned(),
            })?;
        if current.guide.position == position {
            return Err(EditorError::RulerGuideNoChange {
                guide_id: guide_id.to_owned(),
            });
        }
        let mut after = current.clone();
        after.guide.position = position;
        self.consume_canonical_ruler_guide_v1(RulerGuideTransitionV1::Move {
            before: current,
            after,
        })
    }

    pub fn delete_ruler_guide_v1(&mut self, guide_id: &str) -> Result<EditOperation, EditorError> {
        let current = derive_authored_ruler_guides_v1(&self.undo)?
            .remove(guide_id)
            .ok_or_else(|| EditorError::RulerGuideMissing {
                guide_id: guide_id.to_owned(),
            })?;
        self.consume_canonical_ruler_guide_v1(RulerGuideTransitionV1::Delete { before: current })
    }

    pub(super) fn consume_canonical_ruler_guide_v1(
        &mut self,
        transition: RulerGuideTransitionV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        self.validate_ruler_guide_transition_v1(&transition)?;
        let mut current = derive_authored_ruler_guides_v1(&self.undo)?;
        apply_transition_forward(&mut current, &transition)?;
        let operation = EditOperation::RulerGuideV1 { transition };
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }
}
