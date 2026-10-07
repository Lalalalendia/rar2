use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::Path,
};

use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderTextFragmentV1, RenderTextLayoutDispositionV1,
    build_page_render_plan_with_text_layout_resolvers_v1, effective_source_font_family_v1,
};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const EXPECTED_SHA256: &str =
    "bbaefcbdcdd2dbe51a636ed554c14d6f5353c98e795b5f88d26f79542e5d93ba";
const ARIAL_PROBE_RESOURCE_ID: &str = "diag:arial-via-pinned-fallback-bytes";

fn pinned_font(resource_id: &'static str) -> ExplicitRenderTextFontResourceV1<'static> {
    ExplicitRenderTextFontResourceV1 {
        resource_id,
        expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
        face_index: 0,
        default_font_size_emu: chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU,
        default_line_height_emu: chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
        bytes: chaptera_desktop_fallback_font_resource::bytes(),
    }
}

fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn layout_signature(fragment: &RenderTextFragmentV1) -> Value {
    let Some(layout) = fragment.layout.as_ref() else {
        return json!({"kind":"none"});
    };
    match &layout.disposition {
        RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id,
            font_fingerprint_sha256,
            font_size_emu,
            line_height_emu,
        } => json!({
            "kind":"shared_resolved",
            "font_resource_id_sha256":sha256_text(font_resource_id),
            "font_fingerprint_sha256":font_fingerprint_sha256,
            "uses_arial_probe_resource":font_resource_id == ARIAL_PROBE_RESOURCE_ID,
            "font_size_emu":font_size_emu,
            "line_height_emu":line_height_emu,
            "line_count":layout.lines.len(),
            "nonempty_line_count":layout.lines.iter().filter(|line| !line.text.trim().is_empty()).count(),
            "span_backed_line_count":layout.lines.iter().filter(|line| !line.spans.is_empty()).count(),
        }),
        RenderTextLayoutDispositionV1::BackendFallback { reason } => json!({
            "kind":"backend_fallback",
            "reason":reason.code(),
        }),
    }
}

fn signature_kind(signature: &Value) -> String {
    match signature.get("kind").and_then(Value::as_str) {
        Some("backend_fallback") => format!(
            "fallback:{}",
            signature
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        Some(value) => value.to_owned(),
        None => "invalid".to_owned(),
    }
}

#[test]
#[ignore = "exact external fixture diagnostic"]
fn exact_094_page_layout_census() {
    let path = env::var("CHAPTERA_094_PUB")
        .expect("CHAPTERA_094_PUB must point at exact 094 fixture");
    let output = env::var("CHAPTERA_094_LAYOUT_CENSUS_OUTPUT")
        .expect("CHAPTERA_094_LAYOUT_CENSUS_OUTPUT is required");
    let bytes = fs::read(&path).expect("exact 094 fixture must be readable");
    let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(actual_sha256, EXPECTED_SHA256, "094 source identity drift");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("exact 094 must open through current Viewer");
    assert_eq!(bundle.geometry.document.pages.len(), 2, "094 page count drift");

    let fallback = pinned_font(chaptera_desktop_fallback_font_resource::RESOURCE_ID);
    let arial_probe = pinned_font(ARIAL_PROBE_RESOURCE_ID);

    let mut pages = Vec::new();
    for page_index in 0..bundle.geometry.document.pages.len() {
        let fallback_plan = build_page_render_plan_with_text_layout_resolvers_v1(
            &bundle.geometry,
            page_index,
            &fallback,
            |_| None,
            |_, _| None,
        )
        .expect("fallback-only plan must build");

        let arial_plan = build_page_render_plan_with_text_layout_resolvers_v1(
            &bundle.geometry,
            page_index,
            &fallback,
            |fragment| {
                effective_source_font_family_v1(&bundle.geometry, fragment)
                    .is_some_and(|family| family.trim().eq_ignore_ascii_case("Arial"))
                    .then_some(arial_probe)
            },
            |_, run| {
                run.source_font_name
                    .trim()
                    .eq_ignore_ascii_case("Arial")
                    .then_some(arial_probe)
            },
        )
        .expect("Arial-admission probe plan must build");

        let mut nodes = Vec::new();
        let mut fallback_counts = BTreeMap::<String, usize>::new();
        let mut arial_counts = BTreeMap::<String, usize>::new();
        let mut transitions = BTreeMap::<String, usize>::new();
        let mut full_story_fragments = 0_usize;
        let mut partial_story_fragments = 0_usize;
        let mut multi_frame_story_fragments = 0_usize;
        let mut arial_family_fragments = 0_usize;

        for (node_ordinal, fallback_node) in fallback_plan.nodes.iter().enumerate() {
            let Some(text) = fallback_node.text.as_ref() else {
                continue;
            };
            let arial_node = arial_plan
                .nodes
                .iter()
                .find(|candidate| candidate.node_id == fallback_node.node_id)
                .expect("Arial probe node topology must match fallback plan");
            let arial_text = arial_node
                .text
                .as_ref()
                .expect("Arial probe text topology must match fallback plan");

            let story = bundle
                .geometry
                .document
                .stories
                .iter()
                .find(|story| story.id == text.story_id)
                .expect("render fragment must reference current Story");
            let story_scalar_len =
                u32::try_from(story.text.chars().count()).expect("Story scalar count fits u32");
            let fragment_scalar_len = text
                .scalar_end
                .checked_sub(text.scalar_start)
                .expect("fragment scalar range is ordered");
            let fragment_full_story = text.scalar_start == 0
                && text.scalar_end == story_scalar_len
                && u32::try_from(text.text.chars().count()).ok() == Some(story_scalar_len);
            full_story_fragments += usize::from(fragment_full_story);
            partial_story_fragments += usize::from(!fragment_full_story);

            let mut story_frames = bundle
                .geometry
                .story_frames
                .iter()
                .filter(|frame| frame.story_id == text.story_id)
                .map(|frame| frame.ordinal)
                .collect::<Vec<_>>();
            story_frames.sort_unstable();
            multi_frame_story_fragments += usize::from(story_frames.len() > 1);

            let effective_family = effective_source_font_family_v1(&bundle.geometry, text);
            let effective_family_normalized = effective_family
                .as_deref()
                .map(str::trim)
                .map(str::to_lowercase);
            let source_family_is_arial = effective_family_normalized
                .as_deref()
                .is_some_and(|family| family == "arial");
            arial_family_fragments += usize::from(source_family_is_arial);

            let mut typography_cursor = text.scalar_start;
            let mut typography_complete = !text.typography.is_empty();
            let mut typography_family_hashes = BTreeSet::new();
            let mut source_sizes_emu = BTreeSet::new();
            for run in &text.typography {
                typography_complete &= run.scalar_start == typography_cursor
                    && run.scalar_end > run.scalar_start
                    && run.scalar_end <= text.scalar_end;
                typography_cursor = run.scalar_end;
                let family = run.source_font_name.trim().to_lowercase();
                if !family.is_empty() {
                    typography_family_hashes.insert(sha256_text(&family));
                }
                source_sizes_emu.insert(run.text_size_emu);
            }
            typography_complete &= typography_cursor == text.scalar_end;

            let mut alignment_counts = BTreeMap::<String, usize>::new();
            for run in &text.paragraph_alignments {
                *alignment_counts
                    .entry(format!("{:?}", run.alignment))
                    .or_default() += 1;
            }

            let fallback_signature = layout_signature(text);
            let arial_signature = layout_signature(arial_text);
            let fallback_kind = signature_kind(&fallback_signature);
            let arial_kind = signature_kind(&arial_signature);
            *fallback_counts.entry(fallback_kind.clone()).or_default() += 1;
            *arial_counts.entry(arial_kind.clone()).or_default() += 1;
            *transitions
                .entry(format!("{fallback_kind}->{arial_kind}"))
                .or_default() += 1;

            nodes.push(json!({
                "node_ordinal":node_ordinal,
                "story_id_sha256":sha256_text(&serde_json::to_string(&text.story_id).expect("serialize StoryId")),
                "story_scalar_len":story_scalar_len,
                "fragment_scalar_start":text.scalar_start,
                "fragment_scalar_end":text.scalar_end,
                "fragment_scalar_len":fragment_scalar_len,
                "fragment_full_story":fragment_full_story,
                "story_frame_count":story_frames.len(),
                "story_frame_ordinals":story_frames,
                "typography_run_count":text.typography.len(),
                "typography_complete":typography_complete,
                "typography_family_sha256":typography_family_hashes.into_iter().collect::<Vec<_>>(),
                "source_sizes_emu":source_sizes_emu.into_iter().collect::<Vec<_>>(),
                "effective_source_family_sha256":effective_family_normalized.as_deref().map(sha256_text),
                "source_family_is_arial":source_family_is_arial,
                "paragraph_alignment_counts":alignment_counts,
                "bounds_emu":{
                    "x":fallback_node.bounds.x.get(),
                    "y":fallback_node.bounds.y.get(),
                    "width":fallback_node.bounds.width.get(),
                    "height":fallback_node.bounds.height.get(),
                },
                "text_bounds_emu":fallback_node.text_bounds.map(|bounds| json!({
                    "x":bounds.x.get(),
                    "y":bounds.y.get(),
                    "width":bounds.width.get(),
                    "height":bounds.height.get(),
                })),
                "fallback_only":fallback_signature,
                "arial_resource_probe":arial_signature,
            }));
        }

        pages.push(json!({
            "page_number":page_index + 1,
            "text_node_count":nodes.len(),
            "full_story_fragment_count":full_story_fragments,
            "partial_story_fragment_count":partial_story_fragments,
            "multi_frame_story_fragment_count":multi_frame_story_fragments,
            "arial_family_fragment_count":arial_family_fragments,
            "fallback_only_counts":fallback_counts,
            "arial_resource_probe_counts":arial_counts,
            "layout_transitions":transitions,
            "nodes":nodes,
        }));
    }

    let receipt = json!({
        "schema":"chaptera.diag.094-page-layout-census.v1",
        "source_sha256":actual_sha256,
        "source_text_emitted":false,
        "source_font_names_emitted":false,
        "probe_font_bytes_sha256":chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
        "probe_note":"Arial probe reuses pinned redistribution-safe bytes only to discriminate source-family admission from earlier layout gates; it is not an Arial visual-fidelity oracle.",
        "pages":pages,
    });

    let serialized = serde_json::to_string_pretty(&receipt).expect("serialize 094 census");
    assert!(!serialized.contains("Heritage"), "source text leaked into diagnostic");
    if let Some(parent) = Path::new(&output).parent() {
        fs::create_dir_all(parent).expect("create diagnostic output directory");
    }
    fs::write(&output, format!("{serialized}\n")).expect("write diagnostic receipt");
    println!("CHAPTERA_094_LAYOUT_CENSUS={serialized}");
}
