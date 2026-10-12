use super::*;

pub(super) fn page_relative_bounds_from_contents_missing_xe(
    page: &Page,
    anchor: &PublisherFieldRecord,
    contents_width: u32,
    contents_height: u32,
) -> Option<RectEmu> {
    if anchor
        .fields
        .iter()
        .filter(|field| field.id == PUBLISHER_FIELD_XE)
        .count()
        != 0
    {
        return None;
    }

    let xs = signed_field(anchor, PUBLISHER_FIELD_XS)?;
    let ys = signed_field(anchor, PUBLISHER_FIELD_YS)?;
    let ye = signed_field(anchor, PUBLISHER_FIELD_YE)?;
    page_relative_bounds_from_contents_missing_xe_values(
        page,
        xs,
        ys,
        ye,
        contents_width,
        contents_height,
    )
}

pub(super) fn page_relative_bounds_from_contents_missing_xe_values(
    page: &Page,
    xs: i64,
    ys: i64,
    ye: i64,
    contents_width: u32,
    contents_height: u32,
) -> Option<RectEmu> {
    let width = i64::from(contents_width);
    let height = i64::from(contents_height);
    if width <= 0 || height <= 0 || ye.checked_sub(ys)? != height {
        return None;
    }

    xs.checked_add(width)?;
    let x = page.size.width.get().checked_div(2)?.checked_add(xs)?;
    let y = page.size.height.get().checked_div(2)?.checked_add(ys)?;

    Some(RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    ))
}

pub(super) fn page_relative_bounds(page: &Page, anchor: &PublisherFieldRecord) -> Option<RectEmu> {
    let xs = signed_field(anchor, PUBLISHER_FIELD_XS)?;
    let ys = signed_field(anchor, PUBLISHER_FIELD_YS)?;
    let xe = signed_field(anchor, PUBLISHER_FIELD_XE)?;
    let ye = signed_field(anchor, PUBLISHER_FIELD_YE)?;

    let width = xe.checked_sub(xs)?;
    let height = ye.checked_sub(ys)?;
    if width <= 0 || height <= 0 {
        return None;
    }

    let x = page.size.width.get().checked_div(2)?.checked_add(xs)?;
    let y = page.size.height.get().checked_div(2)?.checked_add(ys)?;

    Some(RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    ))
}

/// Return a positive-size paint envelope for an exact horizontal OfficeArt
/// LINE without changing its centered source stroke axis. The caller must
/// independently authorize the source's line type, width, color, and visibility.
#[allow(dead_code)] // Staged geometry primitive; source-line admission is separately gated.
pub(super) fn page_relative_horizontal_stroke_envelope(
    page: &Page,
    anchor: &PublisherFieldRecord,
    stroke_width_emu: i64,
) -> Option<RectEmu> {
    horizontal_stroke_envelope_values(
        page.size.width.get(),
        page.size.height.get(),
        signed_field(anchor, PUBLISHER_FIELD_XS)?,
        signed_field(anchor, PUBLISHER_FIELD_YS)?,
        signed_field(anchor, PUBLISHER_FIELD_XE)?,
        signed_field(anchor, PUBLISHER_FIELD_YE)?,
        stroke_width_emu,
    )
}

#[allow(dead_code)] // Used by the gated source-line bridge and regression tests.
fn horizontal_stroke_envelope_values(
    page_width_emu: i64,
    page_height_emu: i64,
    xs: i64,
    ys: i64,
    xe: i64,
    ye: i64,
    stroke_width_emu: i64,
) -> Option<RectEmu> {
    // Odd-width strokes would have a half-EMU central axis which the integer
    // RectEmu model cannot preserve exactly. Fail closed until that profile
    // has a native-sourced fractional line geometry representation.
    if page_width_emu <= 0
        || page_height_emu <= 0
        || stroke_width_emu <= 0
        || stroke_width_emu % 2 != 0
        || ys != ye
        || xe <= xs
    {
        return None;
    }
    let x0 = page_width_emu.checked_div(2)?.checked_add(xs)?;
    let x1 = page_width_emu.checked_div(2)?.checked_add(xe)?;
    let axis_y = page_height_emu.checked_div(2)?.checked_add(ys)?;
    let top = axis_y.checked_sub(stroke_width_emu / 2)?;
    let width = x1.checked_sub(x0)?;
    top.checked_add(stroke_width_emu)?;
    Some(RectEmu::new(
        LengthEmu::new(x0),
        LengthEmu::new(top),
        LengthEmu::new(width),
        LengthEmu::new(stroke_width_emu),
    ))
}

pub(super) fn signed_field(record: &PublisherFieldRecord, id: u16) -> Option<i64> {
    let field = unique_escher_field(record, id)?;
    Some(i64::from(i32::from_le_bytes(field.value.to_le_bytes())))
}

pub(super) fn unique_escher_field(
    record: &PublisherFieldRecord,
    id: u16,
) -> Option<&PublisherField> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

pub(super) fn anchor_has_unique_geometry_fields(anchor: &PublisherFieldRecord) -> bool {
    [
        PUBLISHER_FIELD_XS,
        PUBLISHER_FIELD_YS,
        PUBLISHER_FIELD_XE,
        PUBLISHER_FIELD_YE,
    ]
    .into_iter()
    .all(|id| unique_escher_field(anchor, id).is_some())
}

pub(super) fn record_missing_anchor_fields(
    anchor: Option<&PublisherFieldRecord>,
    counts: &mut BTreeMap<String, usize>,
) {
    for (id, label) in [
        (PUBLISHER_FIELD_XS, "xs"),
        (PUBLISHER_FIELD_YS, "ys"),
        (PUBLISHER_FIELD_XE, "xe"),
        (PUBLISHER_FIELD_YE, "ye"),
    ] {
        if anchor
            .and_then(|record| unique_escher_field(record, id))
            .is_none()
        {
            *counts.entry(label.to_owned()).or_insert(0) += 1;
        }
    }
}

#[cfg(test)]
mod horizontal_stroke_envelope_tests {
    use super::*;

    #[test]
    fn exact_newsletter_source_line_405_preserves_horizontal_axis() {
        let bounds = horizontal_stroke_envelope_values(
            7_560_000, 10_692_000, -2_721_250, -4_589_726, 4_008_599, -4_589_726, 101_600,
        )
        .expect("actual source-pinned PAGE381 horizontal stroke");
        assert_eq!(bounds.x.get(), 1_058_750);
        assert_eq!(bounds.y.get(), 705_474);
        assert_eq!(bounds.width.get(), 6_729_849);
        assert_eq!(bounds.height.get(), 101_600);
        assert_eq!(bounds.y.get() + bounds.height.get() / 2, 756_274);
        assert_eq!(bounds.x.get() + bounds.width.get(), 7_788_599);
    }

    #[test]
    fn rejects_diagonal_reversed_empty_odd_and_overflowing_envelopes() {
        let good = (
            7_560_000, 10_692_000, -2_721_250, -4_589_726, 4_008_599, -4_589_726,
        );
        assert!(
            horizontal_stroke_envelope_values(
                good.0,
                good.1,
                good.2,
                good.3,
                good.4,
                good.5 + 1,
                101_600,
            )
            .is_none()
        );
        assert!(
            horizontal_stroke_envelope_values(
                good.0, good.1, good.4, good.3, good.2, good.5, 101_600,
            )
            .is_none()
        );
        for width in [0, -1, 101_601] {
            assert!(
                horizontal_stroke_envelope_values(
                    good.0, good.1, good.2, good.3, good.4, good.5, width,
                )
                .is_none()
            );
        }
        assert!(
            horizontal_stroke_envelope_values(
                good.0,
                good.1,
                i64::MAX,
                good.3,
                good.4,
                good.5,
                101_600,
            )
            .is_none()
        );
        assert!(
            horizontal_stroke_envelope_values(0, good.1, good.2, good.3, good.4, good.5, 101_600,)
                .is_none()
        );
    }
}
