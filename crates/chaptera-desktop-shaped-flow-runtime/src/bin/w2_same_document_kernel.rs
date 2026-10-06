use chaptera_desktop_shaped_flow_runtime::{
    ExplicitDesktopFontResourceV1, build_current_story_layout_v1,
};
use pub_editor::{
    EditOperation, EditorEditableTarget, EditorProject, EditorSession, Sha256Digest,
    open_mature_0x2c_editor,
};
use pub_model::{EMU_PER_POINT, LengthEmu, StoryId};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const FONT_SIZE_PT: i64 = 8;
const LINE_HEIGHT_PT: i64 = 16;
const RECEIPT_VERSION: &str = "chaptera.w2-same-document-kernel.v1";

fn digest(bytes: &[u8]) -> Sha256Digest {
    let raw = Sha256::digest(bytes);
    let mut value = [0_u8; 32];
    value.copy_from_slice(&raw);
    Sha256Digest::from_bytes(value)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let raw = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in raw {
        use std::fmt::Write as _;
        write!(&mut out, "{byte:02x}").expect("hex into String");
    }
    out
}

fn string_value<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value).expect("serialize value") {
        Value::String(value) => value,
        other => other.to_string(),
    }
}

fn hash_id<T: Serialize>(value: &T) -> String {
    let raw = string_value(value);
    if raw.starts_with("sha256:") {
        raw
    } else {
        format!("sha256:{raw}")
    }
}

fn read_project(path: &Path) -> Result<EditorProject, String> {
    let raw = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_slice(&raw)
        .map_err(|error| format!("parse EditorProject {}: {error}", path.display()))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|error| format!("write {}: {error}", path.display()))
}

fn replay_recipe(
    editor: &mut EditorSession,
    recipe: &EditorProject,
    replacement: &[u8],
) -> Result<StoryId, String> {
    if recipe.operations.len() != 4 {
        return Err(format!(
            "W2 recipe must contain exactly four operations; found {}",
            recipe.operations.len()
        ));
    }
    if recipe.assets.len() != 1 {
        return Err(format!(
            "W2 recipe must contain exactly one replacement asset; found {}",
            recipe.assets.len()
        ));
    }

    let replacement_sha = digest(replacement);
    if recipe.assets[0].sha256 != replacement_sha {
        return Err("W2 recipe asset identity differs from replacement bytes".into());
    }
    let mime = if replacement.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if replacement.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else {
        return Err("W2 replacement asset is neither PNG nor JPEG".into());
    };

    let mut story_id = None;
    let mut seen_move = false;
    let mut seen_resize = false;
    let mut seen_replace = false;

    for expected in &recipe.operations {
        let observed = match expected {
            EditOperation::ReplaceStoryRange {
                story_id: id,
                start_scalar,
                end_scalar,
                expected_before,
                replacement_text,
                ..
            } => {
                if story_id.replace(*id).is_some() {
                    return Err("W2 recipe contains more than one Story edit".into());
                }
                editor
                    .replace_story_range(
                        *id,
                        *start_scalar,
                        *end_scalar,
                        expected_before.clone(),
                        replacement_text.clone(),
                    )
                    .map_err(|error| format!("replay Story edit: {error}"))?
            }
            EditOperation::MoveNode { node_id, after, .. } => {
                if seen_move {
                    return Err("W2 recipe contains more than one MoveNode".into());
                }
                seen_move = true;
                editor
                    .move_node_to(*node_id, after.x, after.y)
                    .map_err(|error| format!("replay MoveNode: {error}"))?
            }
            EditOperation::ResizeNode { node_id, after, .. } => {
                if seen_resize {
                    return Err("W2 recipe contains more than one ResizeNode".into());
                }
                seen_resize = true;
                editor
                    .resize_node_to(*node_id, *after)
                    .map_err(|error| format!("replay ResizeNode: {error}"))?
            }
            EditOperation::ReplaceImage {
                node_id,
                after_asset,
                ..
            } => {
                if seen_replace {
                    return Err("W2 recipe contains more than one ReplaceImage".into());
                }
                seen_replace = true;
                let imported = editor
                    .import_replacement_asset(mime, replacement.to_vec())
                    .map_err(|error| format!("import replacement asset: {error}"))?;
                if imported != *after_asset {
                    return Err("replayed replacement asset identity differs from recipe".into());
                }
                editor
                    .replace_image(*node_id, imported)
                    .map_err(|error| format!("replay ReplaceImage: {error}"))?
            }
            _ => {
                return Err(
                    "W2 recipe contains an operation outside Story/Move/Resize/ReplaceImage".into(),
                );
            }
        };
        if &observed != expected {
            return Err("canonical replay operation differs from Stage-0.1 recipe".into());
        }
    }

    if !seen_move || !seen_resize || !seen_replace {
        return Err("W2 recipe is missing Move/Resize/ReplaceImage".into());
    }
    story_id.ok_or_else(|| "W2 recipe is missing Story edit".to_owned())
}

fn run(
    source_path: &Path,
    recipe_project_path: &Path,
    replacement_path: &Path,
    font_path: &Path,
    next_project_path: &Path,
    next_odg_path: &Path,
    receipt_path: &Path,
) -> Result<(), String> {
    let source = fs::read(source_path)
        .map_err(|error| format!("read source {}: {error}", source_path.display()))?;
    let source_hash = digest(&source);
    let source_hex = sha256_hex(&source);
    let recipe = read_project(recipe_project_path)?;
    if recipe.source_hash != source_hash {
        return Err("Stage-0.1 recipe source identity differs from immutable PUB".into());
    }

    let replacement = fs::read(replacement_path).map_err(|error| {
        format!(
            "read replacement asset {}: {error}",
            replacement_path.display()
        )
    })?;
    let replacement_sha = digest(&replacement);
    if recipe.assets.len() != 1 || recipe.assets[0].sha256 != replacement_sha {
        return Err("Stage-0.1 recipe does not bind the exact replacement bytes".into());
    }

    let parent_session = open_mature_0x2c_editor(&source, source_hash)
        .map_err(|error| format!("open parent EditorSession: {error}"))?;
    let parent = parent_session.project();
    if !parent.operations.is_empty() || !parent.assets.is_empty() {
        return Err(
            "imported previous issue parent must start with zero Chaptera mutations".into(),
        );
    }
    let parent_identity = parent
        .identity
        .as_ref()
        .ok_or_else(|| "parent project identity missing".to_owned())?
        .clone();
    let parent_state = parent.state_id_v1();

    let fork = parent_session
        .fork_project_next_issue()
        .map_err(|error| format!("fork next issue: {error}"))?;
    let fork_identity = fork
        .identity
        .as_ref()
        .ok_or_else(|| "fork project identity missing".to_owned())?
        .clone();
    let fork_state = fork.state_id_v1();
    let provenance = fork_identity
        .forked_from
        .as_ref()
        .ok_or_else(|| "fork provenance missing".to_owned())?;

    let initial_state_preserved = fork_state == parent_state;
    let identity_rekeyed = fork_identity.project_id != parent_identity.project_id
        && fork_identity.document_id != parent_identity.document_id
        && fork_identity.history_id != parent_identity.history_id
        && fork_identity.genesis_revision_id != parent_identity.genesis_revision_id;
    let provenance_exact = provenance.project_id == parent_identity.project_id
        && provenance.document_id == parent_identity.document_id
        && provenance.history_id == parent_identity.history_id
        && provenance.state_id == parent_state;
    if !initial_state_preserved || !identity_rekeyed || !provenance_exact {
        return Err("canonical next-issue fork invariants failed".into());
    }

    let mut next = open_mature_0x2c_editor(&source, source_hash)
        .map_err(|error| format!("open next-issue EditorSession: {error}"))?;
    next.apply_project(&fork)
        .map_err(|error| format!("apply forked project: {error}"))?;
    let story_id = replay_recipe(&mut next, &recipe, &replacement)?;
    let next_project = next.project();
    let next_state = next_project.state_id_v1();
    let recipe_state = recipe.state_id_v1();

    let recipe_operations_equal = next_project.operations == recipe.operations;
    let recipe_state_equal = next_state == recipe_state;
    let recipe_assets_equal = next_project.assets == recipe.assets;
    let fork_identity_preserved = next_project.identity == fork.identity;
    if !recipe_operations_equal
        || !recipe_state_equal
        || !recipe_assets_equal
        || !fork_identity_preserved
    {
        return Err(
            "forked next issue does not reproduce the accepted Stage-0.1 effective state".into(),
        );
    }

    let encoded_next = serde_json::to_vec_pretty(&next_project)
        .map_err(|error| format!("serialize next issue project: {error}"))?;
    write_bytes(next_project_path, &encoded_next)?;

    let mut parent_reopen = open_mature_0x2c_editor(&source, source_hash)
        .map_err(|error| format!("reopen parent EditorSession: {error}"))?;
    parent_reopen
        .apply_project(&parent)
        .map_err(|error| format!("replay parent project: {error}"))?;
    let parent_unchanged = parent_reopen.project() == parent;
    if !parent_unchanged {
        return Err("editing the fork changed the parent project".into());
    }

    let mut asset_bytes = BTreeMap::new();
    asset_bytes.insert(replacement_sha, replacement.clone());
    let mut next_reopen = open_mature_0x2c_editor(&source, source_hash)
        .map_err(|error| format!("reopen next issue EditorSession: {error}"))?;
    next_reopen
        .apply_project_with_assets(&next_project, &asset_bytes)
        .map_err(|error| format!("fresh next-issue project replay: {error}"))?;
    let next_reopen_exact = next_reopen.project() == next_project;
    if !next_reopen_exact {
        return Err("fresh next-issue replay differs from saved project".into());
    }

    let export = next_reopen
        .export_editable(EditorEditableTarget::Odg, "w2-next-issue")
        .map_err(|error| format!("next-issue ODG export: {error}"))?;
    if export.bytes.is_empty() {
        return Err("next-issue ODG export is empty".into());
    }
    write_bytes(next_odg_path, &export.bytes)?;

    let font_bytes = fs::read(font_path)
        .map_err(|error| format!("read font {}: {error}", font_path.display()))?;
    let font_sha = sha256_hex(&font_bytes);
    let font_resource_id = format!("chaptera:pinned-fixed-pdf:{font_sha}");
    let font = ExplicitDesktopFontResourceV1 {
        resource_id: &font_resource_id,
        expected_sha256: &font_sha,
        face_index: 0,
        font_size_emu: LengthEmu::new(FONT_SIZE_PT * EMU_PER_POINT),
        line_height_emu: LengthEmu::new(LINE_HEIGHT_PT * EMU_PER_POINT),
        bytes: &font_bytes,
    };
    let layout =
        build_current_story_layout_v1(&next_reopen, story_id, "w2-same-document-next-issue", &font)
            .map_err(|error| format!("next-issue authoritative layout: {error}"))?;

    let story_origin = story_id.as_canonical().to_string();
    let has_overset_diagnostic = layout.shaped_flow.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "story_overset" && diagnostic.origin.to_string() == story_origin
    });
    let consumed_scalar_end = layout
        .shaped_flow
        .lines
        .iter()
        .filter(|line| line.story_origin == story_id)
        .map(|line| line.consumed_scalar_end)
        .max()
        .unwrap_or(0);
    let layout_state = if has_overset_diagnostic {
        if consumed_scalar_end >= layout.story_scalar_len && layout.story_scalar_len != 0 {
            return Err("story_overset diagnostic conflicts with consumed Story extent".into());
        }
        "overset"
    } else if consumed_scalar_end == layout.story_scalar_len {
        "fits"
    } else {
        return Err(format!(
            "next-issue Story layout is not explicit: consumed={consumed_scalar_end} total={}",
            layout.story_scalar_len
        ));
    };

    let after_source = fs::read(source_path)
        .map_err(|error| format!("re-read source {}: {error}", source_path.display()))?;
    if after_source != source {
        return Err("immutable source PUB changed during same-document W2 proof".into());
    }

    let receipt = json!({
        "receipt_version": RECEIPT_VERSION,
        "receipt_kind": "real_hosted",
        "source": {
            "sha256": source_hex,
            "byte_len": source.len(),
            "immutable": true
        },
        "parent": {
            "operation_count": parent.operations.len(),
            "state_id": hash_id(&parent_state),
            "project_id": parent_identity.project_id,
            "document_id": parent_identity.document_id,
            "history_id": parent_identity.history_id
        },
        "fork_initial": {
            "state_id": hash_id(&fork_state),
            "project_id": fork_identity.project_id,
            "document_id": fork_identity.document_id,
            "history_id": fork_identity.history_id,
            "initial_state_preserved": initial_state_preserved,
            "identity_rekeyed": identity_rekeyed,
            "provenance_exact": provenance_exact
        },
        "next_issue": {
            "operation_count": next_project.operations.len(),
            "state_id": hash_id(&next_state),
            "recipe_state_id": hash_id(&recipe_state),
            "recipe_state_equal": recipe_state_equal,
            "recipe_operations_equal": recipe_operations_equal,
            "recipe_assets_equal": recipe_assets_equal,
            "fork_identity_preserved": fork_identity_preserved,
            "fresh_reopen_exact": next_reopen_exact,
            "project_sha256": sha256_hex(&encoded_next)
        },
        "layout": {
            "story_id": story_id,
            "state": layout_state,
            "explicit": true
        },
        "editable_output": {
            "format": "odg",
            "byte_len": export.bytes.len(),
            "sha256": sha256_hex(&export.bytes),
            "nonempty": true
        },
        "invariants": {
            "same_document": true,
            "parent_unchanged_after_next_issue_edit": parent_unchanged,
            "source_pub_immutable": true,
            "source_write_count": 0,
            "wrap_preservation_claimed": false,
            "raw_document_text_emitted": false,
            "raw_source_bytes_emitted": false,
            "replacement_asset_sha_emitted": false
        }
    });

    let encoded_receipt = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| format!("serialize same-document receipt: {error}"))?;
    let encoded_text = String::from_utf8_lossy(&encoded_receipt);
    let replacement_hex = sha256_hex(&replacement);
    if encoded_text.contains(&replacement_hex)
        || encoded_text.contains(&source_path.to_string_lossy().to_string())
        || encoded_text.contains(&replacement_path.to_string_lossy().to_string())
    {
        return Err("same-document retained receipt leaked private/local evidence".into());
    }
    write_bytes(receipt_path, &encoded_receipt)
}

fn main() {
    let args = env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let result = match args.as_slice() {
        [
            source,
            recipe,
            replacement,
            font,
            next_project,
            next_odg,
            receipt,
        ] => run(
            source,
            recipe,
            replacement,
            font,
            next_project,
            next_odg,
            receipt,
        ),
        _ => {
            eprintln!(
                "usage: w2_same_document_kernel SOURCE.pub RECIPE_PROJECT.json REPLACEMENT FONT NEXT_PROJECT.json NEXT.odg RECEIPT.json"
            );
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
