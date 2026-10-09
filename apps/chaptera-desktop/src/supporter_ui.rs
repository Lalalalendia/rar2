use crate::supporter::{MarketProfile, SupporterAction, ValueReceipt, ValueReceiptKind};
use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SupporterCopy {
    body: &'static str,
    primary: &'static str,
    later: &'static str,
    already_supported: &'static str,
    share: &'static str,
    report: &'static str,
    archive_help: &'static str,
}

fn control_copy(profile: MarketProfile) -> SupporterCopy {
    match profile {
        MarketProfile::Us => SupporterCopy {
            body: "Chaptera stays free. If it saved you work, you can support continued development.",
            primary: "Support development",
            later: "Later",
            already_supported: "Already supported",
            share: "Share Chaptera",
            report: "Report a problem",
            archive_help: "Large archive?",
        },
        MarketProfile::Uk => SupporterCopy {
            body: "Chaptera stays free. If it helped, you can support continued development.",
            primary: "Support development",
            later: "Later",
            already_supported: "Already supported",
            share: "Share Chaptera",
            report: "Report a problem",
            archive_help: "Large archive?",
        },
        MarketProfile::Ru => SupporterCopy {
            body: "Chaptera остаётся бесплатной. Если программа выручила — можно поддержать разработку.",
            primary: "Поддержать разработку",
            later: "Позже",
            already_supported: "Уже поддержал",
            share: "Поделиться",
            report: "Сообщить о проблеме",
            archive_help: "Большой архив?",
        },
        MarketProfile::NeutralEnglish => SupporterCopy {
            body: "Chaptera stays free. You can support continued development after it has helped you.",
            primary: "Support development",
            later: "Later",
            already_supported: "Already supported",
            share: "Share Chaptera",
            report: "Report a problem",
            archive_help: "Large archive?",
        },
    }
}

fn format_receipt(receipt: ValueReceipt, profile: MarketProfile) -> String {
    match (profile, receipt.kind) {
        (
            MarketProfile::Ru,
            ValueReceiptKind::Reading {
                text_searchable: true,
            },
        ) => format!("Страниц: {} · поиск по тексту", receipt.page_count),
        (
            MarketProfile::Ru,
            ValueReceiptKind::Reading {
                text_searchable: false,
            },
        ) => format!("Страниц открыто: {}", receipt.page_count),
        (MarketProfile::Ru, ValueReceiptKind::SearchMatches { match_count }) => {
            format!(
                "Страниц: {} · совпадений: {match_count}",
                receipt.page_count
            )
        }
        (MarketProfile::Ru, ValueReceiptKind::TextCopied) => {
            "Текст восстановлен и скопирован".to_owned()
        }
        (
            _,
            ValueReceiptKind::Reading {
                text_searchable: true,
            },
        ) => format!("{} pages · text searchable", receipt.page_count),
        (
            _,
            ValueReceiptKind::Reading {
                text_searchable: false,
            },
        ) => format!("{} pages opened", receipt.page_count),
        (_, ValueReceiptKind::SearchMatches { match_count }) => {
            format!("{} pages · {match_count} matches found", receipt.page_count)
        }
        (_, ValueReceiptKind::TextCopied) => "Text recovered and copied".to_owned(),
    }
}

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
    fn market_specific_baseline_copy_is_preserved() {
        assert_eq!(control_copy(MarketProfile::Us).primary, "Support development");
        assert_eq!(control_copy(MarketProfile::Uk).primary, "Support development");
        assert_eq!(control_copy(MarketProfile::Ru).primary, "Поддержать разработку");
        assert!(!control_copy(MarketProfile::Ru).body.contains("пожертв"));
        assert!(!control_copy(MarketProfile::Ru).body.contains("донат"));
    }

    #[test]
    fn receipt_renderer_uses_only_bounded_result_facts() {
        let search = ValueReceipt {
            page_count: 14,
            kind: ValueReceiptKind::SearchMatches { match_count: 7 },
        };
        assert_eq!(
            format_receipt(search, MarketProfile::Us),
            "14 pages · 7 matches found"
        );
        assert_eq!(
            format_receipt(search, MarketProfile::Ru),
            "Страниц: 14 · совпадений: 7"
        );

        let copied = ValueReceipt {
            page_count: 3,
            kind: ValueReceiptKind::TextCopied,
        };
        assert_eq!(
            format_receipt(copied, MarketProfile::Us),
            "Text recovered and copied"
        );
        assert_eq!(
            format_receipt(copied, MarketProfile::Ru),
            "Текст восстановлен и скопирован"
        );
    }
}
