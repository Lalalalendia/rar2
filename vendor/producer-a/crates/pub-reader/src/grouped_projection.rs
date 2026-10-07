//! Bounded production projection for grouped OfficeArt objects.
//!
//! This module owns ChildAnchor/FSPGR/ClientAnchor composition for bounded
//! group ancestry. Direct page-local geometry and graph orchestration remain
//! in the crate root.

use super::*;

#[derive(Debug)]
pub(super) struct GroupedObjectProjection {
    pub(super) page_id: PageId,
    pub(super) bounds: RectEmu,
    pub(super) depth: usize,
    pub(super) group_sources: Vec<RawSpan>,
    pub(super) child_rotation_op: Option<u32>,
}

pub(super) struct GroupedProjectionContext<'a> {
    references: &'a BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &'a BTreeMap<u32, PageId>,
    pages: &'a BTreeMap<PageId, Page>,
    escher_inventory: &'a SpContainerInventory,
    escher_by_contents_seq: &'a BTreeMap<u32, Vec<usize>>,
}

impl<'a> GroupedProjectionContext<'a> {
    pub(super) fn new(
        references: &'a BTreeMap<u32, Contents0x2cChunkReference>,
        page_seq_to_id: &'a BTreeMap<u32, PageId>,
        pages: &'a BTreeMap<PageId, Page>,
        escher_inventory: &'a SpContainerInventory,
        escher_by_contents_seq: &'a BTreeMap<u32, Vec<usize>>,
    ) -> Self {
        Self {
            references,
            page_seq_to_id,
            pages,
            escher_inventory,
            escher_by_contents_seq,
        }
    }
}

pub(super) fn project_grouped_object_shape(
    first_group_seq: u32,
    child_shape: &pub_escher::SpContainerObservation,
    context: &GroupedProjectionContext<'_>,
    admit_translation_only_child_image_rotation: bool,
) -> Result<Option<GroupedObjectProjection>> {
    let child_rotation_op = if shape_has_nonzero_rotation(child_shape) {
        if !admit_translation_only_child_image_rotation {
            bail!("grouped child has nonzero rotation");
        }
        let rotations = child_shape
            .fopts
            .iter()
            .flat_map(|record| record.properties.iter())
            .filter(|property| property.property_id() == OFFICE_ART_PROPERTY_ROTATION)
            .collect::<Vec<_>>();
        match rotations.as_slice() {
            [property] if !property.f_bid() && !property.f_complex() && property.op as i32 != 0 => {
                Some(property.op)
            }
            _ => bail!("grouped child rotation profile is ambiguous"),
        }
    } else {
        None
    };
    if shape_has_fsp_flag(child_shape, OFFICEART_FSP_FLIP_H) {
        bail!("grouped child has horizontal flip");
    }
    if shape_has_fsp_flag(child_shape, OFFICEART_FSP_FLIP_V) {
        bail!("grouped child has vertical flip");
    }

    let child_anchor = child_shape
        .child_anchor
        .as_ref()
        .context("grouped child is missing ChildAnchor")?;
    let mut rect = coordinate_rect_i128(child_anchor)?;
    let mut current_shape = child_shape;
    let mut current_group_seq = first_group_seq;
    let mut seen = BTreeSet::new();
    let mut group_sources = Vec::new();

    for depth in 1..=2 {
        if !seen.insert(current_group_seq) {
            bail!("group ancestry cycle");
        }
        let group_reference = context
            .references
            .get(&current_group_seq)
            .context("group Contents reference missing")?;
        if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
            bail!("group ancestry raw type is not 0x30");
        }

        let group_matches = context
            .escher_by_contents_seq
            .get(&current_group_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let group_shape = match group_matches {
            [index] => &context.escher_inventory.shapes[*index],
            [] => bail!("group Escher shape missing"),
            _ => bail!("group Escher shape ambiguous"),
        };
        if current_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
            bail!("OfficeArt parent-group link does not match Contents ancestry");
        }
        if shape_has_nonzero_rotation(group_shape)
            || shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_H)
            || shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_V)
        {
            bail!("group ancestor has rotation or flip");
        }

        let fspgr = group_shape
            .fspgr
            .as_ref()
            .context("group is missing FSPGR")?;
        let group_coords = coordinate_rect_i128(fspgr)?;
        group_sources.push(group_shape.source.clone());

        let parent_seq =
            single_parent_seq(group_reference).context("group parent is missing or ambiguous")?;
        if let Some(&page_id) = context.page_seq_to_id.get(&parent_seq) {
            let anchor = group_shape
                .client_anchor
                .as_ref()
                .context("top group is missing ClientAnchor")?;
            let absolute = publisher_anchor_rect_i128(anchor)?;
            if child_rotation_op.is_some()
                && (depth != 1 || !translation_only_group_map(group_coords, absolute))
            {
                bail!("grouped child rotation requires translation-only depth-1 parent group");
            }
            rect = project_rect_trunc(rect, group_coords, absolute)?;
            let page = context
                .pages
                .get(&page_id)
                .context("group page id is missing from graph")?;
            let bounds = center_origin_rect_to_page_bounds(page, rect)?;
            return Ok(Some(GroupedObjectProjection {
                page_id,
                bounds,
                depth,
                group_sources,
                child_rotation_op,
            }));
        }

        if depth == 2 {
            bail!("group ancestry exceeds bounded depth 2");
        }
        if context
            .references
            .get(&parent_seq)
            .and_then(single_raw_type)
            != Some(RAW_TYPE_GROUP)
        {
            bail!("group parent is neither DOCUMENT page nor group");
        }

        let placement = group_shape
            .child_anchor
            .as_ref()
            .context("nested group is missing ChildAnchor")?;
        rect = project_rect_trunc(rect, group_coords, coordinate_rect_i128(placement)?)?;
        current_shape = group_shape;
        current_group_seq = parent_seq;
    }

    Ok(None)
}

pub(super) fn coordinate_rect_i128(
    rect: &pub_escher::OfficeArtCoordinateRect,
) -> Result<[i128; 4]> {
    let out = [
        i128::from(rect.x_left),
        i128::from(rect.y_top),
        i128::from(rect.x_right),
        i128::from(rect.y_bottom),
    ];
    if out[2] <= out[0] || out[3] <= out[1] {
        bail!("coordinate rectangle is non-positive");
    }
    Ok(out)
}

fn translation_only_group_map(source: [i128; 4], target: [i128; 4]) -> bool {
    source[2] - source[0] == target[2] - target[0] && source[3] - source[1] == target[3] - target[1]
}

fn publisher_anchor_rect_i128(anchor: &PublisherFieldRecord) -> Result<[i128; 4]> {
    let xs = signed_field(anchor, PUBLISHER_FIELD_XS).context("group ClientAnchor missing XS")?;
    let ys = signed_field(anchor, PUBLISHER_FIELD_YS).context("group ClientAnchor missing YS")?;
    let xe = signed_field(anchor, PUBLISHER_FIELD_XE).context("group ClientAnchor missing XE")?;
    let ye = signed_field(anchor, PUBLISHER_FIELD_YE).context("group ClientAnchor missing YE")?;
    let out = [
        i128::from(xs),
        i128::from(ys),
        i128::from(xe),
        i128::from(ye),
    ];
    if out[2] <= out[0] || out[3] <= out[1] {
        bail!("group ClientAnchor rectangle is non-positive");
    }
    Ok(out)
}

/// Deterministic grouped-geometry projection.
///
/// Integer division in Rust truncates toward zero. Raw FSPGR/ChildAnchor
/// records remain authoritative provenance; these rounded EMU coordinates are
/// a derived runtime projection only.
pub(super) fn project_rect_trunc(
    rect: [i128; 4],
    source_space: [i128; 4],
    target_space: [i128; 4],
) -> Result<[i128; 4]> {
    let x0 = project_axis_trunc(
        rect[0],
        source_space[0],
        source_space[2],
        target_space[0],
        target_space[2],
    )?;
    let y0 = project_axis_trunc(
        rect[1],
        source_space[1],
        source_space[3],
        target_space[1],
        target_space[3],
    )?;
    let x1 = project_axis_trunc(
        rect[2],
        source_space[0],
        source_space[2],
        target_space[0],
        target_space[2],
    )?;
    let y1 = project_axis_trunc(
        rect[3],
        source_space[1],
        source_space[3],
        target_space[1],
        target_space[3],
    )?;
    if x1 <= x0 || y1 <= y0 {
        bail!("projected grouped rectangle is non-positive");
    }
    Ok([x0, y0, x1, y1])
}

fn project_axis_trunc(
    value: i128,
    source_start: i128,
    source_end: i128,
    target_start: i128,
    target_end: i128,
) -> Result<i128> {
    let source_len = source_end - source_start;
    let target_len = target_end - target_start;
    if source_len <= 0 || target_len <= 0 {
        bail!("group projection has non-positive coordinate extent");
    }
    Ok(target_start + (value - source_start) * target_len / source_len)
}

fn center_origin_rect_to_page_bounds(page: &Page, rect: [i128; 4]) -> Result<RectEmu> {
    let half_width = i128::from(page.size.width.get()) / 2;
    let half_height = i128::from(page.size.height.get()) / 2;
    let x = half_width + rect[0];
    let y = half_height + rect[1];
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];

    let to_i64 = |value: i128, label: &str| {
        i64::try_from(value).with_context(|| format!("{label} does not fit i64"))
    };
    Ok(RectEmu::new(
        LengthEmu::new(to_i64(x, "grouped x")?),
        LengthEmu::new(to_i64(y, "grouped y")?),
        LengthEmu::new(to_i64(width, "grouped width")?),
        LengthEmu::new(to_i64(height, "grouped height")?),
    ))
}

#[cfg(test)]
mod grouped_rotation_tests {
    use super::translation_only_group_map;

    #[test]
    fn translation_only_group_map_accepts_equal_extents() {
        assert!(translation_only_group_map(
            [108_041_202, 110_367_519, 113_739_460, 113_040_661],
            [-2_143_998, 182_319, 3_554_260, 2_855_461],
        ));
    }

    #[test]
    fn translation_only_group_map_rejects_anisotropic_scale() {
        assert!(!translation_only_group_map(
            [107_442_022, 108_754_329, 113_987_230, 112_497_502],
            [-3_987_897, -1_596_946, 4_327_889, 2_562_119],
        ));
    }
}
