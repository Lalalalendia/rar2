use super::*;

fn test_bounds() -> RectEmu {
    RectEmu::new(
        LengthEmu::new(100),
        LengthEmu::new(200),
        LengthEmu::new(300),
        LengthEmu::new(500),
    )
}

#[test]
fn direct_image_missing_xe_can_recover_from_exact_contents_extent() {
    let page = Page {
        id: test_page_id(99),
        size: Size2D::new(LengthEmu::new(7_772_400), LengthEmu::new(10_058_400)),
        bleed: None,
        margins: None,
        children: Vec::new(),
        extensions: Vec::new(),
    };
    let bounds = anchor_geometry::page_relative_bounds_from_contents_missing_xe_values(
        &page, -3_429_000, 259_080, 2_899_410, 3_429_000, 2_640_330,
    )
    .expect("Contents extent should recover the measured missing-XE image anchor");
    assert_eq!(bounds.x.get(), 457_200);
    assert_eq!(bounds.width.get(), 3_429_000);
    assert_eq!(bounds.height.get(), 2_640_330);
}

#[test]
fn direct_image_missing_xe_recovery_rejects_cross_stream_height_mismatch() {
    let page = Page {
        id: test_page_id(100),
        size: Size2D::new(LengthEmu::new(7_772_400), LengthEmu::new(10_058_400)),
        bleed: None,
        margins: None,
        children: Vec::new(),
        extensions: Vec::new(),
    };
    assert_eq!(
        anchor_geometry::page_relative_bounds_from_contents_missing_xe_values(
            &page, -3_429_000, 259_080, 2_899_410, 3_429_000, 2_640_329,
        ),
        None
    );
}

#[test]
fn direct_story_rotation_preserves_exact_cardinal_affine_transform() {
    for rotation_op in [90u32 << 16, ((-90i32) << 16) as u32, 180u32 << 16] {
        let transform =
            bounded_direct_story_transform(&[(rotation_op, false, false)], 0, test_bounds())
                .expect("bounded direct Story rotation should be admitted");
        assert_ne!(transform, Affine2D::identity());
    }
}

#[test]
fn direct_story_rotation_identity_and_unsupported_states_fail_closed() {
    assert_eq!(
        bounded_direct_story_transform(&[], 0, test_bounds()),
        Some(Affine2D::identity())
    );
    assert_eq!(
        bounded_direct_story_transform(&[(0, false, false)], 0, test_bounds()),
        Some(Affine2D::identity())
    );
    assert_eq!(
        bounded_direct_story_transform(
            &[(90u32 << 16, false, false)],
            FSP_FLIP_H,
            test_bounds(),
        ),
        None
    );
    assert_eq!(
        bounded_direct_story_transform(
            &[(90u32 << 16, false, false), (180u32 << 16, false, false)],
            0,
            test_bounds(),
        ),
        None
    );
    assert_eq!(
        bounded_direct_story_transform(&[(90u32 << 16, false, true)], 0, test_bounds(),),
        None
    );
}

#[test]
fn direct_image_rotation_absent_or_zero_stays_identity() {
    assert_eq!(
        bounded_direct_image_transform(&[], 0, test_bounds()),
        BoundedDirectImageTransform::Identity
    );
    assert_eq!(
        bounded_direct_image_transform(&[(0, false, false)], 0, test_bounds()),
        BoundedDirectImageTransform::Identity
    );
    assert_eq!(
        bounded_direct_image_transform(&[((360u32) << 16, false, false)], 0, test_bounds()),
        BoundedDirectImageTransform::Identity
    );
}

#[test]
fn direct_image_cardinal_rotation_is_preserved_for_picture_content_only() {
    assert_eq!(
        bounded_direct_image_cardinal_content_rotation_degrees(
            &[((90u32) << 16, false, false)],
            0,
        ),
        Some(90)
    );
    assert_eq!(
        bounded_direct_image_cardinal_content_rotation_degrees(
            &[((180u32) << 16, false, false)],
            0,
        ),
        Some(180)
    );
    assert_eq!(
        bounded_direct_image_cardinal_content_rotation_degrees(
            &[((270u32) << 16, false, false)],
            0,
        ),
        Some(270)
    );
    assert_eq!(
        bounded_direct_image_cardinal_content_rotation_degrees(
            &[((12u32) << 16, false, false)],
            0,
        ),
        None
    );
    assert_eq!(
        bounded_direct_image_cardinal_content_rotation_degrees(
            &[((90u32) << 16, false, false)],
            FSP_FLIP_H,
        ),
        None
    );
}

#[test]
fn direct_image_rotation_keeps_exact_cardinal_angles_fail_closed() {
    for rotation_op in [
        90u32 << 16,
        ((-90i32) << 16) as u32,
        180u32 << 16,
        ((-180i32) << 16) as u32,
    ] {
        assert_eq!(
            bounded_direct_image_transform(&[(rotation_op, false, false)], 0, test_bounds()),
            BoundedDirectImageTransform::Unsupported
        );
    }
}

#[test]
fn direct_image_rotation_keeps_fractional_16_16_angle_nonidentity() {
    let half_degree = 32_768u32;
    let transform =
        bounded_direct_image_transform(&[(half_degree, false, false)], 0, test_bounds());
    let BoundedDirectImageTransform::Applied(transform) = transform else {
        panic!("fractional scalar rotation must be admitted");
    };
    assert_ne!(transform, Affine2D::identity());
    assert_ne!(transform.b.as_str(), "0");
    assert_ne!(transform.c.as_str(), "0");
}

#[test]
fn direct_image_rotation_rejects_flip_duplicate_and_complex_states() {
    assert_eq!(
        bounded_direct_image_transform(&[(1, false, false)], FSP_FLIP_H, test_bounds()),
        BoundedDirectImageTransform::Unsupported
    );
    assert_eq!(
        bounded_direct_image_transform(
            &[(1, false, false), (2, false, false)],
            0,
            test_bounds(),
        ),
        BoundedDirectImageTransform::Unsupported
    );
    assert_eq!(
        bounded_direct_image_transform(&[(1, false, true)], 0, test_bounds()),
        BoundedDirectImageTransform::Unsupported
    );
}

#[test]
#[ignore = "requires CHAPTERA_SOURCE_STACK_FIXTURE and CHAPTERA_SOURCE_STACK_EXPECTED_PAGE_COUNTS"]
fn exact_public_source_stack_order_covers_materialized_grouped_nodes() {
    let fixture = std::env::var_os("CHAPTERA_SOURCE_STACK_FIXTURE")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_SOURCE_STACK_FIXTURE");
    let expected = std::env::var("CHAPTERA_SOURCE_STACK_EXPECTED_PAGE_COUNTS")
        .expect("CHAPTERA_SOURCE_STACK_EXPECTED_PAGE_COUNTS")
        .split(',')
        .map(|value| value.parse::<usize>().expect("page count"))
        .collect::<Vec<_>>();
    let bytes = std::fs::read(fixture).expect("read exact public PUB");
    let source_hash: Sha256Digest = std::env::var("CHAPTERA_SOURCE_STACK_SHA256")
        .expect("CHAPTERA_SOURCE_STACK_SHA256")
        .parse()
        .expect("valid source SHA-256");
    let build = build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash)
        .expect("build mature source graph");

    let page_ordinals = build
        .graph
        .document
        .pages
        .iter()
        .enumerate()
        .map(|(ordinal, page_id)| (*page_id, ordinal))
        .collect::<BTreeMap<_, _>>();
    let mut actual = build
        .source_page_paint_orders
        .iter()
        .filter_map(|order| {
            page_ordinals
                .get(&order.page_id)
                .copied()
                .map(|ordinal| (ordinal, order.node_ids.len()))
        })
        .collect::<Vec<_>>();
    actual.sort_unstable();
    let actual_counts = actual.iter().map(|(_, count)| *count).collect::<Vec<_>>();

    assert_eq!(
        actual_counts, expected,
        "source stack order must cover all already-materialized page visuals in exact serialized OfficeArt order"
    );
    let unique = build
        .source_page_paint_orders
        .iter()
        .flat_map(|order| order.node_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        unique.len(),
        expected.iter().sum::<usize>(),
        "one materialized node may occupy exactly one source stack slot"
    );
}

fn source_hash() -> Sha256Digest {
    "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
        .parse()
        .expect("known SampleNewsletter SHA-256")
}

#[test]
#[ignore = "requires CHAPTERA_SCRIPT_FONT_MAP_FIXTURE and CHAPTERA_SCRIPT_FONT_MAP_OUT"]
fn exact_fonts_pub_preserves_script_font_map_without_scalar_promotion() {
    let fixture = std::env::var_os("CHAPTERA_SCRIPT_FONT_MAP_FIXTURE")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_SCRIPT_FONT_MAP_FIXTURE");
    let output_dir = std::env::var_os("CHAPTERA_SCRIPT_FONT_MAP_OUT")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_SCRIPT_FONT_MAP_OUT");
    std::fs::create_dir_all(&output_dir).expect("create script-font-map output");

    let bytes = std::fs::read(&fixture).expect("read exact fonts.pub");
    let exact_source_hash: Sha256Digest =
        "8d50872a7d8ee6130b889efbe99275ee333747bc7777f3c5256a05f2c6d32048"
            .parse()
            .expect("known fonts.pub SHA-256");
    let build =
        build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), exact_source_hash)
            .expect("build exact fonts.pub source graph");

    assert!(
        build.typography_runs.is_empty(),
        "this preservation slice must not silently promote ScriptFonts into the legacy scalar typography path"
    );
    assert!(
        !build.script_font_maps.is_empty(),
        "exact fonts.pub must preserve at least one source script-font map"
    );

    let mut resolved_entries = 0_usize;
    let mut unresolved_entries = 0_usize;
    let mut invalid_entries = 0_usize;
    let mut times_new_roman_slots = Vec::new();
    let mut map_receipts = Vec::new();

    for map in &build.script_font_maps {
        let mut seen_slots = BTreeSet::new();
        let entries = map
            .entries
            .iter()
            .map(|entry| {
                assert!(
                    seen_slots.insert(entry.script_slot),
                    "one source ScriptFonts map must not repeat a raw script slot"
                );
                match entry.disposition {
                    PubScriptFontEntryDisposition::Resolved => resolved_entries += 1,
                    PubScriptFontEntryDisposition::UnresolvedSentinel => {
                        unresolved_entries += 1
                    }
                    PubScriptFontEntryDisposition::InvalidFontOrdinal => invalid_entries += 1,
                }
                if entry.source_font_index == 0
                    && entry.source_font_name.as_deref() == Some("Times New Roman")
                {
                    times_new_roman_slots.push(entry.script_slot);
                }
                serde_json::json!({
                    "script_slot": entry.script_slot,
                    "source_font_index": entry.source_font_index,
                    "source_font_name": entry.source_font_name,
                    "disposition": entry.disposition,
                })
            })
            .collect::<Vec<_>>();
        map_receipts.push(serde_json::json!({
            "story_id": map.story_id,
            "story_utf16_range": [map.story_utf16_start, map.story_utf16_end],
            "story_scalar_range": [map.story_scalar_start, map.story_scalar_end],
            "entries": entries,
        }));
    }

    times_new_roman_slots.sort_unstable();
    times_new_roman_slots.dedup();
    assert!(
        !times_new_roman_slots.is_empty(),
        "exact fonts.pub must preserve at least one ScriptFonts slot resolving to FONT[0] Times New Roman"
    );
    assert_eq!(
        invalid_entries, 0,
        "exact positive must not invent out-of-range FONT ordinals"
    );

    let receipt = serde_json::json!({
        "schema": "chaptera.viewer-script-font-map-exact-fixture.v1",
        "source_pub_sha256": exact_source_hash,
        "legacy_scalar_typography_run_count": build.typography_runs.len(),
        "script_font_map_count": build.script_font_maps.len(),
        "resolved_entry_count": resolved_entries,
        "unresolved_entry_count": unresolved_entries,
        "invalid_entry_count": invalid_entries,
        "times_new_roman_font_ordinal": 0,
        "times_new_roman_script_slots": times_new_roman_slots,
        "maps": map_receipts,
    });
    std::fs::write(
        output_dir.join("viewer-script-font-map-fonts-pub.json"),
        serde_json::to_vec_pretty(&receipt).expect("serialize script-font-map receipt"),
    )
    .expect("write script-font-map receipt");

    println!(
        "script-font exact fixture: maps={} resolved={} unresolved={} invalid={} times_new_roman_slots={:?}",
        build.script_font_maps.len(),
        resolved_entries,
        unresolved_entries,
        invalid_entries,
        receipt["times_new_roman_script_slots"],
    );
}

#[test]
fn typography_boolean_projection_preserves_xor_operands_and_effective_value() {
    let source = QuillEffectiveBoolean {
        local_toggle: true,
        local_toggle_source: Some(RawSpan {
            stream: StreamPath("/Quill/QuillSub/CONTENTS".into()),
            offset: 100,
            len: 2,
        }),
        inherited_value: true,
        inherited_style_source: RawSpan {
            stream: StreamPath("/Quill/QuillSub/CONTENTS".into()),
            offset: 200,
            len: 12,
        },
        effective_value: false,
    };

    assert_eq!(
        project_effective_boolean_v1(&source),
        PubTypographyBooleanV1 {
            local_toggle: true,
            inherited_value: true,
            effective_value: false,
        }
    );
}

#[test]
fn typography_utf16_to_scalar_range_is_surrogate_safe() {
    let text = "A😀B";
    assert_eq!(utf16_range_to_scalar_range(text, 1, 3), Some((1, 2)));
    assert_eq!(utf16_range_to_scalar_range(text, 0, 4), Some((0, 3)));
    assert_eq!(utf16_range_to_scalar_range(text, 1, 2), None);
    assert_eq!(utf16_range_to_scalar_range(text, 2, 3), None);
    assert_eq!(utf16_range_to_scalar_range(text, 3, 1), None);
}

fn test_page_id(seed: u8) -> PageId {
    PageId::from_canonical(CanonicalId::from_bytes([seed; 16]))
}

fn test_node_id(seed: u8) -> NodeId {
    NodeId::from_canonical(CanonicalId::from_bytes([seed; 16]))
}

#[test]
fn grouped_carrier_participants_expand_at_one_source_order_slot_and_duplicate_fails_closed() {
    let page_id = test_page_id(10);
    let direct_before = test_node_id(11);
    let grouped_first = test_node_id(12);
    let grouped_second = test_node_id(13);
    let carrier_seq = 300_u32;

    let grouped_by_carrier = BTreeMap::from([(
        carrier_seq,
        (
            page_id,
            vec![(4_usize, grouped_first), (5_usize, grouped_second)],
        ),
    )]);
    let mut seen_seq = BTreeSet::from([299_u32]);
    let mut rejected = BTreeSet::new();
    let mut ordered = BTreeMap::from([(page_id, vec![direct_before])]);

    assert!(source_paint_order::append_grouped_carrier_participants(
        carrier_seq,
        &grouped_by_carrier,
        &mut seen_seq,
        &mut rejected,
        &mut ordered,
    ));
    assert_eq!(
        ordered.get(&page_id),
        Some(&vec![direct_before, grouped_first, grouped_second])
    );
    assert!(rejected.is_empty());

    assert!(source_paint_order::append_grouped_carrier_participants(
        carrier_seq,
        &grouped_by_carrier,
        &mut seen_seq,
        &mut rejected,
        &mut ordered,
    ));
    assert!(rejected.contains(&page_id));
    assert_eq!(
        ordered.get(&page_id),
        Some(&vec![direct_before, grouped_first, grouped_second]),
        "duplicate carrier must not duplicate descendants"
    );
}

#[test]
fn scenario_page_observation_uses_unanimous_pgid_order() {
    let p0 = test_page_id(1);
    let p1 = test_page_id(2);
    let p2 = test_page_id(3);
    let pgids = vec![vec![(1, 0), (1, 1), (1, 2)], vec![(1, 0), (1, 1), (1, 2)]];
    let pages_by_oid = BTreeMap::from([
        ((1, 0), vec![p0]),
        ((1, 1), vec![p1]),
        ((1, 2), vec![p2]),
        ((2, 0), vec![test_page_id(9)]),
    ]);

    assert_eq!(
        resolve_scenario_page_ids_from_evidence(&pgids, &pages_by_oid).unwrap(),
        vec![p0, p1, p2]
    );
}

#[test]
fn effective_page_projection_never_drops_raw_page_missing_from_scenario_order() {
    let p0 = test_page_id(1);
    let p1 = test_page_id(2);
    let newly_created_visible_page = test_page_id(3);
    let effective =
        build_effective_page_projection(&[p0, p1, newly_created_visible_page], vec![p0, p1], 2);

    assert_eq!(
        effective.authority,
        PubEffectivePageProjectionAuthority::RawDocumentPageList
    );
    assert_eq!(
        effective.page_ids,
        vec![p0, p1, newly_created_visible_page],
        "scenario Pgid evidence must never suppress a raw page; native Publisher can create visible pages without OplControlling/Pgid"
    );
    assert_eq!(effective.observed_scenario_page_ids, vec![p0, p1]);
}

#[test]
fn scenario_page_observation_rejects_disagreeing_lists() {
    let pgids = vec![vec![(1, 0), (1, 1)], vec![(1, 1), (1, 0)]];
    let pages_by_oid = BTreeMap::from([
        ((1, 0), vec![test_page_id(1)]),
        ((1, 1), vec![test_page_id(2)]),
    ]);

    assert_eq!(
        resolve_scenario_page_ids_from_evidence(&pgids, &pages_by_oid).unwrap_err(),
        "controlling_page_lists_disagree"
    );
}

#[test]
fn scenario_page_observation_rejects_ambiguous_page_oid() {
    let pgids = vec![vec![(1, 0)]];
    let pages_by_oid = BTreeMap::from([((1, 0), vec![test_page_id(1), test_page_id(2)])]);

    assert!(
        resolve_scenario_page_ids_from_evidence(&pgids, &pages_by_oid)
            .unwrap_err()
            .starts_with("pgid_is_ambiguous:")
    );
}

fn crop_test_span(offset: u64, len: u64) -> RawSpan {
    RawSpan {
        stream: StreamPath("Escher/EscherStm".to_owned()),
        offset,
        len,
    }
}

fn crop_test_property(property_id: u16, op: u32) -> pub_escher::Fopte {
    pub_escher::Fopte {
        opid: property_id,
        op,
        source: crop_test_span(0, 6),
        complex_source: None,
        complex_data: None,
    }
}

fn crop_test_shape(properties: Vec<pub_escher::Fopte>) -> pub_escher::SpContainerObservation {
    pub_escher::SpContainerObservation {
        source: crop_test_span(0, 0),
        parent_group_shape_source: None,
        fspgr: None,
        fsp: None,
        fopts: vec![pub_escher::FoptObservation {
            rec_type: 0xF00B,
            source: crop_test_span(0, 0),
            properties,
        }],
        client_anchor: None,
        client_data: None,
        client_textbox: None,
        child_anchor: None,
        unknown_children: Vec::new(),
    }
}

#[test]
fn bounded_image_crop_preserves_unique_raw_scalars_and_marks_ambiguity() {
    let shape = crop_test_shape(vec![
        crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_TOP, 28_954),
        crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_BOTTOM, 21_446),
    ]);
    let crop = bounded_officeart_image_crop(&shape).expect("explicit crop");
    assert_eq!(crop.top_raw, Some(28_954));
    assert_eq!(crop.bottom_raw, Some(21_446));
    assert_eq!(crop.left_raw, None);
    assert_eq!(crop.right_raw, None);
    assert!(!crop.ambiguous);

    let conflicting = crop_test_shape(vec![
        crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_LEFT, 1),
        crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_LEFT, 2),
    ]);
    let crop = bounded_officeart_image_crop(&conflicting).expect("conflicting crop");
    assert_eq!(crop.left_raw, None);
    assert!(crop.ambiguous);

    let mut bid = crop_test_property(OFFICE_ART_PROPERTY_CROP_FROM_RIGHT, 7);
    bid.opid |= 0x4000;
    let crop = bounded_officeart_image_crop(&crop_test_shape(vec![bid]))
        .expect("fBid crop property remains visible but ambiguous");
    assert_eq!(crop.right_raw, None);
    assert!(crop.ambiguous);
}

#[test]
fn grouped_projection_maps_full_fspgr_extent_exactly() {
    let source = [109_743_916, 106_908_792, 113_353_061, 109_780_257];
    let target = [-837_598, -3_276_408, 3_167_861, -89_633];

    assert_eq!(project_rect_trunc(source, source, target).unwrap(), target);
}

#[test]
fn grouped_projection_matches_obs_017_child_298_with_truncation_toward_zero() {
    let group_coords = [109_743_916, 106_908_792, 113_353_061, 109_780_257];
    let group_absolute = [-837_598, -3_276_408, 3_167_861, -89_633];
    let child_anchor = [111_348_265, 107_124_104, 112_981_404, 108_383_122];

    assert_eq!(
        project_rect_trunc(child_anchor, group_coords, group_absolute).unwrap(),
        [942_921, -3_037_454, 2_755_392, -1_640_185]
    );
}

#[test]
fn page_extent_consensus_accepts_one_extent() {
    assert_eq!(
        require_consensus_page_extent(&[(7_560_000, 10_692_000)]).unwrap(),
        (7_560_000, 10_692_000)
    );
}

#[test]
fn page_extent_consensus_accepts_equivalent_duplicates() {
    assert_eq!(
        require_consensus_page_extent(&[
            (7_772_400, 10_058_400),
            (7_772_400, 10_058_400),
            (7_772_400, 10_058_400),
        ])
        .unwrap(),
        (7_772_400, 10_058_400)
    );
}

#[test]
fn page_extent_consensus_rejects_conflicts() {
    let error =
        require_consensus_page_extent(&[(7_772_400, 10_058_400), (7_560_000, 10_692_000)])
            .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("conflicting Margins/OplMg page extents")
    );
}

#[test]
fn page_extent_consensus_rejects_zero_dimension() {
    let error = require_consensus_page_extent(&[(7_772_400, 0)]).unwrap_err();
    assert!(error.to_string().contains("must be positive"));
}

#[test]
fn direct_officeart_rgb_accepts_only_unflagged_colorref() {
    assert_eq!(direct_officeart_rgb(0x0000_00FF), Some([0xFF, 0x00, 0x00]));
    assert_eq!(direct_officeart_rgb(0x0000_FF00), Some([0x00, 0xFF, 0x00]));
    assert_eq!(direct_officeart_rgb(0x00FF_0000), Some([0x00, 0x00, 0xFF]));
    assert_eq!(direct_officeart_rgb(0x0800_0007), None);
}

#[test]
fn officeart_scheme_index_resolves_only_in_range_non_dummy_slots() {
    let scheme = MatureColorScheme {
        source: crop_test_span(100, 40),
        declared_count: 3,
        declared_count_source: crop_test_span(106, 4),
        slots: vec![
            pub_contents::MatureColorSchemeSlot {
                ordinal: 0,
                rgb: Some([0x10, 0x20, 0x30]),
                source: crop_test_span(110, 12),
                rgb_source: Some(crop_test_span(118, 4)),
            },
            pub_contents::MatureColorSchemeSlot {
                ordinal: 1,
                rgb: Some([0, 0, 0]),
                source: crop_test_span(122, 2),
                rgb_source: None,
            },
            pub_contents::MatureColorSchemeSlot {
                ordinal: 2,
                rgb: Some([0xAA, 0xBB, 0xCC]),
                source: crop_test_span(124, 12),
                rgb_source: Some(crop_test_span(132, 4)),
            },
        ],
        name: Some("fixture".into()),
        name_source: Some(crop_test_span(136, 14)),
    };

    assert_eq!(
        bounded_officeart_rgb(0x0800_0000, Some(&scheme)),
        Some([0x10, 0x20, 0x30])
    );
    assert_eq!(
        bounded_officeart_rgb(0x0800_0001, Some(&scheme)),
        Some([0, 0, 0])
    );
    assert_eq!(
        bounded_officeart_rgb(0x0800_0002, Some(&scheme)),
        Some([0xAA, 0xBB, 0xCC])
    );
    assert_eq!(bounded_officeart_rgb(0x0800_0003, Some(&scheme)), None);
    assert_eq!(bounded_officeart_rgb(0x0800_0000, None), None);
    assert_eq!(
        bounded_officeart_rgb(0x0000_00FF, Some(&scheme)),
        Some([0xFF, 0x00, 0x00])
    );
    assert_eq!(bounded_officeart_rgb(0x1000_0000, Some(&scheme)), None);
}

#[test]
fn quill_scheme_text_color_resolves_only_through_publication_scheme() {
    let scheme = MatureColorScheme {
        source: crop_test_span(200, 28),
        declared_count: 2,
        declared_count_source: crop_test_span(206, 4),
        slots: vec![
            pub_contents::MatureColorSchemeSlot {
                ordinal: 0,
                rgb: Some([0, 0, 0]),
                source: crop_test_span(210, 2),
                rgb_source: None,
            },
            pub_contents::MatureColorSchemeSlot {
                ordinal: 1,
                rgb: Some([0x11, 0x22, 0x33]),
                source: crop_test_span(212, 12),
                rgb_source: Some(crop_test_span(220, 4)),
            },
        ],
        name: Some("fixture".into()),
        name_source: Some(crop_test_span(224, 14)),
    };

    assert_eq!(
        bounded_quill_text_rgb(Some([0xAA, 0xBB, 0xCC]), None, None),
        Some([0xAA, 0xBB, 0xCC])
    );
    assert_eq!(
        bounded_quill_text_rgb(None, Some(0), Some(&scheme)),
        Some([0, 0, 0])
    );
    assert_eq!(
        bounded_quill_text_rgb(None, Some(1), Some(&scheme)),
        Some([0x11, 0x22, 0x33])
    );
    assert_eq!(bounded_quill_text_rgb(None, Some(2), Some(&scheme)), None);
    assert_eq!(bounded_quill_text_rgb(None, Some(0), None), None);
    assert_eq!(
        bounded_quill_text_rgb(Some([1, 2, 3]), Some(0), Some(&scheme)),
        None
    );
}

fn dgg_test_defaults(
    primary: Vec<pub_escher::Fopte>,
    tertiary: Vec<pub_escher::Fopte>,
) -> pub_escher::DggDefaultOptionsObservation {
    pub_escher::DggDefaultOptionsObservation {
        source: crop_test_span(500, 40),
        primary_options: (!primary.is_empty())
            .then(|| pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_FOPT,
                source: crop_test_span(504, 16),
                properties: primary,
            })
            .into_iter()
            .collect(),
        tertiary_options: (!tertiary.is_empty())
            .then(|| pub_escher::FoptObservation {
                rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
                source: crop_test_span(520, 16),
                properties: tertiary,
            })
            .into_iter()
            .collect(),
    }
}

#[test]
fn effective_officeart_paint_uses_normative_solid_2d_defaults() {
    let shape = crop_test_shape(Vec::new());
    let paint = resolve_bounded_effective_officeart_paint(&shape, None, None, true)
        .expect("bounded 2-D defaults");

    assert_eq!(paint.fill.solid.as_ref().map(|v| v.value), Some(true));
    assert_eq!(
        paint.fill.color_rgb.as_ref().map(|v| v.value),
        Some([0xFF, 0xFF, 0xFF])
    );
    assert_eq!(paint.fill.visible.as_ref().map(|v| v.value), Some(true));
    assert_eq!(
        paint.line.color_rgb.as_ref().map(|v| v.value),
        Some([0, 0, 0])
    );
    assert_eq!(paint.line.width_emu.as_ref().map(|v| v.value), Some(0x2535));
    assert_eq!(paint.line.visible.as_ref().map(|v| v.value), Some(true));
    assert_eq!(
        paint.fill.color_rgb.as_ref().map(|v| v.authority),
        Some(PubEffectivePaintAuthority::NormativeDefault)
    );
    assert!(paint.line.width_emu.as_ref().unwrap().source.is_none());
}

#[test]
fn effective_officeart_paint_prefers_shape_then_dgg_and_honors_use_bits() {
    let shape = crop_test_shape(vec![
        crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_FF00),
        // Value bit without fUse does not participate; DGG visibility wins.
        crop_test_property(OFFICE_ART_FILL_BOOLEANS, FILL_FILLED_BIT),
        crop_test_property(OFFICE_ART_LINE_WIDTH, 30_000),
    ]);
    let dgg = dgg_test_defaults(
        vec![
            crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_00FF),
            crop_test_property(OFFICE_ART_FILL_BOOLEANS, FILL_USE_FILLED_BIT),
            crop_test_property(OFFICE_ART_LINE_WIDTH, 20_000),
            crop_test_property(OFFICE_ART_LINE_BOOLEANS, LINE_USE_LINE_BIT | LINE_LINE_BIT),
        ],
        vec![crop_test_property(OFFICE_ART_LINE_COLOR, 0x00FF_0000)],
    );

    let paint = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
        .expect("effective paint");

    let fill_color = paint.fill.color_rgb.unwrap();
    assert_eq!(fill_color.value, [0, 0xFF, 0]);
    assert_eq!(fill_color.authority, PubEffectivePaintAuthority::ShapeLocal);

    let fill_visible = paint.fill.visible.unwrap();
    assert!(!fill_visible.value);
    assert_eq!(
        fill_visible.authority,
        PubEffectivePaintAuthority::DrawingGroupPrimary
    );

    let line_color = paint.line.color_rgb.unwrap();
    assert_eq!(line_color.value, [0, 0, 0xFF]);
    assert_eq!(
        line_color.authority,
        PubEffectivePaintAuthority::DrawingGroupTertiary
    );

    let line_width = paint.line.width_emu.unwrap();
    assert_eq!(line_width.value, 30_000);
    assert_eq!(line_width.authority, PubEffectivePaintAuthority::ShapeLocal);
    assert!(paint.line.visible.unwrap().value);
}

#[test]
fn split_shape_fill_boolean_records_resolve_only_the_ffilled_subfield() {
    let mut visible = crop_test_shape(Vec::new());
    visible.fopts = vec![
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_FOPT,
            source: crop_test_span(10, 8),
            properties: vec![crop_test_property(
                OFFICE_ART_FILL_BOOLEANS,
                FILL_USE_FILLED_BIT | FILL_FILLED_BIT,
            )],
        },
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
            source: crop_test_span(20, 8),
            properties: vec![crop_test_property(OFFICE_ART_FILL_BOOLEANS, 0x0060_0020)],
        },
    ];

    assert_eq!(explicit_officeart_paint(&visible, None).fill.visible, None);
    let effective = resolve_bounded_effective_officeart_paint(&visible, None, None, true)
        .expect("bounded 2-D paint");
    let fill = effective.fill.visible.expect("shape-local fill visibility");
    assert!(fill.value);
    assert_eq!(fill.authority, PubEffectivePaintAuthority::ShapeLocal);

    let mut hidden = visible.clone();
    hidden.fopts[0].properties[0] =
        crop_test_property(OFFICE_ART_FILL_BOOLEANS, FILL_USE_FILLED_BIT);
    assert!(
        !resolve_bounded_effective_officeart_paint(&hidden, None, None, true)
            .expect("bounded 2-D paint")
            .fill
            .visible
            .expect("shape-local hidden fill")
            .value
    );
}

#[test]
fn non_participating_fill_boolean_records_fall_through_to_dgg_or_normative_default() {
    let mut shape = crop_test_shape(Vec::new());
    shape.fopts = vec![
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_FOPT,
            source: crop_test_span(10, 8),
            properties: vec![crop_test_property(
                OFFICE_ART_FILL_BOOLEANS,
                FILL_FILLED_BIT,
            )],
        },
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
            source: crop_test_span(20, 8),
            properties: vec![crop_test_property(OFFICE_ART_FILL_BOOLEANS, 0x0060_0020)],
        },
    ];

    let normative = resolve_bounded_effective_officeart_paint(&shape, None, None, true)
        .expect("bounded 2-D paint")
        .fill
        .visible
        .expect("normative fill visibility");
    assert!(normative.value);
    assert_eq!(
        normative.authority,
        PubEffectivePaintAuthority::NormativeDefault
    );

    let dgg = dgg_test_defaults(
        vec![crop_test_property(
            OFFICE_ART_FILL_BOOLEANS,
            FILL_USE_FILLED_BIT,
        )],
        Vec::new(),
    );
    let inherited = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
        .expect("bounded 2-D paint")
        .fill
        .visible
        .expect("DGG fill visibility");
    assert!(!inherited.value);
    assert_eq!(
        inherited.authority,
        PubEffectivePaintAuthority::DrawingGroupPrimary
    );
}

#[test]
fn conflicting_ffilled_use_records_remain_fail_closed() {
    let mut shape = crop_test_shape(Vec::new());
    shape.fopts = vec![
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_FOPT,
            source: crop_test_span(10, 8),
            properties: vec![crop_test_property(
                OFFICE_ART_FILL_BOOLEANS,
                FILL_USE_FILLED_BIT | FILL_FILLED_BIT,
            )],
        },
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
            source: crop_test_span(20, 8),
            properties: vec![crop_test_property(
                OFFICE_ART_FILL_BOOLEANS,
                FILL_USE_FILLED_BIT,
            )],
        },
    ];

    assert_eq!(
        resolve_bounded_effective_officeart_paint(&shape, None, None, true)
            .expect("bounded 2-D paint")
            .fill
            .visible,
        None
    );
}

#[test]
fn split_shape_line_boolean_records_resolve_only_the_fline_subfield() {
    let mut visible = crop_test_shape(Vec::new());
    visible.fopts = vec![
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_FOPT,
            source: crop_test_span(10, 8),
            properties: vec![crop_test_property(
                OFFICE_ART_LINE_BOOLEANS,
                LINE_USE_LINE_BIT | LINE_LINE_BIT,
            )],
        },
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
            source: crop_test_span(20, 8),
            properties: vec![crop_test_property(OFFICE_ART_LINE_BOOLEANS, 0x0060_0020)],
        },
    ];

    let explicit = explicit_officeart_paint(&visible, None);
    assert_eq!(explicit.line.visible, Some(true));
    let effective = resolve_bounded_effective_officeart_paint(&visible, None, None, true)
        .expect("bounded 2-D paint");
    let line = effective.line.visible.expect("shape-local line visibility");
    assert!(line.value);
    assert_eq!(line.authority, PubEffectivePaintAuthority::ShapeLocal);

    let mut hidden = visible.clone();
    hidden.fopts[0].properties[0] =
        crop_test_property(OFFICE_ART_LINE_BOOLEANS, LINE_USE_LINE_BIT);
    hidden.fopts[1].properties[0] = crop_test_property(OFFICE_ART_LINE_BOOLEANS, 0x0040_0000);
    assert_eq!(
        explicit_officeart_paint(&hidden, None).line.visible,
        Some(false)
    );
    assert!(
        !resolve_bounded_effective_officeart_paint(&hidden, None, None, true)
            .expect("bounded 2-D paint")
            .line
            .visible
            .expect("shape-local hidden line")
            .value
    );
}

#[test]
fn conflicting_fline_use_records_remain_fail_closed() {
    let mut shape = crop_test_shape(Vec::new());
    shape.fopts = vec![
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_FOPT,
            source: crop_test_span(10, 8),
            properties: vec![crop_test_property(
                OFFICE_ART_LINE_BOOLEANS,
                LINE_USE_LINE_BIT | LINE_LINE_BIT,
            )],
        },
        pub_escher::FoptObservation {
            rec_type: pub_escher::OFFICE_ART_TERTIARY_FOPT,
            source: crop_test_span(20, 8),
            properties: vec![crop_test_property(
                OFFICE_ART_LINE_BOOLEANS,
                LINE_USE_LINE_BIT,
            )],
        },
    ];

    assert_eq!(explicit_officeart_paint(&shape, None).line.visible, None);
    assert_eq!(
        resolve_bounded_effective_officeart_paint(&shape, None, None, true)
            .expect("bounded 2-D paint")
            .line
            .visible,
        None
    );
}

#[test]
fn effective_officeart_paint_keeps_sparse_explicit_fill_on_normative_color() {
    let mut shape = crop_test_shape(vec![crop_test_property(
        OFFICE_ART_FILL_BOOLEANS,
        FILL_USE_FILLED_BIT | FILL_FILLED_BIT,
    )]);
    shape.fsp = Some(pub_escher::FspRecord {
        spid: 1,
        flags: 0,
        shape_type: 0x0002,
        source: crop_test_span(0, 8),
        trailing_source: None,
    });
    let dgg = dgg_test_defaults(
        vec![crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_00FF)],
        Vec::new(),
    );

    let paint = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
        .expect("sparse RoundRectangle fill remains bounded");

    let fill_color = paint.fill.color_rgb.expect("normative fill color");
    assert_eq!(fill_color.value, [0xFF, 0xFF, 0xFF]);
    assert_eq!(
        fill_color.authority,
        PubEffectivePaintAuthority::NormativeDefault
    );
    assert!(paint.fill.visible.expect("explicit visibility").value);

    shape.fsp.as_mut().expect("fsp").shape_type = 0x00CA;
    let paint = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
        .expect("native-proven sparse TextBox fill remains bounded");
    let fill_color = paint.fill.color_rgb.expect("normative TextBox fill color");
    assert_eq!(fill_color.value, [0xFF, 0xFF, 0xFF]);
    assert_eq!(
        fill_color.authority,
        PubEffectivePaintAuthority::NormativeDefault
    );
    assert!(
        paint
            .fill
            .visible
            .expect("explicit TextBox visibility")
            .value
    );

    shape.fsp.as_mut().expect("fsp").shape_type = 0x0001;
    let paint = resolve_bounded_effective_officeart_paint(&shape, Some(&dgg), None, true)
        .expect("other shape keeps existing DGG fallback");
    assert_eq!(
        paint.fill.color_rgb.expect("DGG fill color").authority,
        PubEffectivePaintAuthority::DrawingGroupPrimary
    );
}

#[test]
fn effective_officeart_paint_fails_closed_on_ambiguous_or_unsupported_override() {
    let ambiguous = crop_test_shape(vec![
        crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_00FF),
        crop_test_property(OFFICE_ART_FILL_COLOR, 0x0000_FF00),
    ]);
    let paint = resolve_bounded_effective_officeart_paint(&ambiguous, None, None, true)
        .expect("other normative fields remain available");
    assert_eq!(paint.fill.color_rgb, None);

    let unsupported =
        crop_test_shape(vec![crop_test_property(OFFICE_ART_FILL_COLOR, 0x1000_0000)]);
    let paint = resolve_bounded_effective_officeart_paint(&unsupported, None, None, true)
        .expect("unsupported color stays partial");
    assert_eq!(paint.fill.color_rgb, None);
}

#[test]
fn officeart_visibility_masks_match_publisher_activation_pairs() {
    let fill_disabled = 0x0010_0000;
    let fill_enabled = 0x0010_0010;
    let fill_value_without_use = 0x0000_0010;
    let line_disabled = 0x0008_0000;
    let line_enabled = 0x0008_0008;
    let line_value_without_use = 0x0000_0008;

    assert_eq!(FILL_USE_FILLED_BIT, 0x0010_0000);
    assert_eq!(FILL_FILLED_BIT, 0x0000_0010);
    assert_eq!(LINE_USE_LINE_BIT, 0x0008_0000);
    assert_eq!(LINE_LINE_BIT, 0x0000_0008);

    assert_eq!(
        (fill_disabled & FILL_USE_FILLED_BIT != 0)
            .then_some(fill_disabled & FILL_FILLED_BIT != 0),
        Some(false)
    );
    assert_eq!(
        (fill_enabled & FILL_USE_FILLED_BIT != 0)
            .then_some(fill_enabled & FILL_FILLED_BIT != 0),
        Some(true)
    );
    assert_eq!(
        (fill_value_without_use & FILL_USE_FILLED_BIT != 0)
            .then_some(fill_value_without_use & FILL_FILLED_BIT != 0),
        None
    );
    assert_eq!(
        (line_disabled & LINE_USE_LINE_BIT != 0).then_some(line_disabled & LINE_LINE_BIT != 0),
        Some(false)
    );
    assert_eq!(
        (line_enabled & LINE_USE_LINE_BIT != 0).then_some(line_enabled & LINE_LINE_BIT != 0),
        Some(true)
    );
    assert_eq!(
        (line_value_without_use & LINE_USE_LINE_BIT != 0)
            .then_some(line_value_without_use & LINE_LINE_BIT != 0),
        None
    );
}

#[test]
fn source_object_key_vocabularies_are_explicit_and_disjoint() {
    assert_eq!(contents_object_key(330), "contents/0x2c/seq/330");
    assert_eq!(quill_story_object_key(22), "quill/syid/22");
}

#[test]
fn node_and_story_identity_do_not_collapse_numeric_namespaces() {
    let hash = source_hash();
    let node = derive_pub_node_id(&hash, 22).unwrap();
    let story = derive_pub_story_id(&hash, 22).unwrap();

    assert_ne!(node.into_canonical(), story.into_canonical());
}
