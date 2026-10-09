use crate::locale;
use crate::supporter::{
    MarketProfile, SupporterAction, SupporterState, ValueReceipt, ValueTracker,
};
use crate::supporter_attribution::SupportClickAttribution;
use crate::supporter_routes::{SupporterRouteEffect, SupporterRoutes};
use crate::supporter_ui;
use eframe::egui;
use std::time::{SystemTime, UNIX_EPOCH};

const SUPPORTER_STORAGE_KEY: &str = "chaptera.supporter.v1";

#[derive(Debug)]
pub(crate) struct SupporterShell {
    state: SupporterState,
    seen_receipt: Option<ValueReceipt>,
    prompt_visible: bool,
    market: MarketProfile,
    routes: SupporterRoutes,
    action_status: Option<String>,
}

impl Default for SupporterShell {
    fn default() -> Self {
        Self::new(MarketProfile::NeutralEnglish, SupporterRoutes::disabled())
    }
}

pub(crate) fn restore_state(storage: Option<&dyn eframe::Storage>) -> SupporterState {
    storage
        .and_then(|storage| storage.get_string(SUPPORTER_STORAGE_KEY))
        .and_then(|raw| SupporterState::from_json_str(&raw))
        .unwrap_or_default()
}

pub(crate) fn save_state(storage: &mut dyn eframe::Storage, state: &SupporterState) {
    storage.set_string(SUPPORTER_STORAGE_KEY, state.to_json_string());
}

impl SupporterShell {
    fn new(market: MarketProfile, routes: SupporterRoutes) -> Self {
        Self {
            state: SupporterState::default(),
            seen_receipt: None,
            prompt_visible: false,
            market,
            routes,
            action_status: None,
        }
    }

    pub(crate) fn from_environment(storage: Option<&dyn eframe::Storage>) -> Self {
        let locale = locale::detect_user_locale();
        let market = MarketProfile::from_locale(locale.as_ref().map(locale::DetectedLocale::raw));
        let routes = std::env::var("CHAPTERA_SITE_ORIGIN")
            .ok()
            .as_deref()
            .and_then(SupporterRoutes::from_https_origin)
            .unwrap_or_else(SupporterRoutes::disabled);
        let mut shell = Self::new(market, routes);
        shell.state = restore_state(storage);
        shell
    }

    pub(crate) fn save(&self, storage: &mut dyn eframe::Storage) {
        save_state(storage, &self.state);
    }

    pub(crate) fn reset_for_workflow(&mut self) {
        self.seen_receipt = None;
        self.prompt_visible = false;
        self.action_status = None;
    }

    pub(crate) fn sync_and_show(&mut self, ctx: &egui::Context, value: &ValueTracker) {
        let now = current_unix_seconds();
        self.sync_prompt(value, now);

        if !self.prompt_visible {
            return;
        }

        let Some(receipt) = value.receipt() else {
            self.reset_for_workflow();
            return;
        };

        let mut action = None;
        egui::TopBottomPanel::bottom("chaptera-supporter")
            .resizable(false)
            .min_height(88.0)
            .max_height(132.0)
            .show(ctx, |ui| {
                action = supporter_ui::show_supporter_panel(ui, self.market, receipt);
                if let Some(status) = self.action_status.as_deref() {
                    ui.small(status);
                }
            });

        if let Some(action) = action {
            self.handle_action(ctx, receipt, action, now);
        }
    }

    fn sync_prompt(&mut self, value: &ValueTracker, now: i64) {
        let receipt = value.receipt();
        if receipt != self.seen_receipt {
            self.seen_receipt = receipt;
            if receipt.is_some() {
                self.state.record_meaningful_success(now);
            }
        }

        if self.prompt_visible || !self.market.is_active_target() || receipt.is_none() {
            return;
        }

        if self.state.can_prompt(now) {
            self.state.record_prompt_shown(now);
            self.prompt_visible = true;
        }
    }

    fn handle_action(
        &mut self,
        ctx: &egui::Context,
        receipt: ValueReceipt,
        action: SupporterAction,
        now: i64,
    ) {
        match action {
            SupporterAction::Later => {
                self.state.record_later(now);
                self.prompt_visible = false;
                self.action_status = None;
            }
            SupporterAction::AlreadySupported => {
                self.state.record_already_supported(now);
                self.prompt_visible = false;
                self.action_status = None;
            }
            SupporterAction::Support => {
                let attribution =
                    self.state
                        .current_prompt_impression_index(now)
                        .and_then(|impression_index| {
                            SupportClickAttribution::for_action(
                                SupporterAction::Support,
                                self.market,
                                receipt,
                                impression_index,
                            )
                        });
                let effect = attribution.and_then(|value| self.routes.support_effect(value));
                if dispatch_effect(ctx, effect) {
                    self.state.record_support_clicked(now);
                    self.prompt_visible = false;
                    self.action_status = None;
                } else {
                    self.action_status = Some(route_unavailable_copy(self.market).to_owned());
                }
            }
            SupporterAction::Share | SupporterAction::Report | SupporterAction::ArchiveHelp => {
                if dispatch_effect(ctx, self.routes.effect(action)) {
                    self.action_status = success_copy(action, self.market).map(str::to_owned);
                } else {
                    self.action_status = Some(route_unavailable_copy(self.market).to_owned());
                }
            }
        }
    }
}

fn dispatch_effect(ctx: &egui::Context, effect: Option<SupporterRouteEffect>) -> bool {
    match effect {
        Some(SupporterRouteEffect::OpenUrl(url)) => {
            ctx.open_url(egui::OpenUrl { url, new_tab: true });
            true
        }
        Some(SupporterRouteEffect::CopyText(text)) => {
            ctx.copy_text(text);
            true
        }
        None => false,
    }
}

fn route_unavailable_copy(profile: MarketProfile) -> &'static str {
    match profile {
        MarketProfile::Ru => "Ссылка поддержки пока не настроена.",
        _ => "Support link is not configured yet.",
    }
}

fn success_copy(action: SupporterAction, profile: MarketProfile) -> Option<&'static str> {
    match (action, profile) {
        (SupporterAction::Share, MarketProfile::Ru) => Some("Ссылка скопирована."),
        (SupporterAction::Share, _) => Some("Link copied."),
        _ => None,
    }
}

fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supporter::{OpenStatus, ValueEvent};

    fn valued_tracker() -> ValueTracker {
        let mut tracker = ValueTracker::default();
        tracker.observe(ValueEvent::DocumentOpened {
            status: OpenStatus::Supported,
            page_count: 4,
            text_searchable: true,
            initial_page: 0,
        });
        tracker.observe(ValueEvent::SearchMatchCopied);
        tracker
    }

    #[test]
    fn neutral_market_records_value_but_never_prompts() {
        let tracker = valued_tracker();
        let mut shell =
            SupporterShell::new(MarketProfile::NeutralEnglish, SupporterRoutes::disabled());

        shell.sync_prompt(&tracker, 1_000);

        assert!(!shell.prompt_visible);
        assert!(shell.state.can_prompt(1_000));
    }

    #[test]
    fn active_market_prompt_is_post_value_and_has_bounded_impression() {
        let mut tracker = ValueTracker::default();
        let mut shell = SupporterShell::new(MarketProfile::Us, SupporterRoutes::disabled());

        shell.sync_prompt(&tracker, 1_000);
        assert!(!shell.prompt_visible);

        tracker = valued_tracker();
        shell.sync_prompt(&tracker, 1_001);

        assert!(shell.prompt_visible);
        assert_eq!(shell.state.current_prompt_impression_index(1_001), Some(1));
    }

    #[test]
    fn reset_clears_only_transient_shell_state() {
        let tracker = valued_tracker();
        let mut shell = SupporterShell::new(MarketProfile::Us, SupporterRoutes::disabled());
        shell.sync_prompt(&tracker, 1_000);
        assert!(shell.prompt_visible);

        shell.reset_for_workflow();

        assert!(!shell.prompt_visible);
        assert_eq!(shell.seen_receipt, None);
        assert_eq!(shell.action_status, None);
        assert!(shell.state.current_prompt_impression_index(1_000).is_some());
    }

    #[test]
    fn desktop_mount_is_thin_and_route_configuration_stays_out_of_main() {
        let source = include_str!("main.rs");
        assert!(source.contains("mod supporter_shell;"));
        assert!(source.contains("supporter_shell: supporter_shell::SupporterShell"));
        assert!(source.contains("self.supporter_shell"));
        assert!(source.contains(".sync_and_show(ctx, &self.supporter_value"));
        assert!(!source.contains("CHAPTERA_SITE_ORIGIN"));
        assert!(!source.contains("/archive-help"));
    }
}
