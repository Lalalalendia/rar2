use pub_model::{Affine2D, Decimal, LengthEmu, RectEmu};

const Q31_ONE: i64 = 1_i64 << 31;
const BAM_HALF_TURN: i64 = 1_i64 << 31;
const BAM_QUARTER_TURN: i64 = 1_i64 << 30;
const BAM_FULL_TURN: i64 = 1_i64 << 32;
const CORDIC_GAIN_Q31: i64 = 1_304_065_748;
const POW5_31: u128 = 4_656_612_873_077_392_578_125;
const CORDIC_ATAN_BAM: [i64; 31] = [
    536_870_912,
    316_933_406,
    167_458_907,
    85_004_756,
    42_667_331,
    21_354_465,
    10_679_838,
    5_340_245,
    2_670_163,
    1_335_087,
    667_544,
    333_772,
    166_886,
    83_443,
    41_722,
    20_861,
    10_430,
    5_215,
    2_608,
    1_304,
    652,
    326,
    163,
    81,
    41,
    20,
    10,
    5,
    3,
    1,
    1,
];

fn div_round_nearest(value: i128, divisor: i128) -> i128 {
    debug_assert!(divisor > 0);
    if value >= 0 {
        (value + divisor / 2) / divisor
    } else {
        -((-value + divisor / 2) / divisor)
    }
}

fn rotation_raw_to_bam(raw: i32) -> i64 {
    // OfficeArt rotation is signed 16.16 degrees. A full binary-angle turn is
    // 2^32, so raw * 2^16 / 360 converts without binary floating point.
    let numerator = i128::from(raw) * i128::from(1_i64 << 16);
    let bam = div_round_nearest(numerator, 360);
    let wrapped = (bam + i128::from(BAM_HALF_TURN)).rem_euclid(i128::from(BAM_FULL_TURN))
        - i128::from(BAM_HALF_TURN);
    i64::try_from(wrapped).expect("normalized BAM angle fits i64")
}

fn cordic_sin_cos_q31(mut angle_bam: i64) -> (i64, i64) {
    if angle_bam == 0 {
        return (Q31_ONE, 0);
    }
    if angle_bam == BAM_QUARTER_TURN {
        return (0, Q31_ONE);
    }
    if angle_bam == -BAM_QUARTER_TURN {
        return (0, -Q31_ONE);
    }
    if angle_bam == -BAM_HALF_TURN {
        return (-Q31_ONE, 0);
    }

    let mut negate = false;
    if angle_bam > BAM_QUARTER_TURN {
        angle_bam -= BAM_HALF_TURN;
        negate = true;
    } else if angle_bam < -BAM_QUARTER_TURN {
        angle_bam += BAM_HALF_TURN;
        negate = true;
    }

    let mut x = CORDIC_GAIN_Q31;
    let mut y = 0_i64;
    let mut z = angle_bam;

    for (shift, atan) in CORDIC_ATAN_BAM.into_iter().enumerate() {
        let (next_x, next_y, next_z) = if z >= 0 {
            (x - (y >> shift), y + (x >> shift), z - atan)
        } else {
            (x + (y >> shift), y - (x >> shift), z + atan)
        };
        x = next_x;
        y = next_y;
        z = next_z;
    }

    if negate {
        x = -x;
        y = -y;
    }

    (
        x.clamp(-Q31_ONE, Q31_ONE),
        y.clamp(-Q31_ONE, Q31_ONE),
    )
}

fn q31_decimal(value: i64) -> Decimal {
    debug_assert!((-Q31_ONE..=Q31_ONE).contains(&value));
    if value == 0 {
        return Decimal::zero();
    }
    if value == Q31_ONE {
        return Decimal::one();
    }
    if value == -Q31_ONE {
        return "-1".parse().expect("constant decimal");
    }

    let negative = value < 0;
    let magnitude = u128::from(value.unsigned_abs());
    let integer = magnitude / u128::from(Q31_ONE as u64);
    let remainder = magnitude % u128::from(Q31_ONE as u64);
    let fraction = remainder * POW5_31;
    let mut fraction_text = format!("{fraction:031}");
    while fraction_text.ends_with('0') {
        fraction_text.pop();
    }
    let sign = if negative { "-" } else { "" };
    format!("{sign}{integer}.{fraction_text}")
        .parse()
        .expect("Q31 decimal expansion is valid")
}

fn center_preserving_translation(
    bounds: RectEmu,
    a_q31: i64,
    b_q31: i64,
    c_q31: i64,
    d_q31: i64,
) -> Option<(LengthEmu, LengthEmu)> {
    let center_x2 = i128::from(bounds.x.get())
        .checked_mul(2)?
        .checked_add(i128::from(bounds.width.get()))?;
    let center_y2 = i128::from(bounds.y.get())
        .checked_mul(2)?
        .checked_add(i128::from(bounds.height.get()))?;
    let q = i128::from(Q31_ONE);

    let tx_numerator = center_x2
        .checked_mul(q)?
        .checked_sub(i128::from(a_q31).checked_mul(center_x2)?)?
        .checked_sub(i128::from(c_q31).checked_mul(center_y2)?)?;
    let ty_numerator = center_y2
        .checked_mul(q)?
        .checked_sub(i128::from(b_q31).checked_mul(center_x2)?)?
        .checked_sub(i128::from(d_q31).checked_mul(center_y2)?)?;
    let denominator = q.checked_mul(2)?;

    let tx = i64::try_from(div_round_nearest(tx_numerator, denominator)).ok()?;
    let ty = i64::try_from(div_round_nearest(ty_numerator, denominator)).ok()?;
    Some((LengthEmu::new(tx), LengthEmu::new(ty)))
}

/// Derive the direct-shape OfficeArt transform in page coordinates.
///
/// OfficeArt stores rotation and FSP mirrors independently. The canonical
/// Reader geometry already uses page-coordinate bounds, so the returned matrix
/// includes translation that keeps the shape center fixed.
pub(crate) fn direct_shape_affine(
    bounds: RectEmu,
    rotation_raw_16_16: Option<i32>,
    flip_h: bool,
    flip_v: bool,
) -> Option<Affine2D> {
    let raw = rotation_raw_16_16.unwrap_or(0);
    if raw == 0 && !flip_h && !flip_v {
        return Some(Affine2D::identity());
    }

    // Existing OfficeArt authority applies rotate then page-axis mirrors.
    // Canonical matrix composition keeps local mirrors on the right, so a
    // single reflection requires the equivalent rotation sign reversal.
    let effective_raw = if flip_h ^ flip_v {
        raw.checked_neg()?
    } else {
        raw
    };
    let angle_bam = rotation_raw_to_bam(effective_raw);
    let (cos_q31, sin_q31) = cordic_sin_cos_q31(angle_bam);
    let sx = if flip_h { -1_i64 } else { 1_i64 };
    let sy = if flip_v { -1_i64 } else { 1_i64 };

    let a = cos_q31.checked_mul(sx)?;
    let b = sin_q31.checked_mul(sx)?;
    let c = sin_q31.checked_neg()?.checked_mul(sy)?;
    let d = cos_q31.checked_mul(sy)?;
    let (tx, ty) = center_preserving_translation(bounds, a, b, c, d)?;

    Some(Affine2D {
        a: q31_decimal(a),
        b: q31_decimal(b),
        c: q31_decimal(c),
        d: q31_decimal(d),
        tx,
        ty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> RectEmu {
        RectEmu::new(
            LengthEmu::new(100),
            LengthEmu::new(200),
            LengthEmu::new(300),
            LengthEmu::new(500),
        )
    }

    fn q31(value: &Decimal) -> i64 {
        let text = value.as_str();
        let negative = text.starts_with('-');
        let body = text.trim_start_matches('-');
        let (whole, fraction) = body.split_once('.').unwrap_or((body, ""));
        let whole = whole.parse::<i128>().unwrap();
        let mut frac = fraction.to_owned();
        while frac.len() < 31 {
            frac.push('0');
        }
        let frac = frac.parse::<i128>().unwrap_or(0);
        let numerator = whole * i128::from(Q31_ONE)
            + div_round_nearest(frac * i128::from(Q31_ONE), 10_i128.pow(31));
        let signed = if negative { -numerator } else { numerator };
        i64::try_from(signed).unwrap()
    }

    #[test]
    fn identity_is_exact() {
        assert_eq!(
            direct_shape_affine(bounds(), None, false, false),
            Some(Affine2D::identity())
        );
    }

    #[test]
    fn quarter_turn_is_exact_and_center_preserving() {
        let transform = direct_shape_affine(bounds(), Some(90_i32 << 16), false, false).unwrap();
        assert_eq!(transform.a.as_str(), "0");
        assert_eq!(transform.b.as_str(), "1");
        assert_eq!(transform.c.as_str(), "-1");
        assert_eq!(transform.d.as_str(), "0");

        let center_x2 = 2 * bounds().x.get() + bounds().width.get();
        let center_y2 = 2 * bounds().y.get() + bounds().height.get();
        let a = q31(&transform.a);
        let b = q31(&transform.b);
        let c = q31(&transform.c);
        let d = q31(&transform.d);
        let transformed_x2 =
            (i128::from(a) * i128::from(center_x2) + i128::from(c) * i128::from(center_y2))
                / i128::from(Q31_ONE)
                + i128::from(2 * transform.tx.get());
        let transformed_y2 =
            (i128::from(b) * i128::from(center_x2) + i128::from(d) * i128::from(center_y2))
                / i128::from(Q31_ONE)
                + i128::from(2 * transform.ty.get());
        assert_eq!(transformed_x2, i128::from(center_x2));
        assert_eq!(transformed_y2, i128::from(center_y2));
    }

    #[test]
    fn fractional_rotation_remains_non_cardinal() {
        let raw = (38_i32 << 16) + 42_926;
        let transform = direct_shape_affine(bounds(), Some(raw), false, false).unwrap();
        assert_ne!(transform, Affine2D::identity());
        assert_ne!(transform.a.as_str(), "0");
        assert_ne!(transform.a.as_str(), "1");
        assert_ne!(transform.b.as_str(), "0");
    }

    #[test]
    fn flips_are_independent_and_follow_xor_rotation_conversion() {
        let none = direct_shape_affine(bounds(), Some(45_i32 << 16), false, false).unwrap();
        let h = direct_shape_affine(bounds(), Some(45_i32 << 16), true, false).unwrap();
        let v = direct_shape_affine(bounds(), Some(45_i32 << 16), false, true).unwrap();
        let hv = direct_shape_affine(bounds(), Some(45_i32 << 16), true, true).unwrap();

        assert_ne!(none, h);
        assert_ne!(none, v);
        assert_ne!(none, hv);
        assert_eq!(h.a.as_str(), v.d.as_str());
        assert_eq!(h.b.as_str(), v.c.as_str());
    }
}
