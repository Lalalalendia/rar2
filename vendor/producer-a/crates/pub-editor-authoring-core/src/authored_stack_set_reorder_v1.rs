//! Canonical, set-atomic authored-lane z-order planning.
//!
//! The persisted AuthoredStackV1 (back -> front) is the sole lane authority.
//! This pure module takes already-admitted member IDs and never infers source
//! provenance, page parentage, selection identity, or user history from Scene.
//! EditorSession must validate actual AuthorCreated ownership before commit.

use crate::{
    AuthoredStackReorderModeV1, AuthoredStackV1, NodeId, PageId, authored_stack_state_id_v1,
    validate_authored_stack_v1,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

pub const AUTHORED_STACK_SET_REORDER_PROTOCOL_V1: &str = "chaptera.authored-stack-set-reorder.v1";

/// One indivisible canonical transition; selected IDs are in BEFORE-lane order,
/// regardless of the caller's enumeration of the selected set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredStackSetReorderTransitionV1 {
    pub protocol_version: String,
    pub page_id: PageId,
    pub selected_node_ids: Vec<NodeId>,
    pub mode: AuthoredStackReorderModeV1,
    pub before: AuthoredStackV1,
    pub after: AuthoredStackV1,
    pub before_state_id: String,
    pub after_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoredStackSetReorderErrorV1 {
    InvalidStack,
    PageMismatch,
    TooFewSelected,
    DuplicateSelected { node_id: NodeId },
    MissingMember { node_id: NodeId },
    NoChange,
    ProtocolMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
    TransitionMismatch,
}

impl fmt::Display for AuthoredStackSetReorderErrorV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStack => f.write_str("invalid authored stacking lane"),
            Self::PageMismatch => f.write_str("authored set reorder page mismatch"),
            Self::TooFewSelected => {
                f.write_str("authored set reorder requires two selected members")
            }
            Self::DuplicateSelected { node_id } => {
                write!(f, "authored set reorder repeats selected {node_id:?}")
            }
            Self::MissingMember { node_id } => {
                write!(f, "authored set reorder member is missing: {node_id:?}")
            }
            Self::NoChange => f.write_str("authored set reorder does not change the lane"),
            Self::ProtocolMismatch => f.write_str("authored set reorder protocol mismatch"),
            Self::BeforeStateMismatch => f.write_str("authored set reorder before state mismatch"),
            Self::AfterStateMismatch => f.write_str("authored set reorder after state mismatch"),
            Self::TransitionMismatch => f.write_str("authored set reorder is not canonical"),
        }
    }
}

impl std::error::Error for AuthoredStackSetReorderErrorV1 {}

/// Compute the stable-set reorder against ONE immutable before-lane.
/// The imported/source-backed lane is never present in this function.
pub fn plan_reorder_authored_stack_set_v1(
    before: &AuthoredStackV1,
    page_id: PageId,
    selected_node_ids: &[NodeId],
    mode: AuthoredStackReorderModeV1,
) -> Result<AuthoredStackSetReorderTransitionV1, AuthoredStackSetReorderErrorV1> {
    validate_authored_stack_v1(before).map_err(|_| AuthoredStackSetReorderErrorV1::InvalidStack)?;
    if page_id != before.page_id {
        return Err(AuthoredStackSetReorderErrorV1::PageMismatch);
    }
    if selected_node_ids.len() < 2 {
        return Err(AuthoredStackSetReorderErrorV1::TooFewSelected);
    }

    let mut selected_set = BTreeSet::new();
    for &node_id in selected_node_ids {
        if !selected_set.insert(node_id) {
            return Err(AuthoredStackSetReorderErrorV1::DuplicateSelected { node_id });
        }
    }
    let present: BTreeSet<_> = before.members.iter().copied().collect();
    for &node_id in selected_node_ids {
        if !present.contains(&node_id) {
            return Err(AuthoredStackSetReorderErrorV1::MissingMember { node_id });
        }
    }
    let is_selected = |member: &NodeId| selected_set.contains(member);
    let selected_before_order = before
        .members
        .iter()
        .copied()
        .filter(|member| selected_set.contains(member))
        .collect::<Vec<_>>();
    let unselected_before_order = before
        .members
        .iter()
        .copied()
        .filter(|member| !is_selected(member))
        .collect::<Vec<_>>();

    let after_members = match mode {
        AuthoredStackReorderModeV1::ToFront => unselected_before_order
            .into_iter()
            .chain(selected_before_order.iter().copied())
            .collect(),
        AuthoredStackReorderModeV1::ToBack => selected_before_order
            .iter()
            .copied()
            .chain(unselected_before_order)
            .collect(),
        AuthoredStackReorderModeV1::StepForward => {
            // S..S U -> U S..S for every maximal selected block, simultaneously.
            let mut result = Vec::with_capacity(before.members.len());
            let mut i = 0;
            while i < before.members.len() {
                if !is_selected(&before.members[i]) {
                    result.push(before.members[i]);
                    i += 1;
                    continue;
                }
                let start = i;
                while i < before.members.len() && is_selected(&before.members[i]) {
                    i += 1;
                }
                if i < before.members.len() {
                    result.push(before.members[i]);
                    result.extend_from_slice(&before.members[start..i]);
                    i += 1;
                } else {
                    result.extend_from_slice(&before.members[start..i]);
                }
            }
            result
        }
        AuthoredStackReorderModeV1::StepBackward => {
            // U S..S -> S..S U for every maximal selected block, simultaneously.
            let mut result = Vec::with_capacity(before.members.len());
            let mut i = 0;
            while i < before.members.len() {
                if !is_selected(&before.members[i])
                    && i + 1 < before.members.len()
                    && is_selected(&before.members[i + 1])
                {
                    let start = i + 1;
                    let mut end = start;
                    while end < before.members.len() && is_selected(&before.members[end]) {
                        end += 1;
                    }
                    result.extend_from_slice(&before.members[start..end]);
                    result.push(before.members[i]);
                    i = end;
                } else {
                    result.push(before.members[i]);
                    i += 1;
                }
            }
            result
        }
    };

    if after_members == before.members {
        return Err(AuthoredStackSetReorderErrorV1::NoChange);
    }
    let after = AuthoredStackV1 {
        page_id,
        members: after_members,
    };
    Ok(AuthoredStackSetReorderTransitionV1 {
        protocol_version: AUTHORED_STACK_SET_REORDER_PROTOCOL_V1.to_owned(),
        page_id,
        selected_node_ids: selected_before_order,
        mode,
        before: before.clone(),
        after_state_id: authored_stack_state_id_v1(&after),
        before_state_id: authored_stack_state_id_v1(before),
        after,
    })
}

fn validate_transition_v1(
    transition: &AuthoredStackSetReorderTransitionV1,
) -> Result<(), AuthoredStackSetReorderErrorV1> {
    if transition.protocol_version != AUTHORED_STACK_SET_REORDER_PROTOCOL_V1 {
        return Err(AuthoredStackSetReorderErrorV1::ProtocolMismatch);
    }
    if transition.page_id != transition.before.page_id
        || transition.page_id != transition.after.page_id
    {
        return Err(AuthoredStackSetReorderErrorV1::PageMismatch);
    }
    if transition.before_state_id != authored_stack_state_id_v1(&transition.before) {
        return Err(AuthoredStackSetReorderErrorV1::BeforeStateMismatch);
    }
    if transition.after_state_id != authored_stack_state_id_v1(&transition.after) {
        return Err(AuthoredStackSetReorderErrorV1::AfterStateMismatch);
    }
    // Recompute both ordering and normalized membership. A forged delta cannot
    // become authoritative just because it carries matching SHA values.
    let canonical = plan_reorder_authored_stack_set_v1(
        &transition.before,
        transition.page_id,
        &transition.selected_node_ids,
        transition.mode,
    )
    .map_err(|_| AuthoredStackSetReorderErrorV1::TransitionMismatch)?;
    if &canonical != transition {
        return Err(AuthoredStackSetReorderErrorV1::TransitionMismatch);
    }
    Ok(())
}

pub fn apply_authored_stack_set_reorder_forward_v1(
    current: &AuthoredStackV1,
    transition: &AuthoredStackSetReorderTransitionV1,
) -> Result<AuthoredStackV1, AuthoredStackSetReorderErrorV1> {
    validate_transition_v1(transition)?;
    if current != &transition.before
        || authored_stack_state_id_v1(current) != transition.before_state_id
    {
        return Err(AuthoredStackSetReorderErrorV1::BeforeStateMismatch);
    }
    Ok(transition.after.clone())
}

pub fn apply_authored_stack_set_reorder_inverse_v1(
    current: &AuthoredStackV1,
    transition: &AuthoredStackSetReorderTransitionV1,
) -> Result<AuthoredStackV1, AuthoredStackSetReorderErrorV1> {
    validate_transition_v1(transition)?;
    if current != &transition.after
        || authored_stack_state_id_v1(current) != transition.after_state_id
    {
        return Err(AuthoredStackSetReorderErrorV1::AfterStateMismatch);
    }
    Ok(transition.before.clone())
}
