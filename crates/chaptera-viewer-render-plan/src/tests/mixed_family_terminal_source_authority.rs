    #[test]
    fn mixed_family_layout_executes_real_shaping_with_per_span_resources() {
        let mut visual = fixture();
        let story_id = visual.document.stories[0].id;
        let node_id = visual.scene.nodes[0].origin;
        let page_id = visual.document.pages[0].id;
        visual.document.stories[0].text = "ABCD".to_owned();
        visual.story_frames.push(pub_viewer::ViewerStoryFrame {
            story_id,
            frame_id: node_id,
            ordinal: 0,
            text_content_bounds: None,
            vertical_alignment: None,
        });

        let fragment = render_fragment(
            story_id,
            "ABCD",
            vec![
                RenderTypographyRunV1 {
                    scalar_start: 0,
                    scalar_end: 2,
                    source_font_name: "Family A".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
                RenderTypographyRunV1 {
                    scalar_start: 2,
                    scalar_end: 4,
                    source_font_name: "Family B".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
            ],
        );

        let first_bytes: &[u8] = font_test_data::AHEM;
        let second_bytes: &[u8] = font_test_data::TINOS_SUBSET;
        let first_sha = font_fingerprint_sha256(first_bytes);
        let second_sha = font_fingerprint_sha256(second_bytes);
        let mut resolver = |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| match run
            .source_font_name
            .as_str()
        {
            "Family A" => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "font-family-a",
                expected_sha256: &first_sha,
                face_index: 0,
                default_font_size_emu: 152_400,
                default_line_height_emu: 190_500,
                bytes: first_bytes,
            }),
            "Family B" => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "font-family-b",
                expected_sha256: &second_sha,
                face_index: 0,
                default_font_size_emu: 152_400,
                default_line_height_emu: 190_500,
                bytes: second_bytes,
            }),
            _ => None,
        };

        let layout = resolve_mixed_family_text_layout_v1(
            &visual,
            RenderTextLayoutTargetV1 {
                page_id,
                page_size: Size2D::new(LengthEmu::new(10_000_000), LengthEmu::new(10_000_000)),
                node_id,
                projected_target_frame_node_id: None,
                vertical_alignment: None,
                bounds: RectEmu::new(
                    LengthEmu::new(0),
                    LengthEmu::new(0),
                    LengthEmu::new(5_000_000),
                    LengthEmu::new(5_000_000),
                ),
                transform: Affine2D::identity(),
            },
            &fragment,
            &mut resolver,
        )
        .expect("real mixed-family shaping must produce one shared layout");

        let RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id, ..
        } = &layout.disposition
        else {
            panic!("mixed-family execution must remain shared-resolved");
        };
        assert_eq!(font_resource_id, "font-family-a");
        assert!(!layout.lines.is_empty());

        let spans = layout
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .collect::<Vec<_>>();
        assert!(spans.iter().any(|span| {
            span.font_resource_id.as_deref() == Some("font-family-a")
                && span.scalar_start == 0
                && span.scalar_end == 2
        }));
        assert!(spans.iter().any(|span| {
            span.font_resource_id.as_deref() == Some("font-family-b")
                && span.scalar_start == 2
                && span.scalar_end == 4
        }));
        assert_eq!(
            layout
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<String>(),
            "ABCD"
        );
        assert!(spans.iter().all(|span| {
            span.shaping
                .as_ref()
                .is_some_and(|shaping| shaping.units_per_em > 0 && !shaping.glyphs.is_empty())
        }));

        // #2339: whole-fragment family authority and one terminal-line
        // family authority are distinct, even with two exact font resources.
        assert_eq!(complete_scalar_source_font_family_v1(&fragment), None);
        let terminal_start = 2_u32;
        let terminal_end = 4_u32;
        let terminal_runs = fragment
            .typography
            .iter()
            .filter(|run| run.scalar_start < terminal_end && run.scalar_end > terminal_start)
            .collect::<Vec<_>>();
        assert_eq!(terminal_runs.len(), 1);
        assert!(terminal_runs[0].scalar_start <= terminal_start);
        assert!(terminal_runs[0].scalar_end >= terminal_end);
        assert_eq!(terminal_runs[0].source_font_name, "Family B");

        // One unavailable face must reject the entire per-span admission,
        // rather than silently painting the missing span with Family A.
        let mut only_first_face =
            |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| {
                (run.source_font_name == "Family A").then_some(
                    ExplicitRenderTextFontResourceV1 {
                        resource_id: "font-family-a",
                        expected_sha256: &first_sha,
                        face_index: 0,
                        default_font_size_emu: 152_400,
                        default_line_height_emu: 190_500,
                        bytes: first_bytes,
                    },
                )
            };
        assert!(admitted_mixed_family_typography_runs_v1(
            &fragment,
            &mut only_first_face,
        ).is_none());
    }

