use pub_editor::{
    EDITOR_PROJECT_VERSION_V0_16, EditOperation, EditorProject, FormatPropertyV1, FormatValueV1,
    Sha256Digest, StoryId, open_mature_0x2c_editor,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    Sha256Digest::from_bytes(out)
}

fn placed_story_ids(editor: &pub_editor::EditorSession) -> Vec<StoryId> {
    editor
        .graph()
        .nodes
        .values()
        .filter_map(|node| node.payload.story_frame.as_ref()?.story_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[test]
fn real_pub_color_blocked_story_persists_scoped_bold_history_v016() {
    let Some(root) = env::var_os("CHAPTERA_TEXT_FORMAT_FIXTURES_DIR") else {
        eprintln!(
            "CHAPTERA_TEXT_FORMAT_FIXTURES_DIR not set; dedicated scoped-history gate owns real evidence"
        );
        return;
    };

    let mut paths = fs::read_dir(root)
        .expect("read pinned text-format fixture corpus")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<PathBuf>>();
    paths.sort();

    let mut inspected = 0_usize;
    let witness = paths.into_iter().find_map(|path| {
        let original = fs::read(&path).ok()?;
        let hash = source_hash(&original);
        let editor = open_mature_0x2c_editor(&original, hash).ok()?;

        for story_id in placed_story_ids(&editor) {
            inspected += 1;
            let full_error = match editor.source_text_format_overlay_v1(story_id) {
                Ok(_) => continue,
                Err(error) => error.to_string(),
            };
            if !full_error.contains("bounded effective direct-RGB text color is unavailable") {
                continue;
            }

            let before = match editor
                .current_text_format_property_state_v1(story_id, FormatPropertyV1::Bold)
            {
                Ok(state) if state.story_scalar_len > 0 && !state.base_runs.is_empty() => state,
                _ => continue,
            };
            let first = match before.base_runs.first().map(|run| &run.value) {
                Some(FormatValueV1::Bool(value)) => *value,
                _ => continue,
            };

            return Some((
                path,
                original,
                hash,
                editor,
                story_id,
                before,
                !first,
                full_error,
            ));
        }
        None
    });

    let (
        path,
        original,
        hash,
        mut editor,
        story_id,
        before,
        desired,
        original_full_error,
    ) = witness.unwrap_or_else(|| {
        panic!(
            "pinned real-PUB corpus exposes no placed Story with color-blocked full overlay and proven scoped Bold after inspecting {inspected} Stories"
        )
    });

    let source_text = editor.graph().stories[&story_id].text.clone();
    let story_len = before.story_scalar_len;
    let before_hash = editor
        .current_text_format_property_state_hash_v1(story_id, FormatPropertyV1::Bold)
        .expect("source scoped Bold hash");

    let operation = editor
        .set_text_format_property_scoped_v1(
            story_id,
            0,
            story_len,
            FormatPropertyV1::Bold,
            FormatValueV1::Bool(desired),
            &before_hash,
        )
        .expect("commit scoped Bold operation on color-blocked real PUB");

    assert!(matches!(
        operation,
        EditOperation::SetTextFormatPropertyScopedV1 {
            story_id: id,
            property: FormatPropertyV1::Bold,
            value: FormatValueV1::Bool(value),
            ..
        } if id == story_id && value == desired
    ));
    assert_eq!(
        editor.graph().stories[&story_id].text, source_text,
        "scoped formatting must not mutate Story text"
    );

    let after = editor
        .current_text_format_property_state_v1(story_id, FormatPropertyV1::Bold)
        .expect("edited scoped Bold state");
    let segments = editor
        .current_text_format_property_segments_v1(
            story_id,
            FormatPropertyV1::Bold,
            0,
            story_len,
        )
        .expect("edited scoped Bold segments");
    assert!(!segments.is_empty());
    assert!(segments.iter().all(|segment| {
        segment.property == FormatPropertyV1::Bold
            && segment.value == FormatValueV1::Bool(desired)
    }));

    let still_unresolved = editor
        .current_text_format_overlay_v1(story_id)
        .expect_err("scoped Bold must not invent unresolved text color")
        .to_string();
    assert!(
        still_unresolved.contains("bounded effective direct-RGB text color is unavailable"),
        "unrelated color authority changed: {still_unresolved}"
    );

    editor.undo().expect("Undo scoped Bold");
    assert_eq!(
        editor
            .current_text_format_property_state_v1(story_id, FormatPropertyV1::Bold)
            .expect("undone scoped Bold state"),
        before
    );

    editor.redo().expect("Redo scoped Bold");
    assert_eq!(
        editor
            .current_text_format_property_state_v1(story_id, FormatPropertyV1::Bold)
            .expect("redone scoped Bold state"),
        after
    );

    let project = editor.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_16);
    assert_eq!(project.operations.len(), 1);
    assert!(matches!(
        &project.operations[0],
        EditOperation::SetTextFormatPropertyScopedV1 { .. }
    ));

    let serialized = serde_json::to_vec(&project).expect("serialize v0.16 scoped project");
    let project_roundtrip: EditorProject =
        serde_json::from_slice(&serialized).expect("deserialize v0.16 scoped project");
    assert_eq!(project_roundtrip, project);

    let mut reopened =
        open_mature_0x2c_editor(&original, hash).expect("fresh reopen pinned source PUB");
    reopened
        .apply_project(&project_roundtrip)
        .expect("replay v0.16 scoped project on fresh source");

    assert_eq!(
        reopened
            .current_text_format_property_state_v1(story_id, FormatPropertyV1::Bold)
            .expect("replayed scoped Bold state"),
        after
    );
    assert!(
        reopened
            .current_text_format_overlay_v1(story_id)
            .expect_err("replay must leave unrelated color unresolved")
            .to_string()
            .contains("bounded effective direct-RGB text color is unavailable")
    );
    assert_eq!(reopened.graph().stories[&story_id].text, source_text);
    assert_eq!(reopened.source_hash(), hash);
    assert_eq!(
        fs::read(&path).expect("re-read pinned source PUB"),
        original,
        "scoped history must not mutate source PUB bytes"
    );

    eprintln!(
        "scoped-history real-PUB witness: file={} sha256={} story={} schema={} full_overlay_error={}",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("<non-utf8>"),
        hash,
        story_id.as_canonical(),
        project.schema_version,
        original_full_error
    );
}
