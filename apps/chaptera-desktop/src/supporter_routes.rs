use crate::supporter::SupporterAction;
use crate::supporter_attribution::SupportClickAttribution;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SupporterRouteEffect {
    OpenUrl(String),
    CopyText(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SupporterRoutes {
    public_base: Option<String>,
}

impl SupporterRoutes {
    pub(crate) fn disabled() -> Self {
        Self::default()
    }

    pub(crate) fn from_https_origin(origin: &str) -> Option<Self> {
        let origin = origin.trim().trim_end_matches('/');
        let host = origin.strip_prefix("https://")?;

        if host.is_empty()
            || host.contains('/')
            || host.contains('?')
            || host.contains('#')
            || host.chars().any(char::is_whitespace)
        {
            return None;
        }

        Some(Self {
            public_base: Some(origin.to_owned()),
        })
    }

    pub(crate) fn support_effect(
        &self,
        attribution: SupportClickAttribution,
    ) -> Option<SupporterRouteEffect> {
        let base = self.public_base.as_deref()?;
        Some(SupporterRouteEffect::OpenUrl(format!(
            "{base}/support?{}",
            attribution.query_string()
        )))
    }

    pub(crate) fn effect(&self, action: SupporterAction) -> Option<SupporterRouteEffect> {
        let base = self.public_base.as_deref()?;
        match action {
            SupporterAction::Support => Some(SupporterRouteEffect::OpenUrl(format!(
                "{base}/support"
            ))),
            SupporterAction::Share => {
                Some(SupporterRouteEffect::CopyText(base.to_owned()))
            }
            SupporterAction::Report => Some(SupporterRouteEffect::OpenUrl(format!(
                "{base}/report"
            ))),
            SupporterAction::ArchiveHelp => Some(SupporterRouteEffect::OpenUrl(format!(
                "{base}/archive-help"
            ))),
            SupporterAction::Later | SupporterAction::AlreadySupported => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_routes_fail_closed_for_every_action() {
        let routes = SupporterRoutes::disabled();
        for action in [
            SupporterAction::Support,
            SupporterAction::Later,
            SupporterAction::AlreadySupported,
            SupporterAction::Share,
            SupporterAction::Report,
            SupporterAction::ArchiveHelp,
        ] {
            assert_eq!(routes.effect(action), None);
        }
    }

    #[test]
    fn explicit_support_click_can_carry_only_closed_attribution() {
        let routes =
            SupporterRoutes::from_https_origin("https://chaptera.example")
                .expect("valid HTTPS origin");
        let attribution = SupportClickAttribution::baseline(
            crate::supporter::MarketProfile::Ru,
            crate::supporter::ValueReceipt {
                page_count: 2_048,
                kind: crate::supporter::ValueReceiptKind::SearchMatches {
                    match_count: 4_096,
                },
            },
            2,
        )
        .expect("bounded attribution");

        assert_eq!(
            routes.support_effect(attribution),
            Some(SupporterRouteEffect::OpenUrl(
                "https://chaptera.example/support?m=ru&v=control-v1&ve=search&i=2".to_owned()
            ))
        );
    }

    #[test]
    fn canonical_origin_maps_only_to_chaptera_owned_routes() {
        let routes =
            SupporterRoutes::from_https_origin("https://chaptera.example/")
                .expect("valid HTTPS origin");

        assert_eq!(
            routes.effect(SupporterAction::Support),
            Some(SupporterRouteEffect::OpenUrl(
                "https://chaptera.example/support".to_owned()
            ))
        );
        assert_eq!(
            routes.effect(SupporterAction::Share),
            Some(SupporterRouteEffect::CopyText(
                "https://chaptera.example".to_owned()
            ))
        );
        assert_eq!(
            routes.effect(SupporterAction::Report),
            Some(SupporterRouteEffect::OpenUrl(
                "https://chaptera.example/report".to_owned()
            ))
        );
        assert_eq!(
            routes.effect(SupporterAction::ArchiveHelp),
            Some(SupporterRouteEffect::OpenUrl(
                "https://chaptera.example/archive-help".to_owned()
            ))
        );
    }

    #[test]
    fn non_https_or_non_origin_inputs_are_rejected() {
        for candidate in [
            "http://chaptera.example",
            "https://chaptera.example/path",
            "https://chaptera.example?x=1",
            "https://chaptera.example/#fragment",
            "https://",
            "boosty.to/something",
            " https://chaptera.example / ",
        ] {
            assert!(
                SupporterRoutes::from_https_origin(candidate).is_none(),
                "{candidate:?} must fail closed"
            );
        }
    }

    #[test]
    fn local_actions_never_generate_external_effects() {
        let routes =
            SupporterRoutes::from_https_origin("https://chaptera.example")
                .expect("valid HTTPS origin");
        assert_eq!(routes.effect(SupporterAction::Later), None);
        assert_eq!(routes.effect(SupporterAction::AlreadySupported), None);
    }
}
