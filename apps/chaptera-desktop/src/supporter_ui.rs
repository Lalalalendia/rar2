use crate::supporter::{
    MarketProfile, SupporterAction, ValueReceipt, control_copy, format_receipt,
};
use eframe::egui;

pub(crate) fn show_supporter_panel(
    ui: &mut egui::Ui,
    market: MarketProfile,
    receipt: ValueReceipt,
) -> Option<SupporterAction> {
    let copy = control_copy(market);
    let receipt = format_receipt(receipt, market);
    let mut action = None;

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.vertical(|ui| {
            ui.strong(receipt);
            ui.label(copy.body);

            ui.horizontal_wrapped(|ui| {
                if ui.button(copy.primary).clicked() {
                    action = Some(SupporterAction::Support);
                }
                if ui.small_button(copy.later).clicked() {
                    action = Some(SupporterAction::Later);
                }
                if ui.small_button(copy.already_supported).clicked() {
                    action = Some(SupporterAction::AlreadySupported);
                }
            });

            ui.horizontal_wrapped(|ui| {
                if ui.small_button(copy.share).clicked() {
                    action = Some(SupporterAction::Share);
                }
                if ui.small_button(copy.report).clicked() {
                    action = Some(SupporterAction::Report);
                }
                if ui.small_button(copy.archive_help).clicked() {
                    action = Some(SupporterAction::ArchiveHelp);
                }
            });
        });
    });
    ui.add_space(6.0);

    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supporter::ValueReceiptKind;

    #[test]
    fn panel_copy_is_complete_without_mascot_artwork() {
        for market in [MarketProfile::Us, MarketProfile::Uk, MarketProfile::Ru] {
            let copy = control_copy(market);
            assert!(!copy.body.is_empty());
            assert!(!copy.primary.is_empty());
            assert!(!copy.later.is_empty());
            assert!(!copy.already_supported.is_empty());
            assert!(!copy.share.is_empty());
            assert!(!copy.report.is_empty());
            assert!(!copy.archive_help.is_empty());
        }
    }

    #[test]
    fn receipt_renderer_does_not_require_document_identity() {
        let receipt = ValueReceipt {
            page_count: 3,
            kind: ValueReceiptKind::TextCopied,
        };
        assert_eq!(
            format_receipt(receipt, MarketProfile::Us),
            "Text recovered and copied"
        );
        assert_eq!(
            format_receipt(receipt, MarketProfile::Ru),
            "Текст восстановлен и скопирован"
        );
    }
}
