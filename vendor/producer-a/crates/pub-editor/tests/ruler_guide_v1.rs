use std::collections::BTreeMap;

use pub_editor::{
    EDITOR_PROJECT_VERSION_V0_24, EditOperation, EditorEditableTarget, EditorError,
    EditorProjectError, EditorSession, LengthEmu, RulerGuideTransitionV1,
};
use pub_model::{
    Document, DocumentId, Page, PageId, ResolvedGraph, RulerGuideAxis, Sha256Digest, Size2D,
    SourceDescriptor,
};
use pub_reader::PubResolvedGraph;

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn source_hash() -> Sha256Digest {
    "7777777777777777777777777777777777777777777777777777777777777777"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("10000000-0000-4000-8000-000000000001")
}

fn graph() -> PubResolvedGraph {
    let page_id = page_id();
    let source_hash = source_hash();
    let mut pages = BTreeMap::new();
    pages.insert(
        page_id,
        Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(5_000_000), LengthEmu::new(7_000_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        },
    );
    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "ruler-guide-test".into(),
        source: SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "pub-rs/test".into(),
            source_hash,
        },
        document: Document {
            id: canonical_id::<DocumentId>("30000000-0000-4000-8000-000000000001"),
            format_origin: "pub".into(),
            source_hash,
            pages: vec![page_id],
            resources: Vec::new(),
            styles: Vec::new(),
        },
        pages,
        nodes: BTreeMap::new(),
        stories: BTreeMap::new(),
        paragraphs: BTreeMap::new(),
        text_runs: BTreeMap::new(),
        resources: BTreeMap::new(),
        styles: BTreeMap::new(),
        extensions: BTreeMap::new(),
    }
}

#[test]
fn add_move_delete_are_exact_history_and_replay_units() {
    let base = graph();
    let mut editor = EditorSession::new(base.clone()).expect("session");

    let add = editor
        .add_ruler_guide_v1(page_id(), RulerGuideAxis::Vertical, LengthEmu::new(914_400))
        .expect("add guide");
    let guide_id = match &add {
        EditOperation::RulerGuideV1 {
            transition: RulerGuideTransitionV1::Add { after },
        } => after.guide_id.clone(),
        other => panic!("unexpected add operation: {other:?}"),
    };
    assert_eq!(editor.operations().len(), 1);
    assert_eq!(
        editor.current_authored_ruler_guides_v1().unwrap()[0]
            .guide
            .position,
        LengthEmu::new(914_400)
    );

    editor
        .move_ruler_guide_v1(&guide_id, LengthEmu::new(1_828_800))
        .expect("move guide");
    assert_eq!(editor.operations().len(), 2);
    assert_eq!(
        editor.current_authored_ruler_guides_v1().unwrap()[0]
            .guide
            .position,
        LengthEmu::new(1_828_800)
    );
    assert!(matches!(
        editor.move_ruler_guide_v1(&guide_id, LengthEmu::new(1_828_800)),
        Err(EditorError::RulerGuideNoChange { .. })
    ));

    editor.undo().expect("undo move");
    assert_eq!(
        editor.current_authored_ruler_guides_v1().unwrap()[0]
            .guide
            .position,
        LengthEmu::new(914_400)
    );
    editor.undo().expect("undo add");
    assert!(
        editor
            .current_authored_ruler_guides_v1()
            .unwrap()
            .is_empty()
    );
    editor.redo().expect("redo add");
    editor.redo().expect("redo move");

    editor
        .delete_ruler_guide_v1(&guide_id)
        .expect("delete guide");
    assert!(
        editor
            .current_authored_ruler_guides_v1()
            .unwrap()
            .is_empty()
    );

    let deleted_project = editor.project();
    assert_eq!(deleted_project.schema_version, EDITOR_PROJECT_VERSION_V0_24);
    assert_eq!(deleted_project.operations.len(), 3);
    let deleted_encoded = serde_json::to_vec(&deleted_project).expect("serialize deleted v0.24");
    let deleted_roundtrip =
        serde_json::from_slice(&deleted_encoded).expect("deserialize deleted v0.24");
    let mut deleted_reopened = EditorSession::new(base.clone()).expect("fresh deleted session");
    deleted_reopened
        .apply_project(&deleted_roundtrip)
        .expect("replay deleted guide history");
    assert!(
        deleted_reopened
            .current_authored_ruler_guides_v1()
            .unwrap()
            .is_empty()
    );
    assert_eq!(deleted_reopened.source_hash(), source_hash());

    editor.undo().expect("undo delete");
    assert_eq!(
        editor.current_authored_ruler_guides_v1().unwrap()[0]
            .guide
            .position,
        LengthEmu::new(1_828_800)
    );
    editor.redo().expect("redo delete");
    assert!(
        editor
            .current_authored_ruler_guides_v1()
            .unwrap()
            .is_empty()
    );
    editor.undo().expect("restore guide after redo proof");

    let project = editor.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_24);
    assert_eq!(project.operations.len(), 2);
    assert_eq!(
        editor.persistence_requirements()[0].feature,
        "page.ruler_guide"
    );

    let encoded = serde_json::to_vec(&project).expect("serialize v0.24");
    let roundtrip = serde_json::from_slice(&encoded).expect("deserialize v0.24");
    let mut reopened = EditorSession::new(base).expect("fresh session");
    reopened
        .apply_project(&roundtrip)
        .expect("replay guide history");
    assert_eq!(
        reopened.current_authored_ruler_guides_v1().unwrap(),
        editor.current_authored_ruler_guides_v1().unwrap()
    );
    assert_eq!(reopened.source_hash(), source_hash());
}

#[test]
fn stale_move_project_replay_fails_closed() {
    let base = graph();
    let mut editor = EditorSession::new(base.clone()).expect("session");
    let add = editor
        .add_ruler_guide_v1(page_id(), RulerGuideAxis::Vertical, LengthEmu::new(914_400))
        .expect("add guide");
    let guide_id = match &add {
        EditOperation::RulerGuideV1 {
            transition: RulerGuideTransitionV1::Add { after },
        } => after.guide_id.clone(),
        other => panic!("unexpected add operation: {other:?}"),
    };
    editor
        .move_ruler_guide_v1(&guide_id, LengthEmu::new(1_828_800))
        .expect("move guide");

    let mut project = editor.project();
    match &mut project.operations[1] {
        EditOperation::RulerGuideV1 {
            transition: RulerGuideTransitionV1::Move { before, .. },
        } => before.guide.position = LengthEmu::new(457_200),
        other => panic!("unexpected move operation: {other:?}"),
    }

    let mut reopened = EditorSession::new(base).expect("fresh session");
    assert!(matches!(
        reopened.apply_project(&project),
        Err(EditorProjectError::Operation {
            index: 1,
            error: EditorError::StaleRulerGuide { .. },
        })
    ));
    assert!(reopened.operations().is_empty());
}

#[test]
fn editable_targets_declare_authored_ruler_guide_loss_explicitly() {
    let mut editor = EditorSession::new(graph()).expect("session");
    editor
        .add_ruler_guide_v1(page_id(), RulerGuideAxis::Vertical, LengthEmu::new(914_400))
        .expect("add guide");

    for target in [EditorEditableTarget::Idml, EditorEditableTarget::Odg] {
        let preview = editor
            .preview_editable_export(target, "ruler-guide-test.pub")
            .expect("editable preview");
        assert!(preview.report.can_serialize);
        let item = preview
            .report
            .items
            .iter()
            .find(|item| item.feature == "page.ruler_guide")
            .expect("explicit ruler-guide loss");
        assert_eq!(item.origin, Some(page_id().into_canonical()));
        assert_eq!(item.property_path.as_deref(), Some("page.ruler_guides"));
        assert_eq!(item.disposition, pub_export::CapabilityLevel::Unsupported);
        assert_eq!(item.loss_kind, Some(pub_export::LossKind::Unsupported));
        assert_eq!(preview.report.counts.blocking, 0);
    }
}

#[test]
fn missing_page_and_missing_target_fail_closed() {
    let mut editor = EditorSession::new(graph()).expect("session");
    let missing_page: PageId = canonical_id("10000000-0000-4000-8000-000000000099");
    assert_eq!(
        editor
            .add_ruler_guide_v1(missing_page, RulerGuideAxis::Horizontal, LengthEmu::new(10),)
            .expect_err("missing page"),
        EditorError::RulerGuideUnsupported {
            message: format!(
                "page {} is not present in the current document",
                missing_page.as_canonical()
            ),
        }
    );
    assert!(editor.operations().is_empty());
    assert!(matches!(
        editor.delete_ruler_guide_v1("01999999-9999-7999-8999-999999999999"),
        Err(EditorError::RulerGuideMissing { .. })
    ));
    assert!(editor.operations().is_empty());
}
