//! Bounded OfficeArt paint and image-style projection.
//!
//! This module owns source-backed shape paint/default authority plus the
//! bounded picture crop/recolor/color helpers. It deliberately does not own
//! Reader graph construction, page-role projection, typography, or geometry.

use super::{
    BTreeSet, MatureColorScheme, OFFICE_ART_PROPERTY_CROP_FROM_BOTTOM,
    OFFICE_ART_PROPERTY_CROP_FROM_LEFT, OFFICE_ART_PROPERTY_CROP_FROM_RIGHT,
    OFFICE_ART_PROPERTY_CROP_FROM_TOP, OFFICE_ART_TERTIARY_FOPT, PubEffectiveFillSource,
    PubEffectiveLineSource, PubEffectivePaintAuthority, PubEffectivePaintValue,
    PubEffectiveShapePaintSource, PubExplicitFillSource, PubExplicitImageCropSource,
    PubExplicitImageRecolorSource, PubExplicitLineSource, PubExplicitShapePaintSource,
};

pub(super) const OFFICE_ART_PICTURE_RECOLOR: u16 = 0x011A;
pub(super) const OFFICE_ART_PICTURE_RECOLOR_EXTRA_START: u16 = 0x011B;
pub(super) const OFFICE_ART_PICTURE_RECOLOR_EXTRA_END: u16 = 0x011D;
pub(super) const OFFICE_ART_BLIP_BOOLEANS: u16 = 0x013F;
pub(super) const BLIP_USE_PICTURE_PRESERVE_GRAYS_BIT: u32 = 1 << 22;
pub(super) const BLIP_PICTURE_PRESERVE_GRAYS_BIT: u32 = 1 << 6;
pub(super) const OFFICE_ART_ADJUST_VALUE: u16 = 0x0147;
pub(super) const OFFICE_ART_FILL_TYPE: u16 = 0x0180;
pub(super) const OFFICE_ART_FILL_COLOR: u16 = 0x0181;
pub(super) const OFFICE_ART_FILL_BOOLEANS: u16 = 0x01BF;
pub(super) const OFFICE_ART_LINE_COLOR: u16 = 0x01C0;
pub(super) const OFFICE_ART_LINE_WIDTH: u16 = 0x01CB;
pub(super) const OFFICE_ART_LINE_DASHING: u16 = 0x01CE;
pub(super) const OFFICE_ART_LINE_BOOLEANS: u16 = 0x01FF;

// OfficeArt boolean property sets persist each use/value pair in mirrored
// high-word/low-word bit positions. Publisher corpus controls preserve the
// exact negative/positive pairs 0x00100000/0x00100010 for fill and
// 0x00080000/0x00080008 for line.
pub(super) const FILL_USE_FILLED_BIT: u32 = 1 << 20;
pub(super) const FILL_FILLED_BIT: u32 = 1 << 4;
pub(super) const LINE_USE_LINE_BIT: u32 = 1 << 19;
pub(super) const LINE_LINE_BIT: u32 = 1 << 3;
pub(super) const OFFICEART_FSP_CONNECTOR_BIT: u32 = 1 << 8;
pub(super) const OFFICEART_SHAPE_TYPE_NOT_PRIMITIVE: u16 = 0x0000;
pub(super) const OFFICEART_SHAPE_TYPE_ELLIPSE: u16 = 0x0003;
pub(super) const OFFICEART_SHAPE_TYPE_LINE: u16 = 0x0014;

// MS-ODRAW normative property defaults for the bounded solid 2-D paint surface.
pub(super) const NORMATIVE_FILL_TYPE: u32 = 0;
pub(super) const NORMATIVE_FILL_COLOR: u32 = 0x00FF_FFFF;
pub(super) const NORMATIVE_LINE_COLOR: u32 = 0x0000_0000;
pub(super) const NORMATIVE_LINE_WIDTH_EMU: u32 = 0x0000_2535;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PaintScalarLayer {
    Absent,
    Value(PubEffectivePaintValue<u32>),
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PaintLineVisibilityLayer {
    Absent,
    Value(PubEffectivePaintValue<bool>),
    Unresolved,
}

pub(super) fn has_default_roundrect_geometry(shape: &pub_escher::SpContainerObservation) -> bool {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(0x0002) {
        return false;
    }
    !shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .any(|property| property.property_id() == OFFICE_ART_ADJUST_VALUE)
}

pub(super) fn has_default_ellipse_geometry(shape: &pub_escher::SpContainerObservation) -> bool {
    shape.fsp.as_ref().map(|fsp| fsp.shape_type) == Some(OFFICEART_SHAPE_TYPE_ELLIPSE)
}

pub(super) fn has_default_line_geometry(shape: &pub_escher::SpContainerObservation) -> bool {
    shape.fsp.as_ref().map(|fsp| fsp.shape_type) == Some(OFFICEART_SHAPE_TYPE_LINE)
}

pub(super) fn has_shape_local_dash_gel(shape: &pub_escher::SpContainerObservation) -> bool {
    unique_explicit_officeart_scalar(shape, OFFICE_ART_LINE_DASHING) == Some(6)
}

pub(super) fn has_explicit_officeart_paint_observation(
    shape: &pub_escher::SpContainerObservation,
) -> bool {
    shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .any(|property| {
            matches!(
                property.property_id(),
                OFFICE_ART_FILL_TYPE
                    | OFFICE_ART_FILL_COLOR
                    | OFFICE_ART_FILL_BOOLEANS
                    | OFFICE_ART_LINE_COLOR
                    | OFFICE_ART_LINE_WIDTH
                    | OFFICE_ART_LINE_BOOLEANS
            )
        })
}

pub(super) fn explicit_officeart_paint(
    shape: &pub_escher::SpContainerObservation,
    color_scheme: Option<&MatureColorScheme>,
) -> PubExplicitShapePaintSource {
    let fill_type = unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_TYPE);
    let fill_color = unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_COLOR)
        .and_then(|value| bounded_officeart_rgb(value, color_scheme));
    let fill_visible =
        unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_BOOLEANS).and_then(|value| {
            (value & FILL_USE_FILLED_BIT != 0).then_some(value & FILL_FILLED_BIT != 0)
        });

    let line_color = unique_explicit_officeart_scalar(shape, OFFICE_ART_LINE_COLOR)
        .and_then(|value| bounded_officeart_rgb(value, color_scheme));
    let line_width = unique_explicit_officeart_scalar(shape, OFFICE_ART_LINE_WIDTH)
        .and_then(|value| (value <= 0x0132_F540).then_some(i64::from(value)));
    let line_visible =
        match line_visibility_from_records(&shape.fopts, PubEffectivePaintAuthority::ShapeLocal) {
            PaintLineVisibilityLayer::Value(value) => Some(value.value),
            PaintLineVisibilityLayer::Absent | PaintLineVisibilityLayer::Unresolved => None,
        };

    PubExplicitShapePaintSource {
        fill: PubExplicitFillSource {
            solid: fill_type == Some(0),
            color_rgb: fill_color,
            visible: fill_visible,
        },
        line: PubExplicitLineSource {
            color_rgb: line_color,
            width_emu: line_width,
            visible: line_visible,
        },
    }
}

/// Resolves only the bounded solid-paint subset of the MS-ODRAW effective
/// property hierarchy. The caller must admit the shape as a 2-D shape before
/// enabling normative 2-D visibility defaults.
pub fn resolve_bounded_effective_officeart_paint(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
    color_scheme: Option<&MatureColorScheme>,
    admit_normative_2d_defaults: bool,
) -> Option<PubEffectiveShapePaintSource> {
    if !admit_normative_2d_defaults {
        return None;
    }

    let fill_solid = resolve_effective_officeart_scalar(
        shape,
        dgg_defaults,
        OFFICE_ART_FILL_TYPE,
        NORMATIVE_FILL_TYPE,
    )
    .and_then(|value| (value.value == 0).then(|| value.map(|_| true)));

    let fill_color = if shape_has_explicit_filled_without_fill_color(shape) {
        bounded_officeart_rgb(NORMATIVE_FILL_COLOR, color_scheme).map(|rgb| {
            PubEffectivePaintValue {
                value: rgb,
                authority: PubEffectivePaintAuthority::NormativeDefault,
                source: None,
            }
        })
    } else {
        resolve_effective_officeart_scalar(
            shape,
            dgg_defaults,
            OFFICE_ART_FILL_COLOR,
            NORMATIVE_FILL_COLOR,
        )
        .and_then(|value| {
            bounded_officeart_rgb(value.value, color_scheme).map(|rgb| value.map(|_| rgb))
        })
    };

    let fill_visible = resolve_effective_officeart_fill_visibility(shape, dgg_defaults);

    let line_color = resolve_effective_officeart_scalar(
        shape,
        dgg_defaults,
        OFFICE_ART_LINE_COLOR,
        NORMATIVE_LINE_COLOR,
    )
    .and_then(|value| {
        bounded_officeart_rgb(value.value, color_scheme).map(|rgb| value.map(|_| rgb))
    });

    let line_width = resolve_effective_officeart_scalar(
        shape,
        dgg_defaults,
        OFFICE_ART_LINE_WIDTH,
        NORMATIVE_LINE_WIDTH_EMU,
    )
    .and_then(|value| (value.value <= 0x0132_F540).then(|| value.map(i64::from)));

    let line_visible = resolve_effective_officeart_line_visibility(shape, dgg_defaults);

    Some(PubEffectiveShapePaintSource {
        fill: PubEffectiveFillSource {
            solid: fill_solid,
            color_rgb: fill_color,
            visible: fill_visible,
        },
        line: PubEffectiveLineSource {
            color_rgb: line_color,
            width_emu: line_width,
            visible: line_visible,
        },
    })
}

pub(super) fn shape_has_explicit_filled_without_fill_color(
    shape: &pub_escher::SpContainerObservation,
) -> bool {
    let shape_type = shape.fsp.as_ref().map(|fsp| fsp.shape_type);
    if !matches!(shape_type, Some(0x0002 | 0x00CA)) {
        return false;
    }

    let fill_color = paint_scalar_from_records(
        &shape.fopts,
        OFFICE_ART_FILL_COLOR,
        PubEffectivePaintAuthority::ShapeLocal,
    );
    if !matches!(fill_color, PaintScalarLayer::Absent) {
        return false;
    }

    if shape_type == Some(0x00CA) {
        return matches!(
            fill_visibility_from_records(
                &shape.fopts,
                PubEffectivePaintAuthority::ShapeLocal,
            ),
            PaintLineVisibilityLayer::Value(value) if value.value
        );
    }

    matches!(
        paint_scalar_from_records(
            &shape.fopts,
            OFFICE_ART_FILL_BOOLEANS,
            PubEffectivePaintAuthority::ShapeLocal,
        ),
        PaintScalarLayer::Value(value)
            if value.value & FILL_USE_FILLED_BIT != 0
                && value.value & FILL_FILLED_BIT != 0
    )
}

pub(super) fn resolve_effective_officeart_scalar(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
    property_id: u16,
    normative_default: u32,
) -> Option<PubEffectivePaintValue<u32>> {
    match paint_scalar_from_records(
        &shape.fopts,
        property_id,
        PubEffectivePaintAuthority::ShapeLocal,
    ) {
        PaintScalarLayer::Value(value) => return Some(value),
        PaintScalarLayer::Unresolved => return None,
        PaintScalarLayer::Absent => {}
    }

    if let Some(dgg) = dgg_defaults {
        match paint_scalar_from_records(
            &dgg.primary_options,
            property_id,
            PubEffectivePaintAuthority::DrawingGroupPrimary,
        ) {
            PaintScalarLayer::Value(value) => return Some(value),
            PaintScalarLayer::Unresolved => return None,
            PaintScalarLayer::Absent => {}
        }
        match paint_scalar_from_records(
            &dgg.tertiary_options,
            property_id,
            PubEffectivePaintAuthority::DrawingGroupTertiary,
        ) {
            PaintScalarLayer::Value(value) => return Some(value),
            PaintScalarLayer::Unresolved => return None,
            PaintScalarLayer::Absent => {}
        }
    }

    Some(PubEffectivePaintValue {
        value: normative_default,
        authority: PubEffectivePaintAuthority::NormativeDefault,
        source: None,
    })
}

pub(super) fn resolve_effective_officeart_fill_visibility(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
) -> Option<PubEffectivePaintValue<bool>> {
    let mut layers = vec![fill_visibility_from_records(
        &shape.fopts,
        PubEffectivePaintAuthority::ShapeLocal,
    )];
    if let Some(dgg) = dgg_defaults {
        layers.push(fill_visibility_from_records(
            &dgg.primary_options,
            PubEffectivePaintAuthority::DrawingGroupPrimary,
        ));
        layers.push(fill_visibility_from_records(
            &dgg.tertiary_options,
            PubEffectivePaintAuthority::DrawingGroupTertiary,
        ));
    }

    for layer in layers {
        match layer {
            PaintLineVisibilityLayer::Absent => {}
            PaintLineVisibilityLayer::Unresolved => return None,
            PaintLineVisibilityLayer::Value(value) => return Some(value),
        }
    }

    Some(PubEffectivePaintValue {
        value: true,
        authority: PubEffectivePaintAuthority::NormativeDefault,
        source: None,
    })
}

pub(super) fn fill_visibility_from_records(
    records: &[pub_escher::FoptObservation],
    authority: PubEffectivePaintAuthority,
) -> PaintLineVisibilityLayer {
    let mut candidates = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == OFFICE_ART_FILL_BOOLEANS);

    let mut resolved = None;
    for property in candidates.by_ref() {
        if property.f_bid() || property.f_complex() {
            return PaintLineVisibilityLayer::Unresolved;
        }
        if property.op & FILL_USE_FILLED_BIT == 0 {
            continue;
        }
        if resolved.is_some() {
            return PaintLineVisibilityLayer::Unresolved;
        }
        resolved = Some(PubEffectivePaintValue {
            value: property.op & FILL_FILLED_BIT != 0,
            authority,
            source: Some(property.source.clone()),
        });
    }

    resolved
        .map(PaintLineVisibilityLayer::Value)
        .unwrap_or(PaintLineVisibilityLayer::Absent)
}

pub(super) fn resolve_effective_officeart_line_visibility(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
) -> Option<PubEffectivePaintValue<bool>> {
    let mut layers = vec![line_visibility_from_records(
        &shape.fopts,
        PubEffectivePaintAuthority::ShapeLocal,
    )];
    if let Some(dgg) = dgg_defaults {
        layers.push(line_visibility_from_records(
            &dgg.primary_options,
            PubEffectivePaintAuthority::DrawingGroupPrimary,
        ));
        layers.push(line_visibility_from_records(
            &dgg.tertiary_options,
            PubEffectivePaintAuthority::DrawingGroupTertiary,
        ));
    }

    for layer in layers {
        match layer {
            PaintLineVisibilityLayer::Absent => {}
            PaintLineVisibilityLayer::Unresolved => return None,
            PaintLineVisibilityLayer::Value(value) => return Some(value),
        }
    }

    Some(PubEffectivePaintValue {
        value: true,
        authority: PubEffectivePaintAuthority::NormativeDefault,
        source: None,
    })
}

pub(super) fn line_visibility_from_records(
    records: &[pub_escher::FoptObservation],
    authority: PubEffectivePaintAuthority,
) -> PaintLineVisibilityLayer {
    let mut candidates = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == OFFICE_ART_LINE_BOOLEANS);

    let mut resolved = None;
    for property in candidates.by_ref() {
        if property.f_bid() || property.f_complex() {
            return PaintLineVisibilityLayer::Unresolved;
        }
        if property.op & LINE_USE_LINE_BIT == 0 {
            continue;
        }
        if resolved.is_some() {
            return PaintLineVisibilityLayer::Unresolved;
        }
        resolved = Some(PubEffectivePaintValue {
            value: property.op & LINE_LINE_BIT != 0,
            authority,
            source: Some(property.source.clone()),
        });
    }

    resolved
        .map(PaintLineVisibilityLayer::Value)
        .unwrap_or(PaintLineVisibilityLayer::Absent)
}

pub(super) fn paint_scalar_from_records(
    records: &[pub_escher::FoptObservation],
    property_id: u16,
    authority: PubEffectivePaintAuthority,
) -> PaintScalarLayer {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => PaintScalarLayer::Absent,
        [property] if !property.f_bid() && !property.f_complex() => {
            PaintScalarLayer::Value(PubEffectivePaintValue {
                value: property.op,
                authority,
                source: Some(property.source.clone()),
            })
        }
        _ => PaintScalarLayer::Unresolved,
    }
}

pub(super) fn admits_normative_2d_paint_defaults(
    shape: &pub_escher::SpContainerObservation,
) -> bool {
    let Some(fsp) = shape.fsp.as_ref() else {
        return false;
    };
    fsp.shape_type != OFFICEART_SHAPE_TYPE_NOT_PRIMITIVE
        && fsp.shape_type != OFFICEART_SHAPE_TYPE_LINE
        && fsp.flags & OFFICEART_FSP_CONNECTOR_BIT == 0
}

pub(super) fn effective_paint_has_dgg_authority(paint: &PubEffectiveShapePaintSource) -> bool {
    let authorities = [
        paint.fill.solid.as_ref().map(|value| value.authority),
        paint.fill.color_rgb.as_ref().map(|value| value.authority),
        paint.fill.visible.as_ref().map(|value| value.authority),
        paint.line.color_rgb.as_ref().map(|value| value.authority),
        paint.line.width_emu.as_ref().map(|value| value.authority),
        paint.line.visible.as_ref().map(|value| value.authority),
    ];
    authorities.into_iter().flatten().any(|authority| {
        matches!(
            authority,
            PubEffectivePaintAuthority::DrawingGroupPrimary
                | PubEffectivePaintAuthority::DrawingGroupTertiary
        )
    })
}

pub(super) fn fopt_records_use_officeart_scheme_color(
    records: &[pub_escher::FoptObservation],
) -> bool {
    records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| {
            matches!(
                property.property_id(),
                OFFICE_ART_FILL_COLOR | OFFICE_ART_LINE_COLOR
            ) && !property.f_bid()
                && !property.f_complex()
        })
        .any(|property| (property.op >> 24) as u8 == 0x08)
}

pub(super) fn paint_context_uses_officeart_scheme_color(
    shape: &pub_escher::SpContainerObservation,
    dgg_defaults: Option<&pub_escher::DggDefaultOptionsObservation>,
) -> bool {
    fopt_records_use_officeart_scheme_color(&shape.fopts)
        || dgg_defaults.is_some_and(|dgg| {
            fopt_records_use_officeart_scheme_color(&dgg.primary_options)
                || fopt_records_use_officeart_scheme_color(&dgg.tertiary_options)
        })
}

pub(super) fn unique_explicit_officeart_scalar(
    shape: &pub_escher::SpContainerObservation,
    property_id: u16,
) -> Option<u32> {
    let values = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| {
            property.property_id() == property_id && !property.f_bid() && !property.f_complex()
        })
        .map(|property| property.op)
        .collect::<BTreeSet<_>>();

    if values.len() == 1 {
        values.iter().next().copied()
    } else {
        None
    }
}

pub(super) fn bounded_officeart_image_crop(
    shape: &pub_escher::SpContainerObservation,
) -> Option<PubExplicitImageCropSource> {
    fn arm(
        shape: &pub_escher::SpContainerObservation,
        property_id: u16,
    ) -> (bool, Option<u32>, bool) {
        let properties = shape
            .fopts
            .iter()
            .flat_map(|record| record.properties.iter())
            .filter(|property| property.property_id() == property_id)
            .collect::<Vec<_>>();

        if properties.is_empty() {
            return (false, None, false);
        }

        if properties
            .iter()
            .any(|property| property.f_bid() || property.f_complex())
        {
            return (true, None, true);
        }

        let values = properties
            .iter()
            .map(|property| property.op)
            .collect::<BTreeSet<_>>();
        match values.len() {
            1 => (true, values.iter().next().copied(), false),
            _ => (true, None, true),
        }
    }

    let (top_present, top_raw, top_ambiguous) = arm(shape, OFFICE_ART_PROPERTY_CROP_FROM_TOP);
    let (bottom_present, bottom_raw, bottom_ambiguous) =
        arm(shape, OFFICE_ART_PROPERTY_CROP_FROM_BOTTOM);
    let (left_present, left_raw, left_ambiguous) = arm(shape, OFFICE_ART_PROPERTY_CROP_FROM_LEFT);
    let (right_present, right_raw, right_ambiguous) =
        arm(shape, OFFICE_ART_PROPERTY_CROP_FROM_RIGHT);

    if !(top_present || bottom_present || left_present || right_present) {
        return None;
    }

    Some(PubExplicitImageCropSource {
        top_raw,
        bottom_raw,
        left_raw,
        right_raw,
        ambiguous: top_ambiguous || bottom_ambiguous || left_ambiguous || right_ambiguous,
    })
}

pub(super) fn bounded_officeart_image_recolor(
    shape: &pub_escher::SpContainerObservation,
    color_scheme: Option<&MatureColorScheme>,
) -> Option<PubExplicitImageRecolorSource> {
    if shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .any(|property| {
            (OFFICE_ART_PICTURE_RECOLOR_EXTRA_START..=OFFICE_ART_PICTURE_RECOLOR_EXTRA_END)
                .contains(&property.property_id())
        })
    {
        return None;
    }

    let recolor = shape
        .fopts
        .iter()
        .flat_map(|record| {
            record
                .properties
                .iter()
                .filter(move |property| property.property_id() == OFFICE_ART_PICTURE_RECOLOR)
                .map(move |property| (record.rec_type, property))
        })
        .collect::<Vec<_>>();
    let [(recolor_record_type, recolor_property)] = recolor.as_slice() else {
        return None;
    };
    if *recolor_record_type != OFFICE_ART_TERTIARY_FOPT
        || recolor_property.f_bid()
        || recolor_property.f_complex()
    {
        return None;
    }
    let target_rgb = bounded_officeart_rgb(recolor_property.op, color_scheme)?;

    let booleans = shape
        .fopts
        .iter()
        .flat_map(|record| {
            record
                .properties
                .iter()
                .filter(move |property| property.property_id() == OFFICE_ART_BLIP_BOOLEANS)
                .map(move |property| (record.rec_type, property))
        })
        .collect::<Vec<_>>();
    let [(boolean_record_type, boolean_property)] = booleans.as_slice() else {
        return None;
    };
    if *boolean_record_type != OFFICE_ART_TERTIARY_FOPT
        || boolean_property.f_bid()
        || boolean_property.f_complex()
        || boolean_property.op & BLIP_USE_PICTURE_PRESERVE_GRAYS_BIT == 0
    {
        return None;
    }
    let preserve_grays = boolean_property.op & BLIP_PICTURE_PRESERVE_GRAYS_BIT != 0;
    if preserve_grays {
        return None;
    }

    Some(PubExplicitImageRecolorSource {
        target_rgb,
        preserve_grays,
    })
}

pub(super) fn direct_officeart_rgb(value: u32) -> Option<[u8; 3]> {
    // OfficeArtCOLORREF uses upper-byte flags for non-direct color forms.
    // This bounded path accepts only the unflagged direct RGB form.
    if value & 0xFF00_0000 != 0 {
        return None;
    }
    let bytes = value.to_le_bytes();
    Some([bytes[0], bytes[1], bytes[2]])
}

pub(super) fn bounded_officeart_rgb(
    value: u32,
    color_scheme: Option<&MatureColorScheme>,
) -> Option<[u8; 3]> {
    match (value >> 24) as u8 {
        0x00 => direct_officeart_rgb(value),
        0x08 => {
            let ordinal = usize::try_from(value & 0x00FF_FFFF).ok()?;
            color_scheme?.slots.get(ordinal)?.rgb
        }
        _ => None,
    }
}
