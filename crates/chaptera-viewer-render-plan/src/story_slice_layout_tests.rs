use super::*;

fn slice_visual(first_node_id: NodeId, story_id: StoryId) -> ViewerGeometryDocument {
    let page_id = PageId::from_canonical(canonical(1));
    let second_node_id = NodeId::from_canonical(canonical(10));
    ViewerGeometryDocument {
        schema_version: "viewer.v1".into(),
        document: ViewerDocument {
            schema_version: "viewer.document.v1".into(),
            source: ViewerSource {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                source_hash: Sha256Digest::from_bytes([0x11; 32]),
                byte_len: 12,
            },
            pages: vec![ViewerPage {
                index: 1,
                id: page_id,
                width_emu: 10_000_000,
                height_emu: 10_000_000,
            }],
            stories: vec![pub_viewer::ViewerStory {
                id: story_id,
                text: "hello world".into(),
            }],
            diagnostics: Vec::new(),
        },
        scene: BoundedResolvedScene {
            environment: BoundedLayoutEnvironment {
                engine_revision: "test".into(),
                font_set_fingerprint: "fonts:test".into(),
                resource_fingerprint: "resources:test".into(),
            },
            surfaces: vec![ResolvedSurface {
                origin: page_id,
                size: Size2D::new(LengthEmu::new(10_000_000), LengthEmu::new(10_000_000)),
                bleed: None,
                margins: None,
            }],
            nodes: vec![ResolvedPhysicalNode {
                origin: first_node_id,
                parent_origin: page_id.into_canonical(),
                bounds: RectEmu::new(
                    LengthEmu::ZERO,
                    LengthEmu::ZERO,
                    LengthEmu::new(5_000_000),
                    LengthEmu::new(5_000_000),
                ),
                transform: Affine2D::identity(),
            }],
            origin_mapping: Vec::new(),
            diagnostics: Vec::new(),
        },
        paints: Vec::new(),
        story_frames: vec![
            pub_viewer::ViewerStoryFrame {
                story_id,
                frame_id: first_node_id,
                ordinal: 0,
                text_content_bounds: None,
                vertical_alignment: None,
            },
            pub_viewer::ViewerStoryFrame {
                story_id,
                frame_id: second_node_id,
                ordinal: 1,
                text_content_bounds: None,
                vertical_alignment: None,
            },
        ],
        text_fragments: Vec::new(),
        #[cfg(feature = "projected-scene-instances")]
        projected_instances: Vec::new(),
        typography_runs: Vec::new(),
        paragraph_alignments: Vec::new(),
        paragraph_line_spacings: Vec::new(),
        paragraph_flow_runs: Vec::new(),
        script_font_maps: Vec::new(),
        tables: Vec::new(),
        images: Vec::new(),
        decorative_borders: Vec::new(),
        decorative_border_resources: Vec::new(),
    }
}

fn fragment(story_id: StoryId, start: u32, end: u32, text: &str) -> RenderTextFragmentV1 {
    RenderTextFragmentV1 {
        story_id,
        scalar_start: start,
        scalar_end: end,
        text: text.to_owned(),
        line_count: 1,
        typography: Vec::new(),
        paragraph_alignments: Vec::new(),
        backend_font_resource_id: None,
        layout: None,
    }
}

#[test]
fn exact_first_frame_prefix_is_admitted_as_zero_based_slice() {
    let node_id = NodeId::from_canonical(canonical(2));
    let story_id = StoryId::from_canonical(canonical(3));
    let visual = slice_visual(node_id, story_id);
    let fragment = fragment(story_id, 0, 5, "hello");

    assert_eq!(
        exact_direct_story_slice_scalar_base_v1(
            &visual,
            visual.document.pages[0].id,
            node_id,
            None,
            &fragment,
            "hello world",
        ),
        Some(0)
    );
}

#[test]
fn modified_first_frame_prefix_stays_fail_closed() {
    let node_id = NodeId::from_canonical(canonical(2));
    let story_id = StoryId::from_canonical(canonical(3));
    let visual = slice_visual(node_id, story_id);
    let fragment = fragment(story_id, 0, 5, "hullo");

    assert_eq!(
        exact_direct_story_slice_scalar_base_v1(
            &visual,
            visual.document.pages[0].id,
            node_id,
            None,
            &fragment,
            "hello world",
        ),
        None
    );
}

#[test]
fn first_and_later_frame_scalar_bases_must_match_frame_ordinal() {
    let first_node_id = NodeId::from_canonical(canonical(2));
    let second_node_id = NodeId::from_canonical(canonical(10));
    let story_id = StoryId::from_canonical(canonical(3));
    let visual = slice_visual(first_node_id, story_id);

    let later = fragment(story_id, 6, 11, "world");
    assert_eq!(
        exact_direct_story_slice_scalar_base_v1(
            &visual,
            visual.document.pages[0].id,
            second_node_id,
            None,
            &later,
            "hello world",
        ),
        Some(6)
    );

    let wrong_first = fragment(story_id, 6, 11, "world");
    assert_eq!(
        exact_direct_story_slice_scalar_base_v1(
            &visual,
            visual.document.pages[0].id,
            first_node_id,
            None,
            &wrong_first,
            "hello world",
        ),
        None
    );

    let wrong_later = fragment(story_id, 0, 5, "hello");
    assert_eq!(
        exact_direct_story_slice_scalar_base_v1(
            &visual,
            visual.document.pages[0].id,
            second_node_id,
            None,
            &wrong_later,
            "hello world",
        ),
        None
    );
}
