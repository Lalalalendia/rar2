use super::*;

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest::from_bytes([byte; 32])
}

fn replace(before_asset: Option<Sha256Digest>, after_asset: Sha256Digest) -> EditOperation {
    EditOperation::ReplaceImage {
        node_id: serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
            .expect("canonical NodeId"),
        before_asset,
        after_asset,
    }
}

#[test]
fn operation_asset_refs_are_exact_and_deterministic() {
    let a = digest(0x11);
    let b = digest(0x22);

    assert_eq!(replace(None, a).durable_editor_asset_refs_v1(), vec![a]);
    assert_eq!(
        replace(Some(a), b).durable_editor_asset_refs_v1(),
        vec![a, b]
    );

    let refs = required_editor_asset_refs_v1(&[
        replace(None, a),
        replace(Some(a), b),
        replace(Some(b), a),
    ])
    .into_iter()
    .collect::<Vec<_>>();
    assert_eq!(refs, vec![a, b]);
}

#[test]
fn current_image_resources_replace_source_bytes_without_fallback() {
    let node_id: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
        .expect("canonical NodeId");
    let source_resource: ResourceId =
        serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
            .expect("canonical ResourceId");
    let replacement_sha = digest(0x55);
    let source_assets = BTreeMap::from([(
        source_resource,
        EditorSourceImageAsset {
            mime: "image/png".into(),
            bytes: vec![1, 2, 3],
        },
    )]);
    let source_nodes = BTreeMap::from([(node_id, source_resource)]);
    let replacement_assets = BTreeMap::from([(
        replacement_sha,
        EditorReplacementAsset {
            sha256: replacement_sha,
            mime: "image/jpeg".into(),
            bytes: vec![9, 8, 7, 6],
        },
    )]);
    let replacements = BTreeMap::from([(node_id, replacement_sha)]);

    let resources = current_image_resources_v1(
        &source_assets,
        &source_nodes,
        &replacement_assets,
        &replacements,
    )
    .expect("current image resources");

    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].node_ids, vec![node_id]);
    assert_eq!(resources[0].mime, "image/jpeg");
    assert_eq!(resources[0].bytes, vec![9, 8, 7, 6]);
    assert_ne!(resources[0].resource_id, source_resource);
}

#[test]
fn current_image_resources_group_shared_source_and_replacement_assets() {
    let node_a: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
        .expect("canonical NodeId");
    let node_b: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000002\"")
        .expect("canonical NodeId");
    let node_c: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000003\"")
        .expect("canonical NodeId");
    let node_d: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000004\"")
        .expect("canonical NodeId");
    let source_resource: ResourceId =
        serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
            .expect("canonical ResourceId");
    let replacement_sha = digest(0x66);

    let resources = current_image_resources_v1(
        &BTreeMap::from([(
            source_resource,
            EditorSourceImageAsset {
                mime: "image/png".into(),
                bytes: vec![1, 2, 3],
            },
        )]),
        &BTreeMap::from([(node_a, source_resource), (node_b, source_resource)]),
        &BTreeMap::from([(
            replacement_sha,
            EditorReplacementAsset {
                sha256: replacement_sha,
                mime: "image/jpeg".into(),
                bytes: vec![4, 5, 6],
            },
        )]),
        &BTreeMap::from([(node_c, replacement_sha), (node_d, replacement_sha)]),
    )
    .expect("current image resources");

    assert_eq!(resources.len(), 2);
    assert_eq!(resources[0].resource_id, source_resource);
    assert_eq!(resources[0].node_ids, vec![node_a, node_b]);
    let replacement_resource = replacement_asset_resource_id(replacement_sha);
    let replacement = resources
        .iter()
        .find(|resource| resource.resource_id == replacement_resource)
        .expect("replacement resource");
    assert_eq!(replacement.node_ids, vec![node_c, node_d]);
    assert_eq!(replacement.bytes, vec![4, 5, 6]);
}

#[test]
fn current_image_resources_fail_closed_when_replacement_bytes_are_missing() {
    let node_id: NodeId = serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
        .expect("canonical NodeId");
    let replacement_sha = digest(0x77);
    let error = current_image_resources_v1(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::from([(node_id, replacement_sha)]),
    )
    .expect_err("missing replacement bytes must fail");

    assert_eq!(
        error,
        EditorCurrentImageResourceError::MissingReplacementAsset {
            sha256: replacement_sha,
        }
    );
}

#[test]
fn source_text_format_base_preserves_effective_bools_without_inventing_defaults() {
    let source_hash: Sha256Digest =
        "1111111111111111111111111111111111111111111111111111111111111111"
            .parse()
            .expect("test source hash");
    let story_id = StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x51; 16]));
    let run = PubTypographyRun {
        story_id,
        story_utf16_start: 0,
        story_utf16_end: 3,
        story_scalar_start: 0,
        story_scalar_end: 3,
        source_font_index: 4,
        source_font_name: "Montserrat".to_owned(),
        text_size_emu: 304_800,
        font_inherited: true,
        size_inherited: true,
        color_rgb: Some([0x11, 0x22, 0x33]),
        color_scheme_slot: None,
        color_inherited: true,
        bold: Some(pub_reader::PubTypographyBooleanV1 {
            local_toggle: true,
            inherited_value: true,
            effective_value: false,
        }),
        italic: Some(pub_reader::PubTypographyBooleanV1 {
            local_toggle: false,
            inherited_value: true,
            effective_value: true,
        }),
    };

    let state = source_text_format_overlay_from_runs_v1(
        source_hash,
        story_id,
        "sha256:source-story",
        3,
        &[run],
    )
    .expect("complete bounded source format");
    assert_eq!(state.base_runs.len(), 1);
    let format = &state.base_runs[0].format;
    assert!(!format.bold);
    assert!(format.italic);
    assert_eq!(format.font_size_emu, 304_800);
    assert_eq!(format.text_color_rgb, "#112233");
    assert!(format.font_resource_id.starts_with("pub-source-font:"));
    assert!(state.overrides.is_empty());
}

#[test]
fn source_text_format_base_refuses_to_invent_missing_color() {
    let source_hash: Sha256Digest =
        "2222222222222222222222222222222222222222222222222222222222222222"
            .parse()
            .expect("test source hash");
    let story_id = StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x52; 16]));
    let run = PubTypographyRun {
        story_id,
        story_utf16_start: 0,
        story_utf16_end: 1,
        story_scalar_start: 0,
        story_scalar_end: 1,
        source_font_index: 0,
        source_font_name: "Arial".to_owned(),
        text_size_emu: 152_400,
        font_inherited: false,
        size_inherited: false,
        color_rgb: None,
        color_scheme_slot: None,
        color_inherited: false,
        bold: Some(pub_reader::PubTypographyBooleanV1 {
            local_toggle: false,
            inherited_value: false,
            effective_value: false,
        }),
        italic: Some(pub_reader::PubTypographyBooleanV1 {
            local_toggle: false,
            inherited_value: false,
            effective_value: false,
        }),
    };

    let error = source_text_format_overlay_from_runs_v1(
        source_hash,
        story_id,
        "sha256:source-story",
        1,
        &[run],
    )
    .expect_err("missing color must fail closed");
    assert!(matches!(
        error,
        EditorTextFormatBaseErrorV1::UnsupportedBase { .. }
    ));
    assert!(error.to_string().contains("text color"));
}

#[test]
fn consumer_proven_typography_override_is_montserrat_only() {
    let montserrat_story =
        StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x31; 16]));
    let arial_story = StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x32; 16]));
    let typography = vec![
        FullStoryTypographyV1 {
            story_id: montserrat_story,
            font_family: "Montserrat".into(),
            font_size_emu: LengthEmu::new(304_800),
        },
        FullStoryTypographyV1 {
            story_id: arial_story,
            font_family: "Arial".into(),
            font_size_emu: LengthEmu::new(152_400),
        },
    ];

    for target in [EditorEditableTarget::Idml, EditorEditableTarget::Odg] {
        let overrides = consumer_proven_typography_overrides_v1(target, &typography);
        assert_eq!(overrides.len(), 2);
        assert!(overrides.iter().all(|item| {
            item.origin == montserrat_story.into_canonical()
                && matches!(
                    item.feature.as_str(),
                    STORY_FONT_FAMILY_FEATURE | STORY_FONT_SIZE_FEATURE
                )
                && item.disposition == CapabilityLevel::Preserved
        }));
        assert!(
            overrides
                .iter()
                .all(|item| { item.origin != arial_story.into_canonical() })
        );
    }
}

#[test]
fn scoped_text_format_wire_has_distinct_kind_and_schema_floor() {
    let story_id = StoryId::from_canonical(pub_model::CanonicalId::from_bytes([0x61; 16]));
    let legacy = EditOperation::SetTextFormatProperty {
        story_id,
        start_scalar: 0,
        end_scalar: 3,
        property: FormatPropertyV1::Bold,
        value: FormatValueV1::Bool(true),
        before_state_hash: "legacy-before".to_owned(),
        after_state_hash: "legacy-after".to_owned(),
    };
    let scoped = EditOperation::SetTextFormatPropertyScopedV1 {
        story_id,
        start_scalar: 0,
        end_scalar: 3,
        property: FormatPropertyV1::Bold,
        value: FormatValueV1::Bool(true),
        before_state_hash: "sha256:scoped-before".to_owned(),
        after_state_hash: "sha256:scoped-after".to_owned(),
    };

    let legacy_json = serde_json::to_value(&legacy).expect("legacy format JSON");
    let scoped_json = serde_json::to_value(&scoped).expect("scoped format JSON");
    assert_eq!(legacy_json["kind"], "set_text_format_property");
    assert_eq!(scoped_json["kind"], "set_text_format_property_scoped_v1");
    assert!(legacy_json.get("state_domain").is_none());
    assert!(scoped_json.get("state_domain").is_none());
    assert_eq!(
        serde_json::from_value::<EditOperation>(legacy_json)
            .expect("legacy JSON remains readable"),
        legacy
    );
    assert_eq!(
        serde_json::from_value::<EditOperation>(scoped_json)
            .expect("scoped JSON is readable by v0.16"),
        scoped
    );

    assert_eq!(
        minimum_identity_project_schema_v1(&[legacy]),
        EDITOR_PROJECT_VERSION_V0_14
    );
    assert_eq!(
        minimum_identity_project_schema_v1(&[scoped]),
        EDITOR_PROJECT_VERSION_V0_16
    );
    assert_eq!(
        minimum_identity_project_schema_v1(&[]),
        EDITOR_PROJECT_VERSION_V0_12
    );
}

#[test]
fn create_line_requires_v017_schema_and_round_trips_exact_wire() {
    let operation = EditOperation::CreateLine {
        node_id: serde_json::from_str("\"01890f47-0c00-7abc-8def-0123456789ab\"")
            .expect("canonical editor UUIDv7 NodeId"),
        page_id: serde_json::from_str("\"11000000-0000-4000-8000-000000000001\"")
            .expect("canonical PageId"),
        parent_id: serde_json::from_str("\"11000000-0000-4000-8000-000000000001\"")
            .expect("canonical PageId"),
        geometry: LineGeometryV1 {
            begin: PointEmuV1 { x: 100, y: 200 },
            end: PointEmuV1 { x: 400, y: 500 },
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 { r: 4, g: 5, b: 6 },
            width_emu: 25_400,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    };

    assert_eq!(
        minimum_identity_project_schema_v1(std::slice::from_ref(&operation)),
        EDITOR_PROJECT_VERSION_V0_17
    );
    assert!(operation.durable_editor_asset_refs_v1().is_empty());

    let json = serde_json::to_value(&operation).expect("CreateLine JSON");
    assert_eq!(json["kind"], "create_line");
    assert_eq!(
        serde_json::from_value::<EditOperation>(json).expect("CreateLine JSON round-trip"),
        operation
    );
}

#[test]
fn non_asset_operations_emit_no_durable_asset_refs() {
    let operation = EditOperation::MoveNode {
        node_id: serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
            .expect("canonical NodeId"),
        before: RectEmu::new(
            LengthEmu::ZERO,
            LengthEmu::ZERO,
            LengthEmu::new(10),
            LengthEmu::new(10),
        ),
        after: RectEmu::new(
            LengthEmu::new(1),
            LengthEmu::new(2),
            LengthEmu::new(10),
            LengthEmu::new(10),
        ),
    };
    assert!(operation.durable_editor_asset_refs_v1().is_empty());
}
