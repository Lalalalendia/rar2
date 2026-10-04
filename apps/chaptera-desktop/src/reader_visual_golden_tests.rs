//! Reader visual/typography golden acceptance owners.
//!
//! These tests are intentionally isolated from the monolithic Desktop shell so
//! Reader visual CI admission follows the render evidence owner rather than any
//! unrelated main.rs change.

use super::*;

struct GoldenPageOnlyApp {
    visual: ViewerGeometryDocument,
    page_index: usize,
    image_textures: BTreeMap<String, CachedImageTexture>,
    texture_upload_enabled: bool,
    painted_text_nodes: usize,
    clipped_text_nodes: usize,
    source_typography_sections: usize,
    fallback_typography_sections: usize,
    shared_resolved_layout_frames: usize,
    backend_fallback_frames: usize,
    projected_text_metrics: BTreeMap<String, render_backend::TextPaintMetrics>,
}

impl GoldenPageOnlyApp {
    fn new(visual: ViewerGeometryDocument, page_index: usize) -> Self {
        Self {
            visual,
            page_index,
            image_textures: BTreeMap::new(),
            texture_upload_enabled: false,
            painted_text_nodes: 0,
            clipped_text_nodes: 0,
            source_typography_sections: 0,
            fallback_typography_sections: 0,
            shared_resolved_layout_frames: 0,
            backend_fallback_frames: 0,
            projected_text_metrics: BTreeMap::new(),
        }
    }

    fn enable_texture_upload(&mut self) {
        self.texture_upload_enabled = true;
    }

    fn ensure_image_textures(&mut self, ctx: &egui::Context) {
        if !self.texture_upload_enabled {
            return;
        }
        let max_texture_side = ctx.input(|input| input.max_texture_side);
        for embedded in &self.visual.images {
            let key = format!("{:?}", embedded.resource_id);
            if self.image_textures.contains_key(&key) {
                continue;
            }
            let expected_sha256 = image_decode_adapter::exact_sha256_hex(&embedded.bytes);
            let admitted = image_decode_adapter::decode_texture_image_v1(
                &embedded.bytes,
                &embedded.mime,
                &expected_sha256,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "clean golden image decode failed for {} / {}: {error}",
                    key, embedded.mime
                )
            });
            let [width, height] = admitted.color_image.size;
            assert!(
                width <= max_texture_side && height <= max_texture_side,
                "Carlton golden image {width}x{height} exceeds active egui texture limit {max_texture_side}"
            );
            let texture = ctx.load_texture(
                format!("carlton-golden-{key}"),
                admitted.color_image,
                egui::TextureOptions::LINEAR,
            );
            self.image_textures.insert(
                key,
                CachedImageTexture {
                    texture,
                    _cache_identity_sha256: admitted.cache_identity_sha256,
                },
            );
        }

        for resource in &self.visual.decorative_border_resources {
            let key = format!("{:?}", resource.resource_id);
            if self.image_textures.contains_key(&key) {
                continue;
            }
            let expected_sha256 = image_decode_adapter::exact_sha256_hex(&resource.bytes);
            let admitted = image_decode_adapter::decode_texture_image_v1(
                &resource.bytes,
                &resource.mime,
                &expected_sha256,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "clean golden BorderArt decode failed for {} / {}: {error}",
                    key, resource.mime
                )
            });
            let [width, height] = admitted.color_image.size;
            assert!(
                width <= max_texture_side && height <= max_texture_side,
                "BorderArt golden image {width}x{height} exceeds active egui texture limit {max_texture_side}"
            );
            let texture = ctx.load_texture(
                format!("borderart-golden-{key}"),
                admitted.color_image,
                egui::TextureOptions::LINEAR,
            );
            self.image_textures.insert(
                key,
                CachedImageTexture {
                    texture,
                    _cache_identity_sha256: admitted.cache_identity_sha256,
                },
            );
        }
    }
}

impl eframe::App for GoldenPageOnlyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_image_textures(ctx);
        let render_plan = build_desktop_page_render_plan(&self.visual, self.page_index)
            .expect("clean golden page render plan");
        let scene_scale = 144.0_f32 / 914_400.0_f32;
        self.painted_text_nodes = 0;
        self.clipped_text_nodes = 0;
        self.source_typography_sections = 0;
        self.fallback_typography_sections = 0;
        self.shared_resolved_layout_frames = 0;
        self.backend_fallback_frames = 0;
        self.projected_text_metrics.clear();

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let page_rect = ui.max_rect();
                let painter = ui.painter_at(page_rect);
                painter.rect_filled(page_rect, 0.0, egui::Color32::WHITE);

                for node in &render_plan.nodes {
                    let Some(node_rect) = render_backend::physical_rect_to_egui(
                        page_rect,
                        scene_scale,
                        node.bounds.x.get(),
                        node.bounds.y.get(),
                        node.bounds.width.get(),
                        node.bounds.height.get(),
                    ) else {
                        continue;
                    };
                    let texture = node.image.as_ref().and_then(|image| {
                        let key = format!("{:?}", image.resource_id);
                        self.image_textures.get(&key)
                    });
                    render_backend::paint_document_node_base(
                        &painter,
                        node,
                        node_rect,
                        texture.map(|cached| cached.texture.id()),
                    );
                    paint_document_node_decorative_border(
                        &painter,
                        page_rect,
                        scene_scale,
                        node,
                        &self.image_textures,
                    );
                    let outcome = render_backend::paint_document_node_foreground(
                        &painter,
                        node,
                        node_rect,
                        scene_scale,
                    );
                    if let Some(metrics) = outcome.text_metrics {
                        if let Some(instance) = node.projected_scene_instance.as_ref() {
                            self.projected_text_metrics
                                .insert(instance.instance_id.clone(), metrics.clone());
                        }
                        self.painted_text_nodes += 1;
                        self.source_typography_sections += metrics.source_typography_sections;
                        self.fallback_typography_sections += metrics.fallback_sections;
                        if metrics.shared_resolved_layout {
                            self.shared_resolved_layout_frames += 1;
                        } else {
                            self.backend_fallback_frames += 1;
                        }
                        if outcome.text_clipped {
                            self.clipped_text_nodes += 1;
                        }
                    }
                }
            });
    }
}

#[test]
#[ignore = "requires CHAPTERA_GOLDEN_CARLTON_MARCH and CHAPTERA_GOLDEN_CARLTON_OUT"]
fn golden_carlton_march_clean_pages_use_current_reader_render_backend() {
    use egui_kittest::Harness;
    use sha2::{Digest, Sha256};

    const RASTER_DPI: f64 = 144.0;
    const EMU_PER_INCH: f64 = 914_400.0;

    let fixture = std::env::var_os("CHAPTERA_GOLDEN_CARLTON_MARCH")
        .map(PathBuf::from)
        .expect("CHAPTERA_GOLDEN_CARLTON_MARCH");
    let output_dir = std::env::var_os("CHAPTERA_GOLDEN_CARLTON_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_GOLDEN_CARLTON_OUT");
    fs::create_dir_all(&output_dir).expect("create Carlton golden output directory");

    let bytes = fs::read(&fixture).expect("read exact Carlton March PUB");
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(
        source_sha256, "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3",
        "Carlton March source identity drifted"
    );

    let visual = diagnostic_sweep::open_for_product(&bytes)
        .expect("exact Carlton March must open through current product Reader");
    assert_eq!(visual.document.pages.len(), 3, "Carlton product page count");
    assert_eq!(
        visual.scene.surfaces.len(),
        3,
        "Carlton product surface count"
    );
    assert!(
        visual.document.diagnostics.iter().any(
            |diagnostic| diagnostic.code == "viewer.page_projection.family_profile_applied"
        ),
        "exact Carlton family presentation profile must be active before visual rendering"
    );
    assert_eq!(
        visual.projected_instances.len(),
        4,
        "exact March must admit exactly four visible canonical Cmo scene instances"
    );
    let projected_instance_ids = visual
        .projected_instances
        .iter()
        .map(|projected| projected.scene_instance.instance_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        projected_instance_ids.len(),
        4,
        "canonical projected SceneInstance identities must be distinct"
    );
    assert!(visual.projected_instances.iter().all(|projected| {
        projected.scene_instance.projection_kind
            == chaptera_scene_instance::SceneProjectionKindV1::CmoStorySlot
    }));
    let direct_scene_origins = visual
        .scene
        .nodes
        .iter()
        .map(|node| node.origin.as_canonical().to_string())
        .collect::<BTreeSet<_>>();
    assert!(
        visual.projected_instances.iter().all(|projected| {
            !direct_scene_origins.contains(&projected.scene_instance.origin_node_id)
        }),
        "projected Cmo carriers must not be reparented into direct customer scene nodes"
    );
    let projected_page_counts = visual
        .document
        .pages
        .iter()
        .map(|page| {
            let page_id = page.id.as_canonical().to_string();
            visual
                .projected_instances
                .iter()
                .filter(|projected| projected.scene_instance.target_page_id == page_id)
                .count()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        projected_page_counts,
        vec![1, 2, 1],
        "exact March projected Cmo distribution must stay 1/2/1"
    );

    let mut page_receipts = Vec::new();
    for (page_index, expected_projected_node_count) in
        projected_page_counts.iter().copied().enumerate()
    {
        let plan = build_desktop_page_render_plan(&visual, page_index)
            .expect("current Reader page render plan");
        let projected_node_count = plan
            .nodes
            .iter()
            .filter(|node| node.projected_scene_instance.is_some())
            .count();
        assert_eq!(
            projected_node_count, expected_projected_node_count,
            "render-plan projected instance count must match canonical Viewer adapter"
        );
        assert!(
            plan.nodes
                .iter()
                .filter_map(|node| node.text.as_ref())
                .all(|fragment| !fragment.text.contains('\u{FFFC}')),
            "admitted projected object markers must not paint as U+FFFC missing-glyph boxes"
        );
        let width_px = ((plan.page_size.width.get() as f64 * RASTER_DPI / EMU_PER_INCH)
            .round()
            .max(1.0)) as u32;
        let height_px = ((plan.page_size.height.get() as f64 * RASTER_DPI / EMU_PER_INCH)
            .round()
            .max(1.0)) as u32;

        let visual_for_app = visual.clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width_px as f32, height_px as f32))
            .with_pixels_per_point(1.0)
            .with_max_steps(12)
            .wgpu()
            .build_eframe(move |cc| {
                fallback_font::install(&cc.egui_ctx)
                    .expect("pinned Chaptera fallback font resource must validate");
                GoldenPageOnlyApp::new(visual_for_app, page_index)
            });
        // Harness construction executes initial frames with RawInput's portable
        // 2048px texture ceiling. Carlton contains a proven 2480x2835 image,
        // so delay exact texture upload until the next frame after raising only
        // this headless input capability; source pixels remain unmodified.
        harness.input_mut().max_texture_side = Some(4096);
        harness.state_mut().enable_texture_upload();
        harness.step();

        let image = harness
            .render()
            .expect("headless clean Reader page render must succeed");
        assert_eq!(image.width(), width_px, "golden raster width drift");
        assert_eq!(image.height(), height_px, "golden raster height drift");
        let executed = harness.state();
        let (planned_shared_resolved_layout_frames, planned_backend_fallback_frames) =
            text_layout_disposition_counts(&plan);
        let projected_instance_receipts = plan
            .nodes
            .iter()
            .filter_map(|node| {
                let instance = node.projected_scene_instance.as_ref()?;
                let viewer_projected = visual
                    .projected_instances
                    .iter()
                    .find(|projected| {
                        projected.scene_instance.instance_id == instance.instance_id
                    })
                    .expect("render-plan projected instance must come from Viewer adapter");
                let target_frame = visual
                    .scene
                    .nodes
                    .iter()
                    .find(|candidate| candidate.origin == viewer_projected.target_frame_node_id)
                    .expect("projected target frame remains in resolved customer scene");
                Some(serde_json::json!({
                    "instance_id": instance.instance_id,
                    "origin_node_id": instance.origin_node_id,
                    "target_frame_node_id": viewer_projected
                        .target_frame_node_id
                        .as_canonical()
                        .to_string(),
                    "cmo_slot_index": instance.cmo_slot_index,
                    "cmo_scalar_index": instance.cmo_scalar_index,
                    "target_frame_paint_scalar_end": viewer_projected
                        .target_frame_paint_scalar_end,
                    "story_authority_present": instance.story_authority_id.is_some(),
                    "target_frame_bounds_emu": [
                        target_frame.bounds.x.get(),
                        target_frame.bounds.y.get(),
                        target_frame.bounds.width.get(),
                        target_frame.bounds.height.get(),
                    ],
                    "projected_bounds_emu": [
                        node.bounds.x.get(),
                        node.bounds.y.get(),
                        node.bounds.width.get(),
                        node.bounds.height.get(),
                    ],
                    "carrier_extent_emu": [
                        node.bounds.width.get(),
                        node.bounds.height.get(),
                    ],
                    "text_scalar_count": node
                        .text
                        .as_ref()
                        .map(|text| text.text.chars().count())
                        .unwrap_or(0),
                    "executed_text_metrics": executed
                        .projected_text_metrics
                        .get(&instance.instance_id),
                }))
            })
            .collect::<Vec<_>>();
        let filename = format!("carlton-march-reader-page-{:03}.png", page_index + 1);
        image
            .save(output_dir.join(&filename))
            .expect("write Carlton clean Reader page PNG");

        let typography_sections = plan
            .nodes
            .iter()
            .filter_map(|node| node.text.as_ref())
            .map(|text| text.typography.len())
            .sum::<usize>();
        page_receipts.push(serde_json::json!({
            "page_number": page_index + 1,
            "page_id": visual.document.pages[page_index].id,
            "width_emu": plan.page_size.width.get(),
            "height_emu": plan.page_size.height.get(),
            "raster_width_px": width_px,
            "raster_height_px": height_px,
            "node_count": plan.nodes.len(),
            "projected_scene_instance_count": projected_node_count,
            "projected_instances": projected_instance_receipts,
            "fill_node_count": plan.nodes.iter().filter(|node| node.solid_fill_rgb.is_some()).count(),
            "line_node_count": plan.nodes.iter().filter(|node| node.solid_line.is_some()).count(),
            "image_node_count": plan.nodes.iter().filter(|node| node.image.is_some()).count(),
            "text_node_count": plan.nodes.iter().filter(|node| node.text.is_some()).count(),
            "typography_sections": typography_sections,
            "planned_shared_resolved_layout_frames": planned_shared_resolved_layout_frames,
            "planned_backend_fallback_frames": planned_backend_fallback_frames,
            "executed_text_node_count": executed.painted_text_nodes,
            "executed_source_typography_sections": executed.source_typography_sections,
            "executed_fallback_typography_sections": executed.fallback_typography_sections,
            "shared_resolved_layout_frames": executed.shared_resolved_layout_frames,
            "backend_fallback_frames": executed.backend_fallback_frames,
            "clipped_text_node_count": executed.clipped_text_nodes,
            "png": filename,
        }));
    }

    let receipt = serde_json::json!({
        "schema": "chaptera.reader-golden-carlton-march.v1",
        "source_sha256": source_sha256,
        "page_count": visual.document.pages.len(),
        "scene_surface_count": visual.scene.surfaces.len(),
        "raster_dpi": RASTER_DPI as u32,
        "render_backend": "chaptera-desktop-egui-document-paint",
        "shell_ui_rendered": false,
        "selection_overlay_rendered": false,
        "preview_warning_overlay_rendered": false,
        "family_profile_applied": true,
        "source_font_face_claimed": false,
        "publisher_exact_reflow_claimed": false,
        "text_layout_authority": "shared_resolved_when_admitted_else_backend_fallback",
        "viewer_diagnostic_codes": visual
            .document
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.clone())
            .collect::<Vec<_>>(),
        "projected_scene_instance_ids": projected_instance_ids,
        "projected_page_counts": projected_page_counts,
        "pages": page_receipts,
    });
    fs::write(
        output_dir.join("carlton-march-reader-golden-receipt.json"),
        serde_json::to_vec_pretty(&receipt).expect("serialize Carlton golden receipt"),
    )
    .expect("write Carlton golden receipt");
}

#[test]
#[ignore = "requires CHAPTERA_GOLDEN_SAMPLE_NEWSLETTER and CHAPTERA_GOLDEN_OUT"]
fn golden_sample_newsletter_reference_customer_page_1_uses_shared_typography_render_plan() {
    use egui_kittest::Harness;
    use sha2::{Digest, Sha256};

    let fixture = std::env::var_os("CHAPTERA_GOLDEN_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_GOLDEN_SAMPLE_NEWSLETTER");
    let output_dir = std::env::var_os("CHAPTERA_GOLDEN_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_GOLDEN_OUT");
    fs::create_dir_all(&output_dir).expect("create golden output directory");

    let bytes = fs::read(&fixture).expect("read pinned SampleNewsletter");
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(
        source_sha256, "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf",
        "golden fixture identity drifted"
    );

    let visual = diagnostic_sweep::open_for_product(&bytes)
        .expect("pinned SampleNewsletter must open through product Reader path");
    assert!(
        !visual.typography_runs.is_empty(),
        "Reader must expose bounded source typography"
    );
    let full_family_typography_run_count = visual
        .typography_runs
        .iter()
        .filter(|run| !run.source_font_name.trim().is_empty())
        .count();
    let inherited_full_family_run_count = visual
        .typography_runs
        .iter()
        .filter(|run| {
            !run.source_font_name.trim().is_empty()
                && (run.font_inherited || run.size_inherited)
        })
        .count();
    let size_only_typography_run_count = visual
        .typography_runs
        .iter()
        .filter(|run| {
            run.source_font_name.trim().is_empty()
                && !run.font_inherited
                && run.size_inherited
                && run.text_size_emu > 0
        })
        .count();
    assert_eq!(
        full_family_typography_run_count, 106,
        "the pre-size-only source authority must preserve all 106 full-family SampleNewsletter typography runs"
    );
    assert_eq!(
        inherited_full_family_run_count, 88,
        "the pre-size-only authority must preserve all 88 explicit-FDPP-selector inherited full-family runs"
    );
    assert_eq!(
        size_only_typography_run_count, 17,
        "bounded implicit style-zero size authority adds exactly 17 family-absent SampleNewsletter size-only runs on this pinned fixture"
    );
    assert_eq!(
        visual.typography_runs.len(),
        full_family_typography_run_count + size_only_typography_run_count,
        "SampleNewsletter typography must contain only proven full-family or bounded size-only runs"
    );
    assert!(
        visual
            .typography_runs
            .iter()
            .any(|run| run.source_font_name == "Rockwell Condensed"
                && run.text_size_emu == 24 * 12_700),
        "proven Rockwell Condensed 24pt anchor must reach Viewer"
    );

    // Fixture-only crosswalk: raw Viewer Page 2 is Publisher customer page 1 for this exact pinned SHA.
    // This must never be reused as generic PAGE-role logic.
    let page_offset = 1_usize;
    let plan = build_desktop_page_render_plan(&visual, page_offset)
        .expect("reference customer page 1 shared render plan");
    let typography_sections = plan
        .nodes
        .iter()
        .filter_map(|node| node.text.as_ref())
        .map(|text| text.typography.len())
        .sum::<usize>();
    assert!(
        typography_sections > 0,
        "source typography must reach shared Reader render plan"
    );
    let (shared_resolved_layout_frames, backend_fallback_frames) =
        text_layout_disposition_counts(&plan);
    assert!(
        shared_resolved_layout_frames > 0,
        "SampleNewsletter must exercise at least one shared resolved text-layout frame"
    );

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(24)
        .wgpu()
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            let mut app = ViewerApp::new_with_storage(Some(fixture_for_app), cc.storage);
            app.selected_page = page_offset;
            app
        });
    harness.step();

    let image = harness
        .render()
        .expect("headless Reader render must succeed");
    let png_path = output_dir.join("samplenewsletter-reference-customer-page-001-reader.png");
    image.save(&png_path).expect("write Reader golden PNG");

    let receipt = serde_json::json!({
        "schema": "chaptera.reader-golden-samplenewsletter.v3",
        "source_sha256": source_sha256,
        "viewer_page_number": 2,
        "publisher_reference_customer_page_number": 1,
        "page_selection_basis": "pinned_same_source_crosswalk_only",
        "generic_page_role_claimed": false,
        "typography_run_count": visual.typography_runs.len(),
        "full_family_typography_run_count": full_family_typography_run_count,
        "inherited_full_family_run_count": inherited_full_family_run_count,
        "size_only_typography_run_count": size_only_typography_run_count,
        "render_plan_typography_sections": typography_sections,
        "shared_resolved_layout_frames": shared_resolved_layout_frames,
        "backend_fallback_frames": backend_fallback_frames,
        "source_font_face_claimed": false,
        "publisher_exact_reflow_claimed": false,
        "text_layout_authority": "shared_resolved_when_admitted_else_backend_fallback",
        "png": "samplenewsletter-reference-customer-page-001-reader.png"
    });
    fs::write(
        output_dir.join("samplenewsletter-reader-receipt.json"),
        serde_json::to_vec_pretty(&receipt).expect("serialize golden receipt"),
    )
    .expect("write golden receipt");
}

