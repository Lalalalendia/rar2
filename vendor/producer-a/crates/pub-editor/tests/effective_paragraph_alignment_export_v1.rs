use pub_editor::{
    AuthoredParagraphAlignmentValueV1, EditorEditableTarget, ParagraphAlignmentV1, Sha256Digest,
    open_mature_0x2c_editor,
};

fn marker_counts(bytes: &[u8], target: EditorEditableTarget) -> (usize, usize) {
    let text = String::from_utf8_lossy(bytes);
    match target {
        EditorEditableTarget::Idml => (
            text.matches("Justification=\"CenterAlign\"").count(),
            text.matches("Justification=\"RightAlign\"").count(),
        ),
        EditorEditableTarget::Odg => (
            text.matches("fo:text-align=\"center\"").count(),
            text.matches("fo:text-align=\"right\"").count(),
        ),
    }
}

fn export_counts(
    session: &pub_editor::EditorSession,
    target: EditorEditableTarget,
) -> (usize, usize) {
    let export = session
        .export_editable(target, "carlton-effective-paragraph-alignment")
        .expect("bounded editable export");
    marker_counts(&export.bytes, target)
}

#[test]
#[ignore = "requires the pinned public Carlton March PUB path"]
fn real_carlton_effective_full_story_alignment_drives_idml_and_odg() {
    let path = std::env::var_os("CHAPTERA_CARLTON_PUB")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_CARLTON_PUB");
    let bytes = std::fs::read(path).expect("read pinned Carlton March PUB");
    let source_hash = "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3"
        .parse::<Sha256Digest>()
        .expect("pinned Carlton source hash");

    let mut session =
        open_mature_0x2c_editor(&bytes, source_hash).expect("open Carlton EditorSession");
    let source_alignments = session
        .effective_full_story_paragraph_alignment_v1()
        .expect("derive effective full-Story alignment");
    let right = source_alignments
        .iter()
        .find(|item| item.alignment == ParagraphAlignmentV1::Right)
        .expect("Carlton must expose one uniform effective Right Story")
        .clone();

    let paragraph_ids = session
        .imported_paragraphs_v1()
        .expect("project Carlton imported ParagraphIds")
        .into_iter()
        .filter(|paragraph| paragraph.story_id == right.story_id)
        .map(|paragraph| paragraph.paragraph_id)
        .collect::<Vec<_>>();
    assert!(!paragraph_ids.is_empty());

    let source_idml = export_counts(&session, EditorEditableTarget::Idml);
    let source_odg = export_counts(&session, EditorEditableTarget::Odg);
    assert!(source_idml.1 > 0);
    assert!(source_odg.1 > 0);

    session
        .set_paragraph_alignment_override_v1(
            paragraph_ids.clone(),
            AuthoredParagraphAlignmentValueV1::Center,
        )
        .expect("set Center across the whole Story");
    let centered = session
        .effective_full_story_paragraph_alignment_v1()
        .expect("derive centered full-Story alignment");
    assert!(centered.iter().any(|item| {
        item.story_id == right.story_id && item.alignment == ParagraphAlignmentV1::Center
    }));

    let centered_idml = export_counts(&session, EditorEditableTarget::Idml);
    let centered_odg = export_counts(&session, EditorEditableTarget::Odg);
    assert_eq!(centered_idml.0, source_idml.0 + 1);
    assert_eq!(centered_idml.1 + 1, source_idml.1);
    assert_eq!(centered_odg.0, source_odg.0 + 1);
    assert_eq!(centered_odg.1 + 1, source_odg.1);

    session
        .clear_paragraph_alignment_override_v1(paragraph_ids.clone())
        .expect("clear full-Story Center override");
    assert_eq!(
        export_counts(&session, EditorEditableTarget::Idml),
        source_idml
    );
    assert_eq!(
        export_counts(&session, EditorEditableTarget::Odg),
        source_odg
    );

    session
        .set_paragraph_alignment_override_v1(
            vec![paragraph_ids[0]],
            AuthoredParagraphAlignmentValueV1::Left,
        )
        .expect("set unsupported Left override");
    assert!(
        !session
            .effective_full_story_paragraph_alignment_v1()
            .expect("derive full-Story alignment after Left override")
            .iter()
            .any(|item| item.story_id == right.story_id)
    );

    let left_idml = export_counts(&session, EditorEditableTarget::Idml);
    let left_odg = export_counts(&session, EditorEditableTarget::Odg);
    assert_eq!(left_idml.0, source_idml.0);
    assert_eq!(left_idml.1 + 1, source_idml.1);
    assert_eq!(left_odg.0, source_odg.0);
    assert_eq!(left_odg.1 + 1, source_odg.1);
}
