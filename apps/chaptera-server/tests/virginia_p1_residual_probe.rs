use std::{collections::BTreeMap, env, fs, path::PathBuf};

use chaptera_server::reader_scene_v1::{ReaderSceneV1, from_viewer_geometry};
use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderTextLayoutDispositionV1,
    build_page_render_plan_with_text_layout_v1,
};
use pub_viewer::{ViewerOpenBundle, open_pub_bundle, viewer_geometry_environment_v0_1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const EXPECTED_REMPLACANTE_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const TARGET_VIEWER_PAGES: [u32; 3] = [1, 2, 3];

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn source_page_census(bundle: &ViewerOpenBundle, one_based_page: u32) -> Value {
    let page = bundle
        .geometry
        .document
        .pages
        .iter()
        .find(|page| page.index == one_based_page)
        .unwrap_or_else(|| panic!("Viewer page {one_based_page} must exist"));
    let page_canonical = page.id.into_canonical();
    let nodes = bundle
        .resolved_graph
        .nodes
        .values()
        .filter(|node| node.header.parent_id == page_canonical)
        .collect::<Vec<_>>();
    let source_order = bundle
        .source_page_paint_orders
        .iter()
        .find(|order| order.page_id == page.id);

    let mut family_histogram = BTreeMap::<String, usize>::new();
    let mut provenance_histogram = BTreeMap::<String, usize>::new();
    let mut fill_state_histogram = BTreeMap::<String, usize>::new();
    let mut line_state_histogram = BTreeMap::<String, usize>::new();
    let mut transform_histogram = BTreeMap::<String, usize>::new();
    let mut positive_bounds = 0_usize;
    let mut source_order_covered_nodes = 0_usize;
    let mut table_cells = 0_usize;
    let mut table_cells_with_bounds = 0_usize;
    let mut table_cells_with_paint = 0_usize;
    let mut spanning_table_cells = 0_usize;

    for node in &nodes {
        let family = if node.payload.table.is_some() {
            "table"
        } else if node.payload.image_slot.is_some() {
            "image"
        } else if node.payload.story_frame.is_some() {
            "story"
        } else {
            "other_shape"
        };
        bump(&mut family_histogram, family);

        let transform = &node.header.transform;
        let transform_class = if transform.a.as_str() == "1"
            && transform.b.as_str() == "0"
            && transform.c.as_str() == "0"
            && transform.d.as_str() == "1"
            && transform.tx.get() == 0
            && transform.ty.get() == 0
        {
            "identity"
        } else if transform.a.as_str() == "1"
            && transform.b.as_str() == "0"
            && transform.c.as_str() == "0"
            && transform.d.as_str() == "1"
        {
            "translate_only"
        } else if transform.b.as_str() == "0" && transform.c.as_str() == "0" {
            "axis_scale_translate"
        } else {
            "general_affine"
        };
        bump(
            &mut transform_histogram,
            format!("{family}:{transform_class}"),
        );

        let grouped = node.header.source_refs.iter().any(|source| {
            source
                .object_key
                .as_deref()
                .is_some_and(|key| key.starts_with("escher/group-ancestor/"))
        });
        bump(
            &mut provenance_histogram,
            if grouped { "grouped" } else { "direct" },
        );

        positive_bounds += usize::from(
            node.header.bounds.width.get() > 0
                && node.header.bounds.height.get() > 0
                && node.header.bounds.right().is_some()
                && node.header.bounds.bottom().is_some(),
        );
        source_order_covered_nodes +=
            usize::from(source_order.is_some_and(|order| order.node_ids.contains(&node.header.id)));

        if let Some(table) = node.payload.table.as_ref() {
            table_cells += table.cells.len();
            table_cells_with_bounds += table
                .cells
                .iter()
                .filter(|cell| cell.bounds.is_some())
                .count();
            table_cells_with_paint += table
                .cells
                .iter()
                .filter(|cell| cell.paint.is_some())
                .count();
            spanning_table_cells += table
                .cells
                .iter()
                .filter(|cell| {
                    cell.coordinates.is_some_and(|coordinates| {
                        coordinates.end_row > coordinates.start_row
                            || coordinates.end_column > coordinates.start_column
                    })
                })
                .count();
        }

        let Some(effective) = node.payload.effective_paint.as_ref() else {
            bump(&mut fill_state_histogram, "effective_paint_none");
            bump(&mut line_state_histogram, "effective_paint_none");
            continue;
        };

        let fill = &effective.fill;
        let fill_any = fill.solid.is_some() || fill.color_rgb.is_some() || fill.visible.is_some();
        let fill_state = match (
            fill.solid.as_ref(),
            fill.color_rgb.as_ref(),
            fill.visible.as_ref(),
        ) {
            (Some(solid), Some(_), Some(visible)) if solid.value && visible.value => {
                "complete_visible_solid"
            }
            (Some(solid), Some(_), Some(visible)) if solid.value && !visible.value => {
                "complete_hidden_solid"
            }
            (Some(solid), Some(_), Some(_)) if !solid.value => "complete_non_solid",
            _ if fill_any => "incomplete",
            _ => "absent",
        };
        bump(&mut fill_state_histogram, fill_state);

        let line = &effective.line;
        let line_any =
            line.color_rgb.is_some() || line.width_emu.is_some() || line.visible.is_some();
        let line_state = match (
            line.color_rgb.as_ref(),
            line.width_emu.as_ref(),
            line.visible.as_ref(),
        ) {
            (Some(_), Some(width), Some(visible)) if width.value > 0 && visible.value => {
                "complete_visible"
            }
            (Some(_), Some(width), Some(visible)) if width.value > 0 && !visible.value => {
                "complete_hidden"
            }
            _ if line_any => "incomplete",
            _ => "absent",
        };
        bump(&mut line_state_histogram, line_state);
    }

    json!({
        "viewer_page": one_based_page,
        "node_count": nodes.len(),
        "family_histogram": family_histogram,
        "provenance_histogram": provenance_histogram,
        "fill_state_histogram": fill_state_histogram,
        "line_state_histogram": line_state_histogram,
        "transform_histogram": transform_histogram,
        "positive_bounds": positive_bounds,
        "source_order_node_count": source_order.map_or(0, |order| order.node_ids.len()),
        "source_order_covered_nodes": source_order_covered_nodes,
        "table_cells": table_cells,
        "table_cells_with_bounds": table_cells_with_bounds,
        "table_cells_with_paint": table_cells_with_paint,
        "spanning_table_cells": spanning_table_cells,
    })
}

fn text_layout_reason_census(bundle: &ViewerOpenBundle, one_based_page: u32) -> Value {
    let font = ExplicitRenderTextFontResourceV1 {
        resource_id: chaptera_desktop_fallback_font_resource::RESOURCE_ID,
        expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
        face_index: 0,
        default_font_size_emu: chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU,
        default_line_height_emu: chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
        bytes: chaptera_desktop_fallback_font_resource::bytes(),
    };
    let plan = build_page_render_plan_with_text_layout_v1(
        &bundle.geometry,
        usize::try_from(one_based_page - 1).expect("Viewer page index fits usize"),
        &font,
    )
    .expect("p1 fallback census render plan must build");

    let mut text_nodes = 0_usize;
    let mut shared_frames = 0_usize;
    let mut shared_lines = 0_usize;
    let mut shared_nonempty_lines = 0_usize;
    let mut layout_none = 0_usize;
    let mut backend_fallbacks = BTreeMap::<String, usize>::new();
    let mut incomplete_story_extent = BTreeMap::<String, usize>::new();
    let mut incomplete_typography_coverage = BTreeMap::<String, usize>::new();
    let mut incomplete_text_bounds = BTreeMap::<String, usize>::new();
    let mut incomplete_viewer_fragment_coverage = BTreeMap::<String, usize>::new();
    let mut incomplete_viewer_fragment_lines = BTreeMap::<String, usize>::new();

    for node in plan.nodes {
        let node_id = node.node_id;
        let text_bounds_present = node.text_bounds.is_some();
        let Some(text) = node.text else {
            continue;
        };
        text_nodes += 1;
        let Some(layout) = text.layout else {
            layout_none += 1;
            continue;
        };
        match layout.disposition {
            RenderTextLayoutDispositionV1::SharedResolved { .. } => {
                shared_frames += 1;
                shared_lines += layout.lines.len();
                shared_nonempty_lines += layout
                    .lines
                    .iter()
                    .filter(|line| !line.text.trim().is_empty())
                    .count();
            }
            RenderTextLayoutDispositionV1::BackendFallback { reason } => {
                bump(&mut backend_fallbacks, reason.code());

                if reason.code() == "shared_layout_incomplete" {
                    let story_len = bundle
                        .geometry
                        .document
                        .stories
                        .iter()
                        .find(|story| story.id == text.story_id)
                        .and_then(|story| u32::try_from(story.text.chars().count()).ok());
                    let full_story_extent = story_len
                        .is_some_and(|len| text.scalar_start == 0 && text.scalar_end == len);
                    bump(
                        &mut incomplete_story_extent,
                        if full_story_extent {
                            "full_story"
                        } else {
                            "partial_or_unknown"
                        },
                    );

                    let typography_class = if text.typography.is_empty() {
                        "absent"
                    } else {
                        let mut cursor = text.scalar_start;
                        let complete = text.typography.iter().all(|run| {
                            let ok = run.scalar_start == cursor
                                && run.scalar_end > run.scalar_start
                                && run.scalar_end <= text.scalar_end
                                && run.text_size_emu > 0;
                            cursor = run.scalar_end;
                            ok
                        }) && cursor == text.scalar_end;
                        if complete {
                            "complete"
                        } else {
                            "gap_or_invalid"
                        }
                    };
                    bump(&mut incomplete_typography_coverage, typography_class);
                    bump(
                        &mut incomplete_text_bounds,
                        if text_bounds_present {
                            "present"
                        } else {
                            "absent"
                        },
                    );

                    let viewer_fragments = bundle
                        .geometry
                        .text_fragments
                        .iter()
                        .filter(|fragment| fragment.frame_id == node_id)
                        .collect::<Vec<_>>();
                    let viewer_fragment_class = match viewer_fragments.as_slice() {
                        [fragment]
                            if story_len.is_some_and(|len| {
                                fragment.scalar_start == 0 && fragment.scalar_end == len
                            }) =>
                        {
                            "one_full_story"
                        }
                        [_] => "one_partial_story",
                        [] => "absent",
                        _ => "multiple",
                    };
                    bump(
                        &mut incomplete_viewer_fragment_coverage,
                        viewer_fragment_class,
                    );
                    let line_class = match viewer_fragments.as_slice() {
                        [fragment] if fragment.line_count == 0 => "zero",
                        [fragment] if fragment.line_count == 1 => "one",
                        [fragment] if fragment.line_count > 1 => "multiple",
                        [] => "absent",
                        _ => "ambiguous",
                    };
                    bump(&mut incomplete_viewer_fragment_lines, line_class);
                }
            }
        }
    }

    json!({
        "viewer_page": one_based_page,
        "text_nodes": text_nodes,
        "shared_frames": shared_frames,
        "shared_lines": shared_lines,
        "shared_nonempty_lines": shared_nonempty_lines,
        "layout_none": layout_none,
        "backend_fallbacks": backend_fallbacks,
        "shared_layout_incomplete_story_extent": incomplete_story_extent,
        "shared_layout_incomplete_typography_coverage": incomplete_typography_coverage,
        "shared_layout_incomplete_text_bounds": incomplete_text_bounds,
        "shared_layout_incomplete_viewer_fragment_coverage": incomplete_viewer_fragment_coverage,
        "shared_layout_incomplete_viewer_fragment_lines": incomplete_viewer_fragment_lines,
    })
}

fn scene_page_census(scene: &ReaderSceneV1, one_based_page: u32) -> Value {
    let order = one_based_page - 1;
    let page = scene
        .pages
        .iter()
        .find(|page| page.order == order)
        .unwrap_or_else(|| panic!("Viewer page {one_based_page} must exist"));
    let nodes = scene
        .nodes
        .iter()
        .filter(|node| node.page_id == page.page_id)
        .collect::<Vec<_>>();

    let mut kind_histogram = BTreeMap::<String, usize>::new();
    let mut text_layout_histogram = BTreeMap::<String, usize>::new();
    let mut resource_availability_histogram = BTreeMap::<String, usize>::new();
    let mut transform_histogram = BTreeMap::<String, usize>::new();
    let mut diagnostic_histogram = BTreeMap::<String, usize>::new();
    let mut fill_nodes = 0_usize;
    let mut line_nodes = 0_usize;
    let mut resource_nodes = 0_usize;
    let mut text_nodes = 0_usize;
    let mut text_layout_nodes = 0_usize;
    let mut positive_bounds = 0_usize;
    let mut table_cells = 0_usize;
    let mut table_cells_with_bounds = 0_usize;
    let mut table_cells_with_fill = 0_usize;

    for node in &nodes {
        bump(&mut kind_histogram, node.kind);
        let transform = &node.transform;
        let transform_class = if transform.a == "1"
            && transform.b == "0"
            && transform.c == "0"
            && transform.d == "1"
            && transform.tx == 0
            && transform.ty == 0
        {
            "identity"
        } else if transform.a == "1"
            && transform.b == "0"
            && transform.c == "0"
            && transform.d == "1"
        {
            "translate_only"
        } else if transform.b == "0" && transform.c == "0" {
            "axis_scale_translate"
        } else {
            "general_affine"
        };
        bump(
            &mut transform_histogram,
            format!("{}:{transform_class}", node.kind),
        );
        positive_bounds += usize::from(node.bounds.width > 0 && node.bounds.height > 0);
        fill_nodes += usize::from(
            node.paint
                .as_ref()
                .and_then(|paint| paint.fill_rgb.as_ref())
                .is_some(),
        );
        line_nodes += usize::from(
            node.paint
                .as_ref()
                .and_then(|paint| paint.line.as_ref())
                .is_some(),
        );

        if let Some(resource_id) = node.resource_id.as_deref() {
            resource_nodes += 1;
            let availability = scene
                .resources
                .iter()
                .find(|resource| resource.resource_id == resource_id)
                .map(|resource| resource.availability)
                .unwrap_or("missing_descriptor");
            bump(&mut resource_availability_histogram, availability);
        }

        if node.text.is_some() {
            text_nodes += 1;
        }
        match node.text_layout.as_ref() {
            Some(layout) => {
                text_layout_nodes += 1;
                bump(&mut text_layout_histogram, layout.disposition);
            }
            None if node.text.is_some() => {
                bump(&mut text_layout_histogram, "text_without_layout");
            }
            None => {}
        }

        if let Some(table) = node.table.as_ref() {
            table_cells += table.cells.len();
            table_cells_with_bounds += table
                .cells
                .iter()
                .filter(|cell| cell.bounds.is_some())
                .count();
            table_cells_with_fill += table
                .cells
                .iter()
                .filter(|cell| cell.fill_rgb.is_some() && cell.fill_visible == Some(true))
                .count();
        }
    }

    for diagnostic in &scene.diagnostics {
        if diagnostic
            .origin_id
            .as_deref()
            .is_some_and(|origin| nodes.iter().any(|node| node.node_id == origin))
        {
            bump(&mut diagnostic_histogram, diagnostic.code.clone());
        }
    }

    json!({
        "viewer_page": one_based_page,
        "node_count": nodes.len(),
        "kind_histogram": kind_histogram,
        "positive_bounds": positive_bounds,
        "fill_nodes": fill_nodes,
        "line_nodes": line_nodes,
        "resource_nodes": resource_nodes,
        "resource_availability_histogram": resource_availability_histogram,
        "transform_histogram": transform_histogram,
        "text_nodes": text_nodes,
        "text_layout_nodes": text_layout_nodes,
        "text_layout_histogram": text_layout_histogram,
        "table_cells": table_cells,
        "table_cells_with_bounds": table_cells_with_bounds,
        "table_cells_with_visible_fill": table_cells_with_fill,
        "diagnostic_histogram": diagnostic_histogram,
    })
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p1_product_boundary_census() {
    let fixture = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_P1_PUB")
            .expect("CHAPTERA_VIRGINIA_P1_PUB must name the exact public fixture"),
    );
    let receipt_path = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_P1_RECEIPT")
            .expect("CHAPTERA_VIRGINIA_P1_RECEIPT must name the sanitized receipt"),
    );

    let bytes = fs::read(&fixture).expect("read exact public Virginia Remplacante fixture");
    let actual_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_sha256, EXPECTED_REMPLACANTE_SHA256,
        "exact Virginia Remplacante source identity drift"
    );

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia Remplacante through shared Viewer bundle");
    assert!(
        bundle.geometry.document.pages.len() >= 3,
        "p1-p3 census requires at least 3 admitted Viewer pages"
    );

    let scene = from_viewer_geometry(
        "probe:virginia-remplacante".to_owned(),
        actual_sha256,
        "probe:p1-residual".to_owned(),
        &bundle.geometry,
        &bundle.source_page_paint_orders,
    )
    .expect("project exact Virginia Remplacante through ReaderScene boundary");

    let pages = TARGET_VIEWER_PAGES
        .iter()
        .map(|page| {
            json!({
                "viewer_page": page,
                "source": source_page_census(&bundle, *page),
                "scene": scene_page_census(&scene, *page),
                "text_layout_reason": text_layout_reason_census(&bundle, *page),
            })
        })
        .collect::<Vec<_>>();

    let receipt = json!({
        "schema": "chaptera.viewer-virginia-p1-residual-probe.v1",
        "target_viewer_page": 1,
        "control_viewer_pages": [2, 3],
        "pages": pages,
        "claims": {
            "exact_public_source_identity_checked": true,
            "publisher_pdf_used_for_semantics": false,
            "raw_story_text_emitted": false,
            "object_ids_emitted": false,
            "resource_ids_emitted": false,
            "coordinates_emitted": false,
            "raw_property_values_emitted": false,
        },
    });

    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).expect("create Virginia p1 receipt directory");
    }
    fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("serialize Virginia p1 receipt"),
    )
    .expect("write Virginia p1 receipt");

    println!(
        "VIRGINIA_P1_RESIDUAL_PROBE {}",
        serde_json::to_string(&receipt).expect("serialize Virginia p1 receipt for log")
    );
}
