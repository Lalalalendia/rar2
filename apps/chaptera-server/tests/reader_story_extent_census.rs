use std::{collections::BTreeMap, env, fs};

use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderTextLayoutDispositionV1,
    RenderTextLayoutFallbackReasonV1, build_page_render_plan_with_text_layout_v1,
};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires CHAPTERA_READER_SCENE_PROBE_PUB and CHAPTERA_READER_SCENE_PROBE_SHA256"]
fn sample_newsletter_story_extent_mismatch_census() {
    let path =
        env::var("CHAPTERA_READER_SCENE_PROBE_PUB").expect("CHAPTERA_READER_SCENE_PROBE_PUB");
    let expected_sha256 =
        env::var("CHAPTERA_READER_SCENE_PROBE_SHA256").expect("CHAPTERA_READER_SCENE_PROBE_SHA256");
    let bytes = fs::read(path).expect("read probe PUB");
    let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(actual_sha256, expected_sha256);

    let bundle =
        open_pub_bundle(&bytes, viewer_geometry_environment_v0_1()).expect("open probe PUB");
    let font = ExplicitRenderTextFontResourceV1 {
        resource_id: chaptera_desktop_fallback_font_resource::RESOURCE_ID,
        expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
        face_index: 0,
        default_font_size_emu: chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU,
        default_line_height_emu: chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
        bytes: chaptera_desktop_fallback_font_resource::bytes(),
    };

    let mut total = 0_usize;
    let mut cause_counts = BTreeMap::<&'static str, usize>::new();
    let mut story_frame_count_distribution = BTreeMap::<usize, usize>::new();
    let mut scalar_gap_distribution = BTreeMap::<i64, usize>::new();

    for page_index in 0..bundle.geometry.document.pages.len() {
        let plan = build_page_render_plan_with_text_layout_v1(&bundle.geometry, page_index, &font)
            .expect("build fallback render plan");

        for node in &plan.nodes {
            let Some(text) = node.text.as_ref() else {
                continue;
            };
            let Some(layout) = text.layout.as_ref() else {
                continue;
            };
            if layout.disposition
                != (RenderTextLayoutDispositionV1::BackendFallback {
                    reason: RenderTextLayoutFallbackReasonV1::StoryExtentMismatch,
                })
            {
                continue;
            }

            total += 1;
            let Some(story) = bundle
                .geometry
                .document
                .stories
                .iter()
                .find(|story| story.id == text.story_id)
            else {
                *cause_counts.entry("story_missing_unexpected").or_default() += 1;
                continue;
            };

            let story_len =
                u32::try_from(story.text.chars().count()).expect("bounded story length");
            let fragment_len =
                u32::try_from(text.text.chars().count()).expect("bounded fragment length");
            let frame_count = bundle
                .geometry
                .story_frames
                .iter()
                .filter(|frame| frame.story_id == text.story_id)
                .count();
            *story_frame_count_distribution
                .entry(frame_count)
                .or_default() += 1;
            *scalar_gap_distribution
                .entry(i64::from(story_len) - i64::from(text.scalar_end))
                .or_default() += 1;

            let cause = if text.scalar_start != 0 {
                if text.scalar_end > story_len {
                    "nonzero_end_beyond_story"
                } else if fragment_len != text.scalar_end.saturating_sub(text.scalar_start) {
                    "nonzero_fragment_len_mismatch"
                } else {
                    let slice_matches = story
                        .text
                        .chars()
                        .skip(usize::try_from(text.scalar_start).expect("bounded scalar start"))
                        .take(
                            usize::try_from(text.scalar_end - text.scalar_start)
                                .expect("bounded scalar extent"),
                        )
                        .eq(text.text.chars());
                    if !slice_matches {
                        "nonzero_story_slice_text_mismatch"
                    } else if frame_count > 1 {
                        "nonzero_multi_frame_exact_story_slice"
                    } else {
                        "nonzero_single_frame_exact_story_slice"
                    }
                }
            } else if text.scalar_end < story_len {
                let prefix_matches = story
                    .text
                    .chars()
                    .take(usize::try_from(text.scalar_end).expect("bounded scalar end"))
                    .eq(text.text.chars());
                if fragment_len != text.scalar_end {
                    "short_extent_fragment_len_mismatch"
                } else if !prefix_matches {
                    "short_extent_prefix_text_mismatch"
                } else if frame_count > 1 {
                    "short_extent_multi_frame_prefix"
                } else {
                    "short_extent_single_frame_prefix"
                }
            } else if text.scalar_end > story_len {
                "scalar_end_beyond_story"
            } else if text.text == story.text {
                "full_extent_equal_unexpected"
            } else {
                "full_extent_text_mismatch"
            };
            *cause_counts.entry(cause).or_default() += 1;
        }
    }

    assert_eq!(
        cause_counts.values().sum::<usize>(),
        total,
        "story extent causal partition must cover every mismatch"
    );

    println!(
        "CLOUD_READER_STORY_EXTENT_CENSUS source_sha256={} total={} causes={} frame_counts={} scalar_gaps={}",
        actual_sha256,
        total,
        serde_json::to_string(&cause_counts).expect("serialize causes"),
        serde_json::to_string(&story_frame_count_distribution).expect("serialize frame counts"),
        serde_json::to_string(&scalar_gap_distribution).expect("serialize scalar gaps"),
    );
}
