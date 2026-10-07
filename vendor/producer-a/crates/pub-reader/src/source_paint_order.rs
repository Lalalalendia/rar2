use super::*;

pub const PUB_SOURCE_PAGE_PAINT_ORDER_SCHEMA_V1: &str = "chaptera.pub-source-page-paint-order.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubSourcePagePaintOrderV1 {
    pub schema_version: String,
    pub page_id: PageId,
    /// Canonical Node identities in persisted OfficeArt back-to-front order.
    pub node_ids: Vec<NodeId>,
}

pub(super) fn index_escher_by_contents_seq(
    inventory: &SpContainerInventory,
) -> BTreeMap<u32, Vec<usize>> {
    let mut index = BTreeMap::<u32, Vec<usize>>::new();

    for (shape_index, shape) in inventory.shapes.iter().enumerate() {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        for field in client_data
            .fields
            .iter()
            .filter(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
        {
            let entry = index.entry(field.value).or_default();
            if entry.last().copied() != Some(shape_index) {
                entry.push(shape_index);
            }
        }
    }

    index
}

type GroupedCarrierParticipant = (usize, NodeId);
type GroupedCarrierMap = BTreeMap<u32, (PageId, Vec<GroupedCarrierParticipant>)>;

pub(super) fn append_grouped_carrier_participants(
    seq_num: u32,
    grouped_by_carrier: &GroupedCarrierMap,
    seen_seq: &mut BTreeSet<u32>,
    rejected: &mut BTreeSet<PageId>,
    ordered: &mut BTreeMap<PageId, Vec<NodeId>>,
) -> bool {
    let Some((page_id, grouped_nodes)) = grouped_by_carrier.get(&seq_num) else {
        return false;
    };
    if !seen_seq.insert(seq_num) {
        rejected.insert(*page_id);
        return true;
    }
    ordered
        .entry(*page_id)
        .or_default()
        .extend(grouped_nodes.iter().map(|(_, node_id)| *node_id));
    true
}

pub(super) fn source_page_paint_orders_v1(
    source_hash: Sha256Digest,
    graph: &PubSourceGraph,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &BTreeMap<u32, PageId>,
    inventory: &SpContainerInventory,
) -> Vec<PubSourcePagePaintOrderV1> {
    let escher_by_contents_seq = index_escher_by_contents_seq(inventory);
    let mut expected = BTreeMap::<PageId, BTreeSet<NodeId>>::new();

    // Depth-1 grouped descendants are projected as page-owned canonical Nodes,
    // but their persisted Contents parent remains the GROUP carrier. Admit that
    // class into page paint order only when every participating descendant on
    // the page has one exact child Escher shape, one exact top-level GROUP
    // Escher carrier, and the OfficeArt parent-group source link agrees.
    let mut grouped_pending = BTreeMap::<PageId, Vec<(u32, usize, NodeId)>>::new();
    let mut grouped_invalid_pages = BTreeSet::<PageId>::new();

    for node in graph.nodes.values() {
        let seq_num = node.payload.contents_seq_num;
        let Some(reference) = references.get(&seq_num) else {
            continue;
        };
        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };

        if let Some(page_id) = page_seq_to_id.get(&parent_seq).copied() {
            if node.header.parent_id == page_id.into_canonical() {
                expected.entry(page_id).or_default().insert(node.header.id);
            }
            continue;
        }

        let Some(group_reference) = references.get(&parent_seq) else {
            continue;
        };
        if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
            continue;
        }
        let Some(group_parent_seq) = single_parent_seq(group_reference) else {
            continue;
        };
        let Some(page_id) = page_seq_to_id.get(&group_parent_seq).copied() else {
            // Nested groups remain outside this first bounded stack-order slice.
            continue;
        };
        if node.header.parent_id != page_id.into_canonical() {
            continue;
        }
        // #632 proves the first carrier-rank class only for visible grouped
        // Story and image descendants. Grouped TABLE/other classes remain
        // outside this slice even when they happen to have exact ancestry.
        if node.payload.story_frame.is_none() && node.payload.image_slot.is_none() {
            continue;
        }

        let child_matches = escher_by_contents_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let group_matches = escher_by_contents_seq
            .get(&parent_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let ([child_index], [group_index]) = (child_matches, group_matches) else {
            grouped_invalid_pages.insert(page_id);
            continue;
        };
        let child_shape = &inventory.shapes[*child_index];
        let group_shape = &inventory.shapes[*group_index];
        if child_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
            grouped_invalid_pages.insert(page_id);
            continue;
        }

        grouped_pending.entry(page_id).or_default().push((
            parent_seq,
            *child_index,
            node.header.id,
        ));
    }

    let mut grouped_by_carrier = GroupedCarrierMap::new();
    for (page_id, entries) in grouped_pending {
        if grouped_invalid_pages.contains(&page_id) {
            continue;
        }
        for (carrier_seq, child_index, node_id) in entries {
            expected.entry(page_id).or_default().insert(node_id);
            let entry = grouped_by_carrier
                .entry(carrier_seq)
                .or_insert_with(|| (page_id, Vec::new()));
            if entry.0 != page_id {
                grouped_invalid_pages.insert(page_id);
                continue;
            }
            entry.1.push((child_index, node_id));
        }
    }
    for (_, grouped) in grouped_by_carrier.values_mut() {
        grouped.sort_by_key(|(child_index, _)| *child_index);
    }

    let mut ordered = BTreeMap::<PageId, Vec<NodeId>>::new();
    let mut rejected = BTreeSet::<PageId>::new();
    let mut seen_seq = BTreeSet::<u32>::new();

    // inspect_sp_containers preserves serialized traversal order. Do not sort
    // SPIDs here: serialized page SpContainer order is the bounded authority.
    // Exact depth-1 grouped descendants occupy the serialized position of their
    // top-level GROUP carrier; their internal order remains the already-proven
    // child SpContainer traversal order.
    for shape in &inventory.shapes {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        let Some(shape_id) = unique_escher_field(client_data, PUBLISHER_FIELD_SHAPE_ID) else {
            continue;
        };
        let seq_num = shape_id.value;

        if append_grouped_carrier_participants(
            seq_num,
            &grouped_by_carrier,
            &mut seen_seq,
            &mut rejected,
            &mut ordered,
        ) {
            continue;
        }

        let Some(reference) = references.get(&seq_num) else {
            continue;
        };
        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };
        let Some(page_id) = page_seq_to_id.get(&parent_seq).copied() else {
            continue;
        };
        let Ok(node_id) = derive_pub_node_id(&source_hash, seq_num) else {
            rejected.insert(page_id);
            continue;
        };
        if !graph.nodes.contains_key(&node_id) {
            continue;
        }
        if !seen_seq.insert(seq_num) {
            rejected.insert(page_id);
            continue;
        }
        ordered.entry(page_id).or_default().push(node_id);
    }

    expected
        .into_iter()
        .filter_map(|(page_id, expected_nodes)| {
            if rejected.contains(&page_id) || expected_nodes.is_empty() {
                return None;
            }
            let node_ids = ordered.remove(&page_id)?;
            let actual_nodes = node_ids.iter().copied().collect::<BTreeSet<_>>();
            (node_ids.len() == actual_nodes.len() && actual_nodes == expected_nodes).then(|| {
                PubSourcePagePaintOrderV1 {
                    schema_version: PUB_SOURCE_PAGE_PAINT_ORDER_SCHEMA_V1.to_owned(),
                    page_id,
                    node_ids,
                }
            })
        })
        .collect()
}

pub(super) fn grouped_object_target_page_trace(
    first_group_seq: u32,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &BTreeMap<u32, PageId>,
) -> (Option<PageId>, Vec<u32>) {
    let mut current_group_seq = first_group_seq;
    let mut seen = BTreeSet::new();
    let mut ancestry = Vec::new();

    for _ in 0..64 {
        if !seen.insert(current_group_seq) {
            return (None, ancestry);
        }
        ancestry.push(current_group_seq);

        let Some(reference) = references.get(&current_group_seq) else {
            return (None, ancestry);
        };
        let Some(parent_seq) = single_parent_seq(reference) else {
            return (None, ancestry);
        };
        if let Some(&page_id) = page_seq_to_id.get(&parent_seq) {
            return (Some(page_id), ancestry);
        }
        if references.get(&parent_seq).and_then(single_raw_type) != Some(RAW_TYPE_GROUP) {
            return (None, ancestry);
        }
        current_group_seq = parent_seq;
    }

    (None, ancestry)
}

// Grouped OfficeArt geometry projection shares exact group carrier/page ancestry with paint order.

#[derive(Debug)]
pub(super) struct GroupedObjectProjection {
    pub(super) page_id: PageId,
    pub(super) bounds: RectEmu,
    pub(super) depth: usize,
    pub(super) group_sources: Vec<RawSpan>,
}

pub(super) fn project_grouped_object_shape(
    first_group_seq: u32,
    child_shape: &pub_escher::SpContainerObservation,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &BTreeMap<u32, PageId>,
    pages: &BTreeMap<PageId, Page>,
    escher_inventory: &SpContainerInventory,
    escher_by_contents_seq: &BTreeMap<u32, Vec<usize>>,
) -> Result<Option<GroupedObjectProjection>> {
    if shape_has_nonzero_rotation(child_shape) {
        bail!("grouped child has nonzero rotation");
    }
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
        let group_reference = references
            .get(&current_group_seq)
            .context("group Contents reference missing")?;
        if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
            bail!("group ancestry raw type is not 0x30");
        }

        let group_matches = escher_by_contents_seq
            .get(&current_group_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let group_shape = match group_matches {
            [index] => &escher_inventory.shapes[*index],
            [] => bail!("group Escher shape missing"),
            _ => bail!("group Escher shape ambiguous"),
        };
        if current_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
            bail!("OfficeArt parent-group link does not match Contents ancestry");
        }
        if shape_has_nonzero_rotation(group_shape) {
            bail!("group ancestor has nonzero rotation");
        }
        if shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_H) {
            bail!("group ancestor has horizontal flip");
        }
        if shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_V) {
            bail!("group ancestor has vertical flip");
        }

        let fspgr = group_shape
            .fspgr
            .as_ref()
            .context("group is missing FSPGR")?;
        let group_coords = coordinate_rect_i128(fspgr)?;
        group_sources.push(group_shape.source.clone());

        let parent_seq =
            single_parent_seq(group_reference).context("group parent is missing or ambiguous")?;
        if let Some(&page_id) = page_seq_to_id.get(&parent_seq) {
            let anchor = group_shape
                .client_anchor
                .as_ref()
                .context("top group is missing ClientAnchor")?;
            let absolute = publisher_anchor_rect_i128(anchor)?;
            rect = project_rect_trunc(rect, group_coords, absolute)?;
            let page = pages
                .get(&page_id)
                .context("group page id is missing from graph")?;
            let bounds = center_origin_rect_to_page_bounds(page, rect)?;
            return Ok(Some(GroupedObjectProjection {
                page_id,
                bounds,
                depth,
                group_sources,
            }));
        }

        if depth == 2 {
            bail!("group ancestry exceeds bounded depth 2");
        }
        if references.get(&parent_seq).and_then(single_raw_type) != Some(RAW_TYPE_GROUP) {
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

pub(super) fn coordinate_rect_i128(rect: &pub_escher::OfficeArtCoordinateRect) -> Result<[i128; 4]> {
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
