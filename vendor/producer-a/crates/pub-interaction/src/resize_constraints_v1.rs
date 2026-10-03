//! Deterministic RectEMU resize constraints V1.
//!
//! This is the Rust projection of the already-proven source-neutral V1 law
//! retained in services/editor-api/resize_constraints_v1.py. The planner sees
//! immutable geometry, one semantic resize handle, and a semantic modifier
//! mask. It owns no pointer pixels, snapping candidates, object identity, or
//! authoring mutation.

use crate::ResizeHandle;
use pub_model::{LengthEmu, RectEmu};
use std::fmt;

pub const RESIZE_CONSTRAINT_PROTOCOL_V1: &str = "chaptera.resize-constraint.v1";
pub const RESIZE_CONSTRAINT_MAX_SAFE_EMU_V1: i64 = 9_007_199_254_740_991;
pub const RESIZE_CONSTRAINT_MIN_SAFE_EMU_V1: i64 = -RESIZE_CONSTRAINT_MAX_SAFE_EMU_V1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResizeModifierMaskV1 {
    pub centered: bool,
    pub aspect_lock: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeAspectControlAxisV1 {
    X,
    Y,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeConstraintPlanV1 {
    pub handle: ResizeHandle,
    pub modifiers: ResizeModifierMaskV1,
    pub base_rect: RectEmu,
    pub raw_target_rect: RectEmu,
    pub constrained_rect: RectEmu,
    pub centered_applied: bool,
    pub aspect_applied: bool,
    pub aspect_control_axis: Option<ResizeAspectControlAxisV1>,
    pub changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeConstraintErrorV1 {
    InvalidRect(&'static str),
    UnsafeEmu(&'static str),
    CrossedAxis(&'static str),
    CenterParity(&'static str),
}

impl fmt::Display for ResizeConstraintErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRect(label) => {
                write!(formatter, "{label} must have positive, non-overflowing bounds")
            }
            Self::UnsafeEmu(label) => {
                write!(formatter, "{label} is outside the V1 JavaScript-safe EMU range")
            }
            Self::CrossedAxis(label) => {
                write!(formatter, "{label} resize crossed the fixed center/opposite edge")
            }
            Self::CenterParity(label) => {
                write!(formatter, "{label} extent parity cannot preserve doubled center")
            }
        }
    }
}

impl std::error::Error for ResizeConstraintErrorV1 {}

fn safe_i128(value: i128, label: &'static str) -> Result<i64, ResizeConstraintErrorV1> {
    if value < i128::from(RESIZE_CONSTRAINT_MIN_SAFE_EMU_V1)
        || value > i128::from(RESIZE_CONSTRAINT_MAX_SAFE_EMU_V1)
    {
        return Err(ResizeConstraintErrorV1::UnsafeEmu(label));
    }
    i64::try_from(value).map_err(|_| ResizeConstraintErrorV1::UnsafeEmu(label))
}

fn checked_value(value: i64, label: &'static str) -> Result<i64, ResizeConstraintErrorV1> {
    safe_i128(i128::from(value), label)
}

fn rect_right(rect: RectEmu, label: &'static str) -> Result<i64, ResizeConstraintErrorV1> {
    safe_i128(
        i128::from(rect.x.get()) + i128::from(rect.width.get()),
        label,
    )
}

fn rect_bottom(rect: RectEmu, label: &'static str) -> Result<i64, ResizeConstraintErrorV1> {
    safe_i128(
        i128::from(rect.y.get()) + i128::from(rect.height.get()),
        label,
    )
}

fn validate_rect(rect: RectEmu, label: &'static str) -> Result<(), ResizeConstraintErrorV1> {
    checked_value(rect.x.get(), label)?;
    checked_value(rect.y.get(), label)?;
    checked_value(rect.width.get(), label)?;
    checked_value(rect.height.get(), label)?;
    if rect.width.get() <= 0 || rect.height.get() <= 0 {
        return Err(ResizeConstraintErrorV1::InvalidRect(label));
    }
    rect_right(rect, label)?;
    rect_bottom(rect, label)?;
    Ok(())
}

const fn active_x(handle: ResizeHandle) -> bool {
    matches!(
        handle,
        ResizeHandle::TopLeft
            | ResizeHandle::TopRight
            | ResizeHandle::Left
            | ResizeHandle::Right
            | ResizeHandle::BottomLeft
            | ResizeHandle::BottomRight
    )
}

const fn active_y(handle: ResizeHandle) -> bool {
    matches!(
        handle,
        ResizeHandle::TopLeft
            | ResizeHandle::Top
            | ResizeHandle::TopRight
            | ResizeHandle::BottomLeft
            | ResizeHandle::Bottom
            | ResizeHandle::BottomRight
    )
}

const fn negative_x(handle: ResizeHandle) -> bool {
    matches!(
        handle,
        ResizeHandle::TopLeft | ResizeHandle::Left | ResizeHandle::BottomLeft
    )
}

const fn positive_x(handle: ResizeHandle) -> bool {
    matches!(
        handle,
        ResizeHandle::TopRight | ResizeHandle::Right | ResizeHandle::BottomRight
    )
}

const fn negative_y(handle: ResizeHandle) -> bool {
    matches!(
        handle,
        ResizeHandle::TopLeft | ResizeHandle::Top | ResizeHandle::TopRight
    )
}

const fn positive_y(handle: ResizeHandle) -> bool {
    matches!(
        handle,
        ResizeHandle::BottomLeft | ResizeHandle::Bottom | ResizeHandle::BottomRight
    )
}

fn center2_x(base: RectEmu) -> Result<i64, ResizeConstraintErrorV1> {
    safe_i128(
        i128::from(base.x.get()) + i128::from(rect_right(base, "base.right")?),
        "base.center2_x",
    )
}

fn center2_y(base: RectEmu) -> Result<i64, ResizeConstraintErrorV1> {
    safe_i128(
        i128::from(base.y.get()) + i128::from(rect_bottom(base, "base.bottom")?),
        "base.center2_y",
    )
}

fn desired_width(
    base: RectEmu,
    raw: RectEmu,
    handle: ResizeHandle,
    centered: bool,
) -> Result<i64, ResizeConstraintErrorV1> {
    let extent = if positive_x(handle) {
        let dragged = rect_right(raw, "raw.right")?;
        if centered {
            i128::from(dragged) * 2 - i128::from(center2_x(base)?)
        } else {
            i128::from(dragged) - i128::from(base.x.get())
        }
    } else if negative_x(handle) {
        let dragged = raw.x.get();
        if centered {
            i128::from(center2_x(base)?) - i128::from(dragged) * 2
        } else {
            i128::from(rect_right(base, "base.right")?) - i128::from(dragged)
        }
    } else {
        return Ok(base.width.get());
    };
    let extent = safe_i128(extent, "desired_width")?;
    if extent <= 0 {
        return Err(ResizeConstraintErrorV1::CrossedAxis("horizontal"));
    }
    Ok(extent)
}

fn desired_height(
    base: RectEmu,
    raw: RectEmu,
    handle: ResizeHandle,
    centered: bool,
) -> Result<i64, ResizeConstraintErrorV1> {
    let extent = if positive_y(handle) {
        let dragged = rect_bottom(raw, "raw.bottom")?;
        if centered {
            i128::from(dragged) * 2 - i128::from(center2_y(base)?)
        } else {
            i128::from(dragged) - i128::from(base.y.get())
        }
    } else if negative_y(handle) {
        let dragged = raw.y.get();
        if centered {
            i128::from(center2_y(base)?) - i128::from(dragged) * 2
        } else {
            i128::from(rect_bottom(base, "base.bottom")?) - i128::from(dragged)
        }
    } else {
        return Ok(base.height.get());
    };
    let extent = safe_i128(extent, "desired_height")?;
    if extent <= 0 {
        return Err(ResizeConstraintErrorV1::CrossedAxis("vertical"));
    }
    Ok(extent)
}

fn nearest_rational(
    numerator: i128,
    denominator: i64,
    label: &'static str,
) -> Result<i64, ResizeConstraintErrorV1> {
    if numerator <= 0 || denominator <= 0 {
        return Err(ResizeConstraintErrorV1::InvalidRect(label));
    }
    let denominator = i128::from(denominator);
    let mut quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder * 2 >= denominator {
        quotient += 1;
    }
    let result = safe_i128(quotient, label)?;
    if result <= 0 {
        return Err(ResizeConstraintErrorV1::InvalidRect(label));
    }
    Ok(result)
}

fn nearest_rational_with_parity(
    numerator: i128,
    denominator: i64,
    required_parity: i64,
    label: &'static str,
) -> Result<i64, ResizeConstraintErrorV1> {
    if numerator <= 0 || denominator <= 0 || !matches!(required_parity, 0 | 1) {
        return Err(ResizeConstraintErrorV1::InvalidRect(label));
    }
    let denominator_i128 = i128::from(denominator);
    let floor = numerator / denominator_i128;
    let mut best: Option<(i128, i128)> = None;
    for offset in -3_i128..=4 {
        let candidate = floor + offset;
        if candidate <= 0 || candidate.rem_euclid(2) != i128::from(required_parity) {
            continue;
        }
        let distance = (candidate * denominator_i128 - numerator).abs();
        match best {
            None => best = Some((distance, candidate)),
            Some((best_distance, best_candidate))
                if distance < best_distance
                    || (distance == best_distance && candidate > best_candidate) =>
            {
                best = Some((distance, candidate));
            }
            _ => {}
        }
    }
    let candidate = best
        .map(|(_, candidate)| candidate)
        .unwrap_or_else(|| if required_parity == 1 { 1 } else { 2 });
    let result = safe_i128(candidate, label)?;
    if result <= 0 {
        return Err(ResizeConstraintErrorV1::InvalidRect(label));
    }
    Ok(result)
}

fn aspect_control_axis(
    base: RectEmu,
    desired_width: i64,
    desired_height: i64,
) -> ResizeAspectControlAxisV1 {
    let dx = (i128::from(desired_width) - i128::from(base.width.get())).abs();
    let dy = (i128::from(desired_height) - i128::from(base.height.get())).abs();
    let x_change = dx * i128::from(base.height.get());
    let y_change = dy * i128::from(base.width.get());
    if x_change >= y_change {
        ResizeAspectControlAxisV1::X
    } else {
        ResizeAspectControlAxisV1::Y
    }
}

fn constrained_extents(
    base: RectEmu,
    desired_width: i64,
    desired_height: i64,
    handle: ResizeHandle,
    modifiers: ResizeModifierMaskV1,
) -> Result<(i64, i64, bool, Option<ResizeAspectControlAxisV1>), ResizeConstraintErrorV1> {
    let aspect_applied = modifiers.aspect_lock && active_x(handle) && active_y(handle);
    if !aspect_applied {
        return Ok((desired_width, desired_height, false, None));
    }

    let axis = aspect_control_axis(base, desired_width, desired_height);
    let (width, height) = match axis {
        ResizeAspectControlAxisV1::X => {
            let width = desired_width;
            let numerator = i128::from(width) * i128::from(base.height.get());
            let height = if modifiers.centered {
                nearest_rational_with_parity(
                    numerator,
                    base.width.get(),
                    base.height.get().rem_euclid(2),
                    "aspect_height",
                )?
            } else {
                nearest_rational(numerator, base.width.get(), "aspect_height")?
            };
            (width, height)
        }
        ResizeAspectControlAxisV1::Y => {
            let height = desired_height;
            let numerator = i128::from(height) * i128::from(base.width.get());
            let width = if modifiers.centered {
                nearest_rational_with_parity(
                    numerator,
                    base.height.get(),
                    base.width.get().rem_euclid(2),
                    "aspect_width",
                )?
            } else {
                nearest_rational(numerator, base.height.get(), "aspect_width")?
            };
            (width, height)
        }
    };
    Ok((width, height, true, Some(axis)))
}

#[allow(clippy::too_many_arguments)]
fn axis_edges(
    base_start: i64,
    base_end: i64,
    extent: i64,
    negative_handle: bool,
    positive_handle: bool,
    active: bool,
    centered: bool,
    center2: i64,
    label: &'static str,
) -> Result<(i64, i64), ResizeConstraintErrorV1> {
    if !active {
        return Ok((base_start, base_end));
    }

    let (start, end) = if centered {
        let delta = i128::from(center2) - i128::from(extent);
        if delta.rem_euclid(2) != 0 {
            return Err(ResizeConstraintErrorV1::CenterParity(label));
        }
        (
            delta / 2,
            (i128::from(center2) + i128::from(extent)) / 2,
        )
    } else if positive_handle {
        (
            i128::from(base_start),
            i128::from(base_start) + i128::from(extent),
        )
    } else if negative_handle {
        (
            i128::from(base_end) - i128::from(extent),
            i128::from(base_end),
        )
    } else {
        return Err(ResizeConstraintErrorV1::InvalidRect(label));
    };

    let start = safe_i128(start, label)?;
    let end = safe_i128(end, label)?;
    if end <= start {
        return Err(ResizeConstraintErrorV1::InvalidRect(label));
    }
    Ok((start, end))
}

pub fn plan_resize_constraint_v1(
    base_rect: RectEmu,
    handle: ResizeHandle,
    raw_target_rect: RectEmu,
    modifiers: ResizeModifierMaskV1,
) -> Result<ResizeConstraintPlanV1, ResizeConstraintErrorV1> {
    validate_rect(base_rect, "base_rect")?;
    validate_rect(raw_target_rect, "raw_target_rect")?;

    let desired_width = desired_width(base_rect, raw_target_rect, handle, modifiers.centered)?;
    let desired_height = desired_height(base_rect, raw_target_rect, handle, modifiers.centered)?;
    let (width, height, aspect_applied, aspect_control_axis) = constrained_extents(
        base_rect,
        desired_width,
        desired_height,
        handle,
        modifiers,
    )?;
    let width = checked_value(width, "constrained.width")?;
    let height = checked_value(height, "constrained.height")?;
    if width <= 0 || height <= 0 {
        return Err(ResizeConstraintErrorV1::InvalidRect("constrained"));
    }

    let base_right = rect_right(base_rect, "base.right")?;
    let base_bottom = rect_bottom(base_rect, "base.bottom")?;
    let center_x = if modifiers.centered && active_x(handle) {
        center2_x(base_rect)?
    } else {
        0
    };
    let center_y = if modifiers.centered && active_y(handle) {
        center2_y(base_rect)?
    } else {
        0
    };
    let (left, right) = axis_edges(
        base_rect.x.get(),
        base_right,
        width,
        negative_x(handle),
        positive_x(handle),
        active_x(handle),
        modifiers.centered,
        center_x,
        "x_axis",
    )?;
    let (top, bottom) = axis_edges(
        base_rect.y.get(),
        base_bottom,
        height,
        negative_y(handle),
        positive_y(handle),
        active_y(handle),
        modifiers.centered,
        center_y,
        "y_axis",
    )?;

    let constrained_rect = RectEmu::new(
        LengthEmu::new(left),
        LengthEmu::new(top),
        LengthEmu::new(right - left),
        LengthEmu::new(bottom - top),
    );
    validate_rect(constrained_rect, "constrained_rect")?;

    if active_x(handle) && modifiers.centered {
        let before_center2 = i128::from(base_rect.x.get()) + i128::from(base_right);
        let after_center2 = i128::from(left) + i128::from(right);
        if before_center2 != after_center2 {
            return Err(ResizeConstraintErrorV1::CenterParity("x_axis"));
        }
    }
    if active_y(handle) && modifiers.centered {
        let before_center2 = i128::from(base_rect.y.get()) + i128::from(base_bottom);
        let after_center2 = i128::from(top) + i128::from(bottom);
        if before_center2 != after_center2 {
            return Err(ResizeConstraintErrorV1::CenterParity("y_axis"));
        }
    }

    if active_x(handle) && !modifiers.centered {
        if positive_x(handle) && left != base_rect.x.get() {
            return Err(ResizeConstraintErrorV1::InvalidRect("fixed_left"));
        }
        if negative_x(handle) && right != base_right {
            return Err(ResizeConstraintErrorV1::InvalidRect("fixed_right"));
        }
    }
    if active_y(handle) && !modifiers.centered {
        if positive_y(handle) && top != base_rect.y.get() {
            return Err(ResizeConstraintErrorV1::InvalidRect("fixed_top"));
        }
        if negative_y(handle) && bottom != base_bottom {
            return Err(ResizeConstraintErrorV1::InvalidRect("fixed_bottom"));
        }
    }

    if !active_x(handle)
        && (left != base_rect.x.get() || right - left != base_rect.width.get())
    {
        return Err(ResizeConstraintErrorV1::InvalidRect("inactive_x"));
    }
    if !active_y(handle)
        && (top != base_rect.y.get() || bottom - top != base_rect.height.get())
    {
        return Err(ResizeConstraintErrorV1::InvalidRect("inactive_y"));
    }

    Ok(ResizeConstraintPlanV1 {
        handle,
        modifiers,
        base_rect,
        raw_target_rect,
        constrained_rect,
        centered_applied: modifiers.centered,
        aspect_applied,
        aspect_control_axis,
        changed: constrained_rect != base_rect,
    })
}
