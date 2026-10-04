use chaptera_scene_instance::{
    GeometrySyncPolicyV1, direct_page_local_instance_v1, geometry_sync_policy_v1,
};
use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, PageRenderPlanV1, RenderTextLayoutDispositionV1,
    build_page_render_plan_with_text_layout_v1,
};
use pub_editor::{
    EditOperation, EditorProject, EditorSession, Sha256Digest, open_mature_0x2c_editor,
};
use pub_layout::font_fingerprint_sha256;
use pub_model::{EMU_PER_POINT, NodeId, RectEmu, ResourceId, StoryId};
use pub_viewer::{
    ViewerGeometryDocument, open_mature_0x2c_geometry, viewer_geometry_environment_v0_1,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const PROTOCOL_VERSION: &str = "chaptera.current-viewer-fixed-pdf-input.v1";
const PROJECT_SCHEMA: &str = "pub-editor-v0.12";
const FONT_SIZE_PT: i64 = 8;
const LINE_HEIGHT_PT: i64 = 16;

#[derive(Debug, Clone, Copy)]
struct StoryMoveTargets {
    story_id: StoryId,
    move_node_id: NodeId,
    move_after: RectEmu,
}

#[derive(Debug, Serialize)]
struct CurrentViewerImageAssetV1 {
    resource_id: ResourceId,
    mime: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct CurrentViewerFontResourceV1 {
    resource_id: String,
    fingerprint_sha256: String,
    face_index: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, Default, Serialize)]
struct CurrentViewerPlanCensusV1 {
    page_count: usize,
    node_count: usize,
    projected_instance_count: usize,
    duplicate_node_id_count: usize,
    shared_resolved_text_node_count: usize,
    backend_fallback_text_node_count: usize,
    shaped_line_count: usize,
    shaped_span_count: usize,
    missing_shaping_evidence_count: usize,
    table_node_count: usize,
    image_resource_count: usize,
    image_node_count: usize,
    image_source_window_count: usize,
    solid_paint_node_count: usize,
    decorative_border_node_count: usize,
    non_identity_transform_node_count: usize,
    text_node_count: usize,
    missing_text_layout_count: usize,
}

#[derive(Debug, Serialize)]
struct CurrentViewerFixedPdfInputV1 {
    protocol_version: &'static str,
    binding: Value,
    pages: Vec<PageRenderPlanV1>,
    images: Vec<CurrentViewerImageAssetV1>,
    font: CurrentViewerFontResourceV1,
    census: CurrentViewerPlanCensusV1,
}

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

fn exact_story_move_targets(project: &EditorProject) -> Result<StoryMoveTargets, String> {
    if project.schema_version != PROJECT_SCHEMA {
        return Err(format!(
            "current Viewer fixed-PDF input requires {PROJECT_SCHEMA}; found {}",
            project.schema_version
        ));
    }
    if !project.assets.is_empty() {
        return Err(format!(
            "current Viewer Story+Move input requires zero assets; found {}",
            project.assets.len()
        ));
    }
    let [
        EditOperation::ReplaceStoryRange { story_id, .. },
        EditOperation::MoveNode { node_id, after, .. },
    ] = project.operations.as_slice()
    else {
        return Err(
            "current Viewer fixed-PDF input requires exact ordered Story+Move operations".into(),
        );
    };
    Ok(StoryMoveTargets {
        story_id: *story_id,
        move_node_id: *node_id,
        move_after: *after,
    })
}

fn synchronize_direct_move(
    visual: &mut ViewerGeometryDocument,
    editor: &EditorSession,
    target: StoryMoveTargets,
) -> Result<(), String> {
    let matching = visual
        .scene
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.origin == target.move_node_id)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(format!(
            "current Viewer MoveNode target must occur exactly once in direct scene; found {}",
            matching.len()
        ));
    }
    let index = matching[0];
    let scene_node = &visual.scene.nodes[index];
    let authored = editor
        .graph()
        .nodes
        .get(&target.move_node_id)
        .ok_or_else(|| "current Editor graph lost MoveNode target".to_owned())?;
    if authored.header.bounds != target.move_after {
        return Err("current Editor MoveNode bounds differ from persisted project after".into());
    }
    if authored.header.parent_id != scene_node.parent_origin {
        return Err("current Editor MoveNode parent differs from Viewer product projection".into());
    }
    let instance = direct_page_local_instance_v1(
        &target.move_node_id.as_canonical().to_string(),
        &scene_node.parent_origin.to_string(),
    )
    .map_err(|error| format!("construct direct page-local Viewer instance: {error}"))?;
    if geometry_sync_policy_v1(&instance) != GeometrySyncPolicyV1::ApplyAuthoredOriginGeometry {
        return Err("current Viewer MoveNode target is not direct-page geometry authority".into());
    }
    visual.scene.nodes[index].bounds = authored.header.bounds;
    Ok(())
}

fn census(plans: &[PageRenderPlanV1]) -> CurrentViewerPlanCensusV1 {
    let mut out = CurrentViewerPlanCensusV1 {
        page_count: plans.len(),
        ..CurrentViewerPlanCensusV1::default()
    };
    let mut node_counts = BTreeMap::<NodeId, usize>::new();
    let mut projected_ids = BTreeSet::new();

    for page in plans {
        out.node_count += page.nodes.len();
        for node in &page.nodes {
            *node_counts.entry(node.node_id).or_default() += 1;
            if let Some(instance) = &node.projected_scene_instance
                && projected_ids.insert(instance.instance_id.clone())
            {
                out.projected_instance_count += 1;
            }
            if node.table.is_some() {
                out.table_node_count += 1;
            }
            if node.image.is_some() {
                out.image_node_count += 1;
            }
            if node
                .image
                .as_ref()
                .is_some_and(|image| image.source_window.is_some())
            {
                out.image_source_window_count += 1;
            }
            if node.solid_fill_rgb.is_some() || node.solid_line.is_some() {
                out.solid_paint_node_count += 1;
            }
            if node.decorative_border.is_some() {
                out.decorative_border_node_count += 1;
            }
            if node.transform != pub_model::Affine2D::identity() {
                out.non_identity_transform_node_count += 1;
            }
            let Some(text) = &node.text else {
                continue;
            };
            out.text_node_count += 1;
            let Some(layout) = &text.layout else {
                out.missing_text_layout_count += 1;
                continue;
            };
            match &layout.disposition {
                RenderTextLayoutDispositionV1::SharedResolved { .. } => {
                    out.shared_resolved_text_node_count += 1;
                    for line in &layout.lines {
                        out.shaped_line_count += 1;
                        if line.spans.is_empty() {
                            if line.shaping.is_none() {
                                out.missing_shaping_evidence_count += 1;
                            }
                        } else {
                            for span in &line.spans {
                                out.shaped_span_count += 1;
                                if span.shaping.is_none() {
                                    out.missing_shaping_evidence_count += 1;
                                }
                            }
                        }
                    }
                }
                RenderTextLayoutDispositionV1::BackendFallback { .. } => {
                    out.backend_fallback_text_node_count += 1;
                }
            }
        }
    }

    out.duplicate_node_id_count = node_counts.values().filter(|count| **count > 1).count();
    out
}

fn run(
    source_path: &Path,
    project_path: &Path,
    font_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let source = fs::read(source_path)
        .map_err(|error| format!("read source {}: {error}", source_path.display()))?;
    let source_hash = digest(&source);
    let source_hex = sha256_hex(&source);

    let project_raw = fs::read(project_path)
        .map_err(|error| format!("read project {}: {error}", project_path.display()))?;
    let project: EditorProject = serde_json::from_slice(&project_raw)
        .map_err(|error| format!("parse EditorProject {}: {error}", project_path.display()))?;
    if project.source_hash != source_hash {
        return Err("EditorProject source identity differs from immutable PUB".into());
    }
    let targets = exact_story_move_targets(&project)?;

    let mut visual = open_mature_0x2c_geometry(&source, viewer_geometry_environment_v0_1())
        .map_err(|error| format!("open product Viewer projection: {error:#}"))?;
    if visual.document.source.source_hash != source_hash {
        return Err("Viewer source identity differs from immutable PUB".into());
    }

    let mut editor = open_mature_0x2c_editor(&source, source_hash)
        .map_err(|error| format!("open fresh EditorSession: {error}"))?;
    editor
        .apply_project(&project)
        .map_err(|error| format!("fresh Story+Move EditorProject replay failed: {error}"))?;

    visual
        .refresh_text_projection_from_resolved(editor.graph())
        .map_err(|error| format!("refresh current Viewer Story projection: {error:#}"))?;
    synchronize_direct_move(&mut visual, &editor, targets)?;

    let font_bytes = fs::read(font_path)
        .map_err(|error| format!("read font {}: {error}", font_path.display()))?;
    let font_sha = font_fingerprint_sha256(&font_bytes);
    let font_resource_id = format!("chaptera:pinned-viewer-fixed-pdf:{font_sha}");
    let font = ExplicitRenderTextFontResourceV1 {
        resource_id: &font_resource_id,
        expected_sha256: &font_sha,
        face_index: 0,
        default_font_size_emu: FONT_SIZE_PT * EMU_PER_POINT,
        default_line_height_emu: LINE_HEIGHT_PT * EMU_PER_POINT,
        bytes: &font_bytes,
    };

    let mut pages = Vec::with_capacity(visual.document.pages.len());
    for page_index in 0..visual.document.pages.len() {
        pages.push(
            build_page_render_plan_with_text_layout_v1(&visual, page_index, &font)
                .map_err(|error| format!("build current Viewer page render plan: {error}"))?,
        );
    }

    let projected_instance_ids = pages
        .iter()
        .flat_map(|page| page.nodes.iter())
        .filter_map(|node| node.projected_scene_instance.as_ref())
        .map(|instance| instance.instance_id.clone())
        .collect::<Vec<_>>();
    let page_ids = pages
        .iter()
        .map(|page| page.page_id.as_canonical().to_string())
        .collect::<Vec<_>>();

    let binding = json!({
        "protocol_version": "chaptera.current-viewer-fixed-pdf-binding.v1",
        "source_hash": source_hex,
        "project_schema_version": project.schema_version,
        "project_state_id": project.state_id_v1(),
        "story_id": targets.story_id,
        "move_node_id": targets.move_node_id,
        "page_ids": page_ids,
        "projected_scene_instance_ids": projected_instance_ids,
    });
    let images = visual
        .images
        .iter()
        .map(|image| CurrentViewerImageAssetV1 {
            resource_id: image.resource_id,
            mime: image.mime.clone(),
            bytes: image.bytes.clone(),
        })
        .collect::<Vec<_>>();
    let mut census = census(&pages);
    census.image_resource_count = visual.images.len();

    let packet = CurrentViewerFixedPdfInputV1 {
        protocol_version: PROTOCOL_VERSION,
        binding,
        pages,
        images,
        font: CurrentViewerFontResourceV1 {
            resource_id: font_resource_id,
            fingerprint_sha256: font_sha,
            face_index: 0,
            bytes: font_bytes,
        },
        census,
    };

    let after = fs::read(source_path)
        .map_err(|error| format!("re-read source {}: {error}", source_path.display()))?;
    if after != source {
        return Err("immutable source PUB changed during current Viewer assembly".into());
    }

    if let Some(parent) = output_path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(
        output_path,
        serde_json::to_vec_pretty(&packet)
            .map_err(|error| format!("serialize current Viewer fixed-PDF input: {error}"))?,
    )
    .map_err(|error| format!("write {}: {error}", output_path.display()))?;

    eprintln!(
        "current_viewer_fixed_pdf_input pages={} nodes={} projected={} shared_resolved={} fallback={} missing_shaping={} duplicate_node_ids={} tables={} images={} image_nodes={} cropped_images={} solid_paint={} decorative_border={} non_identity_transform={} text_nodes={} missing_text_layout={}",
        packet.census.page_count,
        packet.census.node_count,
        packet.census.projected_instance_count,
        packet.census.shared_resolved_text_node_count,
        packet.census.backend_fallback_text_node_count,
        packet.census.missing_shaping_evidence_count,
        packet.census.duplicate_node_id_count,
        packet.census.table_node_count,
        packet.census.image_resource_count,
        packet.census.image_node_count,
        packet.census.image_source_window_count,
        packet.census.solid_paint_node_count,
        packet.census.decorative_border_node_count,
        packet.census.non_identity_transform_node_count,
        packet.census.text_node_count,
        packet.census.missing_text_layout_count,
    );
    Ok(())
}

fn main() {
    let args = env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let [source, project, font, output] = args.as_slice() else {
        eprintln!("usage: current_viewer_fixed_pdf_input SOURCE.pub PROJECT.json FONT OUTPUT.json");
        std::process::exit(2);
    };
    if let Err(error) = run(source, project, font, output) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
