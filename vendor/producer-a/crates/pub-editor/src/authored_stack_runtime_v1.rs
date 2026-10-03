//! Production AuthoredStack reorder contract for pub-editor.
//!
//! Create/Delete membership is owned by authored_stack_lifecycle_v1. This
//! module adds only explicit authored-lane reordering and exact replay
//! evidence. Imported/source-backed Publisher stacking is intentionally out of
//! scope.

use crate::{
    AuthoredStackV1, NodeId, PageId, authored_stack_state_id_v1, validate_authored_stack_v1,
};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoredStackReorderModeV1 {
    StepForward,
    StepBackward,
    ToFront,
    ToBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredStackReorderTransitionV1 {
    pub page_id: PageId,
    pub node_id: NodeId,
    pub mode: AuthoredStackReorderModeV1,
    pub before_index: usize,
    pub after_index: usize,
    pub before: AuthoredStackV1,
    pub after: AuthoredStackV1,
    pub before_state_id: String,
    pub after_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoredStackReorderErrorV1 {
    InvalidStack,
    PageMismatch,
    MissingMember { node_id: NodeId },
    NoChange { node_id: NodeId },
    BeforeStateMismatch,
    AfterStateMismatch,
    TransitionMismatch,
}

impl fmt::Display for AuthoredStackReorderErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStack => formatter.write_str("authored stack is invalid"),
            Self::PageMismatch => formatter.write_str("authored-stack reorder page mismatch"),
            Self::MissingMember { node_id } => {
                write!(formatter, "authored stack does not contain {node_id:?}")
            }
            Self::NoChange { node_id } => {
                write!(formatter, "authored-stack reorder for {node_id:?} is a no-op")
            }
            Self::BeforeStateMismatch => {
                formatter.write_str("authored-stack reorder before state is stale")
            }
            Self::AfterStateMismatch => {
                formatter.write_str("authored-stack reorder after state is stale")
            }
            Self::TransitionMismatch => {
                formatter.write_str("authored-stack reorder transition is non-canonical")
            }
        }
    }
}

impl std::error::Error for AuthoredStackReorderErrorV1 {}

pub fn plan_reorder_authored_stack_v1(
    stack: &AuthoredStackV1,
    node_id: NodeId,
    mode: AuthoredStackReorderModeV1,
) -> Result<AuthoredStackReorderTransitionV1, AuthoredStackReorderErrorV1> {
    validate_authored_stack_v1(stack).map_err(|_| AuthoredStackReorderErrorV1::InvalidStack)?;
    let Some(before_index) = stack.members.iter().position(|member| *member == node_id) else {
        return Err(AuthoredStackReorderErrorV1::MissingMember { node_id });
    };

    let after_index = match mode {
        AuthoredStackReorderModeV1::StepForward => before_index
            .checked_add(1)
            .filter(|index| *index < stack.members.len())
            .ok_or(AuthoredStackReorderErrorV1::NoChange { node_id })?,
        AuthoredStackReorderModeV1::StepBackward => before_index
            .checked_sub(1)
            .ok_or(AuthoredStackReorderErrorV1::NoChange { node_id })?,
        AuthoredStackReorderModeV1::ToFront => {
            let index = stack.members.len().saturating_sub(1);
            if index == before_index {
                return Err(AuthoredStackReorderErrorV1::NoChange { node_id });
            }
            index
        }
        AuthoredStackReorderModeV1::ToBack => {
            if before_index == 0 {
                return Err(AuthoredStackReorderErrorV1::NoChange { node_id });
            }
            0
        }
    };

    let before = stack.clone();
    let mut after = before.clone();
    let moved = after.members.remove(before_index);
    after.members.insert(after_index, moved);

    Ok(AuthoredStackReorderTransitionV1 {
        page_id: stack.page_id,
        node_id,
        mode,
        before_index,
        after_index,
        before_state_id: authored_stack_state_id_v1(&before),
        after_state_id: authored_stack_state_id_v1(&after),
        before,
        after,
    })
}

fn validate_transition_v1(
    transition: &AuthoredStackReorderTransitionV1,
) -> Result<(), AuthoredStackReorderErrorV1> {
    validate_authored_stack_v1(&transition.before)
        .map_err(|_| AuthoredStackReorderErrorV1::InvalidStack)?;
    validate_authored_stack_v1(&transition.after)
        .map_err(|_| AuthoredStackReorderErrorV1::InvalidStack)?;
    if transition.before.page_id != transition.page_id
        || transition.after.page_id != transition.page_id
    {
        return Err(AuthoredStackReorderErrorV1::PageMismatch);
    }
    if authored_stack_state_id_v1(&transition.before) != transition.before_state_id {
        return Err(AuthoredStackReorderErrorV1::BeforeStateMismatch);
    }
    if authored_stack_state_id_v1(&transition.after) != transition.after_state_id {
        return Err(AuthoredStackReorderErrorV1::AfterStateMismatch);
    }

    let canonical =
        plan_reorder_authored_stack_v1(&transition.before, transition.node_id, transition.mode)?;
    if canonical.before_index != transition.before_index
        || canonical.after_index != transition.after_index
        || canonical.after != transition.after
        || canonical.after_state_id != transition.after_state_id
    {
        return Err(AuthoredStackReorderErrorV1::TransitionMismatch);
    }
    Ok(())
}

pub fn apply_authored_stack_reorder_forward_v1(
    current: &AuthoredStackV1,
    transition: &AuthoredStackReorderTransitionV1,
) -> Result<AuthoredStackV1, AuthoredStackReorderErrorV1> {
    validate_transition_v1(transition)?;
    if current != &transition.before
        || authored_stack_state_id_v1(current) != transition.before_state_id
    {
        return Err(AuthoredStackReorderErrorV1::BeforeStateMismatch);
    }
    Ok(transition.after.clone())
}

pub fn apply_authored_stack_reorder_inverse_v1(
    current: &AuthoredStackV1,
    transition: &AuthoredStackReorderTransitionV1,
) -> Result<AuthoredStackV1, AuthoredStackReorderErrorV1> {
    validate_transition_v1(transition)?;
    if current != &transition.after
        || authored_stack_state_id_v1(current) != transition.after_state_id
    {
        return Err(AuthoredStackReorderErrorV1::AfterStateMismatch);
    }
    Ok(transition.before.clone())
}
