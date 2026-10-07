use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BoundedDirectImageTransform {
    Identity,
    Applied(Affine2D),
    Unsupported,
}

fn div_round_nearest_i128(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(denominator > 0);
    if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        -((-numerator + denominator / 2) / denominator)
    }
}

fn mul_affine_scaled(left: i128, right: i128) -> i128 {
    div_round_nearest_i128(left * right, AFFINE_DECIMAL_SCALE)
}

fn decimal_from_affine_scaled(value: i128) -> Decimal {
    let negative = value < 0;
    let absolute = value.abs();
    let integer = absolute / AFFINE_DECIMAL_SCALE;
    let fraction = absolute % AFFINE_DECIMAL_SCALE;
    let mut rendered = if fraction == 0 {
        integer.to_string()
    } else {
        let mut rendered = format!("{integer}.{fraction:012}");
        while rendered.ends_with('0') {
            rendered.pop();
        }
        rendered
    };
    if negative && absolute != 0 {
        rendered.insert(0, '-');
    }
    rendered
        .parse()
        .expect("internally generated affine decimal must be valid")
}

fn officeart_rotation_sin_cos_scaled(rotation_op: u32) -> (i128, i128) {
    const FULL_TURN_UNITS: i64 = 360 * 65_536;
    const HALF_TURN_UNITS: i64 = 180 * 65_536;
    const QUARTER_TURN_UNITS: i64 = 90 * 65_536;

    let mut angle = i64::from(rotation_op as i32) % FULL_TURN_UNITS;
    if angle > HALF_TURN_UNITS {
        angle -= FULL_TURN_UNITS;
    } else if angle < -HALF_TURN_UNITS {
        angle += FULL_TURN_UNITS;
    }

    if angle == 0 {
        return (0, AFFINE_DECIMAL_SCALE);
    }
    if angle == QUARTER_TURN_UNITS {
        return (AFFINE_DECIMAL_SCALE, 0);
    }
    if angle == -QUARTER_TURN_UNITS {
        return (-AFFINE_DECIMAL_SCALE, 0);
    }
    if angle == HALF_TURN_UNITS || angle == -HALF_TURN_UNITS {
        return (0, -AFFINE_DECIMAL_SCALE);
    }

    let mut cosine_sign = 1i128;
    if angle > QUARTER_TURN_UNITS {
        angle = HALF_TURN_UNITS - angle;
        cosine_sign = -1;
    } else if angle < -QUARTER_TURN_UNITS {
        angle = -HALF_TURN_UNITS - angle;
        cosine_sign = -1;
    }

    let radians =
        div_round_nearest_i128(i128::from(angle) * PI_SCALED, i128::from(HALF_TURN_UNITS));
    let radians_sq = mul_affine_scaled(radians, radians);

    let mut sine = radians;
    let mut sine_term = radians;
    for order in 1i128..=9 {
        sine_term = -div_round_nearest_i128(
            mul_affine_scaled(sine_term, radians_sq),
            (2 * order) * (2 * order + 1),
        );
        sine += sine_term;
    }

    let mut cosine = AFFINE_DECIMAL_SCALE;
    let mut cosine_term = AFFINE_DECIMAL_SCALE;
    for order in 1i128..=9 {
        cosine_term = -div_round_nearest_i128(
            mul_affine_scaled(cosine_term, radians_sq),
            (2 * order - 1) * (2 * order),
        );
        cosine += cosine_term;
    }

    (sine, cosine * cosine_sign)
}

fn affine_rotation_about_bounds(rotation_op: u32, bounds: RectEmu) -> Option<Affine2D> {
    let (sine, cosine) = officeart_rotation_sin_cos_scaled(rotation_op);
    let a = cosine;
    let b = sine;
    let c = -sine;
    let d = cosine;

    let center_x_twice = i128::from(bounds.x.get()) * 2 + i128::from(bounds.width.get());
    let center_y_twice = i128::from(bounds.y.get()) * 2 + i128::from(bounds.height.get());
    let denominator = 2 * AFFINE_DECIMAL_SCALE;

    let tx = div_round_nearest_i128(
        AFFINE_DECIMAL_SCALE * center_x_twice - a * center_x_twice - c * center_y_twice,
        denominator,
    );
    let ty = div_round_nearest_i128(
        AFFINE_DECIMAL_SCALE * center_y_twice - b * center_x_twice - d * center_y_twice,
        denominator,
    );

    Some(Affine2D {
        a: decimal_from_affine_scaled(a),
        b: decimal_from_affine_scaled(b),
        c: decimal_from_affine_scaled(c),
        d: decimal_from_affine_scaled(d),
        tx: LengthEmu::new(i64::try_from(tx).ok()?),
        ty: LengthEmu::new(i64::try_from(ty).ok()?),
    })
}

pub(super) fn bounded_direct_image_cardinal_content_rotation_degrees(
    rotation_properties: &[(u32, bool, bool)],
    fsp_flags: u32,
) -> Option<i16> {
    if fsp_flags & (FSP_FLIP_H | FSP_FLIP_V) != 0
        || rotation_properties.len() != 1
        || rotation_properties
            .iter()
            .any(|(_, f_bid, f_complex)| *f_bid || *f_complex)
    {
        return None;
    }

    const FULL_TURN_UNITS: i64 = 360 * 65_536;
    const QUARTER_TURN_UNITS: i64 = 90 * 65_536;
    let (rotation_op, _, _) = rotation_properties[0];
    let mut angle = i64::from(rotation_op as i32) % FULL_TURN_UNITS;
    if angle < 0 {
        angle += FULL_TURN_UNITS;
    }

    match angle {
        QUARTER_TURN_UNITS => Some(90),
        angle if angle == 2 * QUARTER_TURN_UNITS => Some(180),
        angle if angle == 3 * QUARTER_TURN_UNITS => Some(270),
        _ => None,
    }
}

pub(super) fn bounded_direct_image_transform(
    rotation_properties: &[(u32, bool, bool)],
    fsp_flags: u32,
    bounds: RectEmu,
) -> BoundedDirectImageTransform {
    if fsp_flags & (FSP_FLIP_H | FSP_FLIP_V) != 0 {
        return BoundedDirectImageTransform::Unsupported;
    }
    if rotation_properties
        .iter()
        .any(|(_, f_bid, f_complex)| *f_bid || *f_complex)
        || rotation_properties.len() > 1
    {
        return BoundedDirectImageTransform::Unsupported;
    }

    let Some((rotation_op, _, _)) = rotation_properties.first().copied() else {
        return BoundedDirectImageTransform::Identity;
    };
    if rotation_op as i32 == 0 {
        return BoundedDirectImageTransform::Identity;
    }

    const FULL_TURN_UNITS: i64 = 360 * 65_536;
    const HALF_TURN_UNITS: i64 = 180 * 65_536;
    const QUARTER_TURN_UNITS: i64 = 90 * 65_536;
    let mut signed_angle = i64::from(rotation_op as i32) % FULL_TURN_UNITS;
    if signed_angle > HALF_TURN_UNITS {
        signed_angle -= FULL_TURN_UNITS;
    } else if signed_angle < -HALF_TURN_UNITS {
        signed_angle += FULL_TURN_UNITS;
    }
    if signed_angle == 0 {
        return BoundedDirectImageTransform::Identity;
    }
    if matches!(signed_angle.abs(), QUARTER_TURN_UNITS | HALF_TURN_UNITS) {
        return BoundedDirectImageTransform::Unsupported;
    }

    affine_rotation_about_bounds(rotation_op, bounds)
        .map(BoundedDirectImageTransform::Applied)
        .unwrap_or(BoundedDirectImageTransform::Unsupported)
}

pub(super) fn bounded_direct_story_transform(
    rotation_properties: &[(u32, bool, bool)],
    fsp_flags: u32,
    bounds: RectEmu,
) -> Option<Affine2D> {
    if fsp_flags & (FSP_FLIP_H | FSP_FLIP_V) != 0 {
        return None;
    }
    if rotation_properties
        .iter()
        .any(|(_, f_bid, f_complex)| *f_bid || *f_complex)
        || rotation_properties.len() > 1
    {
        return None;
    }

    let Some((rotation_op, _, _)) = rotation_properties.first().copied() else {
        return Some(Affine2D::identity());
    };
    if rotation_op as i32 == 0 {
        return Some(Affine2D::identity());
    }

    affine_rotation_about_bounds(rotation_op, bounds)
}
