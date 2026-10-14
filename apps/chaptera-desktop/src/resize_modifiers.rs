//! Adapter only: toolkit modifiers are converted to the canonical interaction mask.

use eframe::egui;
use pub_interaction::ResizeModifierMaskV1;

pub fn mask_from_egui(modifiers: egui::Modifiers) -> ResizeModifierMaskV1 {
    ResizeModifierMaskV1 {
        centered: modifiers.ctrl || modifiers.command,
        aspect_lock: modifiers.shift,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_command_and_shift_map_to_canonical_resize_semantics() {
        assert_eq!(
            mask_from_egui(egui::Modifiers::default()),
            ResizeModifierMaskV1::default()
        );
        assert_eq!(
            mask_from_egui(egui::Modifiers {
                ctrl: true,
                ..Default::default()
            }),
            ResizeModifierMaskV1 {
                centered: true,
                aspect_lock: false,
            }
        );
        assert_eq!(
            mask_from_egui(egui::Modifiers {
                command: true,
                shift: true,
                ..Default::default()
            }),
            ResizeModifierMaskV1 {
                centered: true,
                aspect_lock: true,
            }
        );
        assert_eq!(
            mask_from_egui(egui::Modifiers::SHIFT),
            ResizeModifierMaskV1 {
                centered: false,
                aspect_lock: true,
            }
        );
    }
}
