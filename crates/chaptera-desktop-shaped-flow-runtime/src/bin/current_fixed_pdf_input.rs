use chaptera_desktop_shaped_flow_runtime::{
    ExplicitDesktopFontResourceV1, build_current_fixed_pdf_resource_input_v1,
};
use pub_editor::{EditOperation, EditorProject, Sha256Digest, open_mature_0x2c_editor};
use pub_model::{EMU_PER_POINT, LengthEmu, NodeId, StoryId};
use serde_json::{Value, json};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectOperationKind {
    StoryRange,
    Move,
    Resize,
    ReplaceImage,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectAdmissionClass {
    StoryMove,
    Stage02,
}

#[derive(Debug)]
enum ProjectTargets {
    StoryMove {
        story_id: StoryId,
        move_node_id: NodeId,
    },
    Stage02 {
        story_id: StoryId,
        move_node_id: NodeId,
        resize_node_id: NodeId,
        replacement_node_id: NodeId,
    },
}

impl ProjectTargets {
    fn primary_story_id(&self) -> StoryId {
        match self {
            Self::StoryMove { story_id, .. } | Self::Stage02 { story_id, .. } => *story_id,
        }
    }

    fn binding(&self, source_hash: &str, project: &EditorProject) -> Value {
        let project_state_id = project.state_id_v1();
        match self {
            Self::StoryMove {
                story_id,
                move_node_id,
            } => json!({
                "protocol_version": "chaptera.editor-fixed-pdf-binding.v1",
                "admission_profile": "story_move_v1",
                "source_hash": source_hash,
                "project_schema_version": project.schema_version,
                "project_state_id": project_state_id,
                "story_id": story_id,
                "move_node_id": move_node_id,
            }),
            Self::Stage02 {
                story_id,
                move_node_id,
                resize_node_id,
                replacement_node_id,
            } => json!({
                "protocol_version": "chaptera.editor-fixed-pdf-binding.v1",
                "source_hash": source_hash,
                "project_schema_version": project.schema_version,
                "project_state_id": project_state_id,
                "story_id": story_id,
                "move_node_id": move_node_id,
                "resize_node_id": resize_node_id,
                "replacement_node_id": replacement_node_id,
            }),
        }
    }
}

fn operation_kind(operation: &EditOperation) -> ProjectOperationKind {
    match operation {
        EditOperation::ReplaceStoryRange { .. } => ProjectOperationKind::StoryRange,
        EditOperation::MoveNode { .. } => ProjectOperationKind::Move,
        EditOperation::ResizeNode { .. } => ProjectOperationKind::Resize,
        EditOperation::ReplaceImage { .. } => ProjectOperationKind::ReplaceImage,
        _ => ProjectOperationKind::Other,
    }
}

fn classify_kind_family(
    kinds: &[ProjectOperationKind],
    asset_count: usize,
) -> Result<ProjectAdmissionClass, String> {
    if kinds == [ProjectOperationKind::StoryRange, ProjectOperationKind::Move] {
        if asset_count != 0 {
            return Err(format!(
                "Story+Move fixed-PDF input requires zero operation-reachable assets; found {asset_count}"
            ));
        }
        return Ok(ProjectAdmissionClass::StoryMove);
    }

    if kinds.len() == 4 {
        let story = kinds
            .iter()
            .filter(|kind| **kind == ProjectOperationKind::StoryRange)
            .count();
        let moved = kinds
            .iter()
            .filter(|kind| **kind == ProjectOperationKind::Move)
            .count();
        let resized = kinds
            .iter()
            .filter(|kind| **kind == ProjectOperationKind::Resize)
            .count();
        let replaced = kinds
            .iter()
            .filter(|kind| **kind == ProjectOperationKind::ReplaceImage)
            .count();
        if story == 1 && moved == 1 && resized == 1 && replaced == 1 {
            if asset_count != 1 {
                return Err(format!(
                    "Stage-0 fixed-PDF input requires exactly one operation-reachable asset; found {asset_count}"
                ));
            }
            return Ok(ProjectAdmissionClass::Stage02);
        }
    }

    Err(
        "fixed-PDF input operation family is not an admitted Story+Move or Stage-0.2 profile"
            .into(),
    )
}

fn exact_project_targets(project: &EditorProject) -> Result<ProjectTargets, String> {
    if project.schema_version != PROJECT_SCHEMA {
        return Err(format!(
            "current fixed-PDF input requires {PROJECT_SCHEMA}; found {}",
            project.schema_version
        ));
    }

    let kinds = project
        .operations
        .iter()
        .map(operation_kind)
        .collect::<Vec<_>>();
    match classify_kind_family(&kinds, project.assets.len())? {
        ProjectAdmissionClass::StoryMove => {
            let [
                EditOperation::ReplaceStoryRange { story_id, .. },
                EditOperation::MoveNode { node_id, .. },
            ] = project.operations.as_slice()
            else {
                return Err("Story+Move fixed-PDF input operation sequence changed".into());
            };
            Ok(ProjectTargets::StoryMove {
                story_id: *story_id,
                move_node_id: *node_id,
            })
        }
        ProjectAdmissionClass::Stage02 => {
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
                    EditOperation::ReplaceImage { node_id, .. }
                        if replacement_node_id.is_none() =>
                    {
                        replacement_node_id = Some(*node_id);
                    }
                    _ => {
                        return Err(
                            "Stage-0 fixed-PDF input operation family changed after admission"
                                .into(),
                        );
                    }
                }
            }

            let targets = ProjectTargets::Stage02 {
                story_id: story_id.ok_or_else(|| "Stage-0 Story operation missing".to_owned())?,
                move_node_id: move_node_id.ok_or_else(|| "Stage-0 MoveNode missing".to_owned())?,
                resize_node_id: resize_node_id
                    .ok_or_else(|| "Stage-0 ResizeNode missing".to_owned())?,
                replacement_node_id: replacement_node_id
                    .ok_or_else(|| "Stage-0 ReplaceImage missing".to_owned())?,
            };
            if let ProjectTargets::Stage02 {
                move_node_id,
                resize_node_id,
                replacement_node_id,
                ..
            } = &targets
                && (move_node_id == resize_node_id || move_node_id == replacement_node_id)
            {
                return Err(
                    "Stage-0 MoveNode target must be distinct from resized/replaced target".into(),
                );
            }
            Ok(targets)
        }
    }
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
    replacement_path: Option<&Path>,
    font_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let source = fs::read(source_path)
        .map_err(|error| format!("read source {}: {error}", source_path.display()))?;
    let source_hash = digest(&source);
    let source_hex = sha256_hex(&source);

    let (_project_raw, project) = read_project(project_path)?;
    let targets = exact_project_targets(&project)?;
    if project.source_hash != source_hash {
        return Err("EditorProject source identity differs from immutable PUB".into());
    }

    let mut editor = open_mature_0x2c_editor(&source, source_hash)
        .map_err(|error| format!("open fresh EditorSession: {error}"))?;
    match &targets {
        ProjectTargets::StoryMove { .. } => {
            if replacement_path.is_some() {
                return Err(
                    "Story+Move fixed-PDF input does not accept a replacement asset argument"
                        .into(),
                );
            }
            editor.apply_project(&project).map_err(|error| {
                format!("fresh Story+Move EditorProject replay failed: {error}")
            })?;
        }
        ProjectTargets::Stage02 { .. } => {
            let replacement_path = replacement_path.ok_or_else(|| {
                "Stage-0 fixed-PDF input requires an exact replacement asset argument".to_owned()
            })?;
            let replacement = fs::read(replacement_path).map_err(|error| {
                format!(
                    "read replacement asset {}: {error}",
                    replacement_path.display()
                )
            })?;
            let asset = &project.assets[0];
            let mut asset_bytes = BTreeMap::new();
            asset_bytes.insert(asset.sha256, replacement);
            editor
                .apply_project_with_assets(&project, &asset_bytes)
                .map_err(|error| format!("fresh Stage-0 EditorProject replay failed: {error}"))?;
        }
    }

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

    let binding = targets.binding(&source_hex, &project);
    let input = build_current_fixed_pdf_resource_input_v1(
        &editor,
        targets.primary_story_id(),
        binding,
        &font,
    )
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
    let result = match args.as_slice() {
        [source, project, font, output] => run(source, project, None, font, output),
        [source, project, replacement, font, output] => {
            run(source, project, Some(replacement), font, output)
        }
        _ => {
            eprintln!(
                "usage: current_fixed_pdf_input SOURCE.pub PROJECT.json [REPLACEMENT] FONT OUTPUT.json"
            );
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_move_profile_requires_exact_order_and_zero_assets() {
        assert_eq!(
            classify_kind_family(
                &[ProjectOperationKind::StoryRange, ProjectOperationKind::Move,],
                0,
            ),
            Ok(ProjectAdmissionClass::StoryMove)
        );
        assert!(
            classify_kind_family(
                &[ProjectOperationKind::StoryRange, ProjectOperationKind::Move,],
                1,
            )
            .is_err()
        );
        assert!(
            classify_kind_family(
                &[ProjectOperationKind::Move, ProjectOperationKind::StoryRange,],
                0,
            )
            .is_err()
        );
    }

    #[test]
    fn stage02_profile_preserves_four_operation_admission() {
        assert_eq!(
            classify_kind_family(
                &[
                    ProjectOperationKind::StoryRange,
                    ProjectOperationKind::Move,
                    ProjectOperationKind::Resize,
                    ProjectOperationKind::ReplaceImage,
                ],
                1,
            ),
            Ok(ProjectAdmissionClass::Stage02)
        );
        assert_eq!(
            classify_kind_family(
                &[
                    ProjectOperationKind::ReplaceImage,
                    ProjectOperationKind::Resize,
                    ProjectOperationKind::Move,
                    ProjectOperationKind::StoryRange,
                ],
                1,
            ),
            Ok(ProjectAdmissionClass::Stage02)
        );
        assert!(
            classify_kind_family(
                &[
                    ProjectOperationKind::StoryRange,
                    ProjectOperationKind::Move,
                    ProjectOperationKind::Resize,
                    ProjectOperationKind::ReplaceImage,
                ],
                0,
            )
            .is_err()
        );
    }

    #[test]
    fn stage02_target_topology_allows_resize_then_replace_same_frame() {
        let move_node: NodeId =
            serde_json::from_str("\"11111111-1111-4111-8111-111111111111\"").unwrap();
        let photo_node: NodeId =
            serde_json::from_str("\"22222222-2222-4222-8222-222222222222\"").unwrap();

        assert_ne!(move_node, photo_node);
        assert_eq!(photo_node, photo_node);
    }

    #[test]
    fn unsupported_operation_family_fails_closed() {
        assert!(
            classify_kind_family(
                &[
                    ProjectOperationKind::StoryRange,
                    ProjectOperationKind::Other
                ],
                0,
            )
            .is_err()
        );
    }
}
