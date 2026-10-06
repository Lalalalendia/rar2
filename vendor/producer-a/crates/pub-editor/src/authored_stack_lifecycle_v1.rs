//! Canonical lifecycle law for the Chaptera-authored page-object lane.
//!
//! This module is intentionally a pure transition planner. It does not install
//! AuthoredStackV1 into EditorSession and does not change EditorProject schema.
//! A runtime owner can atomically commit an entity mutation together with the
//! returned exact lane transition without consulting Scene or Page.children.

use crate::{
    AuthoredLineRuntimeV1, AuthoredShapeRuntimeV1, NodeId, PageId,
    validate_authored_line_runtime_v1, validate_authored_shape_runtime_v1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;

pub const AUTHORED_STACK_PROTOCOL_V1: &str = "chaptera.authored-stack.v1";

/// Canonical per-page authored overlay order, stored back-to-front.
///
/// The final member is the authored front/top object. Imported/source-backed
/// Publisher objects never belong to this lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredStackV1 {
    pub page_id: PageId,
    pub members: Vec<NodeId>,
}

impl AuthoredStackV1 {
    pub fn empty(page_id: PageId) -> Self {
        Self {
            page_id,
            members: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoredStackLifecycleKindV1 {
    AppendCreated,
    RemoveDeleted,
}

/// Exact durable evidence for one object-lifecycle lane transition.
///
/// member_index is the position occupied by the member in the state that
/// contains it: after for AppendCreated, before for RemoveDeleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredStackLifecycleTransitionV1 {
    pub kind: AuthoredStackLifecycleKindV1,
    pub page_id: PageId,
    pub node_id: NodeId,
    pub member_index: usize,
    pub before: AuthoredStackV1,
    pub after: AuthoredStackV1,
    pub before_state_id: String,
    pub after_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoredStackLifecycleErrorV1 {
    InvalidAuthoredShape,
    InvalidAuthoredLine,
    StackPageMismatch {
        stack_page_id: PageId,
        shape_page_id: PageId,
    },
    DuplicateMembership {
        node_id: NodeId,
    },
    MissingMembership {
        node_id: NodeId,
    },
    DuplicateLaneMember {
        node_id: NodeId,
    },
    TransitionPageMismatch,
    TransitionShapeMismatch,
    MemberIndexMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
}

impl fmt::Display for AuthoredStackLifecycleErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAuthoredShape => formatter
                .write_str("authored-stack lifecycle requires a canonical AuthorCreated shape"),
            Self::InvalidAuthoredLine => formatter
                .write_str("authored-stack lifecycle requires a canonical AuthorCreated line"),
            Self::StackPageMismatch {
                stack_page_id,
                shape_page_id,
            } => write!(
                formatter,
                "authored stack page {stack_page_id:?} does not match authored shape page {shape_page_id:?}"
            ),
            Self::DuplicateMembership { node_id } => {
                write!(formatter, "authored stack already contains {node_id:?}")
            }
            Self::MissingMembership { node_id } => {
                write!(formatter, "authored stack does not contain {node_id:?}")
            }
            Self::DuplicateLaneMember { node_id } => {
                write!(
                    formatter,
                    "authored stack contains duplicate member {node_id:?}"
                )
            }
            Self::TransitionPageMismatch => {
                formatter.write_str("authored-stack transition page identity is inconsistent")
            }
            Self::TransitionShapeMismatch => formatter.write_str(
                "authored-stack transition is not the canonical one-member lifecycle delta",
            ),
            Self::MemberIndexMismatch => {
                formatter.write_str("authored-stack transition member index is inconsistent")
            }
            Self::BeforeStateMismatch => {
                formatter.write_str("authored-stack transition before state is stale")
            }
            Self::AfterStateMismatch => {
                formatter.write_str("authored-stack transition after state is stale")
            }
        }
    }
}

impl std::error::Error for AuthoredStackLifecycleErrorV1 {}

pub fn authored_stack_state_id_v1(stack: &AuthoredStackV1) -> String {
    let payload = serde_json::json!({
        "protocol_version": AUTHORED_STACK_PROTOCOL_V1,
        "page_id": stack.page_id,
        "members": stack.members,
    });
    let bytes =
        serde_json::to_vec(&payload).expect("canonical AuthoredStackV1 serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing lowercase hex into String cannot fail");
    }
    format!("sha256:{encoded}")
}

pub fn validate_authored_stack_v1(
    stack: &AuthoredStackV1,
) -> Result<(), AuthoredStackLifecycleErrorV1> {
    let mut seen = BTreeSet::new();
    for node_id in &stack.members {
        if !seen.insert(*node_id) {
            return Err(AuthoredStackLifecycleErrorV1::DuplicateLaneMember { node_id: *node_id });
        }
    }
    Ok(())
}

fn validate_shape_for_stack(
    stack: &AuthoredStackV1,
    shape: &AuthoredShapeRuntimeV1,
) -> Result<(), AuthoredStackLifecycleErrorV1> {
    validate_authored_stack_v1(stack)?;
    validate_authored_shape_runtime_v1(shape)
        .map_err(|_| AuthoredStackLifecycleErrorV1::InvalidAuthoredShape)?;
    if stack.page_id != shape.page_id {
        return Err(AuthoredStackLifecycleErrorV1::StackPageMismatch {
            stack_page_id: stack.page_id,
            shape_page_id: shape.page_id,
        });
    }
    Ok(())
}

fn plan_created_member_append_v1(
    stack: &AuthoredStackV1,
    node_id: NodeId,
    page_id: PageId,
) -> Result<AuthoredStackLifecycleTransitionV1, AuthoredStackLifecycleErrorV1> {
    validate_authored_stack_v1(stack)?;
    if stack.page_id != page_id {
        return Err(AuthoredStackLifecycleErrorV1::StackPageMismatch {
            stack_page_id: stack.page_id,
            shape_page_id: page_id,
        });
    }
    if stack.members.contains(&node_id) {
        return Err(AuthoredStackLifecycleErrorV1::DuplicateMembership { node_id });
    }

    let before = stack.clone();
    let member_index = before.members.len();
    let mut after = before.clone();
    after.members.push(node_id);

    Ok(AuthoredStackLifecycleTransitionV1 {
        kind: AuthoredStackLifecycleKindV1::AppendCreated,
        page_id,
        node_id,
        member_index,
        before_state_id: authored_stack_state_id_v1(&before),
        after_state_id: authored_stack_state_id_v1(&after),
        before,
        after,
    })
}

pub fn plan_create_shape_append_v1(
    stack: &AuthoredStackV1,
    shape: &AuthoredShapeRuntimeV1,
) -> Result<AuthoredStackLifecycleTransitionV1, AuthoredStackLifecycleErrorV1> {
    validate_shape_for_stack(stack, shape)?;
    plan_created_member_append_v1(stack, shape.node_id, shape.page_id)
}

pub fn plan_create_line_append_v1(
    stack: &AuthoredStackV1,
    line: &AuthoredLineRuntimeV1,
) -> Result<AuthoredStackLifecycleTransitionV1, AuthoredStackLifecycleErrorV1> {
    validate_authored_stack_v1(stack)?;
    validate_authored_line_runtime_v1(line)
        .map_err(|_| AuthoredStackLifecycleErrorV1::InvalidAuthoredLine)?;
    plan_created_member_append_v1(stack, line.node_id, line.page_id)
}

pub fn plan_create_table_append_v1(
    stack: &AuthoredStackV1,
    node_id: NodeId,
    page_id: PageId,
) -> Result<AuthoredStackLifecycleTransitionV1, AuthoredStackLifecycleErrorV1> {
    // CreateTable owns TABLE payload/topology validation. AuthoredStack owns
    // only exact page-local membership and ordering, so tables reuse the same
    // canonical append transition without pretending to be shapes or lines.
    plan_created_member_append_v1(stack, node_id, page_id)
}

pub fn plan_delete_shape_remove_v1(
    stack: &AuthoredStackV1,
    shape: &AuthoredShapeRuntimeV1,
) -> Result<AuthoredStackLifecycleTransitionV1, AuthoredStackLifecycleErrorV1> {
    validate_shape_for_stack(stack, shape)?;
    let Some(member_index) = stack
        .members
        .iter()
        .position(|node_id| *node_id == shape.node_id)
    else {
        return Err(AuthoredStackLifecycleErrorV1::MissingMembership {
            node_id: shape.node_id,
        });
    };

    let before = stack.clone();
    let mut after = before.clone();
    after.members.remove(member_index);

    Ok(AuthoredStackLifecycleTransitionV1 {
        kind: AuthoredStackLifecycleKindV1::RemoveDeleted,
        page_id: shape.page_id,
        node_id: shape.node_id,
        member_index,
        before_state_id: authored_stack_state_id_v1(&before),
        after_state_id: authored_stack_state_id_v1(&after),
        before,
        after,
    })
}

fn validate_transition_v1(
    transition: &AuthoredStackLifecycleTransitionV1,
) -> Result<(), AuthoredStackLifecycleErrorV1> {
    validate_authored_stack_v1(&transition.before)?;
    validate_authored_stack_v1(&transition.after)?;
    if transition.before.page_id != transition.page_id
        || transition.after.page_id != transition.page_id
    {
        return Err(AuthoredStackLifecycleErrorV1::TransitionPageMismatch);
    }
    if authored_stack_state_id_v1(&transition.before) != transition.before_state_id {
        return Err(AuthoredStackLifecycleErrorV1::BeforeStateMismatch);
    }
    if authored_stack_state_id_v1(&transition.after) != transition.after_state_id {
        return Err(AuthoredStackLifecycleErrorV1::AfterStateMismatch);
    }

    match transition.kind {
        AuthoredStackLifecycleKindV1::AppendCreated => {
            if transition.member_index != transition.before.members.len()
                || transition.after.members.len() != transition.before.members.len() + 1
                || transition.after.members.get(transition.member_index)
                    != Some(&transition.node_id)
                || transition.after.members[..transition.member_index]
                    != transition.before.members[..]
            {
                return Err(AuthoredStackLifecycleErrorV1::TransitionShapeMismatch);
            }
        }
        AuthoredStackLifecycleKindV1::RemoveDeleted => {
            if transition.before.members.get(transition.member_index) != Some(&transition.node_id)
                || transition.before.members.len() != transition.after.members.len() + 1
            {
                return Err(AuthoredStackLifecycleErrorV1::MemberIndexMismatch);
            }
            let mut expected = transition.before.members.clone();
            expected.remove(transition.member_index);
            if expected != transition.after.members {
                return Err(AuthoredStackLifecycleErrorV1::TransitionShapeMismatch);
            }
        }
    }
    Ok(())
}

pub fn apply_authored_stack_transition_forward_v1(
    current: &AuthoredStackV1,
    transition: &AuthoredStackLifecycleTransitionV1,
) -> Result<AuthoredStackV1, AuthoredStackLifecycleErrorV1> {
    validate_transition_v1(transition)?;
    validate_authored_stack_v1(current)?;
    if current != &transition.before
        || authored_stack_state_id_v1(current) != transition.before_state_id
    {
        return Err(AuthoredStackLifecycleErrorV1::BeforeStateMismatch);
    }
    Ok(transition.after.clone())
}

pub fn apply_authored_stack_transition_inverse_v1(
    current: &AuthoredStackV1,
    transition: &AuthoredStackLifecycleTransitionV1,
) -> Result<AuthoredStackV1, AuthoredStackLifecycleErrorV1> {
    validate_transition_v1(transition)?;
    validate_authored_stack_v1(current)?;
    if current != &transition.after
        || authored_stack_state_id_v1(current) != transition.after_state_id
    {
        return Err(AuthoredStackLifecycleErrorV1::AfterStateMismatch);
    }
    Ok(transition.before.clone())
}
