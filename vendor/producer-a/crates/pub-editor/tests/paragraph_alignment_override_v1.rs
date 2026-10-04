use pub_editor::{
    AuthoredParagraphAlignmentValueV1, EDITOR_PROJECT_VERSION_V0_15,
    EffectiveParagraphAlignmentValueV1, ImportedParagraphAlignmentValueV1,
    ParagraphAlignmentAuthorityV1, Sha256Digest, open_mature_0x2c_editor,
};

#[test]
#[ignore = "requires the pinned public Carlton March PUB path"]
fn real_carlton_paragraph_alignment_override_roundtrips_history_and_project() {
    let path = std::env::var_os("CHAPTERA_CARLTON_PUB")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_CARLTON_PUB");
    let bytes = std::fs::read(path).expect("read pinned Carlton March PUB");
    let source_hash = "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3"
        .parse::<Sha256Digest>()
        .expect("pinned Carlton source hash");

    let mut session =
        open_mature_0x2c_editor(&bytes, source_hash).expect("open Carlton EditorSession");
    let base = session
        .imported_paragraph_base_alignments_v1()
        .expect("bind Carlton imported paragraph base alignment");
    let paragraph = base
        .iter()
        .find(|item| item.alignment == ImportedParagraphAlignmentValueV1::Right)
        .expect("Carlton must expose one grounded Right paragraph base");
    let paragraph_id = paragraph.paragraph_id;

    let source_effective = session
        .effective_paragraph_alignment_v1(paragraph_id)
        .expect("source effective alignment");
    assert_eq!(
        source_effective.effective,
        Some(EffectiveParagraphAlignmentValueV1::Right)
    );
    assert_eq!(
        source_effective.authority,
        Some(ParagraphAlignmentAuthorityV1::ImportedBase)
    );

    session
        .set_paragraph_alignment_override_v1(
            vec![paragraph_id],
            AuthoredParagraphAlignmentValueV1::Center,
        )
        .expect("set authored Center override");
    assert_eq!(
        session
            .authored_paragraph_alignment_override_v1(paragraph_id)
            .expect("derive authored override from history"),
        Some(AuthoredParagraphAlignmentValueV1::Center)
    );
    let authored = session
        .effective_paragraph_alignment_v1(paragraph_id)
        .expect("authored effective alignment");
    assert_eq!(
        authored.effective,
        Some(EffectiveParagraphAlignmentValueV1::Center)
    );
    assert_eq!(
        authored.authority,
        Some(ParagraphAlignmentAuthorityV1::ChapteraOverride)
    );
    let authored_export = session
        .effective_paragraph_alignment_export_v1()
        .expect("project authored effective paragraph alignment for export");
    let authored_export_paragraph = authored_export
        .iter()
        .find(|item| item.paragraph_id == paragraph_id)
        .expect("export projection must retain the canonical ParagraphId");
    assert_eq!(authored_export_paragraph.story_id, paragraph.story_id);
    assert_eq!(authored_export_paragraph.range, paragraph.range);
    assert_eq!(
        authored_export_paragraph.alignment,
        Some(pub_export::ParagraphAlignmentV1::Center)
    );

    let lifecycle_error = session
        .replace_story_text(
            paragraph.story_id,
            "blocked while paragraph override is active",
        )
        .expect_err("Story text mutation must be fenced while ParagraphId override is active");
    assert_eq!(
        lifecycle_error.code(),
        "paragraph_alignment_lifecycle_unsupported"
    );

    session.undo().expect("undo authored paragraph alignment");
    assert_eq!(
        session
            .authored_paragraph_alignment_override_v1(paragraph_id)
            .expect("derive undo override state from history"),
        None
    );
    let undone = session
        .effective_paragraph_alignment_v1(paragraph_id)
        .expect("undo effective alignment");
    assert_eq!(
        undone.effective,
        Some(EffectiveParagraphAlignmentValueV1::Right)
    );
    assert_eq!(
        undone.authority,
        Some(ParagraphAlignmentAuthorityV1::ImportedBase)
    );

    session.redo().expect("redo authored paragraph alignment");
    assert_eq!(
        session
            .authored_paragraph_alignment_override_v1(paragraph_id)
            .expect("derive redo override state from history"),
        Some(AuthoredParagraphAlignmentValueV1::Center)
    );
    assert_eq!(
        session
            .effective_paragraph_alignment_v1(paragraph_id)
            .expect("redo effective alignment")
            .effective,
        Some(EffectiveParagraphAlignmentValueV1::Center)
    );

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_15);

    let mut reopened =
        open_mature_0x2c_editor(&bytes, source_hash).expect("fresh Carlton EditorSession");
    reopened
        .apply_project(&project)
        .expect("replay paragraph alignment project");
    assert_eq!(
        reopened
            .authored_paragraph_alignment_override_v1(paragraph_id)
            .expect("derive reopened override from replayed history"),
        Some(AuthoredParagraphAlignmentValueV1::Center)
    );
    let reopened_effective = reopened
        .effective_paragraph_alignment_v1(paragraph_id)
        .expect("fresh reopen effective alignment");
    assert_eq!(
        reopened_effective.effective,
        Some(EffectiveParagraphAlignmentValueV1::Center)
    );
    assert_eq!(
        reopened_effective.authority,
        Some(ParagraphAlignmentAuthorityV1::ChapteraOverride)
    );

    reopened
        .clear_paragraph_alignment_override_v1(vec![paragraph_id])
        .expect("clear authored paragraph alignment");
    assert_eq!(
        reopened
            .authored_paragraph_alignment_override_v1(paragraph_id)
            .expect("derive cleared override state from history"),
        None
    );
    let cleared = reopened
        .effective_paragraph_alignment_v1(paragraph_id)
        .expect("cleared effective alignment");
    assert_eq!(
        cleared.effective,
        Some(EffectiveParagraphAlignmentValueV1::Right)
    );
    assert_eq!(
        cleared.authority,
        Some(ParagraphAlignmentAuthorityV1::ImportedBase)
    );
    let cleared_export = reopened
        .effective_paragraph_alignment_export_v1()
        .expect("project cleared effective paragraph alignment for export");
    assert_eq!(
        cleared_export
            .iter()
            .find(|item| item.paragraph_id == paragraph_id)
            .expect("cleared export projection keeps ParagraphId")
            .alignment,
        Some(pub_export::ParagraphAlignmentV1::Right)
    );

    let redundant = reopened
        .set_paragraph_alignment_override_v1(
            vec![paragraph_id],
            AuthoredParagraphAlignmentValueV1::Right,
        )
        .expect_err("setting the known Right base must normalize to inherit/no change");
    assert_eq!(redundant.code(), "paragraph_alignment_no_change");
    assert_eq!(
        reopened
            .authored_paragraph_alignment_override_v1(paragraph_id)
            .expect("derive redundant override state from history"),
        None
    );

    reopened
        .set_paragraph_alignment_override_v1(
            vec![paragraph_id],
            AuthoredParagraphAlignmentValueV1::Left,
        )
        .expect("set authored Left override");
    assert_eq!(
        reopened
            .authored_paragraph_alignment_override_v1(paragraph_id)
            .expect("derive Left override state from history"),
        Some(AuthoredParagraphAlignmentValueV1::Left)
    );
    let left = reopened
        .effective_paragraph_alignment_v1(paragraph_id)
        .expect("Left effective alignment");
    assert_eq!(
        left.effective,
        Some(EffectiveParagraphAlignmentValueV1::Left)
    );
    assert_eq!(
        left.authority,
        Some(ParagraphAlignmentAuthorityV1::ChapteraOverride)
    );
    assert_eq!(
        reopened
            .effective_paragraph_alignment_export_v1()
            .expect("project Left effective paragraph alignment for export")
            .iter()
            .find(|item| item.paragraph_id == paragraph_id)
            .expect("Left export projection keeps ParagraphId")
            .alignment,
        Some(pub_export::ParagraphAlignmentV1::Left)
    );
}
