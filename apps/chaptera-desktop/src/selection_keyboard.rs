//! Focus-safe keyboard routing for the current Chaptera Desktop shell.
//!
//! This module owns only routing decisions. Selection, text-session authority,
//! and durable document mutation remain in the existing Desktop/editor layers.

pub const BASE_NUDGE_EMU: i64 = 118_872;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusOwnerV1 {
    Canvas,
    StoryText,
    HostControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowDirectionV1 {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiersV1 {
    pub shift: bool,
    pub control_or_command: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowRouteV1 {
    RouteStory,
    MoveObject,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrowDecisionV1 {
    pub route: ArrowRouteV1,
    pub dx_emu: i64,
    pub dy_emu: i64,
}

fn delta(direction: ArrowDirectionV1) -> (i64, i64) {
    match direction {
        ArrowDirectionV1::Left => (-BASE_NUDGE_EMU, 0),
        ArrowDirectionV1::Right => (BASE_NUDGE_EMU, 0),
        ArrowDirectionV1::Up => (0, -BASE_NUDGE_EMU),
        ArrowDirectionV1::Down => (0, BASE_NUDGE_EMU),
    }
}

/// Restore the already-approved Desktop routing law without creating a second
/// focus or selection model.
///
/// - ordinary Story arrows remain text-owned;
/// - Alt+Arrow may escape Story focus only for the one selected movable owning
///   text object;
/// - canvas arrows admit exactly one movable selected object;
/// - Shift/Ctrl/Cmd never silently become object-nudge modifiers.
pub fn route_arrow_v1(
    focus: FocusOwnerV1,
    direction: ArrowDirectionV1,
    modifiers: KeyModifiersV1,
    selected_count: usize,
    selected_object_movable: bool,
    active_story_owns_selected_object: bool,
) -> ArrowDecisionV1 {
    if focus == FocusOwnerV1::HostControl {
        return ArrowDecisionV1 {
            route: ArrowRouteV1::Ignored,
            dx_emu: 0,
            dy_emu: 0,
        };
    }

    if focus == FocusOwnerV1::StoryText && !modifiers.alt {
        return ArrowDecisionV1 {
            route: ArrowRouteV1::RouteStory,
            dx_emu: 0,
            dy_emu: 0,
        };
    }

    if modifiers.shift || modifiers.control_or_command {
        return ArrowDecisionV1 {
            route: ArrowRouteV1::Ignored,
            dx_emu: 0,
            dy_emu: 0,
        };
    }

    let admitted = match focus {
        FocusOwnerV1::Canvas => {
            !modifiers.alt && selected_count == 1 && selected_object_movable
        }
        FocusOwnerV1::StoryText => {
            modifiers.alt
                && selected_count == 1
                && selected_object_movable
                && active_story_owns_selected_object
        }
        FocusOwnerV1::HostControl => false,
    };

    if !admitted {
        return ArrowDecisionV1 {
            route: ArrowRouteV1::Ignored,
            dx_emu: 0,
            dy_emu: 0,
        };
    }

    let (dx_emu, dy_emu) = delta(direction);
    ArrowDecisionV1 {
        route: ArrowRouteV1::MoveObject,
        dx_emu,
        dy_emu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_arrow_stays_text_owned_while_alt_arrow_nudges_exact_base_distance() {
        let ordinary = route_arrow_v1(
            FocusOwnerV1::StoryText,
            ArrowDirectionV1::Left,
            KeyModifiersV1::default(),
            1,
            true,
            true,
        );
        assert_eq!(ordinary.route, ArrowRouteV1::RouteStory);

        let alt = route_arrow_v1(
            FocusOwnerV1::StoryText,
            ArrowDirectionV1::Right,
            KeyModifiersV1 {
                alt: true,
                ..Default::default()
            },
            1,
            true,
            true,
        );
        assert_eq!(alt.route, ArrowRouteV1::MoveObject);
        assert_eq!((alt.dx_emu, alt.dy_emu), (BASE_NUDGE_EMU, 0));
    }

    #[test]
    fn canvas_arrow_requires_exactly_one_movable_selection() {
        let admitted = route_arrow_v1(
            FocusOwnerV1::Canvas,
            ArrowDirectionV1::Up,
            KeyModifiersV1::default(),
            1,
            true,
            false,
        );
        assert_eq!(admitted.route, ArrowRouteV1::MoveObject);
        assert_eq!(admitted.dy_emu, -BASE_NUDGE_EMU);

        for selected_count in [0, 2] {
            assert_eq!(
                route_arrow_v1(
                    FocusOwnerV1::Canvas,
                    ArrowDirectionV1::Up,
                    KeyModifiersV1::default(),
                    selected_count,
                    true,
                    false,
                )
                .route,
                ArrowRouteV1::Ignored
            );
        }
    }

    #[test]
    fn modifiers_and_host_focus_do_not_leak_into_object_nudge() {
        for modifiers in [
            KeyModifiersV1 {
                shift: true,
                ..Default::default()
            },
            KeyModifiersV1 {
                control_or_command: true,
                ..Default::default()
            },
            KeyModifiersV1 {
                alt: true,
                ..Default::default()
            },
        ] {
            assert_eq!(
                route_arrow_v1(
                    FocusOwnerV1::Canvas,
                    ArrowDirectionV1::Down,
                    modifiers,
                    1,
                    true,
                    false,
                )
                .route,
                ArrowRouteV1::Ignored
            );
        }

        assert_eq!(
            route_arrow_v1(
                FocusOwnerV1::HostControl,
                ArrowDirectionV1::Down,
                KeyModifiersV1::default(),
                1,
                true,
                false,
            )
            .route,
            ArrowRouteV1::Ignored
        );
    }
}
