use chaptera_scene_instance::{
    GeometrySyncPolicyV1, ObjectMutationKindV1, SceneInstanceV1, SceneProjectionKindV1,
    admit_object_mutation_v1, geometry_sync_policy_v1,
};
use pub_editor::{
    EditOperation, EditorProject, EditorSession, ImageCropStateV1, LengthEmu, NodeId, RectEmu,
    Sha256Digest,
};
use pub_interaction::{DocumentPoint, ResizeHandle, ResizeTransaction, ResizeUpdate};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use super::acceptance::{
    REPLACEMENT_WITNESS, direct_instance, explicit_loss_flags, export_target, rect_json,
    select_move, select_story_edit, sha256_hex, state_id, story_state,
};

const PROTOCOL_VERSION: &str = "chaptera.editor-desktop-continuity-observation.v2";

fn select_resize(
    editor: &EditorSession,
    visual: &pub_viewer::ViewerGeometryDocument,
    excluded: &[NodeId],
) -> Option<(SceneInstanceV1, NodeId, RectEmu, ResizeTransaction)> {
    const DELTAS: &[(i64, i64)] = &[(127_000, 127_000), (254_000, 127_000), (127_000, 254_000)];

    for page in &visual.document.pages {
        let page_origin = page.id.into_canonical();
        let target_page_id = page.id.as_canonical().to_string();
        for scene_node in visual
            .scene
            .nodes
            .iter()
            .filter(|node| node.parent_origin == page_origin)
        {
            if excluded.contains(&scene_node.origin) {
                continue;
            }
            let Some(instance) = direct_instance(editor, &target_page_id, scene_node.origin) else {
                continue;
            };
            let admission = admit_object_mutation_v1(&instance, ObjectMutationKindV1::ResizeNode);
            let origin_node_id = scene_node.origin.as_canonical().to_string();
            if !admission.admitted
                || admission.origin_node_id.as_deref() != Some(origin_node_id.as_str())
                || geometry_sync_policy_v1(&instance)
                    != GeometrySyncPolicyV1::ApplyAuthoredOriginGeometry
                || editor.can_resize_node(scene_node.origin).is_err()
            {
                continue;
            }

            let before = editor.graph().nodes.get(&scene_node.origin)?.header.bounds;
            for &(dx, dy) in DELTAS {
                let Ok(mut resize) = ResizeTransaction::begin(
                    scene_node.origin,
                    before,
                    ResizeHandle::BottomRight,
                    DocumentPoint::new(LengthEmu::ZERO, LengthEmu::ZERO),
                ) else {
                    continue;
                };
                let Ok(ResizeUpdate::Preview(after)) =
                    resize.update(DocumentPoint::new(LengthEmu::new(dx), LengthEmu::new(dy)))
                else {
                    continue;
                };
                if editor.can_resize_node_to(scene_node.origin, after).is_ok() {
                    return Some((instance, scene_node.origin, before, resize));
                }
            }
        }
    }
    None
}

fn select_replace_image(
    editor: &EditorSession,
    visual: &pub_viewer::ViewerGeometryDocument,
    replacement_asset: Sha256Digest,
    excluded: &[NodeId],
    require_explicit_crop: bool,
) -> Option<(SceneInstanceV1, NodeId, RectEmu, Option<ImageCropStateV1>)> {
    for page in &visual.document.pages {
        let page_origin = page.id.into_canonical();
        let target_page_id = page.id.as_canonical().to_string();
        for scene_node in visual
            .scene
            .nodes
            .iter()
            .filter(|node| node.parent_origin == page_origin)
        {
            if excluded.contains(&scene_node.origin) {
                continue;
            }
            let Some(instance) = direct_instance(editor, &target_page_id, scene_node.origin) else {
                continue;
            };
            let admission = admit_object_mutation_v1(&instance, ObjectMutationKindV1::ReplaceImage);
            let origin_node_id = scene_node.origin.as_canonical().to_string();
            if !admission.admitted
                || admission.origin_node_id.as_deref() != Some(origin_node_id.as_str())
                || editor
                    .can_replace_image(scene_node.origin, replacement_asset)
                    .is_err()
            {
                continue;
            }
            let crop = editor.image_crop_for(scene_node.origin);
            if require_explicit_crop && crop.is_none() {
                continue;
            }
            let frame = editor.graph().nodes.get(&scene_node.origin)?.header.bounds;
            return Some((instance, scene_node.origin, frame, crop));
        }
    }
    None
}

fn projected_mutation_is_denied(direct: &SceneInstanceV1, mutation: ObjectMutationKindV1) -> bool {
    let projected = SceneInstanceV1 {
        schema_version: direct.schema_version.clone(),
        instance_id: direct.instance_id.clone(),
        projection_kind: SceneProjectionKindV1::InheritedMaster,
        origin_node_id: direct.origin_node_id.clone(),
        target_page_id: direct.target_page_id.clone(),
        source_parent_origin: direct.source_parent_origin.clone(),
        story_authority_id: None,
        cmo_slot_index: None,
        cmo_scalar_index: None,
    };
    let decision = admit_object_mutation_v1(&projected, mutation);
    !decision.admitted
        && decision.origin_node_id.is_none()
        && geometry_sync_policy_v1(&projected) == GeometrySyncPolicyV1::ReprojectFromContext
}

fn replacement_mime(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok("image/jpeg")
    } else {
        Err("V2 replacement witness must be signature-valid PNG or JPEG".to_owned())
    }
}

fn count_operations(project: &EditorProject) -> (usize, usize, usize, usize) {
    let mut story = 0;
    let mut moved = 0;
    let mut resized = 0;
    let mut image = 0;
    for operation in &project.operations {
        match operation {
            EditOperation::ReplaceStoryRange { .. } => story += 1,
            EditOperation::MoveNode { .. } => moved += 1,
            EditOperation::ResizeNode { .. } => resized += 1,
            EditOperation::ReplaceImage { .. } => image += 1,
            _ => {}
        }
    }
    (story, moved, resized, image)
}

pub fn run(
    fixture: &Path,
    replacement_path: &Path,
    project_path: &Path,
    export_path: &Path,
) -> Result<Value, String> {
    if env::var("CHAPTERA_DESKTOP_CONTINUITY_V2").as_deref() != Ok("1") {
        return Err("CHAPTERA_DESKTOP_CONTINUITY_V2=1 is required".to_owned());
    }
    let rar_commit = env::var("CHAPTERA_RAR_COMMIT")
        .map_err(|_| "CHAPTERA_RAR_COMMIT is required".to_owned())?;
    let expected_source_hash = env::var("CHAPTERA_SOURCE_HASH")
        .map_err(|_| "CHAPTERA_SOURCE_HASH is required".to_owned())?;
    let replacement_binding_id = env::var("CHAPTERA_REPLACEMENT_BINDING_ID")
        .map_err(|_| "CHAPTERA_REPLACEMENT_BINDING_ID is required".to_owned())?;
    let require_explicit_crop =
        env::var("CHAPTERA_CONTINUITY_REQUIRE_EXPLICIT_CROP").as_deref() == Ok("1");
    if !replacement_binding_id.starts_with("continuity-v2-")
        || replacement_binding_id.len() != "continuity-v2-".len() + 32
        || !replacement_binding_id["continuity-v2-".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(
            "CHAPTERA_REPLACEMENT_BINDING_ID must be continuity-v2- + 32 lowercase hex chars"
                .to_owned(),
        );
    }

    let source_before =
        fs::read(fixture).map_err(|error| format!("read {}: {error}", fixture.display()))?;
    if sha256_hex(&source_before) != expected_source_hash {
        return Err("fixture SHA-256 differs from CHAPTERA_SOURCE_HASH".to_owned());
    }
    let replacement_bytes = fs::read(replacement_path)
        .map_err(|error| format!("read {}: {error}", replacement_path.display()))?;
    let replacement_mime = replacement_mime(&replacement_bytes)?;

    let visual = pub_viewer::open_mature_0x2c_geometry(
        &source_before,
        pub_viewer::viewer_geometry_environment_v0_1(),
    )
    .map_err(|error| format!("open Viewer geometry: {error:#}"))?;
    if visual.document.source.source_hash.to_string() != expected_source_hash {
        return Err("Viewer source identity differs from acceptance binding".to_owned());
    }

    let source_hash = visual.document.source.source_hash;
    let mut editor = pub_editor::open_mature_0x2c_editor(&source_before, source_hash)
        .map_err(|error| format!("open EditorSession: {error}"))?;
    let replacement_asset = editor
        .import_replacement_asset(replacement_mime, replacement_bytes.clone())
        .map_err(|error| format!("import replacement asset: {error}"))?;

    let (story_id, start_scalar, expected_before) = select_story_edit(&editor)
        .ok_or_else(|| "no capability-approved ordinary Story".to_owned())?;
    let end_scalar = start_scalar
        .checked_add(1)
        .ok_or_else(|| "Story scalar range overflow".to_owned())?;
    let story_operation = editor
        .replace_story_range(
            story_id,
            start_scalar,
            end_scalar,
            expected_before,
            REPLACEMENT_WITNESS,
        )
        .map_err(|error| format!("replace Story range: {error}"))?;
    let (before_story_state_id, after_story_state_id) = match &story_operation {
        EditOperation::ReplaceStoryRange {
            before_story_state_id,
            after_story_state_id,
            ..
        } => (before_story_state_id.clone(), after_story_state_id.clone()),
        _ => return Err("EditorSession did not emit ReplaceStoryRange".to_owned()),
    };
    let after_story_state_id_effective = state_id(&editor)?;

    let operations_before_drag = editor.operations().len();
    let (move_instance, moved_node_id, before_move, drag) = select_move(&editor, &visual)
        .ok_or_else(|| "no admitted direct page-local MoveNode target".to_owned())?;
    let after_move = drag.preview_bounds();
    if editor.operations().len() != operations_before_drag {
        return Err("transient drag emitted a durable editor operation".to_owned());
    }
    editor
        .move_node_to(moved_node_id, after_move.x, after_move.y)
        .map_err(|error| format!("commit MoveNode: {error}"))?;
    if editor.operations().len() != operations_before_drag + 1 {
        return Err("drag release did not emit exactly one durable MoveNode".to_owned());
    }
    let after_move_state_id = state_id(&editor)?;

    let operations_before_resize = editor.operations().len();
    let (resize_instance, resized_node_id, before_resize, mut resize) =
        select_resize(&editor, &visual, &[moved_node_id])
            .ok_or_else(|| "no distinct admitted direct page-local ResizeNode target".to_owned())?;
    let resize_commit = resize
        .commit()
        .map_err(|error| format!("commit transient resize transaction: {error}"))?;
    if editor.operations().len() != operations_before_resize {
        return Err("transient resize emitted a durable editor operation".to_owned());
    }
    editor
        .resize_node_to(resized_node_id, resize_commit.after)
        .map_err(|error| format!("commit ResizeNode: {error}"))?;
    if editor.operations().len() != operations_before_resize + 1 {
        return Err("resize release did not emit exactly one durable ResizeNode".to_owned());
    }
    let after_resize_state_id = state_id(&editor)?;

    let (replace_instance, replaced_node_id, replace_frame, replace_crop_before) =
        select_replace_image(
        &editor,
        &visual,
        replacement_asset,
        &[moved_node_id, resized_node_id],
        require_explicit_crop,
    )
    .ok_or_else(|| {
        if require_explicit_crop {
            "no distinct admitted direct page-local ReplaceImage target with explicit source crop"
                .to_owned()
        } else {
            "no distinct admitted direct page-local ReplaceImage target".to_owned()
        }
    })?;
    let before_asset = editor.image_replacement_for(replaced_node_id);
    let operations_before_replace = editor.operations().len();
    editor
        .replace_image(replaced_node_id, replacement_asset)
        .map_err(|error| format!("commit ReplaceImage: {error}"))?;
    if editor.operations().len() != operations_before_replace + 1 {
        return Err("ReplaceImage did not emit exactly one durable operation".to_owned());
    }
    let replace_frame_after = editor
        .graph()
        .nodes
        .get(&replaced_node_id)
        .ok_or_else(|| "replaced image node disappeared".to_owned())?
        .header
        .bounds;
    if replace_frame_after != replace_frame {
        return Err("ReplaceImage changed frame geometry".to_owned());
    }
    let replace_crop_after = editor.image_crop_for(replaced_node_id);
    if replace_crop_after != replace_crop_before {
        return Err("ReplaceImage changed source crop state".to_owned());
    }
    let after_replace_state_id = state_id(&editor)?;
    let story_state_after_replace = story_state(&editor, story_id)?;

    editor
        .undo()
        .map_err(|error| format!("undo ReplaceImage: {error}"))?;
    let undo_replace_state_id = state_id(&editor)?;
    if editor.image_replacement_for(replaced_node_id) != before_asset {
        return Err("undo ReplaceImage did not restore prior replacement identity".to_owned());
    }

    editor
        .redo()
        .map_err(|error| format!("redo ReplaceImage: {error}"))?;
    let redo_replace_state_id = state_id(&editor)?;
    if editor.image_replacement_for(replaced_node_id) != Some(replacement_asset) {
        return Err("redo ReplaceImage did not restore replacement identity".to_owned());
    }

    let project = editor.project();
    let (story_count, move_count, resize_count, image_count) = count_operations(&project);
    if project.operations.len() != 4
        || (story_count, move_count, resize_count, image_count) != (1, 1, 1, 1)
    {
        return Err(
            "V2 project must contain exactly Story + Move + Resize + ReplaceImage".to_owned(),
        );
    }
    let project_bytes = serde_json::to_vec_pretty(&project)
        .map_err(|error| format!("serialize EditorProject: {error}"))?;
    fs::write(project_path, &project_bytes)
        .map_err(|error| format!("write {}: {error}", project_path.display()))?;

    let persisted: EditorProject = serde_json::from_slice(&project_bytes)
        .map_err(|error| format!("parse persisted EditorProject: {error}"))?;
    let mut reopened = pub_editor::open_mature_0x2c_editor(&source_before, source_hash)
        .map_err(|error| format!("open fresh EditorSession: {error}"))?;
    let mut asset_bytes = BTreeMap::new();
    asset_bytes.insert(replacement_asset, replacement_bytes.clone());
    reopened
        .apply_project_with_assets(&persisted, &asset_bytes)
        .map_err(|error| format!("replay persisted EditorProject + assets: {error}"))?;

    let reopened_state_id = state_id(&reopened)?;
    let story_state_reopened = story_state(&reopened, story_id)?;
    let reopened_move = reopened
        .graph()
        .nodes
        .get(&moved_node_id)
        .ok_or_else(|| "moved node disappeared after reopen".to_owned())?
        .header
        .bounds;
    let reopened_resize = reopened
        .graph()
        .nodes
        .get(&resized_node_id)
        .ok_or_else(|| "resized node disappeared after reopen".to_owned())?
        .header
        .bounds;
    let reopened_asset = reopened.image_replacement_for(replaced_node_id);
    let reopened_crop = reopened.image_crop_for(replaced_node_id);

    if reopened_state_id != after_replace_state_id
        || story_state_reopened != after_story_state_id
        || reopened_move != after_move
        || reopened_resize != resize_commit.after
        || reopened_asset != Some(replacement_asset)
        || reopened_crop != replace_crop_before
    {
        return Err("fresh reopen did not reproduce the full accepted V2 state".to_owned());
    }

    let target = export_target(export_path)?;
    let preview = reopened
        .preview_editable_export(target, "desktop-continuity-v2.pub")
        .map_err(|error| format!("preview edited export: {error}"))?;
    let (blocking_loss_count, approximations_explicit, unsupported_explicit) =
        explicit_loss_flags(&preview.report)?;
    if !preview.report.can_serialize || blocking_loss_count != 0 {
        return Err("V2 editable export is blocked by capability/loss preview".to_owned());
    }
    let export = reopened
        .export_editable(target, "desktop-continuity-v2.pub")
        .map_err(|error| format!("export edited package: {error}"))?;
    fs::write(export_path, &export.bytes)
        .map_err(|error| format!("write {}: {error}", export_path.display()))?;

    let source_after =
        fs::read(fixture).map_err(|error| format!("re-read {}: {error}", fixture.display()))?;
    if source_before != source_after || sha256_hex(&source_after) != expected_source_hash {
        return Err("source PUB changed during V2 desktop acceptance".to_owned());
    }

    let projected_denied =
        projected_mutation_is_denied(&move_instance, ObjectMutationKindV1::MoveNode)
            && projected_mutation_is_denied(&resize_instance, ObjectMutationKindV1::ResizeNode)
            && projected_mutation_is_denied(&replace_instance, ObjectMutationKindV1::ReplaceImage);
    if !projected_denied {
        return Err("projected SceneInstance unexpectedly admits a V2 object mutation".to_owned());
    }

    Ok(json!({
        "protocol_version": PROTOCOL_VERSION,
        "source_hash": expected_source_hash,
        "rar_commit": rar_commit,
        "story_edit": {
            "story_id": story_id.as_canonical().to_string(),
            "operation_kind": "replace_story_range",
            "capability_admitted": true,
            "before_state_id": before_story_state_id,
            "after_state_id": after_story_state_id,
        },
        "object_move": {
            "instance_id": move_instance.instance_id,
            "projection_kind": "direct_page_local",
            "origin_node_id": moved_node_id.as_canonical().to_string(),
            "capability_admitted": true,
            "geometry_sync_policy": "apply_authored_origin_geometry",
            "before": rect_json(before_move),
            "after": rect_json(after_move),
            "durable_move_count": 1,
            "transient_geometry_operation_count": 0,
        },
        "object_resize": {
            "instance_id": resize_instance.instance_id,
            "projection_kind": "direct_page_local",
            "origin_node_id": resized_node_id.as_canonical().to_string(),
            "capability_admitted": true,
            "geometry_sync_policy": "apply_authored_origin_geometry",
            "before": rect_json(before_resize),
            "after": rect_json(resize_commit.after),
            "durable_resize_count": 1,
            "transient_geometry_operation_count": 0,
        },
        "image_replace": {
            "instance_id": replace_instance.instance_id,
            "projection_kind": "direct_page_local",
            "origin_node_id": replaced_node_id.as_canonical().to_string(),
            "capability_admitted": true,
            "replacement_binding_id": replacement_binding_id,
            "replacement_binding_content_derived": false,
            "asset_sha_redacted": true,
            "after_asset_mime": replacement_mime,
            "after_asset_byte_len": replacement_bytes.len(),
            "frame_before": rect_json(replace_frame),
            "frame_after": rect_json(replace_frame_after),
            "explicit_crop_present": replace_crop_before.is_some(),
            "durable_replace_count": 1,
        },
        "history": {
            "after_story_state_id": after_story_state_id_effective,
            "after_move_state_id": after_move_state_id,
            "after_resize_state_id": after_resize_state_id,
            "after_replace_state_id": after_replace_state_id,
            "undo_replace_state_id": undo_replace_state_id,
            "redo_replace_state_id": redo_replace_state_id,
        },
        "reopen": {
            "fresh_session": true,
            "state_id": reopened_state_id,
            "story_state_id": story_state_reopened,
            "moved_rect": rect_json(reopened_move),
            "resized_rect": rect_json(reopened_resize),
            "replacement_binding_preserved": reopened_asset == Some(replacement_asset),
        },
        "project": {
            "schema_version": persisted.schema_version,
            "operation_count": persisted.operations.len(),
            "story_operation_count": story_count,
            "move_operation_count": move_count,
            "resize_operation_count": resize_count,
            "replace_image_operation_count": image_count,
        },
        "capability_loss": {
            "observed_before_export": true,
            "blocking_loss_count": blocking_loss_count,
            "approximations_explicit": approximations_explicit,
            "unsupported_partial_semantics_explicit": unsupported_explicit,
        },
        "export": {
            "format": target.extension(),
            "edited_story_present": story_state_after_replace == after_story_state_id,
            "moved_geometry_present": reopened_move == after_move,
            "resized_geometry_present": reopened_resize == resize_commit.after,
            "replacement_image_present": reopened_asset == Some(replacement_asset),
        },
        "invariants": {
            "native_pub_write_used": false,
            "no_hidden_network_upload": true,
            "direct_page_local_gate_used": true,
            "projected_object_mutation_fails_closed": projected_denied,
            "reopen_used_fresh_session": true,
            "export_from_current_editor_state": true,
            "replacement_asset_sha_emitted": false,
        },
    }))
}
