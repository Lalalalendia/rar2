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

pub(super) fn signed_field(record: &PublisherFieldRecord, id: u16) -> Option<i64> {
    let field = unique_escher_field(record, id)?;
    Some(i64::from(i32::from_le_bytes(field.value.to_le_bytes())))
}

fn unique_escher_field(record: &PublisherFieldRecord, id: u16) -> Option<&PublisherField> {
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

fn record_missing_anchor_fields(
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
