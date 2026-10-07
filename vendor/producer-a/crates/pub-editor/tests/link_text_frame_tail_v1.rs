use std::collections::BTreeMap;

use pub_editor::{
    AuthoringTextPresetV1, EDITOR_PROJECT_VERSION_V0_21, EDITOR_PROJECT_VERSION_V0_22,
    EditOperation, EditorEditableTarget, EditorSession, LengthEmu, RectEmu,
};
use pub_model::{
    Document, DocumentId, Page, PageId, ResolvedGraph, Sha256Digest, Size2D, SourceDescriptor,
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
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn text_node_id() -> pub_editor::NodeId {
    canonical_id("01890f47-0c00-7abc-8def-0123456789ab")
}

fn story_id() -> pub_editor::StoryId {
    canonical_id("01890f47-0c01-7abc-8def-0123456789ab")
}

fn rect() -> RectEmu {
    RectEmu::new(
        LengthEmu::new(100_000),
        LengthEmu::new(200_000),
        LengthEmu::new(2_400_000),
        LengthEmu::new(900_000),
    )
}

fn preset() -> AuthoringTextPresetV1 {
    AuthoringTextPresetV1 {
        resource_id: "chaptera.desktop.fallback-font.ubuntu-light.v1".into(),
        font_fingerprint_sha256: "80307b8da7649aa4ee4d484b232140e3ce1ec0ca093073d3c53c8f5a5ced7a70"
            .into(),
        face_index: 0,
        font_size_emu: LengthEmu::new(114_300),
        line_height_emu: LengthEmu::new(142_875),
    }
}

fn graph() -> PubResolvedGraph {
    let source_hash = source_hash();
    let page_id = page_id();
    let mut pages = BTreeMap::new();
    pages.insert(
        page_id,
        Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(8_000_000), LengthEmu::new(10_000_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        },
    );

    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "create-textbox-rust-test".into(),
        source: SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "pub-rs/test".into(),
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

fn target_id() -> pub_editor::NodeId {
    canonical_id("01890f47-0c02-7abc-8def-0123456789ab")
}

fn target_story_id() -> pub_editor::StoryId {
    canonical_id("01890f47-0c03-7abc-8def-0123456789ab")
}

fn third_id() -> pub_editor::NodeId {
    canonical_id("01890f47-0c04-7abc-8def-0123456789ab")
}

fn third_story_id() -> pub_editor::StoryId {
    canonical_id("01890f47-0c05-7abc-8def-0123456789ab")
}

fn session() -> EditorSession {
    let mut session = EditorSession::new(graph()).unwrap();
    session
        .create_text_box(text_node_id(), story_id(), page_id(), rect(), preset())
        .unwrap();
    session
        .replace_story_text(
            story_id(),
            "Text continues without copying or splitting the Story.",
        )
        .unwrap();
    session
        .create_text_box(target_id(), target_story_id(), page_id(), rect(), preset())
        .unwrap();
    session
}

#[test]
fn one_link_preserves_frames_and_story_then_undo_redo_and_fresh_replay_are_exact() {
    let mut session = session();
    let before = session.graph().clone();
    let count = session.operations().len();
    let operation = session
        .link_text_frame_tail(text_node_id(), target_id())
        .unwrap();
    assert!(matches!(operation, EditOperation::LinkTextFrameTail { .. }));
    assert_eq!(session.operations().len(), count + 1);
    let after = session.graph().clone();
    assert_eq!(after.stories[&story_id()], before.stories[&story_id()]);
    assert!(!after.stories.contains_key(&target_story_id()));
    assert_eq!(after.pages, before.pages);
    for id in [text_node_id(), target_id()] {
        assert_eq!(after.nodes[&id].header, before.nodes[&id].header);
        let mut after_payload = after.nodes[&id].payload.clone();
        after_payload.story_frame = before.nodes[&id].payload.story_frame.clone();
        assert_eq!(after_payload, before.nodes[&id].payload);
    }
    let source = after.nodes[&text_node_id()]
        .payload
        .story_frame
        .as_ref()
        .unwrap();
    let target = after.nodes[&target_id()]
        .payload
        .story_frame
        .as_ref()
        .unwrap();
    assert_eq!(source.next_frame, Some(target_id()));
    assert_eq!(target.previous_frame, Some(text_node_id()));
    assert_eq!(target.story_id, Some(story_id()));
    assert_eq!(target.ordinal, 1);
    assert_eq!(
        session.project().schema_version,
        EDITOR_PROJECT_VERSION_V0_22
    );
    let proof = session
        .prove_author_created_story_v1(story_id())
        .unwrap()
        .unwrap();
    assert_eq!(proof.frame_id, text_node_id());
    assert_eq!(proof.text_preset, preset());

    session.undo().unwrap();
    assert_eq!(session.graph(), &before);
    assert_eq!(
        session.graph().stories[&target_story_id()].id,
        target_story_id()
    );
    session.redo().unwrap();
    assert_eq!(session.graph(), &after);
    assert_eq!(session.operations().last(), Some(&operation));
    let project = serde_json::from_slice(&serde_json::to_vec(&session.project()).unwrap()).unwrap();
    let mut fresh = EditorSession::new(graph()).unwrap();
    fresh.apply_project(&project).unwrap();
    assert_eq!(fresh.graph(), &after);
    assert_eq!(fresh.project(), project);
    assert_eq!(
        fresh.project().state_id_v1(),
        session.project().state_id_v1()
    );
    assert_eq!(fresh.source_hash(), source_hash());
}

#[test]
fn created_chain_stays_editable_and_can_continue_again() {
    let mut session = session();
    session
        .link_text_frame_tail(text_node_id(), target_id())
        .unwrap();
    session
        .replace_story_range(story_id(), 0, 4, "Text", "New text")
        .unwrap();
    session
        .create_text_box(third_id(), third_story_id(), page_id(), rect(), preset())
        .unwrap();
    session
        .link_text_frame_tail(target_id(), third_id())
        .unwrap();
    assert!(
        session
            .prove_author_created_story_v1(story_id())
            .unwrap()
            .is_some()
    );
    assert_eq!(
        session.graph().nodes[&third_id()]
            .payload
            .story_frame
            .as_ref()
            .unwrap()
            .ordinal,
        2
    );
    assert!(
        session.graph().stories[&story_id()]
            .text
            .starts_with("New text")
    );
    let project = session.project();
    let mut fresh = EditorSession::new(graph()).unwrap();
    fresh.apply_project(&project).unwrap();
    assert_eq!(fresh.graph(), session.graph());
    // All inverse operations also restore the original two empty target identities.
    while !fresh.operations().is_empty() {
        fresh.undo().unwrap();
    }
    assert_eq!(fresh.graph(), &graph());
}

#[test]
fn denied_targets_and_non_tail_sources_commit_nothing() {
    let mut session = session();
    session
        .replace_story_text(target_story_id(), "Not empty")
        .unwrap();
    let before = session.graph().clone();
    let operations = session.operations().to_vec();
    assert!(
        session
            .link_text_frame_tail(text_node_id(), target_id())
            .is_err()
    );
    assert!(
        session
            .link_text_frame_tail(text_node_id(), text_node_id())
            .is_err()
    );
    assert!(
        session
            .link_text_frame_tail(text_node_id(), third_id())
            .is_err()
    );
    assert_eq!(session.graph(), &before);
    assert_eq!(session.operations(), operations);
    session.undo().unwrap();
    session
        .link_text_frame_tail(text_node_id(), target_id())
        .unwrap();
    session
        .create_text_box(third_id(), third_story_id(), page_id(), rect(), preset())
        .unwrap();
    let before = session.graph().clone();
    assert!(
        session
            .link_text_frame_tail(text_node_id(), third_id())
            .is_err()
    );
    assert!(
        session
            .link_text_frame_tail(target_id(), text_node_id())
            .is_err()
    );
    assert_eq!(session.graph(), &before);
}

#[test]
fn standalone_import_is_denied_but_existing_explicit_chain_tail_is_admitted() {
    let source = session();
    let imported = EditorSession::new(source.graph().clone()).unwrap();
    assert!(
        imported
            .can_link_text_frame_source_v1(text_node_id())
            .is_err()
    );
    let mut source = source;
    source
        .link_text_frame_tail(text_node_id(), target_id())
        .unwrap();
    let mut imported = EditorSession::new(source.graph().clone()).unwrap();
    imported
        .create_text_box(third_id(), third_story_id(), page_id(), rect(), preset())
        .unwrap();
    imported
        .link_text_frame_tail(target_id(), third_id())
        .unwrap();
    assert_eq!(
        imported.graph().nodes[&third_id()]
            .payload
            .story_frame
            .as_ref()
            .unwrap()
            .story_id,
        Some(story_id())
    );
}

#[test]
fn cross_page_link_replays_with_stable_page_ownership() {
    let second_page: PageId = canonical_id("11000000-0000-4000-8000-000000000002");
    let mut base = graph();
    let mut second = base.pages[&page_id()].clone();
    second.id = second_page;
    base.pages.insert(second_page, second);
    base.document.pages.push(second_page);
    let mut session = EditorSession::new(base.clone()).unwrap();
    session
        .create_text_box(text_node_id(), story_id(), page_id(), rect(), preset())
        .unwrap();
    session
        .create_text_box(
            target_id(),
            target_story_id(),
            second_page,
            rect(),
            preset(),
        )
        .unwrap();
    session
        .link_text_frame_tail(text_node_id(), target_id())
        .unwrap();
    assert_eq!(
        session.graph().nodes[&target_id()].header.parent_id,
        second_page.into_canonical()
    );
    let mut fresh = EditorSession::new(base).unwrap();
    fresh.apply_project(&session.project()).unwrap();
    assert_eq!(fresh.graph(), session.graph());
}

#[test]
fn legacy_or_forged_project_is_rejected_transactionally() {
    let mut session = session();
    session
        .link_text_frame_tail(text_node_id(), target_id())
        .unwrap();
    let mut project = session.project();
    project.schema_version = EDITOR_PROJECT_VERSION_V0_21.into();
    let mut fresh = EditorSession::new(graph()).unwrap();
    assert!(fresh.apply_project(&project).is_err());
    assert_eq!(fresh.graph(), &graph());
    assert!(fresh.operations().is_empty());
    project.schema_version = EDITOR_PROJECT_VERSION_V0_22.into();
    if let Some(EditOperation::LinkTextFrameTail { transition }) = project.operations.last_mut() {
        transition.after_frames[1].ordinal = 99;
    } else {
        panic!("link operation");
    }
    assert!(fresh.apply_project(&project).is_err());
    assert_eq!(fresh.graph(), &graph());
    assert!(fresh.operations().is_empty());
}

fn zip_text(bytes: &[u8], name: &str) -> String {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

#[test]
fn current_idml_and_odg_preserve_chain_and_one_live_story() {
    let mut session = session();
    session
        .link_text_frame_tail(text_node_id(), target_id())
        .unwrap();
    session
        .replace_story_text(story_id(), "Current linked publication text")
        .unwrap();
    let odg = session
        .export_editable(EditorEditableTarget::Odg, "link-test")
        .unwrap();
    let content = zip_text(&odg.bytes, "content.xml");
    assert!(content.contains("draw:chain-next-name="));
    assert_eq!(
        content.matches("Current linked publication text").count(),
        1
    );
    assert!(content.contains(&format!(
        "draw:chain-next-name=\"Frame_{}\"",
        target_id().as_canonical().to_string().replace('-', "")
    )));
    let idml = session
        .export_editable(EditorEditableTarget::Idml, "link-test")
        .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&idml.bytes)).unwrap();
    let mut all = String::new();
    use std::io::Read;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        if file.name().ends_with(".xml") {
            file.read_to_string(&mut all).unwrap();
        }
    }
    assert!(all.contains("NextTextFrame="));
    assert_eq!(all.matches("Current linked publication text").count(), 1);
    assert!(!all.contains(&format!("Story_{}", target_story_id().as_canonical())));
}

#[test]
fn frames_outside_the_current_document_cannot_be_link_sources() {
    let mut authored = session();
    authored
        .link_text_frame_tail(text_node_id(), target_id())
        .unwrap();
    let mut imported = authored.graph().clone();
    let other_page: PageId = canonical_id("44000000-0000-4000-8000-000000000009");
    let mut detached = imported.pages[&page_id()].clone();
    detached.id = other_page;
    imported.pages.get_mut(&page_id()).unwrap().children.clear();
    for id in [text_node_id(), target_id()] {
        imported.nodes.get_mut(&id).unwrap().header.parent_id = other_page.into_canonical();
    }
    imported.pages.insert(other_page, detached);
    assert!(!imported.document.pages.contains(&other_page));
    let mut session = EditorSession::new(imported).unwrap();
    session
        .create_text_box(third_id(), third_story_id(), page_id(), rect(), preset())
        .unwrap();
    let before = session.graph().clone();
    let operations = session.operations().to_vec();
    assert!(
        session
            .link_text_frame_tail(target_id(), third_id())
            .is_err()
    );
    assert_eq!(session.graph(), &before);
    assert_eq!(session.operations(), operations);
}
