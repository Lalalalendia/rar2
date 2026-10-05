use chaptera_scene_instance::{
    GeometrySyncPolicyV1, direct_page_local_instance_v1, geometry_sync_policy_v1,
};
use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, PageRenderPlanV1, RenderTextFragmentV1,
    RenderTextLayoutDispositionV1, build_page_render_plan_with_text_layout_v1,
    complete_scalar_source_font_family_v1, effective_source_font_family_v1,
};
use pub_editor::{
    EditOperation, EditorProject, EditorSession, Sha256Digest, open_mature_0x2c_editor,
};
use pub_layout::{compatible_natural_line_height_emu_v1, font_fingerprint_sha256};
use pub_model::{EMU_PER_POINT, LengthEmu, NodeId, RectEmu, ResourceId, StoryId};
use pub_viewer::{
    ViewerGeometryDocument, ViewerParagraphLineSpacing, open_mature_0x2c_geometry,
    viewer_geometry_environment_v0_1,
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
    backend_fallback_reason_counts: BTreeMap<String, usize>,
    text_font_binding_counts: BTreeMap<String, usize>,
    shared_layout_incomplete_spacing_authority_counts: BTreeMap<String, usize>,
    shared_layout_incomplete_family_authority_counts: BTreeMap<String, usize>,
    shared_layout_incomplete_font_binding_counts: BTreeMap<String, usize>,
    shared_layout_incomplete_spacing_family_binding_counts: BTreeMap<String, usize>,
    first_line_capacity_recovery_count: usize,
    first_line_capacity_recovery_signature_counts: BTreeMap<String, usize>,
    story_prefix_whole_story_candidate_count: usize,
    story_prefix_whole_story_candidate_signature_counts: BTreeMap<String, usize>,
    story_prefix_whole_story_candidate_outcome_counts: BTreeMap<String, usize>,
    story_extent_mismatch_profile_counts: BTreeMap<String, usize>,
    configured_fallback_font_size_emu: i64,
    configured_fallback_line_height_emu: i64,
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
    node_id_sort_reordered_page_count: usize,
    node_id_sort_position_mismatch_count: usize,
    visible_node_id_sort_reordered_page_count: usize,
    visible_node_id_sort_position_mismatch_count: usize,
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

fn paragraph_spacing_authority_class(
    visual: &ViewerGeometryDocument,
    fragment: &RenderTextFragmentV1,
) -> &'static str {
    let Some(story) = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)
    else {
        return "story_missing";
    };

    let mut intersecting = visual
        .paragraph_line_spacings
        .iter()
        .filter(|run| run.story_id == fragment.story_id)
        .filter(|run| run.applies_to_story_text(&story.text))
        .filter(|run| {
            run.scalar_end > fragment.scalar_start && run.scalar_start < fragment.scalar_end
        });

    let Some(run) = intersecting.next() else {
        return "none";
    };
    if intersecting.next().is_some()
        || run.scalar_start > fragment.scalar_start
        || run.scalar_end < fragment.scalar_end
    {
        return "ambiguous_or_incomplete";
    }

    match run.line_spacing {
        ViewerParagraphLineSpacing::Absolute { .. } => "absolute_complete",
        ViewerParagraphLineSpacing::Proportional { .. } => "proportional_complete",
    }
}

fn source_family_authority_class(
    visual: &ViewerGeometryDocument,
    fragment: &RenderTextFragmentV1,
) -> &'static str {
    if complete_scalar_source_font_family_v1(fragment).is_some() {
        "complete_scalar"
    } else if effective_source_font_family_v1(visual, fragment).is_some() {
        "script_font_map"
    } else {
        "unavailable"
    }
}

fn font_binding_class(fragment: &RenderTextFragmentV1) -> &'static str {
    if fragment.backend_font_resource_id.is_some() {
        "source_bound"
    } else {
        "fallback_only"
    }
}

fn story_prefix_whole_story_candidates(
    visual: &ViewerGeometryDocument,
) -> (BTreeSet<NodeId>, BTreeMap<String, usize>) {
    let mut candidates = BTreeSet::new();
    let mut signatures = BTreeMap::new();

    for fragment in &visual.text_fragments {
        if fragment.scalar_start != 0 {
            continue;
        }
        let Some(story) = visual
            .document
            .stories
            .iter()
            .find(|story| story.id == fragment.story_id)
        else {
            continue;
        };
        let Ok(story_scalar_len) = u32::try_from(story.text.chars().count()) else {
            continue;
        };
        if fragment.scalar_end >= story_scalar_len
            || u32::try_from(fragment.text.chars().count()).ok() != Some(fragment.scalar_end)
        {
            continue;
        }

        let Ok(prefix_end) = usize::try_from(fragment.scalar_end) else {
            continue;
        };
        let story_prefix = story.text.chars().take(prefix_end).collect::<String>();
        if story_prefix != fragment.text {
            continue;
        }

        let mut frames = visual
            .story_frames
            .iter()
            .filter(|frame| frame.story_id == fragment.story_id);
        let Some(frame) = frames.next() else {
            continue;
        };
        if frames.next().is_some() || frame.frame_id != fragment.frame_id {
            continue;
        }

        let Some(node) = visual
            .scene
            .nodes
            .iter()
            .find(|node| node.origin == fragment.frame_id)
        else {
            continue;
        };
        if !visual
            .document
            .pages
            .iter()
            .any(|page| page.id.into_canonical() == node.parent_origin)
        {
            continue;
        }

        if visual.projected_instances.iter().any(|projected| {
            projected.target_frame_node_id == Some(fragment.frame_id)
                && projected.scene_instance.target_page_id == node.parent_origin.to_string()
        }) {
            continue;
        }

        if candidates.insert(fragment.frame_id) {
            let delta = story_scalar_len - fragment.scalar_end;
            *signatures
                .entry(format!(
                    "end_delta={delta}|source_lines={}",
                    fragment.line_count
                ))
                .or_default() += 1;
        }
    }

    (candidates, signatures)
}

fn census(
    visual: &ViewerGeometryDocument,
    plans: &[PageRenderPlanV1],
    fallback_font_bytes: &[u8],
    fallback_face_index: u32,
) -> CurrentViewerPlanCensusV1 {
    let mut out = CurrentViewerPlanCensusV1 {
        page_count: plans.len(),
        ..CurrentViewerPlanCensusV1::default()
    };
    let mut node_counts = BTreeMap::<NodeId, usize>::new();
    let mut projected_ids = BTreeSet::new();
    let (story_prefix_candidates, story_prefix_signatures) =
        story_prefix_whole_story_candidates(visual);
    out.story_prefix_whole_story_candidate_count = story_prefix_candidates.len();
    out.story_prefix_whole_story_candidate_signature_counts = story_prefix_signatures;

    for page in plans {
        let plan_order = page
            .nodes
            .iter()
            .map(|node| node.node_id)
            .collect::<Vec<_>>();
        let mut sorted_order = plan_order.clone();
        sorted_order.sort();
        let position_mismatches = plan_order
            .iter()
            .zip(&sorted_order)
            .filter(|(left, right)| left != right)
            .count();
        if position_mismatches > 0 {
            out.node_id_sort_reordered_page_count += 1;
            out.node_id_sort_position_mismatch_count += position_mismatches;
        }

        let visible_order = page
            .nodes
            .iter()
            .filter(|node| {
                node.solid_fill_rgb.is_some()
                    || node.solid_line.is_some()
                    || node.decorative_border.is_some()
                    || node.image.is_some()
                    || node.text.is_some()
                    || node.table.is_some()
            })
            .map(|node| node.node_id)
            .collect::<Vec<_>>();
        let mut visible_sorted_order = visible_order.clone();
        visible_sorted_order.sort();
        let visible_position_mismatches = visible_order
            .iter()
            .zip(&visible_sorted_order)
            .filter(|(left, right)| left != right)
            .count();
        if visible_position_mismatches > 0 {
            out.visible_node_id_sort_reordered_page_count += 1;
            out.visible_node_id_sort_position_mismatch_count += visible_position_mismatches;
        }

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
            *out.text_font_binding_counts
                .entry(font_binding_class(text).to_owned())
                .or_default() += 1;
            let is_story_prefix_candidate = node.projected_scene_instance.is_none()
                && story_prefix_candidates.contains(&node.node_id);
            let Some(layout) = &text.layout else {
                out.missing_text_layout_count += 1;
                if is_story_prefix_candidate {
                    *out.story_prefix_whole_story_candidate_outcome_counts
                        .entry("missing_layout".to_owned())
                        .or_default() += 1;
                }
                continue;
            };
            match &layout.disposition {
                RenderTextLayoutDispositionV1::SharedResolved {
                    font_size_emu,
                    line_height_emu,
                    ..
                } => {
                    out.shared_resolved_text_node_count += 1;
                    if is_story_prefix_candidate {
                        *out.story_prefix_whole_story_candidate_outcome_counts
                            .entry("shared_resolved".to_owned())
                            .or_default() += 1;
                    }

                    let bounds = node.text_bounds.unwrap_or(node.bounds);
                    let frame_height_emu = bounds.height.get();
                    let resolved_line_count = i64::try_from(layout.lines.len()).unwrap_or(i64::MAX);
                    if node.projected_scene_instance.is_none()
                        && frame_height_emu > 0
                        && *font_size_emu > 0
                        && *line_height_emu > 0
                        && resolved_line_count > 0
                        && let Some(natural_extent) = compatible_natural_line_height_emu_v1(
                            fallback_font_bytes,
                            fallback_face_index,
                            LengthEmu::new(*font_size_emu),
                        )
                    {
                        let first_line_extent_emu = natural_extent.get().min(*line_height_emu);
                        let old_capacity = frame_height_emu / *line_height_emu;
                        let new_capacity = if frame_height_emu < first_line_extent_emu {
                            0
                        } else {
                            1 + (frame_height_emu - first_line_extent_emu) / *line_height_emu
                        };
                        if old_capacity < resolved_line_count && new_capacity >= resolved_line_count
                        {
                            out.first_line_capacity_recovery_count += 1;
                            *out.first_line_capacity_recovery_signature_counts
                                .entry(format!(
                                    "size={font_size_emu}|lines={resolved_line_count}|old={old_capacity}|new={new_capacity}"
                                ))
                                .or_default() += 1;
                        }
                    }

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
                RenderTextLayoutDispositionV1::BackendFallback { reason } => {
                    out.backend_fallback_text_node_count += 1;
                    if is_story_prefix_candidate {
                        *out.story_prefix_whole_story_candidate_outcome_counts
                            .entry(format!("fallback:{}", reason.code()))
                            .or_default() += 1;
                    }
                    *out.backend_fallback_reason_counts
                        .entry(reason.code().to_owned())
                        .or_default() += 1;

                    if reason.code() == "story_extent_mismatch"
                        && let Some(story) = visual
                            .document
                            .stories
                            .iter()
                            .find(|story| story.id == text.story_id)
                    {
                        let story_scalars = story.text.chars().collect::<Vec<_>>();
                        let fragment_scalars = text.text.chars().collect::<Vec<_>>();
                        if let (Ok(story_len), Ok(fragment_len)) = (
                            u32::try_from(story_scalars.len()),
                            u32::try_from(fragment_scalars.len()),
                        ) {
                            let profile = if text.scalar_start == 0
                                && text.scalar_end == story_len
                                && fragment_len == story_len
                                && text.text != story.text
                            {
                                let mut differences = 0_usize;
                                let mut marker_suppressions = 0_usize;
                                let mut other_differences = 0_usize;
                                for (source, rendered) in
                                    story_scalars.iter().zip(fragment_scalars.iter())
                                {
                                    if source == rendered {
                                        continue;
                                    }
                                    differences += 1;
                                    if *source == '\u{FFFC}' && *rendered == '\u{200B}' {
                                        marker_suppressions += 1;
                                    } else {
                                        other_differences += 1;
                                    }
                                }

                                if differences > 0
                                    && marker_suppressions == differences
                                    && other_differences == 0
                                {
                                    format!(
                                        "full_extent:object_marker_to_zero_width_only:diffs={differences}"
                                    )
                                } else {
                                    format!(
                                        "full_extent:same_extent_other:diffs={differences}:other={other_differences}"
                                    )
                                }
                            } else {
                                format!(
                                    "partial_or_other:start={}:end_delta={}:len_delta={}",
                                    text.scalar_start,
                                    i64::from(text.scalar_end) - i64::from(story_len),
                                    i64::from(fragment_len) - i64::from(story_len),
                                )
                            };
                            *out.story_extent_mismatch_profile_counts
                                .entry(profile)
                                .or_default() += 1;
                        }
                    }

                    if reason.code() == "shared_layout_incomplete" {
                        let spacing = paragraph_spacing_authority_class(visual, text);
                        let family = source_family_authority_class(visual, text);
                        let binding = font_binding_class(text);
                        *out.shared_layout_incomplete_spacing_authority_counts
                            .entry(spacing.to_owned())
                            .or_default() += 1;
                        *out.shared_layout_incomplete_family_authority_counts
                            .entry(family.to_owned())
                            .or_default() += 1;
                        *out.shared_layout_incomplete_font_binding_counts
                            .entry(binding.to_owned())
                            .or_default() += 1;
                        *out.shared_layout_incomplete_spacing_family_binding_counts
                            .entry(format!("{spacing}@{family}@{binding}"))
                            .or_default() += 1;
                    }
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
    let mut census = census(&visual, &pages, &font_bytes, font.face_index);
    census.image_resource_count = visual.images.len();
    census.configured_fallback_font_size_emu = font.default_font_size_emu;
    census.configured_fallback_line_height_emu = font.default_line_height_emu;

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
        "current_viewer_fixed_pdf_input pages={} nodes={} projected={} shared_resolved={} fallback={} missing_shaping={} duplicate_node_ids={} tables={} images={} image_nodes={} cropped_images={} solid_paint={} decorative_border={} non_identity_transform={} text_nodes={} missing_text_layout={} reordered_pages={} reordered_positions={} visible_reordered_pages={} visible_reordered_positions={} text_font_bindings={:?} shared_layout_incomplete_spacing={:?} shared_layout_incomplete_family={:?} shared_layout_incomplete_binding={:?} first_line_capacity_recoveries={} first_line_capacity_recovery_signatures={:?} story_extent_mismatch_profiles={:?} fallback_font_size_emu={} fallback_line_height_emu={}",
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
        packet.census.node_id_sort_reordered_page_count,
        packet.census.node_id_sort_position_mismatch_count,
        packet.census.visible_node_id_sort_reordered_page_count,
        packet.census.visible_node_id_sort_position_mismatch_count,
        packet.census.text_font_binding_counts,
        packet
            .census
            .shared_layout_incomplete_spacing_authority_counts,
        packet
            .census
            .shared_layout_incomplete_family_authority_counts,
        packet.census.shared_layout_incomplete_font_binding_counts,
        packet.census.first_line_capacity_recovery_count,
        packet.census.first_line_capacity_recovery_signature_counts,
        packet.census.story_extent_mismatch_profile_counts,
        packet.census.configured_fallback_font_size_emu,
        packet.census.configured_fallback_line_height_emu,
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
