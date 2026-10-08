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

/// Group carrier-rank extension is deliberately limited to locally filled
/// rectangles. Existing grouped ellipses and unproven primitives retain the
/// original source-order exclusion.
pub(super) fn admit_grouped_paint_participant(
    has_story: bool,
    has_image: bool,
    bounded_solid_rectangle: bool,
) -> bool {
    has_story || has_image || bounded_solid_rectangle
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
        // Grouped Story/image ordering remains unchanged. The only newly
        // admitted primitive is a source-backed solid rectangle, with exact
        // group ancestry and positive local fill/color evidence.
        let solid_rectangle = node.payload.officeart_shape_type
            == Some(OFFICEART_SHAPE_TYPE_RECTANGLE)
            && node.payload.explicit_paint.fill.visible == Some(true)
            && node.payload.explicit_paint.fill.color_rgb.is_some();
        if !admit_grouped_paint_participant(
            node.payload.story_frame.is_some(),
            node.payload.image_slot.is_some(),
            solid_rectangle,
        ) {
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

#[cfg(test)]
mod grouped_paint_order_tests {
    use super::*;

    #[test]
    fn grouped_rectangle_extends_only_the_proven_participant_class() {
        assert!(admit_grouped_paint_participant(true, false, false));
        assert!(admit_grouped_paint_participant(false, true, false));
        assert!(admit_grouped_paint_participant(false, false, true));
        assert!(!admit_grouped_paint_participant(false, false, false));
    }
}
