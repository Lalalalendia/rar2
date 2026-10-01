use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderTextFragmentV1, build_page_render_plan_v1,
    effective_source_font_family_v1,
};
use pub_viewer::ViewerGeometryDocument;
#[cfg(target_os = "windows")]
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
struct ResolvedDesktopFont {
    source_family: String,
    resolved_family: String,
    resource_id: String,
    sha256: String,
    face_index: u32,
    bytes: Vec<u8>,
}

pub struct DesktopSourceFontRegistry {
    #[cfg(target_os = "windows")]
    database: fontdb::Database,
    resolved: BTreeMap<String, ResolvedDesktopFont>,
    unavailable: BTreeSet<String>,
}

impl Default for DesktopSourceFontRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopSourceFontRegistry {
    pub fn new() -> Self {
        #[cfg(target_os = "windows")]
        let database = {
            let mut database = fontdb::Database::new();
            database.load_system_fonts();
            database
        };

        Self {
            #[cfg(target_os = "windows")]
            database,
            resolved: BTreeMap::new(),
            unavailable: BTreeSet::new(),
        }
    }

    pub fn ensure_visual_fonts(&mut self, visual: &ViewerGeometryDocument) -> bool {
        let mut families = visual
            .typography_runs
            .iter()
            .map(|run| run.source_font_name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();

        for page_index in 0..visual.document.pages.len() {
            let Ok(plan) = build_page_render_plan_v1(visual, page_index) else {
                continue;
            };
            for fragment in plan.nodes.iter().filter_map(|node| node.text.as_ref()) {
                if let Some(family) = effective_source_font_family_v1(visual, fragment) {
                    families.insert(family);
                }
            }
        }

        let mut changed = false;
        for family in families {
            changed |= self.ensure_family(&family);
        }
        changed
    }

    pub fn egui_fonts(&self) -> Vec<crate::fallback_font::AdditionalFont<'_>> {
        self.resolved
            .values()
            .map(|font| crate::fallback_font::AdditionalFont {
                resource_id: &font.resource_id,
                bytes: &font.bytes,
                face_index: font.face_index,
            })
            .collect()
    }

    pub fn resource_for_fragment<'a>(
        &'a self,
        visual: &ViewerGeometryDocument,
        fragment: &RenderTextFragmentV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>> {
        let family = effective_source_font_family_v1(visual, fragment)?;
        self.resource_for_family(&family)
    }

    fn resource_for_family<'a>(
        &'a self,
        family: &str,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>> {
        let key = normalize_family(family);
        let font = self.resolved.get(&key)?;
        Some(ExplicitRenderTextFontResourceV1 {
            resource_id: &font.resource_id,
            expected_sha256: &font.sha256,
            face_index: font.face_index,
            default_font_size_emu: chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU,
            default_line_height_emu: chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
            bytes: &font.bytes,
        })
    }

    pub fn resolved_count(&self) -> usize {
        self.resolved.len()
    }

    pub fn resolved_families(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.resolved.values().map(|font| {
            (
                font.source_family.as_str(),
                font.resolved_family.as_str(),
                font.sha256.as_str(),
            )
        })
    }

    #[cfg(target_os = "windows")]
    fn ensure_family(&mut self, family: &str) -> bool {
        let key = normalize_family(family);
        if self.resolved.contains_key(&key) || self.unavailable.contains(&key) {
            return false;
        }

        let matching_faces = self
            .database
            .faces()
            .filter(|info| {
                info.weight == fontdb::Weight::NORMAL
                    && info.stretch == fontdb::Stretch::Normal
                    && info.style == fontdb::Style::Normal
                    && info
                        .families
                        .iter()
                        .any(|(name, _)| normalize_family(name) == key)
            })
            .map(|info| info.id)
            .collect::<Vec<_>>();
        let [id] = matching_faces.as_slice() else {
            self.unavailable.insert(key);
            return false;
        };
        let id = *id;
        let Some(info) = self.database.face(id) else {
            self.unavailable.insert(key);
            return false;
        };
        let resolved_family = info
            .families
            .first()
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| family.to_owned());

        let Some((bytes, face_index)) = self
            .database
            .with_face_data(id, |bytes, face_index| (bytes.to_vec(), face_index))
        else {
            self.unavailable.insert(key);
            return false;
        };
        if bytes.is_empty() {
            self.unavailable.insert(key);
            return false;
        }

        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let resource_id = format!(
            "chaptera.desktop.environment-font.{}.face{}",
            sha256, face_index
        );
        self.resolved.insert(
            key,
            ResolvedDesktopFont {
                source_family: family.to_owned(),
                resolved_family,
                resource_id,
                sha256,
                face_index,
                bytes,
            },
        );
        true
    }

    #[cfg(not(target_os = "windows"))]
    fn ensure_family(&mut self, family: &str) -> bool {
        self.unavailable.insert(normalize_family(family));
        false
    }
}

fn normalize_family(name: &str) -> String {
    name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_environment_font_binds_identical_layout_and_paint_bytes() {
        let mut registry = DesktopSourceFontRegistry::new();
        let source_family = ["Arial", "Times New Roman", "Segoe UI", "Calibri"]
            .into_iter()
            .find(|family| registry.ensure_family(family))
            .expect("windows-latest should expose at least one unique normal system face");

        let layout_font = registry
            .resource_for_family(source_family)
            .expect("resolved layout font resource");
        let paint_fonts = registry.egui_fonts();
        let paint_font = paint_fonts
            .iter()
            .find(|font| font.resource_id == layout_font.resource_id)
            .expect("same resource must be registered for egui paint");

        assert_eq!(paint_font.face_index, layout_font.face_index);
        assert_eq!(paint_font.bytes, layout_font.bytes);
        assert_eq!(
            format!("{:x}", Sha256::digest(layout_font.bytes)),
            layout_font.expected_sha256
        );

        let ctx = eframe::egui::Context::default();
        crate::fallback_font::install_with_additional(&ctx, &paint_fonts)
            .expect("exact source font bytes should register in egui");

        let resolved = registry
            .resolved
            .get(&normalize_family(source_family))
            .expect("resolved source family");
        let receipt = serde_json::json!({
            "schema": "chaptera.desktop-source-font-environment-receipt.v1",
            "environment": "windows-hosted",
            "source_family": resolved.source_family,
            "resolved_family": resolved.resolved_family,
            "resource_id": resolved.resource_id,
            "sha256": resolved.sha256,
            "face_index": resolved.face_index,
            "byte_len": resolved.bytes.len(),
            "layout_and_paint_bytes_identical": true,
            "egui_registration_succeeded": true
        });

        if let Ok(path) = std::env::var("CHAPTERA_SOURCE_FONT_RECEIPT") {
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&receipt).expect("serialize source-font receipt"),
            )
            .expect("write source-font receipt");
        }
        println!("{}", serde_json::to_string(&receipt).expect("receipt json"));
    }
}
