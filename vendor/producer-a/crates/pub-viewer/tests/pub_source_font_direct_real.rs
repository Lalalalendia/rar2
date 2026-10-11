//! Actual Apache POI SampleNewsletter.pub: current Reader => Viewer source
//! font index provenance. No document text, proprietary font bytes or
//! substitute-font admission is emitted by this test.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    process::{Command, Stdio},
};

const SOURCE_SHA: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";

#[test]
#[ignore = "requires pinned 291840-byte Apache POI SampleNewsletter.pub"]
fn real_sample_newsletter_viewer_preserves_direct_source_quill_indices() {
    let fixture = env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .expect("source fixture environment must be explicitly configured");
    let bytes = fs::read(fixture).expect("read exact public PUB fixture");
    assert_eq!(bytes.len(), 291_840);
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(actual_sha, SOURCE_SHA);

    let viewer =
        pub_viewer::open_pub_geometry(&bytes, pub_viewer::viewer_geometry_environment_v0_1())
            .expect("current Reader can project the actual Publisher document");
    assert_eq!(viewer.document.source.byte_len, bytes.len() as u64);
    assert_eq!(
        serde_json::to_value(viewer.document.source.source_hash).expect("source hash serializable"),
        serde_json::json!(SOURCE_SHA),
    );

    let mut direct_runs = 0_usize;
    let mut rockwell_runs = 0_usize;
    let mut names = BTreeSet::new();
    let mut size_only_runs = 0_usize;
    for run in &viewer.typography_runs {
        let story = viewer
            .document
            .stories
            .iter()
            .find(|s| s.id == run.story_id)
            .expect("every typography run must bind an actual Reader Story");
        assert!(
            run.applies_to_story_text(&story.text),
            "current source typography hash must match immutable Story"
        );
        assert!(run.scalar_start < run.scalar_end);
        assert!(run.scalar_end as usize <= story.text.chars().count());

        if run.source_font_name.is_empty() {
            assert_eq!(
                run.source_font_index, None,
                "size-only run may not invent direct Quill font authority"
            );
            size_only_runs += 1;
            continue;
        }
        assert_eq!(run.source_font_name.trim(), run.source_font_name);
        assert!(
            run.source_font_index.is_some(),
            "all actual source-family typography ranges must preserve Quill index"
        );
        direct_runs += 1;
        names.insert(run.source_font_name.as_str());
        if run.source_font_name == "Rockwell Condensed" {
            assert_eq!(
                run.source_font_index,
                Some(18),
                "actual Rockwell Condensed direct range must map to Quill index 18"
            );
            rockwell_runs += 1;
        }
    }
    assert!(
        direct_runs >= 80,
        "actual document must expose ample direct ranges"
    );
    assert!(
        rockwell_runs >= 20,
        "original Rockwell source ranges must be independently proven"
    );
    assert!(names.contains("Franklin Gothic Book"));
    assert!(names.contains("Rockwell Condensed"));

    // The current real PUB must pass the *actual* source-font planner, not
    // only the DTO serde witness or an obsolete persisted Viewer fixture.
    // Stream private Viewer JSON via stdin: do not write raw Story text to
    // files or upload it as an Actions artifact.
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tools/pub_source_font_requirements_v1.py");
    assert!(script.is_file(), "source font planner must be available");
    let mut child = Command::new("python")
        .arg(script)
        .args(["--viewer", "-", "--output", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run bounded source font requirements planner");
    {
        let mut input = child.stdin.take().expect("source planner stdin");
        serde_json::to_writer(&mut input, &viewer)
            .expect("stream actual PUB Viewer without writing raw Story files");
    }
    let output = child
        .wait_with_output()
        .expect("wait for source-font planner");
    assert!(
        output.status.success(),
        "actual PUB source font requirements planner must accept direct indexes"
    );
    let plan: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse source-safe plan");
    assert_eq!(plan["document_source_sha256"], SOURCE_SHA);
    assert_eq!(
        plan["direct_source_binding_count"].as_u64(),
        Some(direct_runs as u64)
    );
    assert_eq!(plan["physical_font_bytes_included"], false);
    assert_eq!(plan["fixed_pdf_allowed"], false);
    assert_eq!(plan["original_publisher_layout_authoritative"], false);
    let bindings = plan["direct_source_bindings"]
        .as_array()
        .expect("exact source font bindings");
    assert_eq!(bindings.len(), direct_runs);
    assert!(bindings.iter().any(|binding| {
        binding["source_family"] == "Rockwell Condensed"
            && binding["source_font_index"] == 18
            && binding["source_font_binding_id"]
                == "pub-source-font:b402e726-72b4-5d52-bb0d-07a0c13dc05d"
    }));
    assert!(
        plan.get("document").is_none(),
        "no raw Viewer Story in private plan"
    );
    println!(
        "REAL_PUB_SOURCE_FONT_PLAN_OK direct_bindings={} source_identity_only=true physical_font_files=false fixed_pdf_allowed=false",
        bindings.len()
    );
    // No original Story text or physical font paths enter the CI receipt.
    println!(
        "REAL_PUB_DIRECT_QUILL_FONT_ID_OK direct_runs={direct_runs} rockwell_runs={rockwell_runs} families={} size_only_runs={size_only_runs} source_font_index_18=true font_files_admitted=false fixed_pdf_allowed=false",
        names.len()
    );
}
