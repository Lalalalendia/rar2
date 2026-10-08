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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct BoundedGroupedImageTransform {
    pub(super) child_rotation_op: Option<u32>,
    pub(super) ancestor_horizontal_flip: bool,
    pub(super) ancestor_rotation_op: Option<u32>,
    pub(super) ancestor_bounds: Option<RectEmu>,
}

fn affine_horizontal_flip_about_bounds(bounds: RectEmu) -> Option<Affine2D> {
    let center_x_twice = i128::from(bounds.x.get()) * 2 + i128::from(bounds.width.get());
    Some(Affine2D {
        a: decimal_from_affine_scaled(-AFFINE_DECIMAL_SCALE),
        b: decimal_from_affine_scaled(0),
        c: decimal_from_affine_scaled(0),
        d: decimal_from_affine_scaled(AFFINE_DECIMAL_SCALE),
        tx: LengthEmu::new(i64::try_from(center_x_twice).ok()?),
        ty: LengthEmu::new(0),
    })
}

fn checked_div_round_nearest_i128(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator <= 0 {
        return None;
    }
    let half = denominator / 2;
    if numerator >= 0 {
        numerator.checked_add(half)?.checked_div(denominator)
    } else {
        numerator
            .checked_neg()?
            .checked_add(half)?
            .checked_div(denominator)?
            .checked_neg()
    }
}

fn checked_mul_affine_scaled(left: i128, right: i128) -> Option<i128> {
    checked_div_round_nearest_i128(left.checked_mul(right)?, AFFINE_DECIMAL_SCALE)
}

fn affine_scaled_about_bounds(
    a: i128,
    b: i128,
    c: i128,
    d: i128,
    bounds: RectEmu,
) -> Option<[i128; 6]> {
    let center_x_twice = i128::from(bounds.x.get())
        .checked_mul(2)?
        .checked_add(i128::from(bounds.width.get()))?;
    let center_y_twice = i128::from(bounds.y.get())
        .checked_mul(2)?
        .checked_add(i128::from(bounds.height.get()))?;
    let denominator = AFFINE_DECIMAL_SCALE.checked_mul(2)?;
    let tx_numerator = AFFINE_DECIMAL_SCALE
        .checked_mul(center_x_twice)?
        .checked_sub(a.checked_mul(center_x_twice)?)?
        .checked_sub(c.checked_mul(center_y_twice)?)?;
    let ty_numerator = AFFINE_DECIMAL_SCALE
        .checked_mul(center_y_twice)?
        .checked_sub(b.checked_mul(center_x_twice)?)?
        .checked_sub(d.checked_mul(center_y_twice)?)?;
    Some([
        a,
        b,
        c,
        d,
        checked_div_round_nearest_i128(tx_numerator, denominator)?,
        checked_div_round_nearest_i128(ty_numerator, denominator)?,
    ])
}

fn compose_affine_scaled(left: [i128; 6], right: [i128; 6]) -> Option<[i128; 6]> {
    let a = checked_mul_affine_scaled(left[0], right[0])?
        .checked_add(checked_mul_affine_scaled(left[2], right[1])?)?;
    let b = checked_mul_affine_scaled(left[1], right[0])?
        .checked_add(checked_mul_affine_scaled(left[3], right[1])?)?;
    let c = checked_mul_affine_scaled(left[0], right[2])?
        .checked_add(checked_mul_affine_scaled(left[2], right[3])?)?;
    let d = checked_mul_affine_scaled(left[1], right[2])?
        .checked_add(checked_mul_affine_scaled(left[3], right[3])?)?;
    let tx = checked_div_round_nearest_i128(
        left[0]
            .checked_mul(right[4])?
            .checked_add(left[2].checked_mul(right[5])?)?,
        AFFINE_DECIMAL_SCALE,
    )?
    .checked_add(left[4])?;
    let ty = checked_div_round_nearest_i128(
        left[1]
            .checked_mul(right[4])?
            .checked_add(left[3].checked_mul(right[5])?)?,
        AFFINE_DECIMAL_SCALE,
    )?
    .checked_add(left[5])?;
    Some([a, b, c, d, tx, ty])
}

fn affine_from_scaled(values: [i128; 6]) -> Option<Affine2D> {
    Some(Affine2D {
        a: decimal_from_affine_scaled(values[0]),
        b: decimal_from_affine_scaled(values[1]),
        c: decimal_from_affine_scaled(values[2]),
        d: decimal_from_affine_scaled(values[3]),
        tx: LengthEmu::new(i64::try_from(values[4]).ok()?),
        ty: LengthEmu::new(i64::try_from(values[5]).ok()?),
    })
}

fn bounded_grouped_image_rotation_transform(
    projection: BoundedGroupedImageTransform,
    child_bounds: RectEmu,
) -> Option<Affine2D> {
    let child_rotation_op = projection.child_rotation_op?;
    let ancestor_rotation_op = projection.ancestor_rotation_op?;
    let ancestor_bounds = projection.ancestor_bounds?;
    if projection.ancestor_horizontal_flip {
        return None;
    }

    // FSPGR/ChildAnchor scaling has already produced child_bounds in page space.
    // OfficeArt rotation remains a separate shape transform and therefore rotates
    // the projected rectangle without conjugating the rotation through group scale.
    let (child_sine, child_cosine) = officeart_rotation_sin_cos_scaled(child_rotation_op);
    let child = affine_scaled_about_bounds(
        child_cosine,
        child_sine,
        -child_sine,
        child_cosine,
        child_bounds,
    )?;

    let (ancestor_sine, ancestor_cosine) = officeart_rotation_sin_cos_scaled(ancestor_rotation_op);
    let ancestor = affine_scaled_about_bounds(
        ancestor_cosine,
        ancestor_sine,
        -ancestor_sine,
        ancestor_cosine,
        ancestor_bounds,
    )?;

    affine_from_scaled(compose_affine_scaled(ancestor, child)?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoundedNodeTransformProjection {
    pub(super) transform: Affine2D,
    pub(super) image_cardinal_rotation_degrees: Option<i16>,
    pub(super) image_rotation_applied: bool,
}

pub(super) fn bounded_node_transform_projection(
    shape: &pub_escher::SpContainerObservation,
    bounds: RectEmu,
    direct_image_candidate: bool,
    grouped_image_transform: Option<BoundedGroupedImageTransform>,
    direct_story_candidate: bool,
    image_crop_present: bool,
) -> BoundedNodeTransformProjection {
    let direct_rotation_properties = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == OFFICE_ART_PROPERTY_ROTATION)
        .map(|property| (property.op, property.f_bid(), property.f_complex()))
        .collect::<Vec<_>>();
    let image_rotation_properties = if direct_image_candidate {
        direct_rotation_properties.clone()
    } else {
        grouped_image_transform
            .and_then(|projection| projection.child_rotation_op)
            .map(|rotation_op| vec![(rotation_op, false, false)])
            .unwrap_or_default()
    };
    let fsp_flags = shape.fsp.as_ref().map(|fsp| fsp.flags).unwrap_or(0);
    let image_candidate = direct_image_candidate || grouped_image_transform.is_some();
    let image_transform = if let Some(projection) =
        grouped_image_transform.filter(|projection| projection.ancestor_rotation_op.is_some())
    {
        if direct_image_candidate
            || fsp_flags & (FSP_FLIP_H | FSP_FLIP_V) != 0
            || projection.ancestor_horizontal_flip
        {
            BoundedDirectImageTransform::Unsupported
        } else {
            bounded_grouped_image_rotation_transform(projection, bounds)
                .map(BoundedDirectImageTransform::Applied)
                .unwrap_or(BoundedDirectImageTransform::Unsupported)
        }
    } else if grouped_image_transform.is_some_and(|projection| projection.ancestor_horizontal_flip)
    {
        if direct_image_candidate
            || !image_rotation_properties.is_empty()
            || fsp_flags & (FSP_FLIP_H | FSP_FLIP_V) != 0
        {
            BoundedDirectImageTransform::Unsupported
        } else {
            affine_horizontal_flip_about_bounds(bounds)
                .map(BoundedDirectImageTransform::Applied)
                .unwrap_or(BoundedDirectImageTransform::Unsupported)
        }
    } else if image_candidate {
        bounded_direct_image_transform(&image_rotation_properties, fsp_flags, bounds)
    } else {
        BoundedDirectImageTransform::Identity
    };
    let story_transform = if direct_story_candidate {
        bounded_direct_story_transform(&direct_rotation_properties, fsp_flags, bounds)
    } else {
        None
    };
    let image_cardinal_rotation_degrees = if image_candidate && !image_crop_present {
        bounded_direct_image_cardinal_content_rotation_degrees(
            &image_rotation_properties,
            fsp_flags,
        )
    } else {
        None
    };
    let ancestor_horizontal_flip =
        grouped_image_transform.is_some_and(|projection| projection.ancestor_horizontal_flip);
    let ancestor_rotation_applied =
        grouped_image_transform.is_some_and(|projection| projection.ancestor_rotation_op.is_some());
    let (transform, image_rotation_applied) = if let Some(transform) = story_transform {
        (transform, false)
    } else {
        match image_transform {
            BoundedDirectImageTransform::Identity | BoundedDirectImageTransform::Unsupported => {
                (Affine2D::identity(), false)
            }
            BoundedDirectImageTransform::Applied(transform) => (
                transform,
                !ancestor_horizontal_flip || ancestor_rotation_applied,
            ),
        }
    };

    BoundedNodeTransformProjection {
        transform,
        image_cardinal_rotation_degrees,
        image_rotation_applied,
    }
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

#[cfg(test)]
mod grouped_flip_tests {
    use super::*;

    #[test]
    fn projected_child_rotation_composes_with_ancestor_rotation() {
        let projection = BoundedGroupedImageTransform {
            child_rotation_op: Some(45 * 65_536),
            ancestor_horizontal_flip: false,
            ancestor_rotation_op: Some(30 * 65_536),
            ancestor_bounds: Some(RectEmu::new(
                LengthEmu::new(0),
                LengthEmu::new(0),
                LengthEmu::new(2_000),
                LengthEmu::new(1_000),
            )),
        };
        let transform = bounded_grouped_image_rotation_transform(
            projection,
            RectEmu::new(
                LengthEmu::new(500),
                LengthEmu::new(250),
                LengthEmu::new(400),
                LengthEmu::new(200),
            ),
        )
        .unwrap();

        assert_ne!(transform, Affine2D::identity());
        assert_ne!(transform.b.as_str(), "0");
        assert_ne!(transform.c.as_str(), "0");
        assert_eq!(
            transform.b.as_str().trim_start_matches('-'),
            transform.c.as_str().trim_start_matches('-')
        );
    }

    #[test]
    fn horizontal_flip_about_bounds_reflects_around_center() {
        let bounds = RectEmu::new(
            LengthEmu::new(100),
            LengthEmu::new(200),
            LengthEmu::new(40),
            LengthEmu::new(60),
        );
        let transform = affine_horizontal_flip_about_bounds(bounds).unwrap();
        let minus_one: Decimal = "-1".parse().unwrap();
        let zero: Decimal = "0".parse().unwrap();
        let one: Decimal = "1".parse().unwrap();

        assert_eq!(transform.a, minus_one);
        assert_eq!(transform.b, zero);
        assert_eq!(transform.c, zero);
        assert_eq!(transform.d, one);
        assert_eq!(transform.tx, LengthEmu::new(240));
        assert_eq!(transform.ty, LengthEmu::new(0));
    }
}
