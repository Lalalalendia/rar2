use pub_cfb::read_stream_path;
use pub_contents::{
    MatureColorScheme, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
    parse_confirmed_mature_color_scheme,
};
use pub_core::StreamPath;
use pub_escher::{
    DggDefaultOptionsObservation, FoptObservation, PUBLISHER_FIELD_SHAPE_ID, PUBLISHER_FIELD_XE,
    PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE, PUBLISHER_FIELD_YS, PublisherFieldRecord,
    inspect_dgg_default_options, inspect_sp_containers,
};
use pub_model::{Affine2D, LengthEmu, NodeId, PageId, RectEmu, Sha256Digest};
use pub_reader::{PubEffectivePaintAuthority, build_mature_0x2c_source_graph};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

const ROTATION: u16 = 0x0004;
const FILL_TYPE: u16 = 0x0180;
const FILL_COLOR: u16 = 0x0181;
const FILL_OPACITY: u16 = 0x0182;
const FILL_RECT_LEFT: u16 = 0x0191;
const FILL_RECT_TOP: u16 = 0x0192;
const FILL_RECT_RIGHT: u16 = 0x0193;
const FILL_RECT_BOTTOM: u16 = 0x0194;
const FILL_COLOR_EXT: u16 = 0x019E;
const FILL_COLOR_EXT_MOD: u16 = 0x01A0;
const FILL_COLOR_EXT_DEFAULT: u32 = 0xFFFF_FFFF;
const FILL_COLOR_EXT_MOD_DEFAULT: u32 = 0x2000_0000;
const FILL_BOOLEANS: u16 = 0x01BF;
const FILL_USE_FILLED_BIT: u32 = 1 << 20;
const FILL_FILLED_BIT: u32 = 1 << 4;
const FILL_USE_RECT_USE_BIT: u32 = 1 << 17;
const FILL_USE_RECT_BIT: u32 = 1 << 1;
const GROUP_SHAPE_BOOLEANS: u16 = 0x03BF;
const GROUP_USE_HIDDEN_BIT: u32 = 1 << 17;
const GROUP_HIDDEN_BIT: u32 = 1 << 1;
const GROUP_USE_PRINT_BIT: u32 = 1 << 16;
const GROUP_PRINT_BIT: u32 = 1 << 0;
const GEO_LEFT: u16 = 0x0140;
const GEO_TOP: u16 = 0x0141;
const GEO_RIGHT: u16 = 0x0142;
const GEO_BOTTOM: u16 = 0x0143;
const SHAPE_PATH: u16 = 0x0144;
const P_VERTICES: u16 = 0x0145;
const P_SEGMENT_INFO: u16 = 0x0146;
const SHAPE_PATH_LINES_CLOSED: u32 = 0x0000_0001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyScalarLayer {
    Absent,
    Value(u32),
    Unresolved,
}

fn scalar_layer(records: &[FoptObservation], property_id: u16) -> LegacyScalarLayer {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => LegacyScalarLayer::Absent,
        [property] if !property.f_bid() && !property.f_complex() => {
            LegacyScalarLayer::Value(property.op)
        }
        _ => LegacyScalarLayer::Unresolved,
    }
}

fn legacy_scalar_layer(records: &[FoptObservation]) -> LegacyScalarLayer {
    scalar_layer(records, FILL_BOOLEANS)
}

fn effective_scalar_value(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
    property_id: u16,
    normative_default: u32,
) -> Result<u32, ()> {
    match scalar_layer(&shape.fopts, property_id) {
        LegacyScalarLayer::Value(value) => return Ok(value),
        LegacyScalarLayer::Unresolved => return Err(()),
        LegacyScalarLayer::Absent => {}
    }
    if let Some(dgg) = dgg {
        match scalar_layer(&dgg.primary_options, property_id) {
            LegacyScalarLayer::Value(value) => return Ok(value),
            LegacyScalarLayer::Unresolved => return Err(()),
            LegacyScalarLayer::Absent => {}
        }
        match scalar_layer(&dgg.tertiary_options, property_id) {
            LegacyScalarLayer::Value(value) => return Ok(value),
            LegacyScalarLayer::Unresolved => return Err(()),
            LegacyScalarLayer::Absent => {}
        }
    }
    Ok(normative_default)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeBoolLayer {
    Absent,
    Value(bool),
    Unresolved,
}

fn officeart_bool_layer(
    records: &[FoptObservation],
    property_id: u16,
    use_bit: u32,
    value_bit: u32,
) -> ProbeBoolLayer {
    let mut resolved = None;
    for property in records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
    {
        if property.f_bid() || property.f_complex() {
            return ProbeBoolLayer::Unresolved;
        }
        if property.op & use_bit == 0 {
            continue;
        }
        if resolved.is_some() {
            return ProbeBoolLayer::Unresolved;
        }
        resolved = Some(property.op & value_bit != 0);
    }
    resolved
        .map(ProbeBoolLayer::Value)
        .unwrap_or(ProbeBoolLayer::Absent)
}

fn officeart_bool_profile(
    records: &[FoptObservation],
    property_id: u16,
    use_bit: u32,
    value_bit: u32,
) -> &'static str {
    match officeart_bool_layer(records, property_id, use_bit, value_bit) {
        ProbeBoolLayer::Absent => "absent",
        ProbeBoolLayer::Value(true) => "explicit_true",
        ProbeBoolLayer::Value(false) => "explicit_false",
        ProbeBoolLayer::Unresolved => "unresolved",
    }
}

fn effective_officeart_bool_profile(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
    property_id: u16,
    use_bit: u32,
    value_bit: u32,
    normative_default: bool,
) -> String {
    let mut layers = vec![(
        "shape_local",
        officeart_bool_layer(&shape.fopts, property_id, use_bit, value_bit),
    )];
    if let Some(dgg) = dgg {
        layers.push((
            "drawing_group_primary",
            officeart_bool_layer(&dgg.primary_options, property_id, use_bit, value_bit),
        ));
        layers.push((
            "drawing_group_tertiary",
            officeart_bool_layer(&dgg.tertiary_options, property_id, use_bit, value_bit),
        ));
    }
    for (authority, layer) in layers {
        match layer {
            ProbeBoolLayer::Absent => {}
            ProbeBoolLayer::Value(value) => return format!("{authority}:{value}"),
            ProbeBoolLayer::Unresolved => return format!("{authority}:unresolved"),
        }
    }
    format!("normative_default:{normative_default}")
}

fn fill_use_rect_layer(records: &[FoptObservation]) -> ProbeBoolLayer {
    let mut resolved = None;
    for property in records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_BOOLEANS)
    {
        if property.f_bid() || property.f_complex() {
            return ProbeBoolLayer::Unresolved;
        }
        if property.op & FILL_USE_RECT_USE_BIT == 0 {
            continue;
        }
        if resolved.is_some() {
            return ProbeBoolLayer::Unresolved;
        }
        resolved = Some(property.op & FILL_USE_RECT_BIT != 0);
    }
    resolved
        .map(ProbeBoolLayer::Value)
        .unwrap_or(ProbeBoolLayer::Absent)
}

fn fill_use_rect_profile(records: &[FoptObservation]) -> &'static str {
    match fill_use_rect_layer(records) {
        ProbeBoolLayer::Absent => "absent",
        ProbeBoolLayer::Value(true) => "explicit_true",
        ProbeBoolLayer::Value(false) => "explicit_false",
        ProbeBoolLayer::Unresolved => "unresolved",
    }
}

fn effective_fill_use_rect_profile(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> String {
    let mut layers = vec![("shape_local", fill_use_rect_layer(&shape.fopts))];
    if let Some(dgg) = dgg {
        layers.push((
            "drawing_group_primary",
            fill_use_rect_layer(&dgg.primary_options),
        ));
        layers.push((
            "drawing_group_tertiary",
            fill_use_rect_layer(&dgg.tertiary_options),
        ));
    }
    for (authority, layer) in layers {
        match layer {
            ProbeBoolLayer::Absent => {}
            ProbeBoolLayer::Value(value) => return format!("{authority}:{value}"),
            ProbeBoolLayer::Unresolved => return format!("{authority}:unresolved"),
        }
    }
    "normative_default:false".to_owned()
}

fn fill_rect_presence_profile(records: &[FoptObservation]) -> &'static str {
    let profiles = [
        scalar_property_profile(records, FILL_RECT_LEFT),
        scalar_property_profile(records, FILL_RECT_TOP),
        scalar_property_profile(records, FILL_RECT_RIGHT),
        scalar_property_profile(records, FILL_RECT_BOTTOM),
    ];
    if profiles.iter().all(|profile| *profile == "absent") {
        "absent"
    } else if profiles.iter().all(|profile| *profile == "single_scalar") {
        "complete_scalars"
    } else if profiles
        .iter()
        .any(|profile| matches!(*profile, "malformed_or_complex" | "duplicate_scalar"))
    {
        "ambiguous"
    } else {
        "partial_scalars"
    }
}

fn effective_fill_rect_geometry_profile(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> &'static str {
    let values = [
        effective_scalar_value(shape, dgg, FILL_RECT_LEFT, 0),
        effective_scalar_value(shape, dgg, FILL_RECT_TOP, 0),
        effective_scalar_value(shape, dgg, FILL_RECT_RIGHT, 0),
        effective_scalar_value(shape, dgg, FILL_RECT_BOTTOM, 0),
    ];
    if values.iter().any(Result::is_err) {
        return "unresolved";
    }
    let [left, top, right, bottom] = values.map(|value| {
        i64::from(i32::from_le_bytes(
            value
                .expect("checked effective fillRect scalar")
                .to_le_bytes(),
        ))
    });
    if left == 0 && top == 0 && right == 0 && bottom == 0 {
        "all_zero_or_default"
    } else if right > left && bottom > top {
        "positive_rect"
    } else {
        "degenerate_or_nonpositive"
    }
}

fn complex_property_profile(records: &[FoptObservation], property_id: u16) -> &'static str {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => "absent",
        [property] if property.f_complex() => "single_complex",
        [property] if !property.f_bid() && property.op == 0 => "empty_default",
        [_] => "malformed_or_noncomplex",
        _ => "duplicate",
    }
}

fn geometry_layer_profile(records: &[FoptObservation]) -> &'static str {
    let geo_profiles = [
        scalar_property_profile(records, GEO_LEFT),
        scalar_property_profile(records, GEO_TOP),
        scalar_property_profile(records, GEO_RIGHT),
        scalar_property_profile(records, GEO_BOTTOM),
    ];
    let shape_path = scalar_layer(records, SHAPE_PATH);
    let vertices = complex_property_profile(records, P_VERTICES);
    let segments = complex_property_profile(records, P_SEGMENT_INFO);

    let unresolved = geo_profiles
        .iter()
        .any(|profile| matches!(*profile, "malformed_or_complex" | "duplicate_scalar"))
        || matches!(shape_path, LegacyScalarLayer::Unresolved)
        || matches!(vertices, "malformed_or_noncomplex" | "duplicate")
        || matches!(segments, "malformed_or_noncomplex" | "duplicate");
    if unresolved {
        return "unresolved";
    }

    if vertices == "single_complex"
        || segments == "single_complex"
        || matches!(
            shape_path,
            LegacyScalarLayer::Value(value) if value != SHAPE_PATH_LINES_CLOSED
        )
    {
        return "custom_path";
    }

    if geo_profiles.iter().any(|profile| *profile != "absent") {
        return "explicit_rect_space";
    }

    if matches!(shape_path, LegacyScalarLayer::Value(_))
        || vertices == "empty_default"
        || segments == "empty_default"
    {
        return "default_rect";
    }

    "absent"
}

fn effective_geometry_profile(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> String {
    let local = geometry_layer_profile(&shape.fopts);
    if local != "absent" {
        return format!("shape_local:{local}");
    }
    if let Some(dgg) = dgg {
        let primary = geometry_layer_profile(&dgg.primary_options);
        if primary != "absent" {
            return format!("drawing_group_primary:{primary}");
        }
        let tertiary = geometry_layer_profile(&dgg.tertiary_options);
        if tertiary != "absent" {
            return format!("drawing_group_tertiary:{tertiary}");
        }
    }
    "normative_default:default_rect".to_owned()
}

fn legacy_fill_visibility(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> Option<bool> {
    let mut layers = vec![legacy_scalar_layer(&shape.fopts)];
    if let Some(dgg) = dgg {
        layers.push(legacy_scalar_layer(&dgg.primary_options));
        layers.push(legacy_scalar_layer(&dgg.tertiary_options));
    }

    for layer in layers {
        match layer {
            LegacyScalarLayer::Absent => {}
            LegacyScalarLayer::Unresolved => return None,
            LegacyScalarLayer::Value(raw) => {
                if raw & FILL_USE_FILLED_BIT == 0 {
                    continue;
                }
                return Some(raw & FILL_FILLED_BIT != 0);
            }
        }
    }

    Some(true)
}

fn authority_bucket(authority: PubEffectivePaintAuthority) -> &'static str {
    match authority {
        PubEffectivePaintAuthority::ShapeLocal => "shape_local",
        PubEffectivePaintAuthority::DrawingGroupPrimary => "drawing_group_primary",
        PubEffectivePaintAuthority::DrawingGroupTertiary => "drawing_group_tertiary",
        PubEffectivePaintAuthority::NormativeDefault => "normative_default",
    }
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn colorref_profile(records: &[FoptObservation], property_id: u16) -> &'static str {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return "absent";
    }
    if matches
        .iter()
        .any(|property| property.f_bid() || property.f_complex())
    {
        return "malformed_or_complex";
    }
    if matches.len() != 1 {
        return "duplicate_scalar";
    }
    match (matches[0].op >> 24) as u8 {
        0x00 => "direct_rgb",
        0x08 => "scheme_color",
        _ => "other_flagged",
    }
}

fn extended_color_profile(records: &[FoptObservation]) -> &'static str {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_COLOR_EXT)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => "absent",
        [property] if property.f_bid() || property.f_complex() => "malformed_or_complex",
        [_, _, ..] => "duplicate_scalar",
        [property] if property.op == FILL_COLOR_EXT_DEFAULT => "default_sentinel",
        [property] => match (property.op >> 24) as u8 {
            0x00 => "nondefault_direct_rgb",
            0x08 => "nondefault_scheme_color",
            _ => "nondefault_other_flagged",
        },
    }
}

fn extended_color_mod_profile(records: &[FoptObservation]) -> &'static str {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_COLOR_EXT_MOD)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => "absent",
        [property] if property.f_bid() || property.f_complex() => "malformed_or_complex",
        [_, _, ..] => "duplicate_scalar",
        [property] if property.op == FILL_COLOR_EXT_MOD_DEFAULT => "normative_default",
        [_] => "nondefault_modifier",
    }
}

fn effective_extended_color_profile(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> String {
    let mut layers = vec![(
        "shape_local",
        extended_color_profile(&shape.fopts),
        extended_color_mod_profile(&shape.fopts),
    )];
    if let Some(dgg) = dgg {
        layers.push((
            "drawing_group_primary",
            extended_color_profile(&dgg.primary_options),
            extended_color_mod_profile(&dgg.primary_options),
        ));
        layers.push((
            "drawing_group_tertiary",
            extended_color_profile(&dgg.tertiary_options),
            extended_color_mod_profile(&dgg.tertiary_options),
        ));
    }

    for (authority, color, modifier) in layers {
        let color_absent_or_default = matches!(color, "absent" | "default_sentinel");
        let modifier_absent_or_default = matches!(modifier, "absent" | "normative_default");
        if color_absent_or_default && modifier_absent_or_default {
            continue;
        }
        if color.starts_with("malformed")
            || color == "duplicate_scalar"
            || modifier.starts_with("malformed")
            || modifier == "duplicate_scalar"
        {
            return format!("{authority}:unresolved");
        }
        if color_absent_or_default {
            return format!("{authority}:unresolved_modifier_without_extended_color");
        }
        if modifier == "nondefault_modifier" {
            return format!("{authority}:extended_color_plus_modifier");
        }
        return format!("{authority}:extended_color_present");
    }
    "none_or_default".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtendedColorIntent {
    NoExtended,
    Supported([u8; 3]),
    Unsupported,
}

fn publication_color_scheme(fixture: &PathBuf) -> MatureColorScheme {
    let contents = read_stream_path(fixture, "/Contents").expect("read exact Publisher Contents");
    let stream = StreamPath("/Contents".to_owned());
    let header = parse_0x2c_header(stream.clone(), &contents).expect("parse Contents header");
    let trailer =
        parse_confirmed_0x2c_trailer_root(&contents, &header).expect("parse Contents trailer");

    let mut candidates = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(&contents, &trailer.directory, seq_num)
                .expect("parse Contents directory reference")
        else {
            continue;
        };
        if matches!(reference.raw_types.as_slice(), [field] if field.value == 0x5C) {
            candidates.push(reference);
        }
    }
    let [reference] = candidates.as_slice() else {
        panic!("exact Virginia fixture must contain one current ColorScheme");
    };
    let [offset] = reference.chunk_offsets.as_slice() else {
        panic!("current ColorScheme must have one chunk offset");
    };
    let chunk = parse_confirmed_0x2c_chunk(stream, &contents, offset.value)
        .expect("parse current ColorScheme chunk");
    parse_confirmed_mature_color_scheme(&contents, &chunk)
        .expect("parse current publication ColorScheme")
}

fn probe_colorref_rgb(value: u32, scheme: &MatureColorScheme) -> Option<[u8; 3]> {
    match (value >> 24) as u8 {
        0x00 => {
            let bytes = value.to_le_bytes();
            Some([bytes[0], bytes[1], bytes[2]])
        }
        0x08 => {
            let ordinal = usize::try_from(value & 0x00FF_FFFF).ok()?;
            scheme.slots.get(ordinal)?.rgb
        }
        _ => None,
    }
}

fn rgb_to_hsl(rgb: [u8; 3]) -> (f64, f64, f64) {
    let r = f64::from(rgb[0]) / 255.0;
    let g = f64::from(rgb[1]) / 255.0;
    let b = f64::from(rgb[2]) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let delta = max - min;
    let s = if l > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let h = if (max - r).abs() < f64::EPSILON {
        (g - b) / delta + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f64::EPSILON {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;
    (h, s, l)
}

fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [u8; 3] {
    let (r, g, b) = if s.abs() < f64::EPSILON {
        (l, l, l)
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        (
            hue_to_rgb(p, q, h + 1.0 / 3.0),
            hue_to_rgb(p, q, h),
            hue_to_rgb(p, q, h - 1.0 / 3.0),
        )
    };
    [
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn apply_msotintshade(rgb: [u8; 3], value: u32) -> Option<[u8; 3]> {
    if value == FILL_COLOR_EXT_MOD_DEFAULT {
        return Some(rgb);
    }
    let reserved = value & 0x0000_FFFF;
    let amount = ((value >> 16) & 0xFF) as u8;
    let high = (value >> 24) as u8;
    if high != 0x10 {
        return None;
    }
    let (h, s, l) = rgb_to_hsl(rgb);
    let factor = f64::from(amount) / 255.0;
    let modified_l = match reserved {
        0x01F4 => l * factor,
        0x02F4 => 1.0 - (1.0 - l) * factor,
        _ => return None,
    };
    Some(hsl_to_rgb(h, s, modified_l))
}

fn extended_color_intent_from_records(
    records: &[FoptObservation],
    scheme: &MatureColorScheme,
) -> ExtendedColorIntent {
    let colors = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_COLOR_EXT)
        .collect::<Vec<_>>();
    let modifiers = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_COLOR_EXT_MOD)
        .collect::<Vec<_>>();

    let color = match colors.as_slice() {
        [] => {
            return if modifiers.iter().all(|property| {
                !property.f_bid()
                    && !property.f_complex()
                    && property.op == FILL_COLOR_EXT_MOD_DEFAULT
            }) {
                ExtendedColorIntent::NoExtended
            } else {
                ExtendedColorIntent::Unsupported
            };
        }
        [property] if !property.f_bid() && !property.f_complex() => *property,
        _ => return ExtendedColorIntent::Unsupported,
    };
    if color.op == FILL_COLOR_EXT_DEFAULT {
        return ExtendedColorIntent::NoExtended;
    }
    let Some(mut rgb) = probe_colorref_rgb(color.op, scheme) else {
        return ExtendedColorIntent::Unsupported;
    };
    match modifiers.as_slice() {
        [] => {}
        [property] if !property.f_bid() && !property.f_complex() => {
            let Some(modified) = apply_msotintshade(rgb, property.op) else {
                return ExtendedColorIntent::Unsupported;
            };
            rgb = modified;
        }
        _ => return ExtendedColorIntent::Unsupported,
    }
    ExtendedColorIntent::Supported(rgb)
}

fn effective_extended_color_intent(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
    scheme: &MatureColorScheme,
) -> ExtendedColorIntent {
    for records in std::iter::once(shape.fopts.as_slice())
        .chain(
            dgg.into_iter()
                .map(|group| group.primary_options.as_slice()),
        )
        .chain(
            dgg.into_iter()
                .map(|group| group.tertiary_options.as_slice()),
        )
    {
        match extended_color_intent_from_records(records, scheme) {
            ExtendedColorIntent::NoExtended => continue,
            other => return other,
        }
    }
    ExtendedColorIntent::NoExtended
}

fn main_vs_extended_profile(
    main_rgb: [u8; 3],
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
    scheme: &MatureColorScheme,
) -> &'static str {
    match effective_extended_color_intent(shape, dgg, scheme) {
        ExtendedColorIntent::NoExtended => "no_extended",
        ExtendedColorIntent::Supported(rgb) if rgb == main_rgb => "main_equals_extended",
        ExtendedColorIntent::Supported(_) => "main_differs_from_extended",
        ExtendedColorIntent::Unsupported => "unsupported_or_ambiguous",
    }
}

fn stage_b_j_covers_property(property_id: u16) -> bool {
    matches!(
        property_id,
        ROTATION
            | GEO_LEFT
            | GEO_TOP
            | GEO_RIGHT
            | GEO_BOTTOM
            | SHAPE_PATH
            | P_VERTICES
            | P_SEGMENT_INFO
            | FILL_TYPE
            | FILL_COLOR
            | FILL_OPACITY
            | FILL_RECT_LEFT
            | FILL_RECT_TOP
            | FILL_RECT_RIGHT
            | FILL_RECT_BOTTOM
            | FILL_COLOR_EXT
            | FILL_COLOR_EXT_MOD
            | FILL_BOOLEANS
            | GROUP_SHAPE_BOOLEANS
    )
}

fn local_fopt_property_id_signature(records: &[FoptObservation]) -> String {
    let ids = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .map(|property| property.property_id())
        .collect::<BTreeSet<_>>();
    if ids.is_empty() {
        "absent".to_owned()
    } else {
        ids.into_iter()
            .map(|property_id| format!("0x{property_id:04X}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn classify_local_fopt_structure(
    records: &[FoptObservation],
    property_id_histogram: &mut BTreeMap<String, usize>,
    property_signature_histogram: &mut BTreeMap<String, usize>,
    storage_form_histogram: &mut BTreeMap<String, usize>,
    coverage_histogram: &mut BTreeMap<String, usize>,
) {
    bump(
        property_signature_histogram,
        local_fopt_property_id_signature(records),
    );

    let mut properties_by_id = BTreeMap::<u16, Vec<_>>::new();
    for property in records.iter().flat_map(|record| record.properties.iter()) {
        properties_by_id
            .entry(property.property_id())
            .or_default()
            .push(property);
    }

    for (property_id, properties) in properties_by_id {
        let id = format!("0x{property_id:04X}");
        *property_id_histogram.entry(id.clone()).or_default() += properties.len();

        let storage_form = match properties.as_slice() {
            [_] if properties[0].f_bid() => "fBid",
            [_] if properties[0].f_complex() => "complex",
            [_] => "scalar",
            _ => "malformed_duplicate",
        };
        bump(storage_form_histogram, format!("{id}:{storage_form}"));
        bump(
            coverage_histogram,
            format!(
                "{id}:{}",
                if stage_b_j_covers_property(property_id) {
                    "covered"
                } else {
                    "untested"
                }
            ),
        );
    }
}

fn scalar_property_profile(records: &[FoptObservation], property_id: u16) -> &'static str {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return "absent";
    }
    if matches
        .iter()
        .any(|property| property.f_bid() || property.f_complex())
    {
        return "malformed_or_complex";
    }
    if matches.len() == 1 {
        "single_scalar"
    } else {
        "duplicate_scalar"
    }
}

fn rotation_profile(records: &[FoptObservation]) -> String {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == ROTATION)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return "absent".to_owned();
    }
    if matches
        .iter()
        .any(|property| property.f_bid() || property.f_complex())
    {
        return "malformed_or_complex".to_owned();
    }
    if matches.len() != 1 {
        return "duplicate_scalar".to_owned();
    }
    if matches[0].op == 0 {
        "single_scalar:zero".to_owned()
    } else {
        "single_scalar:nonzero".to_owned()
    }
}

fn anchor_signed_field(anchor: &PublisherFieldRecord, id: u16) -> Option<i64> {
    let mut matches = anchor.fields.iter().filter(|field| field.id == id);
    let field = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(i64::from(i32::from_le_bytes(field.value.to_le_bytes())))
}

fn page_relative_anchor_bounds(
    page_width: i64,
    page_height: i64,
    anchor: &PublisherFieldRecord,
) -> Option<RectEmu> {
    let xs = anchor_signed_field(anchor, PUBLISHER_FIELD_XS)?;
    let ys = anchor_signed_field(anchor, PUBLISHER_FIELD_YS)?;
    let xe = anchor_signed_field(anchor, PUBLISHER_FIELD_XE)?;
    let ye = anchor_signed_field(anchor, PUBLISHER_FIELD_YE)?;
    let width = xe.checked_sub(xs)?;
    let height = ye.checked_sub(ys)?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let x = page_width.checked_div(2)?.checked_add(xs)?;
    let y = page_height.checked_div(2)?.checked_add(ys)?;
    Some(RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    ))
}

fn page_area_bucket(bounds: RectEmu, page_width: i64, page_height: i64) -> &'static str {
    if page_width <= 0 || page_height <= 0 || bounds.width.get() <= 0 || bounds.height.get() <= 0 {
        return "invalid";
    }
    let area = i128::from(bounds.width.get()) * i128::from(bounds.height.get());
    let page_area = i128::from(page_width) * i128::from(page_height);
    if area <= 0 || page_area <= 0 {
        return "invalid";
    }
    let basis_points = area.saturating_mul(10_000) / page_area;
    match basis_points {
        0..=99 => "lt_1pct",
        100..=499 => "1_to_5pct",
        500..=1_999 => "5_to_20pct",
        2_000..=4_999 => "20_to_50pct",
        _ => "ge_50pct",
    }
}

fn source_node_family(node: &pub_model::SourceNode) -> &'static str {
    if node.payload.table.is_some() {
        "table"
    } else if node.payload.image_slot.is_some() {
        "image"
    } else if node.payload.story_frame.is_some() {
        "story"
    } else {
        "other_shape"
    }
}

fn effective_paint_execution_class(node: &pub_model::SourceNode) -> &'static str {
    let Some(paint) = node.payload.effective_paint.as_ref() else {
        return "effective_paint_absent";
    };

    let visible_solid = paint.fill.solid.as_ref().is_some_and(|value| value.value)
        && paint.fill.visible.as_ref().is_some_and(|value| value.value)
        && paint.fill.color_rgb.is_some();
    if visible_solid {
        return "complete_visible_solid";
    }

    let visible_line = paint.line.visible.as_ref().is_some_and(|value| value.value)
        && paint.line.color_rgb.is_some()
        && paint
            .line
            .width_emu
            .as_ref()
            .is_some_and(|value| value.value > 0);
    if visible_line {
        return "complete_visible_line";
    }

    let has_any_fill = paint.fill.solid.is_some()
        || paint.fill.visible.is_some()
        || paint.fill.color_rgb.is_some();
    let has_any_line = paint.line.visible.is_some()
        || paint.line.color_rgb.is_some()
        || paint.line.width_emu.is_some();
    if has_any_fill || has_any_line {
        "effective_paint_incomplete_or_hidden"
    } else {
        "effective_paint_empty"
    }
}

fn rects_overlap(a: RectEmu, b: RectEmu) -> bool {
    let (Some(ar), Some(ab), Some(br), Some(bb)) = (a.right(), a.bottom(), b.right(), b.bottom())
    else {
        return false;
    };
    a.x.get() < br.get() && b.x.get() < ar.get() && a.y.get() < bb.get() && b.y.get() < ab.get()
}

fn fill_opacity_profile(records: &[FoptObservation]) -> String {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_OPACITY)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return "absent".to_owned();
    }
    if matches
        .iter()
        .any(|property| property.f_bid() || property.f_complex())
    {
        return "malformed_or_complex".to_owned();
    }
    if matches.len() != 1 {
        return "duplicate_scalar".to_owned();
    }
    let class = match matches[0].op {
        0 => "transparent",
        0x0001_0000 => "opaque",
        1..=0x0000_FFFF => "partial",
        _ => "invalid_out_of_range",
    };
    format!("single_scalar:{class}")
}

fn effective_fill_opacity_profile(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> String {
    let mut layers = vec![("shape_local", fill_opacity_profile(&shape.fopts))];
    if let Some(dgg) = dgg {
        layers.push((
            "drawing_group_primary",
            fill_opacity_profile(&dgg.primary_options),
        ));
        layers.push((
            "drawing_group_tertiary",
            fill_opacity_profile(&dgg.tertiary_options),
        ));
    }
    for (authority, profile) in layers {
        if profile == "absent" {
            continue;
        }
        if let Some(class) = profile.strip_prefix("single_scalar:") {
            return format!("{authority}:{class}");
        }
        return format!("{authority}:{profile}");
    }
    "normative_default:opaque".to_owned()
}

fn fill_boolean_profile(records: &[FoptObservation]) -> String {
    let mut total = 0usize;
    let mut malformed = 0usize;
    let mut participating_true = 0usize;
    let mut participating_false = 0usize;
    let mut nonparticipating = 0usize;
    for property in records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_BOOLEANS)
    {
        total += 1;
        if property.f_bid() || property.f_complex() {
            malformed += 1;
            continue;
        }
        if property.op & FILL_USE_FILLED_BIT == 0 {
            nonparticipating += 1;
        } else if property.op & FILL_FILLED_BIT != 0 {
            participating_true += 1;
        } else {
            participating_false += 1;
        }
    }
    format!(
        "total={total};participating_true={participating_true};participating_false={participating_false};nonparticipating={nonparticipating};malformed={malformed}"
    )
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    raw_page_ordinal: usize,
    node_count: usize,
    source_order_node_count: usize,
    complete_visible_solid_fill_count: usize,
    restored_visible_solid_fill_count: usize,
    restored_in_source_order_count: usize,
    restored_outside_source_order_count: usize,
    restored_direct_count: usize,
    restored_grouped_count: usize,
    restored_shape_join_unavailable_count: usize,
    restored_family_histogram: BTreeMap<String, usize>,
    restored_shape_type_histogram: BTreeMap<String, usize>,
    restored_solid_authority_histogram: BTreeMap<String, usize>,
    restored_visibility_authority_histogram: BTreeMap<String, usize>,
    restored_color_authority_histogram: BTreeMap<String, usize>,
    restored_local_fill_type_profile_histogram: BTreeMap<String, usize>,
    restored_local_fill_color_profile_histogram: BTreeMap<String, usize>,
    restored_local_fill_color_form_histogram: BTreeMap<String, usize>,
    restored_local_fill_color_ext_histogram: BTreeMap<String, usize>,
    restored_local_fill_color_ext_mod_histogram: BTreeMap<String, usize>,
    restored_effective_fill_color_ext_histogram: BTreeMap<String, usize>,
    restored_main_vs_extended_histogram: BTreeMap<String, usize>,
    restored_effective_color_distinct_count: usize,
    restored_dgg_color_materialization_histogram: BTreeMap<String, usize>,
    restored_local_fill_use_rect_histogram: BTreeMap<String, usize>,
    restored_effective_fill_use_rect_histogram: BTreeMap<String, usize>,
    restored_local_group_shape_profile_histogram: BTreeMap<String, usize>,
    restored_local_hidden_histogram: BTreeMap<String, usize>,
    restored_effective_hidden_histogram: BTreeMap<String, usize>,
    restored_local_print_histogram: BTreeMap<String, usize>,
    restored_effective_print_histogram: BTreeMap<String, usize>,
    restored_local_geometry_profile_histogram: BTreeMap<String, usize>,
    restored_effective_geometry_profile_histogram: BTreeMap<String, usize>,
    restored_local_geo_rect_profile_histogram: BTreeMap<String, usize>,
    restored_local_shape_path_profile_histogram: BTreeMap<String, usize>,
    restored_local_vertices_profile_histogram: BTreeMap<String, usize>,
    restored_local_segment_info_profile_histogram: BTreeMap<String, usize>,
    restored_local_fill_rect_profile_histogram: BTreeMap<String, usize>,
    restored_effective_fill_rect_geometry_histogram: BTreeMap<String, usize>,
    restored_local_fill_opacity_profile_histogram: BTreeMap<String, usize>,
    restored_effective_fill_opacity_histogram: BTreeMap<String, usize>,
    restored_anchor_geometry_histogram: BTreeMap<String, usize>,
    restored_transform_histogram: BTreeMap<String, usize>,
    restored_rotation_profile_histogram: BTreeMap<String, usize>,
    restored_area_bucket_histogram: BTreeMap<String, usize>,
    restored_overlap_profile_histogram: BTreeMap<String, usize>,
    restored_local_fill_boolean_profile_histogram: BTreeMap<String, usize>,
    restored_local_fopt_property_id_histogram: BTreeMap<String, usize>,
    restored_local_fopt_property_signature_histogram: BTreeMap<String, usize>,
    restored_local_fopt_storage_form_histogram: BTreeMap<String, usize>,
    restored_local_fopt_coverage_histogram: BTreeMap<String, usize>,
    restored_later_overlap_count: usize,
    restored_later_overlap_family_histogram: BTreeMap<String, usize>,
    restored_later_overlap_paint_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    raw_page_count: usize,
    dgg_default_group_count: usize,
    dgg_primary_fill_type_profile: String,
    dgg_primary_fill_color_profile: String,
    dgg_primary_fill_color_form: String,
    dgg_primary_fill_color_ext_profile: String,
    dgg_primary_fill_color_ext_mod_profile: String,
    dgg_primary_fill_use_rect_profile: String,
    dgg_primary_fill_rect_profile: String,
    dgg_primary_group_shape_profile: String,
    dgg_primary_hidden_profile: String,
    dgg_primary_print_profile: String,
    dgg_primary_geometry_profile: String,
    dgg_primary_geo_rect_profile: String,
    dgg_primary_shape_path_profile: String,
    dgg_primary_vertices_profile: String,
    dgg_primary_segment_info_profile: String,
    dgg_primary_fill_opacity_profile: String,
    dgg_primary_fill_boolean_profile: String,
    dgg_tertiary_fill_type_profile: String,
    dgg_tertiary_fill_color_profile: String,
    dgg_tertiary_fill_color_form: String,
    dgg_tertiary_fill_color_ext_profile: String,
    dgg_tertiary_fill_color_ext_mod_profile: String,
    dgg_tertiary_fill_use_rect_profile: String,
    dgg_tertiary_fill_rect_profile: String,
    dgg_tertiary_group_shape_profile: String,
    dgg_tertiary_hidden_profile: String,
    dgg_tertiary_print_profile: String,
    dgg_tertiary_geometry_profile: String,
    dgg_tertiary_geo_rect_profile: String,
    dgg_tertiary_shape_path_profile: String,
    dgg_tertiary_vertices_profile: String,
    dgg_tertiary_segment_info_profile: String,
    dgg_tertiary_fill_opacity_profile: String,
    dgg_tertiary_fill_boolean_profile: String,
    pages: Vec<PageReceipt>,
    restored_visible_solid_fill_total: usize,
    restored_in_source_order_total: usize,
    restored_outside_source_order_total: usize,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_restored_fill_stack_probe() {
    let fixture = env::var_os("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE");
    let output = env::var_os("CHAPTERA_VIRGINIA_RESTORED_FILL_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_OUT");
    let expected_sha = env::var("CHAPTERA_VIRGINIA_RESTORED_FILL_SHA256")
        .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_SHA256");

    let bytes = fs::read(&fixture).expect("read exact public Virginia PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, expected_sha, "exact Virginia source identity");

    let source_hash: Sha256Digest = expected_sha.parse().expect("valid source SHA-256");
    let build = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash)
        .expect("build mature source graph");

    let escher = read_stream_path(&fixture, "/Escher/EscherStm")
        .expect("read exact Publisher Escher stream");
    let inventory = inspect_sp_containers(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .expect("inspect OfficeArt SpContainers");
    let dgg_inventory =
        inspect_dgg_default_options(StreamPath("/Escher/EscherStm".to_owned()), &escher)
            .expect("inspect OfficeArt DGG defaults");
    assert!(
        dgg_inventory.drawing_groups.len() <= 1,
        "Stage-A fixture requires unambiguous DGG defaults"
    );
    let dgg = dgg_inventory.drawing_groups.first();
    let color_scheme = publication_color_scheme(&fixture);

    let mut shapes_by_seq = BTreeMap::<u32, Vec<usize>>::new();
    for (index, shape) in inventory.shapes.iter().enumerate() {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        let mut seqs = client_data
            .fields
            .iter()
            .filter(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
            .map(|field| field.value)
            .collect::<Vec<_>>();
        seqs.sort_unstable();
        seqs.dedup();
        for seq in seqs {
            shapes_by_seq.entry(seq).or_default().push(index);
        }
    }

    let source_order_sequence_by_page = build
        .source_page_paint_orders
        .iter()
        .map(|order| (order.page_id, order.node_ids.clone()))
        .collect::<BTreeMap<PageId, Vec<NodeId>>>();
    let source_order_by_page = build
        .source_page_paint_orders
        .iter()
        .map(|order| {
            (
                order.page_id,
                order.node_ids.iter().copied().collect::<BTreeSet<NodeId>>(),
            )
        })
        .collect::<BTreeMap<PageId, BTreeSet<NodeId>>>();

    let mut pages = Vec::new();
    for (page_index, page_id) in build.graph.document.pages.iter().copied().enumerate() {
        let page_canonical = page_id.into_canonical();
        let page_model = build
            .graph
            .pages
            .get(&page_id)
            .expect("document PAGE must exist in SourceGraph registry");
        let source_order = source_order_by_page.get(&page_id);
        let source_order_sequence = source_order_sequence_by_page.get(&page_id);
        let mut page = PageReceipt {
            raw_page_ordinal: page_index + 1,
            source_order_node_count: source_order.map_or(0, BTreeSet::len),
            ..PageReceipt::default()
        };
        let mut restored_effective_colors = BTreeSet::<[u8; 3]>::new();

        for node in build
            .graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == page_canonical)
        {
            page.node_count += 1;

            let Some(paint) = node.payload.effective_paint.as_ref() else {
                continue;
            };
            let complete_visible_solid = paint.fill.solid.as_ref().is_some_and(|value| value.value)
                && paint.fill.visible.as_ref().is_some_and(|value| value.value)
                && paint.fill.color_rgb.is_some();
            if !complete_visible_solid {
                continue;
            }
            page.complete_visible_solid_fill_count += 1;

            let matches = shapes_by_seq
                .get(&node.payload.contents_seq_num)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let [shape_index] = matches else {
                page.restored_shape_join_unavailable_count += 1;
                continue;
            };
            let shape = &inventory.shapes[*shape_index];
            if legacy_fill_visibility(shape, dgg).is_some() {
                continue;
            }

            page.restored_visible_solid_fill_count += 1;
            if source_order.is_some_and(|order| order.contains(&node.header.id)) {
                page.restored_in_source_order_count += 1;
            } else {
                page.restored_outside_source_order_count += 1;
            }

            let grouped = node.header.source_refs.iter().any(|source| {
                source
                    .object_key
                    .as_deref()
                    .is_some_and(|key| key.starts_with("escher/group-ancestor/"))
            });
            if grouped {
                page.restored_grouped_count += 1;
            } else {
                page.restored_direct_count += 1;
            }

            let family = if node.payload.table.is_some() {
                "table"
            } else if node.payload.image_slot.is_some() {
                "image"
            } else if node.payload.story_frame.is_some() {
                "story"
            } else {
                "other_shape"
            };
            bump(&mut page.restored_family_histogram, family);
            bump(
                &mut page.restored_shape_type_histogram,
                shape
                    .fsp
                    .as_ref()
                    .map(|fsp| format!("0x{:04X}", fsp.shape_type))
                    .unwrap_or_else(|| "none".to_owned()),
            );
            if let Some(solid) = paint.fill.solid.as_ref() {
                bump(
                    &mut page.restored_solid_authority_histogram,
                    authority_bucket(solid.authority),
                );
            }
            if let Some(visible) = paint.fill.visible.as_ref() {
                bump(
                    &mut page.restored_visibility_authority_histogram,
                    authority_bucket(visible.authority),
                );
            }
            if let Some(color) = paint.fill.color_rgb.as_ref() {
                bump(
                    &mut page.restored_color_authority_histogram,
                    authority_bucket(color.authority),
                );
                restored_effective_colors.insert(color.value);
                if matches!(
                    color.authority,
                    PubEffectivePaintAuthority::DrawingGroupPrimary
                        | PubEffectivePaintAuthority::DrawingGroupTertiary
                ) {
                    let form = match color.authority {
                        PubEffectivePaintAuthority::DrawingGroupPrimary => dgg
                            .map(|group| colorref_profile(&group.primary_options, FILL_COLOR))
                            .unwrap_or("absent"),
                        PubEffectivePaintAuthority::DrawingGroupTertiary => dgg
                            .map(|group| colorref_profile(&group.tertiary_options, FILL_COLOR))
                            .unwrap_or("absent"),
                        PubEffectivePaintAuthority::ShapeLocal
                        | PubEffectivePaintAuthority::NormativeDefault => unreachable!(),
                    };
                    bump(
                        &mut page.restored_dgg_color_materialization_histogram,
                        format!("{form}:effective_rgb_present"),
                    );
                }
            }
            bump(
                &mut page.restored_local_fill_type_profile_histogram,
                scalar_property_profile(&shape.fopts, FILL_TYPE),
            );
            bump(
                &mut page.restored_local_fill_color_profile_histogram,
                scalar_property_profile(&shape.fopts, FILL_COLOR),
            );
            bump(
                &mut page.restored_local_fill_color_form_histogram,
                colorref_profile(&shape.fopts, FILL_COLOR),
            );
            bump(
                &mut page.restored_local_fill_color_ext_histogram,
                extended_color_profile(&shape.fopts),
            );
            bump(
                &mut page.restored_local_fill_color_ext_mod_histogram,
                extended_color_mod_profile(&shape.fopts),
            );
            bump(
                &mut page.restored_effective_fill_color_ext_histogram,
                effective_extended_color_profile(shape, dgg),
            );
            bump(
                &mut page.restored_main_vs_extended_histogram,
                main_vs_extended_profile(
                    paint
                        .fill
                        .color_rgb
                        .as_ref()
                        .expect("restored visible solid fill has main RGB")
                        .value,
                    shape,
                    dgg,
                    &color_scheme,
                ),
            );
            bump(
                &mut page.restored_local_fill_use_rect_histogram,
                fill_use_rect_profile(&shape.fopts),
            );
            bump(
                &mut page.restored_effective_fill_use_rect_histogram,
                effective_fill_use_rect_profile(shape, dgg),
            );
            bump(
                &mut page.restored_local_group_shape_profile_histogram,
                scalar_property_profile(&shape.fopts, GROUP_SHAPE_BOOLEANS),
            );
            bump(
                &mut page.restored_local_hidden_histogram,
                officeart_bool_profile(
                    &shape.fopts,
                    GROUP_SHAPE_BOOLEANS,
                    GROUP_USE_HIDDEN_BIT,
                    GROUP_HIDDEN_BIT,
                ),
            );
            bump(
                &mut page.restored_effective_hidden_histogram,
                effective_officeart_bool_profile(
                    shape,
                    dgg,
                    GROUP_SHAPE_BOOLEANS,
                    GROUP_USE_HIDDEN_BIT,
                    GROUP_HIDDEN_BIT,
                    false,
                ),
            );
            bump(
                &mut page.restored_local_print_histogram,
                officeart_bool_profile(
                    &shape.fopts,
                    GROUP_SHAPE_BOOLEANS,
                    GROUP_USE_PRINT_BIT,
                    GROUP_PRINT_BIT,
                ),
            );
            bump(
                &mut page.restored_effective_print_histogram,
                effective_officeart_bool_profile(
                    shape,
                    dgg,
                    GROUP_SHAPE_BOOLEANS,
                    GROUP_USE_PRINT_BIT,
                    GROUP_PRINT_BIT,
                    true,
                ),
            );
            bump(
                &mut page.restored_local_geometry_profile_histogram,
                geometry_layer_profile(&shape.fopts),
            );
            bump(
                &mut page.restored_effective_geometry_profile_histogram,
                effective_geometry_profile(shape, dgg),
            );
            let local_geo_rect_profile = [
                scalar_property_profile(&shape.fopts, GEO_LEFT),
                scalar_property_profile(&shape.fopts, GEO_TOP),
                scalar_property_profile(&shape.fopts, GEO_RIGHT),
                scalar_property_profile(&shape.fopts, GEO_BOTTOM),
            ];
            let local_geo_rect_profile = if local_geo_rect_profile
                .iter()
                .all(|profile| *profile == "absent")
            {
                "absent"
            } else if local_geo_rect_profile
                .iter()
                .all(|profile| *profile == "single_scalar")
            {
                "complete_scalars"
            } else if local_geo_rect_profile
                .iter()
                .any(|profile| matches!(*profile, "malformed_or_complex" | "duplicate_scalar"))
            {
                "ambiguous"
            } else {
                "partial_scalars"
            };
            bump(
                &mut page.restored_local_geo_rect_profile_histogram,
                local_geo_rect_profile,
            );
            bump(
                &mut page.restored_local_shape_path_profile_histogram,
                scalar_property_profile(&shape.fopts, SHAPE_PATH),
            );
            bump(
                &mut page.restored_local_vertices_profile_histogram,
                complex_property_profile(&shape.fopts, P_VERTICES),
            );
            bump(
                &mut page.restored_local_segment_info_profile_histogram,
                complex_property_profile(&shape.fopts, P_SEGMENT_INFO),
            );
            bump(
                &mut page.restored_local_fill_rect_profile_histogram,
                fill_rect_presence_profile(&shape.fopts),
            );
            bump(
                &mut page.restored_effective_fill_rect_geometry_histogram,
                effective_fill_rect_geometry_profile(shape, dgg),
            );
            bump(
                &mut page.restored_local_fill_opacity_profile_histogram,
                fill_opacity_profile(&shape.fopts),
            );
            bump(
                &mut page.restored_effective_fill_opacity_histogram,
                effective_fill_opacity_profile(shape, dgg),
            );

            let anchor_profile = match shape.client_anchor.as_ref().and_then(|anchor| {
                page_relative_anchor_bounds(
                    page_model.size.width.get(),
                    page_model.size.height.get(),
                    anchor,
                )
            }) {
                Some(anchor_bounds) if anchor_bounds == node.header.bounds => "exact",
                Some(_) => "mismatch",
                None if shape.client_anchor.is_some() => "invalid_or_incomplete",
                None => "absent",
            };
            bump(&mut page.restored_anchor_geometry_histogram, anchor_profile);
            bump(
                &mut page.restored_transform_histogram,
                if node.header.transform == Affine2D::identity() {
                    "identity"
                } else {
                    "non_identity"
                },
            );
            bump(
                &mut page.restored_rotation_profile_histogram,
                rotation_profile(&shape.fopts),
            );
            bump(
                &mut page.restored_area_bucket_histogram,
                page_area_bucket(
                    node.header.bounds,
                    page_model.size.width.get(),
                    page_model.size.height.get(),
                ),
            );

            let overlap_profile = if let Some(order) = source_order_sequence {
                if let Some(rank) = order.iter().position(|id| *id == node.header.id) {
                    let mut earlier = 0usize;
                    let mut later = 0usize;
                    for (other_rank, other_id) in order.iter().enumerate() {
                        if other_rank == rank {
                            continue;
                        }
                        let Some(other) = build.graph.nodes.get(other_id) else {
                            continue;
                        };
                        if !rects_overlap(node.header.bounds, other.header.bounds) {
                            continue;
                        }
                        if other_rank < rank {
                            earlier += 1;
                        } else {
                            later += 1;
                            page.restored_later_overlap_count += 1;
                            bump(
                                &mut page.restored_later_overlap_family_histogram,
                                source_node_family(other),
                            );
                            bump(
                                &mut page.restored_later_overlap_paint_histogram,
                                effective_paint_execution_class(other),
                            );
                        }
                    }
                    format!("earlier={earlier};later={later}")
                } else {
                    "not_in_source_order".to_owned()
                }
            } else {
                "source_order_unavailable".to_owned()
            };
            bump(
                &mut page.restored_overlap_profile_histogram,
                overlap_profile,
            );

            bump(
                &mut page.restored_local_fill_boolean_profile_histogram,
                fill_boolean_profile(&shape.fopts),
            );
            classify_local_fopt_structure(
                &shape.fopts,
                &mut page.restored_local_fopt_property_id_histogram,
                &mut page.restored_local_fopt_property_signature_histogram,
                &mut page.restored_local_fopt_storage_form_histogram,
                &mut page.restored_local_fopt_coverage_histogram,
            );
        }

        page.restored_effective_color_distinct_count = restored_effective_colors.len();
        pages.push(page);
    }

    let restored_visible_solid_fill_total = pages
        .iter()
        .map(|page| page.restored_visible_solid_fill_count)
        .sum();
    let restored_in_source_order_total = pages
        .iter()
        .map(|page| page.restored_in_source_order_count)
        .sum();
    let restored_outside_source_order_total = pages
        .iter()
        .map(|page| page.restored_outside_source_order_count)
        .sum();

    let receipt = Receipt {
        schema: "chaptera.virginia-restored-fill-stack-probe.v1",
        source_sha256: actual_sha,
        raw_page_count: pages.len(),
        dgg_default_group_count: dgg_inventory.drawing_groups.len(),
        dgg_primary_fill_type_profile: dgg
            .map(|group| scalar_property_profile(&group.primary_options, FILL_TYPE))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_fill_color_profile: dgg
            .map(|group| scalar_property_profile(&group.primary_options, FILL_COLOR))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_fill_color_form: dgg
            .map(|group| colorref_profile(&group.primary_options, FILL_COLOR))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_fill_color_ext_profile: dgg
            .map(|group| extended_color_profile(&group.primary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_fill_color_ext_mod_profile: dgg
            .map(|group| extended_color_mod_profile(&group.primary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_fill_use_rect_profile: dgg
            .map(|group| fill_use_rect_profile(&group.primary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_fill_rect_profile: dgg
            .map(|group| fill_rect_presence_profile(&group.primary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_group_shape_profile: dgg
            .map(|group| scalar_property_profile(&group.primary_options, GROUP_SHAPE_BOOLEANS))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_hidden_profile: dgg
            .map(|group| {
                officeart_bool_profile(
                    &group.primary_options,
                    GROUP_SHAPE_BOOLEANS,
                    GROUP_USE_HIDDEN_BIT,
                    GROUP_HIDDEN_BIT,
                )
            })
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_print_profile: dgg
            .map(|group| {
                officeart_bool_profile(
                    &group.primary_options,
                    GROUP_SHAPE_BOOLEANS,
                    GROUP_USE_PRINT_BIT,
                    GROUP_PRINT_BIT,
                )
            })
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_geometry_profile: dgg
            .map(|group| geometry_layer_profile(&group.primary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_geo_rect_profile: dgg
            .map(|group| {
                let profiles = [
                    scalar_property_profile(&group.primary_options, GEO_LEFT),
                    scalar_property_profile(&group.primary_options, GEO_TOP),
                    scalar_property_profile(&group.primary_options, GEO_RIGHT),
                    scalar_property_profile(&group.primary_options, GEO_BOTTOM),
                ];
                if profiles.iter().all(|profile| *profile == "absent") {
                    "absent"
                } else if profiles.iter().all(|profile| *profile == "single_scalar") {
                    "complete_scalars"
                } else if profiles
                    .iter()
                    .any(|profile| matches!(*profile, "malformed_or_complex" | "duplicate_scalar"))
                {
                    "ambiguous"
                } else {
                    "partial_scalars"
                }
            })
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_shape_path_profile: dgg
            .map(|group| scalar_property_profile(&group.primary_options, SHAPE_PATH))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_vertices_profile: dgg
            .map(|group| complex_property_profile(&group.primary_options, P_VERTICES))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_segment_info_profile: dgg
            .map(|group| complex_property_profile(&group.primary_options, P_SEGMENT_INFO))
            .unwrap_or("absent")
            .to_owned(),
        dgg_primary_fill_opacity_profile: dgg
            .map(|group| fill_opacity_profile(&group.primary_options))
            .unwrap_or_else(|| "absent".to_owned()),
        dgg_primary_fill_boolean_profile: dgg
            .map(|group| fill_boolean_profile(&group.primary_options))
            .unwrap_or_else(|| fill_boolean_profile(&[])),
        dgg_tertiary_fill_type_profile: dgg
            .map(|group| scalar_property_profile(&group.tertiary_options, FILL_TYPE))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_fill_color_profile: dgg
            .map(|group| scalar_property_profile(&group.tertiary_options, FILL_COLOR))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_fill_color_form: dgg
            .map(|group| colorref_profile(&group.tertiary_options, FILL_COLOR))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_fill_color_ext_profile: dgg
            .map(|group| extended_color_profile(&group.tertiary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_fill_color_ext_mod_profile: dgg
            .map(|group| extended_color_mod_profile(&group.tertiary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_fill_use_rect_profile: dgg
            .map(|group| fill_use_rect_profile(&group.tertiary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_fill_rect_profile: dgg
            .map(|group| fill_rect_presence_profile(&group.tertiary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_group_shape_profile: dgg
            .map(|group| scalar_property_profile(&group.tertiary_options, GROUP_SHAPE_BOOLEANS))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_hidden_profile: dgg
            .map(|group| {
                officeart_bool_profile(
                    &group.tertiary_options,
                    GROUP_SHAPE_BOOLEANS,
                    GROUP_USE_HIDDEN_BIT,
                    GROUP_HIDDEN_BIT,
                )
            })
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_print_profile: dgg
            .map(|group| {
                officeart_bool_profile(
                    &group.tertiary_options,
                    GROUP_SHAPE_BOOLEANS,
                    GROUP_USE_PRINT_BIT,
                    GROUP_PRINT_BIT,
                )
            })
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_geometry_profile: dgg
            .map(|group| geometry_layer_profile(&group.tertiary_options))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_geo_rect_profile: dgg
            .map(|group| {
                let profiles = [
                    scalar_property_profile(&group.tertiary_options, GEO_LEFT),
                    scalar_property_profile(&group.tertiary_options, GEO_TOP),
                    scalar_property_profile(&group.tertiary_options, GEO_RIGHT),
                    scalar_property_profile(&group.tertiary_options, GEO_BOTTOM),
                ];
                if profiles.iter().all(|profile| *profile == "absent") {
                    "absent"
                } else if profiles.iter().all(|profile| *profile == "single_scalar") {
                    "complete_scalars"
                } else if profiles
                    .iter()
                    .any(|profile| matches!(*profile, "malformed_or_complex" | "duplicate_scalar"))
                {
                    "ambiguous"
                } else {
                    "partial_scalars"
                }
            })
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_shape_path_profile: dgg
            .map(|group| scalar_property_profile(&group.tertiary_options, SHAPE_PATH))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_vertices_profile: dgg
            .map(|group| complex_property_profile(&group.tertiary_options, P_VERTICES))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_segment_info_profile: dgg
            .map(|group| complex_property_profile(&group.tertiary_options, P_SEGMENT_INFO))
            .unwrap_or("absent")
            .to_owned(),
        dgg_tertiary_fill_opacity_profile: dgg
            .map(|group| fill_opacity_profile(&group.tertiary_options))
            .unwrap_or_else(|| "absent".to_owned()),
        dgg_tertiary_fill_boolean_profile: dgg
            .map(|group| fill_boolean_profile(&group.tertiary_options))
            .unwrap_or_else(|| fill_boolean_profile(&[])),
        pages,
        restored_visible_solid_fill_total,
        restored_in_source_order_total,
        restored_outside_source_order_total,
        guardrails: vec![
            "The legacy resolver is reproduced only inside this measurement test to classify the #614 A/B boundary.",
            "MS-ODRAW effective fill semantics are not changed by this probe.",
            "PDF/raster comparison is validation only and is not used as stack, color, or visibility authority.",
            "No source text, object ids, SPIDs, offsets, filenames, or raw bytes are emitted.",
            "Grouped/direct and source-order buckets come only from current persisted source provenance.",
            "Stage-B property profiles emit only presence/participation classes and effective-authority buckets, never raw property values.",
            "Stage-C fillOpacity profiles classify only transparent/partial/opaque/invalid and never emit the persisted fixed-point scalar.",
            "Stage-D geometry profiles emit only anchor-equality, transform/rotation classes, page-area buckets and overlap counts; no coordinates or object identities are emitted.",
            "Stage-E COLORREF profiles emit only direct/scheme/other/ambiguous form classes plus effective-color distinct counts; no RGB or scheme ordinal is emitted.",
            "Stage-F fillUseRect profiles emit only participation authority, fillRect completeness, and positive/degenerate/unresolved geometry classes; no fillRect coordinates are emitted.",
            "Stage-G Group Shape Boolean profiles emit only documented hidden/print participation and effective authority classes; raw 0x03BF values are never emitted.",
            "Stage-H Geometry profiles emit only scalar/complex presence classes and default-rect/explicit-rect-space/custom-path/unresolved buckets; no geometry coordinates, vertices, segments, or raw property values are emitted.",
            "Stage-I extended foreground-color profiles emit only absent/default/non-default representation/modifier and aggregate participation classes; no RGB, scheme ordinal, tint/shade scalar, or raw property value is emitted.",
            "Stage-J resolves supported extended foreground intent only inside the probe and emits equality/no-extended/unsupported classes; no RGB, scheme ordinal, HSL component, tint/shade amount, or raw property value is emitted.",
            "Stage-K local FOPT census emits only property IDs, per-shape unique ID-set signatures, scalar/complex/fBid/duplicate storage classes, and whether an ID was already covered by Stages B-J; no property values are emitted.",
            "Stage-L source occlusion census emits only counts, node-family classes, and coarse effective-paint execution classes for later source-ordered overlapping nodes; no identities, coordinates, colors, text, offsets, or bytes are emitted.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    println!(
        "VIRGINIA_RESTORED_FILL_STACK restored={} ordered={} outside_order={} raw_pages={}",
        receipt.restored_visible_solid_fill_total,
        receipt.restored_in_source_order_total,
        receipt.restored_outside_source_order_total,
        receipt.raw_page_count
    );
}
