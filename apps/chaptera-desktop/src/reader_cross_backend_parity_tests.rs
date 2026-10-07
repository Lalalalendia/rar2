//! Cross-backend visual parity probe for the local Reader vs Cloud Reader.
//! Manual CI only: produces clean page-only 144 DPI rasters from the desktop backend.

use super::*;
use egui_kittest::Harness;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};

const CROSS_BACKEND_DPI: f32 = 144.0;
const EMU_PER_INCH: f32 = 914_400.0;

struct CrossBackendPageOnlyApp {
    visual: ViewerGeometryDocument,
    page_index: usize,
    image_textures: BTreeMap<String, CachedImageTexture>,
    texture_upload_enabled: bool,
}

impl CrossBackendPageOnlyApp {
    fn new(visual: ViewerGeometryDocument, page_index: usize) -> Self {
        Self {
            visual,
            page_index,
            image_textures: BTreeMap::new(),
            texture_upload_enabled: false,
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
            .unwrap_or_else(|error| panic!("cross-backend image decode failed for {key}: {error}"));
            let [width, height] = admitted.color_image.size;
            assert!(
                width <= max_texture_side && height <= max_texture_side,
                "cross-backend image {width}x{height} exceeds texture limit {max_texture_side}"
            );
            let texture = ctx.load_texture(
                format!("cross-backend-{key}"),
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
                panic!("cross-backend BorderArt decode failed for {key}: {error}")
            });
            let [width, height] = admitted.color_image.size;
            assert!(
                width <= max_texture_side && height <= max_texture_side,
                "cross-backend BorderArt {width}x{height} exceeds texture limit {max_texture_side}"
            );
            let texture = ctx.load_texture(
                format!("cross-backend-border-{key}"),
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

impl eframe::App for CrossBackendPageOnlyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_image_textures(ctx);
        let render_plan = build_desktop_page_render_plan(&self.visual, self.page_index)
            .expect("cross-backend desktop render plan");
        let scene_scale = CROSS_BACKEND_DPI / EMU_PER_INCH;

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
                    let _ = render_backend::paint_document_node_foreground(
                        &painter,
                        node,
                        node_rect,
                        scene_scale,
                    );
                }
            });
    }
}

#[test]
#[ignore = "manual cross-backend visual parity probe"]
fn sample_newsletter_cross_backend_page_rasters_144dpi() {
    let fixture = std::env::var_os("CHAPTERA_CROSS_BACKEND_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_CROSS_BACKEND_SAMPLE_NEWSLETTER");
    let output = std::env::var_os("CHAPTERA_CROSS_BACKEND_DESKTOP_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_CROSS_BACKEND_DESKTOP_OUT");
    fs::create_dir_all(&output).expect("create cross-backend desktop output");

    let bytes = fs::read(&fixture).expect("read SampleNewsletter");
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(
        source_sha256,
        "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
    );

    let visual = diagnostic_sweep::open_for_product(&bytes)
        .expect("SampleNewsletter must open through local Reader product path");
    assert_eq!(visual.document.pages.len(), 4);

    let mut pages = Vec::new();
    for page_index in 0..visual.document.pages.len() {
        let page = &visual.document.pages[page_index];
        let width_px =
            ((page.width_emu as f32 * CROSS_BACKEND_DPI / EMU_PER_INCH).round().max(1.0)) as u32;
        let height_px =
            ((page.height_emu as f32 * CROSS_BACKEND_DPI / EMU_PER_INCH).round().max(1.0)) as u32;

        let visual_for_app = visual.clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width_px as f32, height_px as f32))
            .with_pixels_per_point(1.0)
            .with_max_steps(12)
            .wgpu()
            .build_eframe(move |cc| {
                fallback_font::install(&cc.egui_ctx)
                    .expect("pinned shared fallback font must install");
                CrossBackendPageOnlyApp::new(visual_for_app, page_index)
            });
        harness.input_mut().max_texture_side = Some(4096);
        harness.state_mut().enable_texture_upload();
        harness.step();

        let image = harness.render().expect("render cross-backend desktop page");
        assert_eq!(image.width(), width_px);
        assert_eq!(image.height(), height_px);

        let filename = format!("SampleNewsletter-page-{}.png", page_index + 1);
        image.save(output.join(&filename)).expect("save desktop page raster");
        pages.push(serde_json::json!({
            "page": page_index + 1,
            "page_id": page.id,
            "width_emu": page.width_emu,
            "height_emu": page.height_emu,
            "width_px": width_px,
            "height_px": height_px,
            "png": filename,
        }));
    }

    let receipt = serde_json::json!({
        "schema": "chaptera.reader-cross-backend-desktop.v1",
        "source_sha256": source_sha256,
        "fixture": "SampleNewsletter",
        "dpi": CROSS_BACKEND_DPI as u32,
        "backend": "chaptera-desktop-egui-wgpu",
        "page_count": pages.len(),
        "pages": pages,
    });
    fs::write(
        output.join("desktop-receipt.json"),
        serde_json::to_vec_pretty(&receipt).expect("serialize desktop receipt"),
    )
    .expect("write desktop receipt");
}
