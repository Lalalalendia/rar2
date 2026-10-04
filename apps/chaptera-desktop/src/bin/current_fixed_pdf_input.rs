use chaptera_desktop_shaped_flow_runtime::{
    ExplicitDesktopFontResourceV1, build_current_fixed_pdf_resource_input_v1,
};
use pub_editor::{EditOperation, EditorProject, Sha256Digest, open_mature_0x2c_editor};
use pub_model::{EMU_PER_POINT, LengthEmu};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const PROJECT_SCHEMA: &str = "pub-editor-v0.12";
const FONT_SIZE_PT: i64 = 8;
const LINE_HEIGHT_PT: i64 = 16;

fn digest(bytes: &[u8]) -> Sha256Digest {
    let raw = Sha256::digest(bytes);
    let mut value = [0_u8; 32];
    value.copy_from_slice(&raw);
    Sha256Digest::from_bytes(value)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut out, "{byte:02x}").expect("hex into String");
    }
    out
}

#[derive(Debug)]
struct Stage0Targets {
    story_id: pub_model::StoryId,
    move_node_id: pub_model::NodeId,
    resize_node_id: pub_model::NodeId,
    replacement_node_id: pub_model::NodeId,
}

fn exact_stage0_targets(project: &EditorProject) -> Result<Stage0Targets, String> {
    if project.schema_version != PROJECT_SCHEMA {
        return Err(format!(
            "Stage-0 fixed-PDF input requires {PROJECT_SCHEMA}; found {}",
            project.schema_version
        ));
    }
    if project.operations.len() != 4 {
        return Err(format!(
            "Stage-0 fixed-PDF input requires exactly four operations; found {}",
            project.operations.len()
        ));
    }

    let mut story_id = None;
    let mut move_node_id = None;
    let mut resize_node_id = None;
    let mut replacement_node_id = None;

    for operation in &project.operations {
        match operation {
            EditOperation::ReplaceStoryRange { story_id: id, .. } if story_id.is_none() => {
                story_id = Some(*id);
            }
            EditOperation::MoveNode { node_id, .. } if move_node_id.is_none() => {
                move_node_id = Some(*node_id);
            }
            EditOperation::ResizeNode { node_id, .. } if resize_node_id.is_none() => {
                resize_node_id = Some(*node_id);
            }
            EditOperation::ReplaceImage { node_id, .. } if replacement_node_id.is_none() => {
                replacement_node_id = Some(*node_id);
            }
            _ => {
                return Err(
                    "Stage-0 fixed-PDF input operation family is not exactly Story/Move/Resize/ReplaceImage"
                        .into(),
                );
            }
        }
    }

    let targets = Stage0Targets {
        story_id: story_id.ok_or_else(|| "Stage-0 Story operation missing".to_owned())?,
        move_node_id: move_node_id.ok_or_else(|| "Stage-0 MoveNode missing".to_owned())?,
        resize_node_id: resize_node_id.ok_or_else(|| "Stage-0 ResizeNode missing".to_owned())?,
        replacement_node_id: replacement_node_id
            .ok_or_else(|| "Stage-0 ReplaceImage missing".to_owned())?,
    };
    if targets.move_node_id == targets.resize_node_id
        || targets.move_node_id == targets.replacement_node_id
        || targets.resize_node_id == targets.replacement_node_id
    {
        return Err("Stage-0 object mutation targets must be distinct".into());
    }
    Ok(targets)
}

fn read_project(path: &Path) -> Result<(Vec<u8>, EditorProject), String> {
    let raw = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let project = serde_json::from_slice(&raw)
        .map_err(|error| format!("parse EditorProject {}: {error}", path.display()))?;
    Ok((raw, project))
}

fn run(
    source_path: &Path,
    project_path: &Path,
    replacement_path: &Path,
    font_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let source = fs::read(source_path)
        .map_err(|error| format!("read source {}: {error}", source_path.display()))?;
    let source_hash = digest(&source);
    let source_hex = sha256_hex(&source);

    let (_project_raw, project) = read_project(project_path)?;
    let targets = exact_stage0_targets(&project)?;
    if project.source_hash != source_hash {
        return Err("EditorProject source identity differs from immutable PUB".into());
    }
    if project.assets.len() != 1 {
        return Err(format!(
            "Stage-0 fixed-PDF input requires exactly one operation-reachable asset; found {}",
            project.assets.len()
        ));
    }

    let replacement = fs::read(replacement_path).map_err(|error| {
        format!(
            "read replacement asset {}: {error}",
            replacement_path.display()
        )
    })?;
    let asset = &project.assets[0];
    let mut asset_bytes = BTreeMap::new();
    asset_bytes.insert(asset.sha256, replacement);

    let mut editor = open_mature_0x2c_editor(&source, source_hash)
        .map_err(|error| format!("open fresh EditorSession: {error}"))?;
    editor
        .apply_project_with_assets(&project, &asset_bytes)
        .map_err(|error| format!("fresh EditorProject replay failed: {error}"))?;

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

    let project_state_id = project.state_id_v1();
    let binding = json!({
        "protocol_version": "chaptera.editor-fixed-pdf-binding.v1",
        "source_hash": source_hex,
        "project_schema_version": project.schema_version,
        "project_state_id": project_state_id,
        "story_id": targets.story_id,
        "move_node_id": targets.move_node_id,
        "resize_node_id": targets.resize_node_id,
        "replacement_node_id": targets.replacement_node_id,
    });
    let input =
        build_current_fixed_pdf_resource_input_v1(&editor, targets.story_id, binding, &font)
            .map_err(|error| format!("build current fixed-PDF resource input: {error}"))?;

    let after = fs::read(source_path)
        .map_err(|error| format!("re-read source {}: {error}", source_path.display()))?;
    if after != source {
        return Err("immutable source PUB changed during current-state assembly".into());
    }

    if let Some(parent) = output_path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let encoded = serde_json::to_vec_pretty(&input)
        .map_err(|error| format!("serialize current fixed-PDF resource input: {error}"))?;
    fs::write(output_path, encoded)
        .map_err(|error| format!("write {}: {error}", output_path.display()))?;
    Ok(())
}

fn main() {
    let args = env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if args.len() != 5 {
        eprintln!(
            "usage: chaptera-current-fixed-pdf-input SOURCE.pub PROJECT.json REPLACEMENT FONT OUTPUT.json"
        );
        std::process::exit(2);
    }
    if let Err(error) = run(&args[0], &args[1], &args[2], &args[3], &args[4]) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
