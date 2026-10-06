use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderTextFragmentV1, RenderTypographyRunV1,
    build_page_render_plan_v1, complete_scalar_source_font_family_v1,
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
    resolved: BTreeMap<(String, bool, bool), ResolvedDesktopFont>,
    unavailable: BTreeSet<(String, bool, bool)>,
    effective_fragment_families: BTreeMap<(String, u32, u32, String), String>,
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
            effective_fragment_families: BTreeMap::new(),
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

        self.effective_fragment_families.clear();
        for page_index in 0..visual.document.pages.len() {
            let Ok(plan) = build_page_render_plan_v1(visual, page_index) else {
                continue;
            };
            for fragment in plan.nodes.iter().filter_map(|node| node.text.as_ref()) {
                if let Some(family) = effective_source_font_family_v1(visual, fragment) {
                    families.insert(family.clone());
                    if complete_scalar_source_font_family_v1(fragment).is_none() {
                        self.effective_fragment_families
                            .insert(fragment_family_key(fragment), family);
                    }
                }
            }
        }

        let mut changed = false;
        for family in families {
            for bold in [false, true] {
                for italic in [false, true] {
                    changed |= self.ensure_family_style(&family, bold, italic);
                }
            }
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
        fragment: &RenderTextFragmentV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>> {
        let family = complete_scalar_source_font_family_v1(fragment).or_else(|| {
            self.effective_fragment_families
                .get(&fragment_family_key(fragment))
                .cloned()
        })?;
        self.resource_for_family(&family)
    }

    fn resource_for_family<'a>(
        &'a self,
        family: &str,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>> {
        self.resource_for_family_style(family, false, false)
    }

    pub fn resource_for_typography_run<'a>(
        &'a self,
        run: &RenderTypographyRunV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>> {
        let (Some(bold), Some(italic)) = (run.bold, run.italic) else {
            return None;
        };
        self.resource_for_family_style(&run.source_font_name, bold, italic)
    }

    pub fn resource_for_current_fragment<'a>(
        &'a self,
        fragment: &RenderTextFragmentV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>> {
        let family = complete_scalar_source_font_family_v1(fragment).or_else(|| {
            self.effective_fragment_families
                .get(&fragment_family_key(fragment))
                .cloned()
        })?;
        if fragment.typography.is_empty() {
            return self.resource_for_family(&family);
        }

        let mut cursor = fragment.scalar_start;
        let mut resolved_style = None;
        let mut saw_absent_style = false;
        for run in &fragment.typography {
            if run.scalar_start != cursor
                || run.scalar_end <= run.scalar_start
                || run.scalar_end > fragment.scalar_end
            {
                return None;
            }
            match (run.bold, run.italic) {
                (None, None) => {
                    if resolved_style.is_some() {
                        return None;
                    }
                    saw_absent_style = true;
                }
                (Some(bold), Some(italic)) => {
                    if saw_absent_style {
                        return None;
                    }
                    match resolved_style {
                        None => resolved_style = Some((bold, italic)),
                        Some(existing) if existing == (bold, italic) => {}
                        Some(_) => return None,
                    }
                }
                _ => return None,
            }
            cursor = run.scalar_end;
        }
        if cursor != fragment.scalar_end {
            return None;
        }

        match resolved_style {
            Some((bold, italic)) => self.resource_for_family_style(&family, bold, italic),
            None if saw_absent_style => self.resource_for_family(&family),
            _ => None,
        }
    }

    fn resource_for_family_style<'a>(
        &'a self,
        family: &str,
        bold: bool,
        italic: bool,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>> {
        let key = (normalize_family(family), bold, italic);
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
        self.resolved
            .iter()
            .filter_map(|((_, bold, italic), font)| {
                (!*bold && !*italic).then_some((
                    font.source_family.as_str(),
                    font.resolved_family.as_str(),
                    font.sha256.as_str(),
                ))
            })
    }

    #[cfg(target_os = "windows")]
    fn ensure_family(&mut self, family: &str) -> bool {
        self.ensure_family_style(family, false, false)
    }

    #[cfg(target_os = "windows")]
    fn ensure_family_style(&mut self, family: &str, bold: bool, italic: bool) -> bool {
        let normalized_family = normalize_family(family);
        let key = (normalized_family.clone(), bold, italic);
        if self.resolved.contains_key(&key) || self.unavailable.contains(&key) {
            return false;
        }

        let expected_weight = if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        };
        let expected_style = if italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        };
        let matching_faces = self
            .database
            .faces()
            .filter(|info| {
                info.weight == expected_weight
                    && info.stretch == fontdb::Stretch::Normal
                    && info.style == expected_style
                    && info
                        .families
                        .iter()
                        .any(|(name, _)| normalize_family(name) == normalized_family)
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
        self.ensure_family_style(family, false, false)
    }

    #[cfg(not(target_os = "windows"))]
    fn ensure_family_style(&mut self, family: &str, bold: bool, italic: bool) -> bool {
        self.unavailable
            .insert((normalize_family(family), bold, italic));
        false
    }
}

fn normalize_family(name: &str) -> String {
    name.trim().to_lowercase()
}

fn fragment_family_key(fragment: &RenderTextFragmentV1) -> (String, u32, u32, String) {
    (
        format!("{:?}", fragment.story_id),
        fragment.scalar_start,
        fragment.scalar_end,
        fragment.text.clone(),
    )
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_environment_font_binds_exact_style_tuple_resources() {
        let mut registry = DesktopSourceFontRegistry::new();
        let source_family = ["Arial", "Times New Roman", "Calibri", "Segoe UI"]
            .into_iter()
            .find(|family| {
                for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                    registry.ensure_family_style(family, bold, italic);
                }
                [(false, false), (true, false), (false, true), (true, true)]
                    .into_iter()
                    .all(|(bold, italic)| {
                        registry
                            .resolved
                            .contains_key(&(normalize_family(family), bold, italic))
                    })
            })
            .expect("windows-latest should expose one bounded family with four unique style faces");

        let resources = [(false, false), (true, false), (false, true), (true, true)]
            .into_iter()
            .map(|(bold, italic)| {
                registry
                    .resource_for_family_style(source_family, bold, italic)
                    .expect("exact style tuple resource")
            })
            .collect::<Vec<_>>();

        let identities = resources
            .iter()
            .map(|resource| {
                (
                    resource.resource_id.to_owned(),
                    resource.expected_sha256.to_owned(),
                    resource.face_index,
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            identities.len(),
            4,
            "Regular/Bold/Italic/BoldItalic must resolve to distinct exact physical identities"
        );
        assert!(resources.iter().all(|resource| !resource.bytes.is_empty()));
        assert!(resources.iter().all(|resource| {
            format!("{:x}", Sha256::digest(resource.bytes)) == resource.expected_sha256
        }));

        let paint_fonts = registry.egui_fonts();
        assert!(resources.iter().all(|resource| {
            paint_fonts.iter().any(|font| {
                font.resource_id == resource.resource_id
                    && font.face_index == resource.face_index
                    && font.bytes == resource.bytes
            })
        }));

        let ctx = eframe::egui::Context::default();
        crate::fallback_font::install_with_additional(&ctx, &paint_fonts)
            .expect("all exact styled resources should register in egui");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "exact Carlton source-font fit measurement; invoked by dedicated Windows acceptance"]
    fn windows_carlton_table_source_font_fit_receipt() {
        use pub_layout::{
            BoundedLayoutEnvironment, BoundedShapingRuntime,
            compatible_natural_line_height_emu_v1, font_fingerprint_sha256,
            shape_bounded_ltr_segment,
        };
        use pub_model::LengthEmu;

        #[derive(serde::Deserialize)]
        struct MeasurementPacket {
            pages: Vec<chaptera_viewer_render_plan::PageRenderPlanV1>,
        }

        let packet_path = std::env::var("CHAPTERA_CARLTON_TABLE_SOURCE_FONT_PACKET")
            .expect("dedicated Carlton measurement must provide packet path");
        let receipt_path = std::env::var("CHAPTERA_CARLTON_TABLE_SOURCE_FONT_RECEIPT")
            .expect("dedicated Carlton measurement must provide receipt path");
        let packet: MeasurementPacket = serde_json::from_slice(
            &std::fs::read(&packet_path).expect("read current Viewer packet"),
        )
        .expect("parse current Viewer packet");

        let mut bounded = Vec::new();
        let mut family_counts = BTreeMap::<String, usize>::new();
        let mut absent_style_count = 0usize;

        for page in &packet.pages {
            for node in &page.nodes {
                let Some(table) = node.table.as_ref() else {
                    continue;
                };
                let Some(inset) = table.uniform_cell_text_inset_emu.filter(|value| *value >= 0)
                else {
                    continue;
                };
                for cell in table.cells.iter().filter(|cell| !cell.text.is_empty()) {
                    let (Some(start), Some(end), Some(bounds)) =
                        (cell.story_scalar_start, cell.story_scalar_end, cell.bounds)
                    else {
                        continue;
                    };
                    let Ok(text_len) = u32::try_from(cell.text.chars().count()) else {
                        continue;
                    };
                    if start >= end || start.checked_add(text_len) != Some(end) {
                        continue;
                    }
                    if cell.typography.is_empty() {
                        continue;
                    }

                    let mut cursor = start;
                    let mut size = None;
                    let mut color = None;
                    let mut family = None::<String>;
                    let mut typography_ok = true;
                    let mut all_style_absent = true;
                    for run in &cell.typography {
                        if run.scalar_start != cursor
                            || run.scalar_end <= run.scalar_start
                            || run.scalar_end > end
                            || run.text_size_emu == 0
                        {
                            typography_ok = false;
                            break;
                        }
                        match size {
                            None => size = Some(run.text_size_emu),
                            Some(existing) if existing == run.text_size_emu => {}
                            Some(_) => {
                                typography_ok = false;
                                break;
                            }
                        }
                        match (color, run.color_rgb) {
                            (None, Some(rgb)) => color = Some(rgb),
                            (Some(existing), Some(rgb)) if existing == rgb => {}
                            _ => {
                                typography_ok = false;
                                break;
                            }
                        }
                        let display = run.source_font_name.trim();
                        if display.is_empty() {
                            typography_ok = false;
                            break;
                        }
                        let normalized = display.to_lowercase();
                        match family.as_ref() {
                            None => family = Some(normalized),
                            Some(existing) if *existing == normalized => {}
                            Some(_) => {
                                typography_ok = false;
                                break;
                            }
                        }
                        all_style_absent &= run.bold.is_none() && run.italic.is_none();
                        cursor = run.scalar_end;
                    }
                    if !typography_ok || cursor != end || !all_style_absent {
                        continue;
                    }

                    let Some(double_inset) = inset.checked_mul(2) else {
                        continue;
                    };
                    let Some(inner_width) = bounds.width.get().checked_sub(double_inset) else {
                        continue;
                    };
                    let Some(inner_height) = bounds.height.get().checked_sub(double_inset) else {
                        continue;
                    };
                    if inner_width <= 0 || inner_height <= 0 || cell.text.contains(&['\r', '\n'][..]) {
                        continue;
                    }

                    let family = family.expect("validated family");
                    *family_counts.entry(family.clone()).or_default() += 1;
                    absent_style_count += 1;
                    bounded.push((
                        family,
                        start,
                        cell.text.clone(),
                        i64::from(size.expect("validated size")),
                        inner_width,
                        inner_height,
                    ));
                }
            }
        }

        assert_eq!(bounded.len(), 13, "exact Carlton bounded TABLE cohort drift");
        assert_eq!(absent_style_count, 13, "Carlton style-absence authority drift");
        assert_eq!(family_counts, BTreeMap::from([("arial".to_owned(), 13)]));

        let mut registry = DesktopSourceFontRegistry::new();
        assert!(registry.ensure_family("Arial"), "Windows host must resolve unique Arial Regular");
        let resource = registry
            .resource_for_family("Arial")
            .expect("environment-resolved Arial Regular resource");
        let actual_sha = format!("{:x}", Sha256::digest(resource.bytes));
        assert_eq!(actual_sha, resource.expected_sha256);

        let mut width_fit = 0usize;
        let mut natural_extent_fit = 0usize;
        let mut width_and_natural_fit = 0usize;
        let mut missing_glyph_cells = 0usize;
        for (_family, scalar_start, text, font_size_emu, inner_width, inner_height) in &bounded {
            let runtime = BoundedShapingRuntime {
                layout: BoundedLayoutEnvironment {
                    engine_revision: "chaptera.carlton-table-source-font-fit.v1".into(),
                    font_set_fingerprint: font_fingerprint_sha256(resource.bytes),
                    resource_fingerprint: resource.resource_id.to_owned(),
                },
                face_index: resource.face_index,
                font_size_emu: LengthEmu::new(*font_size_emu),
                font_bytes: resource.bytes,
            };
            let shaped = shape_bounded_ltr_segment(text, *scalar_start, &runtime)
                .expect("shape bounded Carlton TABLE cell with environment-resolved Arial");
            let cell_width_fit = shaped.total_x_advance.get() <= *inner_width;
            let natural_extent = compatible_natural_line_height_emu_v1(
                resource.bytes,
                resource.face_index,
                LengthEmu::new(*font_size_emu),
            )
            .expect("Arial Regular must satisfy bounded natural metric authority");
            let cell_natural_fit = natural_extent.get() <= *inner_height;
            width_fit += usize::from(cell_width_fit);
            natural_extent_fit += usize::from(cell_natural_fit);
            width_and_natural_fit += usize::from(cell_width_fit && cell_natural_fit);
            missing_glyph_cells += usize::from(shaped.glyphs.iter().any(|glyph| glyph.glyph_id == 0));
        }

        let receipt = serde_json::json!({
            "schema": "chaptera.carlton-table-source-font-fit.v1",
            "environment": "windows-hosted",
            "bounded_cells": bounded.len(),
            "source_family_counts": family_counts,
            "style_absent_cells": absent_style_count,
            "resolved_family": "Arial",
            "resource_id": resource.resource_id,
            "font_sha256": resource.expected_sha256,
            "face_index": resource.face_index,
            "byte_len": resource.bytes.len(),
            "fallback_width_fit_reference": 10,
            "source_font_width_fit": width_fit,
            "source_font_natural_extent_fit": natural_extent_fit,
            "source_font_width_and_natural_fit": width_and_natural_fit,
            "missing_glyph_cells": missing_glyph_cells,
            "host_resolution_disposition": "environment_resolved_not_source_exact"
        });

        std::fs::write(
            receipt_path,
            serde_json::to_vec_pretty(&receipt).expect("serialize Carlton source-font fit receipt"),
        )
        .expect("write Carlton source-font fit receipt");
        println!("{}", serde_json::to_string(&receipt).expect("receipt json"));
    }

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
            .get(&(normalize_family(source_family), false, false))
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
