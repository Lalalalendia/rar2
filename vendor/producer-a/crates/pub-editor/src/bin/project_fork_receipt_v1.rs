use std::collections::BTreeMap;

use pub_editor::{EditorEditableTarget, EditorSession, LengthEmu, NodeId, RectEmu, Sha256Digest};
use pub_model::{
    Affine2D, Document, DocumentId, Node, NodeHeader, NodeKind, Page, PageId, ResolvedGraph,
    Size2D, SourceDescriptor,
};
use pub_reader::{PubExplicitShapePaintSource, PubResolvedGraph, PubResolvedNodePayload};
use serde_json::{json, Value};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn graph() -> PubResolvedGraph {
    let source_hash = Sha256Digest::from_bytes([0x44; 32]);
    let page_id = canonical_id::<PageId>("11000000-0000-4000-8000-000000000001");
    let node_id = canonical_id::<NodeId>("22000000-0000-4000-8000-000000000001");

    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "project-fork-receipt-v1".into(),
        source: SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "pub-rs/receipt".into(),
            source_hash,
        },
        document: Document {
            id: canonical_id::<DocumentId>("33000000-0000-4000-8000-000000000001"),
            format_origin: "pub".into(),
            source_hash,
            pages: vec![page_id],
            resources: Vec::new(),
            styles: Vec::new(),
        },
        pages: BTreeMap::from([(
            page_id,
            Page {
                id: page_id,
                size: Size2D::new(LengthEmu::new(5_000_000), LengthEmu::new(5_000_000)),
                bleed: None,
                margins: None,
                children: vec![node_id],
                extensions: Vec::new(),
            },
        )]),
        nodes: BTreeMap::from([(
            node_id,
            Node {
                kind: NodeKind::Shape,
                header: NodeHeader {
                    id: node_id,
                    parent_id: page_id.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(100_000),
                        LengthEmu::new(200_000),
                        LengthEmu::new(300_000),
                        LengthEmu::new(400_000),
                    ),
                    transform: Affine2D::identity(),
                    source_refs: Vec::new(),
                    extensions: Vec::new(),
                },
                payload: PubResolvedNodePayload {
                    contents_seq_num: 1,
                    officeart_shape_type: Some(1),
                    officeart_spid: Some(1),
                    image_slot: None,
                    legacy_ole: None,
                    explicit_image_crop: None,
                    explicit_image_cardinal_rotation_degrees: None,
                    explicit_paint: PubExplicitShapePaintSource::default(),
                    effective_paint: None,
                    story_frame: None,
                    text_frame_inset: None,
                    table_story: None,
                    table: None,
                },
            },
        )]),
        stories: BTreeMap::new(),
        paragraphs: BTreeMap::new(),
        text_runs: BTreeMap::new(),
        resources: BTreeMap::new(),
        styles: BTreeMap::new(),
        extensions: BTreeMap::new(),
    }
}

fn string_value<T: serde::Serialize>(value: &T) -> String {
    let v = serde_json::to_value(value).expect("serialize value");
    match v {
        Value::String(s) => s,
        other => other.to_string(),
    }
}

fn main() {
    let base = graph();
    let node_id = canonical_id::<NodeId>("22000000-0000-4000-8000-000000000001");

    let parent_session = EditorSession::new(base.clone()).expect("parent session");
    let parent = parent_session.project();
    let parent_state = parent.state_id_v1();
    let parent_identity = parent.identity.as_ref().expect("parent identity").clone();
    let parent_bytes = serde_json::to_vec(&parent).expect("serialize parent");

    let fork = parent_session
        .fork_project_next_issue()
        .expect("fork current project");
    let fork_identity = fork.identity.as_ref().expect("fork identity").clone();
    let fork_initial_state = fork.state_id_v1();

    let mut next_issue = EditorSession::new(base.clone()).expect("next issue session");
    next_issue.apply_project(&fork).expect("reopen fork");
    next_issue
        .move_node_to(node_id, LengthEmu::new(700_000), LengthEmu::new(800_000))
        .expect("edit only next issue");
    let edited_fork = next_issue.project();
    let edited_fork_state = edited_fork.state_id_v1();

    let mut reopened_parent = EditorSession::new(base.clone()).expect("reopen parent");
    reopened_parent.apply_project(&parent).expect("apply parent");
    let reopened_parent_bytes =
        serde_json::to_vec(&reopened_parent.project()).expect("serialize reopened parent");

    let mut reopened_fork = EditorSession::new(base).expect("reopen fork");
    reopened_fork
        .apply_project(&edited_fork)
        .expect("apply edited fork");

    let parent_export = reopened_parent
        .export_editable(EditorEditableTarget::Odg, "project-fork-parent")
        .expect("parent export");
    let fork_export = reopened_fork
        .export_editable(EditorEditableTarget::Odg, "project-fork-next")
        .expect("fork export");

    let provenance = fork_identity
        .forked_from
        .as_ref()
        .expect("fork provenance");

    let receipt = json!({
        "receipt_version": "chaptera.project-fork-receipt.v1",
        "source_hash": string_value(&parent.source_hash),
        "parent": {
            "project_id": parent_identity.project_id,
            "document_id": parent_identity.document_id,
            "history_id": parent_identity.history_id,
            "genesis_revision_id": parent_identity.genesis_revision_id,
            "state_id": string_value(&parent_state)
        },
        "fork_initial": {
            "project_id": fork_identity.project_id,
            "document_id": fork_identity.document_id,
            "history_id": fork_identity.history_id,
            "genesis_revision_id": fork_identity.genesis_revision_id,
            "state_id": string_value(&fork_initial_state),
            "forked_from": {
                "project_id": provenance.project_id,
                "document_id": provenance.document_id,
                "history_id": provenance.history_id,
                "state_id": string_value(&provenance.state_id)
            }
        },
        "fork_after_edit": {
            "state_id": string_value(&edited_fork_state),
            "operation_count": edited_fork.operations.len()
        },
        "reopen": {
            "parent_exact": reopened_parent_bytes == parent_bytes,
            "fork_exact": reopened_fork.project() == edited_fork
        },
        "editable_output": {
            "parent_nonempty": !parent_export.bytes.is_empty(),
            "fork_nonempty": !fork_export.bytes.is_empty()
        },
        "invariants": {
            "initial_state_preserved": fork_initial_state == parent_state,
            "project_id_rekeyed": fork_identity.project_id != parent_identity.project_id,
            "document_id_rekeyed": fork_identity.document_id != parent_identity.document_id,
            "history_id_rekeyed": fork_identity.history_id != parent_identity.history_id,
            "genesis_revision_id_rekeyed": fork_identity.genesis_revision_id != parent_identity.genesis_revision_id,
            "provenance_exact": provenance.project_id == parent_identity.project_id
                && provenance.document_id == parent_identity.document_id
                && provenance.history_id == parent_identity.history_id
                && provenance.state_id == parent_state,
            "fork_edit_diverged": edited_fork_state != parent_state,
            "parent_unchanged_after_fork_edit": reopened_parent_bytes == parent_bytes,
            "source_hash_equal_after_reopen": reopened_parent.source_hash() == reopened_fork.source_hash(),
            "source_write_count": 0,
            "raw_document_content_emitted": false
        }
    });

    println!("{}", serde_json::to_string_pretty(&receipt).expect("encode receipt"));
}
