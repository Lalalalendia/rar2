use chaptera_viewer_render_plan::{ExplicitRenderTextFontResourceV1, RenderTextFragmentV1};
use pub_viewer::ViewerGeometryDocument;
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
        let families = visual
            .typography_runs
            .iter()
            .map(|run| run.source_font_name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();

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
        fragment: &RenderTextFragmentV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>> {
        let family = admitted_single_family(fragment)?;
        let key = normalize_family(family);
        let font = self.resolved.get(&key)?;
        Some(ExplicitRenderTextFontResourceV1 {
            resource_id: &font.resource_id,
            expected_sha256: &font.sha256,
            face_index: font.face_index,
            default_font_size_emu:
                chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU,
            default_line_height_emu:
                chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
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

        let families = [fontdb::Family::Name(family)];
        let query = fontdb::Query {
            families: &families,
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        let Some(id) = self.database.query(&query) else {
            self.unavailable.insert(key);
            return false;
        };
        let Some(info) = self.database.face(id) else {
            self.unavailable.insert(key);
            return false;
        };
        if !info
            .families
            .iter()
            .any(|(name, _)| normalize_family(name) == key)
        {
            self.unavailable.insert(key);
            return false;
        }
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

fn admitted_single_family(fragment: &RenderTextFragmentV1) -> Option<&str> {
    if fragment.typography.is_empty() {
        return None;
    }

    let mut cursor = fragment.scalar_start;
    let mut family: Option<&str> = None;
    for run in &fragment.typography {
        if run.scalar_start != cursor
            || run.scalar_end <= run.scalar_start
            || run.scalar_end > fragment.scalar_end
        {
            return None;
        }
        let name = run.source_font_name.trim();
        if name.is_empty() {
            return None;
        }
        match family {
            None => family = Some(name),
            Some(existing) if normalize_family(existing) == normalize_family(name) => {}
            Some(_) => return None,
        }
        cursor = run.scalar_end;
    }

    let family = family?;
    (cursor == fragment.scalar_end).then_some(family)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(runs: serde_json::Value) -> RenderTextFragmentV1 {
        serde_json::from_value(serde_json::json!({
            "story_id": "00000000-0000-0000-0000-000000000001",
            "scalar_start": 0,
            "scalar_end": 4,
            "text": "ABCD",
            "line_count": 1,
            "typography": runs
        }))
        .expect("render fragment")
    }

    #[test]
    fn single_family_requires_complete_contiguous_coverage() {
        let value = fragment(serde_json::json!([
            {"scalar_start":0,"scalar_end":2,"source_font_name":"Arial","text_size_emu":152400,"font_inherited":false,"size_inherited":false},
            {"scalar_start":2,"scalar_end":4,"source_font_name":"Arial","text_size_emu":152400,"font_inherited":false,"size_inherited":false}
        ]));
        assert_eq!(admitted_single_family(&value), Some("Arial"));

        let gap = fragment(serde_json::json!([
            {"scalar_start":0,"scalar_end":2,"source_font_name":"Arial","text_size_emu":152400,"font_inherited":false,"size_inherited":false},
            {"scalar_start":3,"scalar_end":4,"source_font_name":"Arial","text_size_emu":152400,"font_inherited":false,"size_inherited":false}
        ]));
        assert_eq!(admitted_single_family(&gap), None);
    }

    #[test]
    fn mixed_family_fails_closed() {
        let value = fragment(serde_json::json!([
            {"scalar_start":0,"scalar_end":2,"source_font_name":"Arial","text_size_emu":152400,"font_inherited":false,"size_inherited":false},
            {"scalar_start":2,"scalar_end":4,"source_font_name":"Times New Roman","text_size_emu":152400,"font_inherited":false,"size_inherited":false}
        ]));
        assert_eq!(admitted_single_family(&value), None);
    }
}
