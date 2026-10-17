//! Chaptera visual tokens reused by the offline Rescue workspace.
//! Based on the existing Chaptera Editor Canva concept; no remote assets.
use eframe::egui::{self, Color32, RichText, Stroke};

pub fn accent() -> Color32 {
    Color32::from_rgb(105, 83, 232)
}
pub fn accent_soft() -> Color32 {
    Color32::from_rgb(240, 237, 255)
}
pub fn canvas() -> Color32 {
    Color32::from_rgb(233, 235, 240)
}
pub fn border() -> Color32 {
    Color32::from_rgb(227, 229, 237)
}
pub fn ink() -> Color32 {
    Color32::from_rgb(34, 37, 54)
}
pub fn muted() -> Color32 {
    Color32::from_rgb(96, 103, 121)
}
pub fn success() -> Color32 {
    Color32::from_rgb(27, 130, 101)
}
pub fn warning() -> Color32 {
    Color32::from_rgb(158, 105, 27)
}
pub fn danger() -> Color32 {
    Color32::from_rgb(180, 61, 71)
}

pub fn configure(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = canvas();
    visuals.window_fill = Color32::WHITE;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.selection.bg_fill = accent();
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, ink());
    visuals.widgets.hovered.weak_bg_fill = accent_soft();
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, accent());
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(13.0, 8.0);
    });
}

pub fn card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, border()))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_width((width - 38.0).max(200.0));
            add_contents(ui);
        });
}

pub fn kicker(ui: &mut egui::Ui, label: &str) {
    ui.label(RichText::new(label).size(11.0).strong().color(accent()));
}

pub fn headline(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(27.0).strong().color(ink()));
    ui.label(RichText::new(subtitle).size(13.0).color(muted()));
}

pub fn pill(ui: &mut egui::Ui, title: &str, color: Color32) {
    egui::Frame::new()
        .fill(accent_soft())
        .corner_radius(egui::CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.label(RichText::new(title).size(11.0).strong().color(color));
        });
}

pub fn metric(ui: &mut egui::Ui, caption: &str, number: usize, tint: Color32) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(number.to_string())
                .size(24.0)
                .strong()
                .color(tint),
        );
        ui.label(RichText::new(caption).size(12.0).color(muted()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chaptera_editor_palette_is_stable() {
        assert_eq!(accent(), Color32::from_rgb(105, 83, 232));
        assert_eq!(ink(), Color32::from_rgb(34, 37, 54));
        assert_eq!(canvas(), Color32::from_rgb(233, 235, 240));
        assert_eq!(border(), Color32::from_rgb(227, 229, 237));
        assert_eq!(muted(), Color32::from_rgb(96, 103, 121));
    }
}
