use super::*;

    #[test]
    fn mixed_family_terminal_114300_needs_exact_fonts_and_fresh_unique_spacing() {
        let mut visual = fixture();
        let story_id = visual.document.stories[0].id;
        let page_id = visual.document.pages[0].id;
        let node_id = visual.scene.nodes[0].origin;
        let text = "AB\rCD";
        visual.document.stories[0].text = text.to_owned();
        visual.story_frames.push(pub_viewer::ViewerStoryFrame {
            story_id,
            frame_id: node_id,
            ordinal: 0,
            text_content_bounds: None,
            vertical_alignment: None,
        });

        let typography = vec![
            RenderTypographyRunV1 {
                scalar_start: 0, scalar_end: 3,
                source_font_name: "Family A".to_owned(),
                text_size_emu: 12 * 12_700,
                font_inherited: false, size_inherited: false,
                color_rgb: None, color_inherited: false, bold: None, italic: None,
            },
            RenderTypographyRunV1 {
                scalar_start: 3, scalar_end: 5,
                source_font_name: "Family B".to_owned(),
                text_size_emu: 18 * 12_700,
                font_inherited: false, size_inherited: false,
                color_rgb: None, color_inherited: false, bold: None, italic: None,
            },
        ];
        let fragment = render_fragment(story_id, text, typography);
        assert_eq!(effective_source_font_family_v1(&visual, &fragment), None);

        let first_bytes: &[u8] = font_test_data::AHEM;
        let second_bytes: &[u8] = font_test_data::TINOS_SUBSET;
        let first_sha = font_fingerprint_sha256(first_bytes);
        let second_sha = font_fingerprint_sha256(second_bytes);
        let mut resolve = |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| {
            match run.source_font_name.as_str() {
                "Family A" => Some(ExplicitRenderTextFontResourceV1 {
                    resource_id: "exact-a", expected_sha256: &first_sha,
                    face_index: 0, default_font_size_emu: 12 * 12_700,
                    default_line_height_emu: 30 * 12_700, bytes: first_bytes,
                }),
                "Family B" => Some(ExplicitRenderTextFontResourceV1 {
                    resource_id: "exact-b", expected_sha256: &second_sha,
                    face_index: 0, default_font_size_emu: 12 * 12_700,
                    default_line_height_emu: 30 * 12_700, bytes: second_bytes,
                }),
                _ => None,
            }
        };
        let first_extent = compatible_natural_line_height_emu_v1(
            first_bytes, 0, LengthEmu::new(12 * 12_700),
        ).map(LengthEmu::get).expect("exact first physical metric");
        let terminal_extent = compatible_natural_line_height_emu_v1(
            second_bytes, 0, LengthEmu::new(18 * 12_700),
        ).map(LengthEmu::get).expect("exact terminal physical metric");
        let expected_terminal_advance = (terminal_extent * 3 + 2) / 4;
        let frame_height = first_extent.min(30 * 12_700) + expected_terminal_advance;
        assert!(30 * 12_700 + 45 * 12_700 > frame_height,
            "ordinary baseline must overflow the selected narrow frame");
        let target = RenderTextLayoutTargetV1 {
            page_id,
            page_size: Size2D::new(LengthEmu::new(10_000_000), LengthEmu::new(10_000_000)),
            node_id,
            projected_target_frame_node_id: None,
            vertical_alignment: None,
            bounds: RectEmu::new(
                LengthEmu::ZERO, LengthEmu::ZERO,
                LengthEmu::new(10_000_000), LengthEmu::new(frame_height),
            ),
            transform: Affine2D::identity(),
        };

        // Missing spacing is a negative even though both exact fonts exist.
        assert!(resolve_mixed_family_text_layout_v1(
            &visual, target.clone(), &fragment, &mut resolve,
        ).is_none());

        visual.paragraph_line_spacings = vec![ViewerParagraphLineSpacingRun {
            story_id,
            scalar_start: 3,
            scalar_end: 5,
            line_spacing: ViewerParagraphLineSpacing::Proportional {
                point_equivalent_emu: 9 * 12_700,
            },
            source_value: Some(914_402),
            source_story_text_sha256: viewer_story_text_sha256(text),
        }];
        let layout = resolve_mixed_family_text_layout_v1(
            &visual, target.clone(), &fragment, &mut resolve,
        ).expect("one uniquely sourced mixed-family terminal line must fit");
        assert!(matches!(layout.disposition, RenderTextLayoutDispositionV1::SharedResolved { .. }));
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].text, "AB");
        assert_eq!(layout.lines[1].text, "CD");
        assert_eq!(layout.lines[1].scalar_start, 3);
        assert_eq!(layout.lines[1].scalar_end, 5);
        assert_eq!(layout.lines[1].consumed_scalar_end, 5);
        assert_eq!(layout.lines[1].line_height_emu, expected_terminal_advance);
        assert_eq!(layout.lines[1].spans[0].font_resource_id.as_deref(), Some("exact-b"));
        assert!(layout.lines[1].spans[0].shaping.is_some());

        // The source predicate is fail-closed, not a numeric/page heuristic.
        visual.paragraph_line_spacings[0].source_story_text_sha256 =
            viewer_story_text_sha256("stale");
        assert!(resolve_mixed_family_text_layout_v1(
            &visual, target.clone(), &fragment, &mut resolve,
        ).is_none());
        visual.paragraph_line_spacings[0].source_story_text_sha256 =
            viewer_story_text_sha256(text);
        visual.paragraph_line_spacings.push(visual.paragraph_line_spacings[0].clone());
        assert!(resolve_mixed_family_text_layout_v1(
            &visual, target.clone(), &fragment, &mut resolve,
        ).is_none());
        visual.paragraph_line_spacings.truncate(1);
        visual.paragraph_line_spacings[0].line_spacing =
            ViewerParagraphLineSpacing::Proportional {
                point_equivalent_emu: 12 * 12_700,
            };
        assert!(resolve_mixed_family_text_layout_v1(
            &visual, target.clone(), &fragment, &mut resolve,
        ).is_none());
        visual.paragraph_line_spacings[0].line_spacing =
            ViewerParagraphLineSpacing::Proportional {
                point_equivalent_emu: 9 * 12_700,
            };
        let mut missing_face = |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| {
            (run.source_font_name == "Family A").then_some(ExplicitRenderTextFontResourceV1 {
                resource_id: "exact-a", expected_sha256: &first_sha,
                face_index: 0, default_font_size_emu: 12 * 12_700,
                default_line_height_emu: 30 * 12_700, bytes: first_bytes,
            })
        };
        assert!(resolve_mixed_family_text_layout_v1(
            &visual, target.clone(), &fragment, &mut missing_face,
        ).is_none());

        let mut styled_fragment = fragment.clone();
        styled_fragment.typography[1].bold = Some(true);
        assert!(resolve_mixed_family_text_layout_v1(
            &visual, target, &styled_fragment, &mut resolve,
        ).is_none(), "Regular family packet cannot authorize Bold source-face retry");
    }

