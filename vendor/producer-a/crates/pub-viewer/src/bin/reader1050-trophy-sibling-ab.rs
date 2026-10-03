use anyhow::{Context, Result, bail};
use pub_reader::{
    PubStory65ContinuationDiagnostic, build_story65_continuation_diagnostic,
};
use pub_viewer::{open_pub_geometry, viewer_geometry_environment_v0_1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::{Path, PathBuf}};

const SCHEMA: &str = "chaptera.reader1050-trophy-sibling-ab.v1";
const TARGET_SHA: &str =
    "32b857475ae5ca8207942a40dc708d63153c9140bb06ee740d7e235944c0a027";
const CONTROL_SHA: &str =
    "7a5393159155ad6b7e1e47fa2769e05ffdb7124298470bae15faa556b3c06cd6";

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn reader_summary(bytes: &[u8]) -> (bool, Value) {
    match open_pub_geometry(bytes, viewer_geometry_environment_v0_1()) {
        Ok(visual) => {
            let mut diagnostic_codes = visual
                .document
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect::<Vec<_>>();
            diagnostic_codes.sort();
            diagnostic_codes.dedup();
            (
                true,
                json!({
                    "opened": true,
                    "format_version": visual.document.source.format_version,
                    "viewer_page_count": visual.document.pages.len(),
                    "scene_surface_count": visual.scene.surfaces.len(),
                    "scene_node_count": visual.scene.nodes.len(),
                    "story_count": visual.document.stories.len(),
                    "story_frame_count": visual.story_frames.len(),
                    "text_fragment_count": visual.text_fragments.len(),
                    "typography_run_count": visual.typography_runs.len(),
                    "image_resource_count": visual.images.len(),
                    "paint_node_count": visual.paints.len(),
                    "diagnostic_codes": diagnostic_codes,
                    "visual_fidelity_proven": false,
                }),
            )
        }
        Err(error) => {
            let error_text = format!("{error:#}");
            (
                false,
                json!({
                    "opened": false,
                    "open_error_kind": "viewer_open_failed",
                    "open_error_signature_sha256": sha256_hex(error_text.as_bytes()),
                    "visual_fidelity_proven": false,
                }),
            )
        }
    }
}

fn decision(
    target: &PubStory65ContinuationDiagnostic,
    target_opened: bool,
    control: &PubStory65ContinuationDiagnostic,
    control_opened: bool,
) -> Value {
    if target_opened {
        return json!({
            "status": "obsolete_target_now_opens",
            "typed_corruption_authorized": false,
            "next_operation": "Stop this discriminator: current Reader now opens the exact target."
        });
    }
    if !control_opened {
        return json!({
            "status": "control_no_longer_opens",
            "typed_corruption_authorized": false,
            "next_operation": "Do not interpret the A/B until the pinned sibling control is again normal-open."
        });
    }

    if target.story65.is_physical_empty() != control.story65.is_physical_empty() {
        return json!({
            "status": "story65_admission_surface_divergence",
            "typed_corruption_authorized": false,
            "next_operation": "Localize the exact Story65 structural difference; do not classify corruption from sibling disagreement alone."
        });
    }

    if control.story65_geometry_only_gate_eligible && !target.story65_geometry_only_gate_eligible {
        if target.live_story_demand.distinct_story_id_count > 0
            && control.live_story_demand.distinct_story_id_count == 0
        {
            return json!({
                "status": "post_story65_live_story_demand_divergence",
                "typed_corruption_authorized": false,
                "next_operation": "Ground whether target live SHAPE/TABLE Story demand is legitimate persisted semantics or a malformed dangling relation before changing Reader admission."
            });
        }
        if target.quill.coarse_key() != control.quill.coarse_key() {
            return json!({
                "status": "post_story65_quill_service_plane_divergence",
                "typed_corruption_authorized": false,
                "next_operation": "Run a bounded Quill service-plane A/B on the exact target/control pair; test self-consistency before any corruption promotion."
            });
        }
        return json!({
            "status": "post_story65_geometry_gate_predicate_divergence",
            "typed_corruption_authorized": false,
            "next_operation": "Localize the remaining geometry-only admission predicate on the exact pair."
        });
    }

    if target.quill.coarse_key() != control.quill.coarse_key() {
        return json!({
            "status": "post_story65_quill_service_plane_divergence",
            "typed_corruption_authorized": false,
            "next_operation": "Run a bounded Quill service-plane A/B on the exact target/control pair; test self-consistency before any corruption promotion."
        });
    }

    if target.story65_geometry_only_gate_eligible && control.story65_geometry_only_gate_eligible {
        return json!({
            "status": "post_story65_geometry_only_continuation_divergence",
            "typed_corruption_authorized": false,
            "next_operation": "Both files satisfy the same Story65/Quill geometry-only gate; localize the first later Viewer/source-graph rejection rather than widening salvage."
        });
    }

    json!({
        "status": "post_story65_structural_divergence_unlocalized",
        "typed_corruption_authorized": false,
        "next_operation": "Continue with a narrower source-safe structural A/B; the current receipt does not authorize corruption."
    })
}

fn render_markdown(payload: &Value) -> String {
    let target = &payload["target"];
    let control = &payload["control"];
    let decision = &payload["decision"];
    format!(
        "# Reader-1050 trophy sibling A/B\n\n- Target: `{}` — Reader opened **{}**\n- Control: `{}` — Reader opened **{}**\n- Target Story65: `{}`\n- Control Story65: `{}`\n- Target live Story IDs: **{}**\n- Control live Story IDs: **{}**\n- Target Quill: `{}`\n- Control Quill: `{}`\n- Target Story65 geometry-only gate: **{}**\n- Control Story65 geometry-only gate: **{}**\n- Decision: **{}**\n- Typed corruption authorized: **{}**\n\n## Next bounded operation\n\n{}\n\nEvidence boundary: exact-SHA pair only; no document text, raw stream bytes, filenames/paths or repaired PUB materialization are retained.\n",
        target["source_sha256"].as_str().unwrap_or(""),
        target["reader"]["opened"].as_bool().unwrap_or(false),
        control["source_sha256"].as_str().unwrap_or(""),
        control["reader"]["opened"].as_bool().unwrap_or(false),
        target["diagnostic"]["story65"]["state"].as_str().unwrap_or("unknown"),
        control["diagnostic"]["story65"]["state"].as_str().unwrap_or("unknown"),
        target["diagnostic"]["live_story_demand"]["distinct_story_id_count"]
            .as_u64()
            .unwrap_or(0),
        control["diagnostic"]["live_story_demand"]["distinct_story_id_count"]
            .as_u64()
            .unwrap_or(0),
        target["quill_coarse_key"].as_str().unwrap_or("unknown"),
        control["quill_coarse_key"].as_str().unwrap_or("unknown"),
        target["diagnostic"]["story65_geometry_only_gate_eligible"]
            .as_bool()
            .unwrap_or(false),
        control["diagnostic"]["story65_geometry_only_gate_eligible"]
            .as_bool()
            .unwrap_or(false),
        decision["status"].as_str().unwrap_or("unknown"),
        decision["typed_corruption_authorized"]
            .as_bool()
            .unwrap_or(false),
        decision["next_operation"].as_str().unwrap_or(""),
    )
}

fn inspect(path: &Path, expected_sha: &str) -> Result<(PubStory65ContinuationDiagnostic, bool, Value)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let actual_sha = sha256_hex(&bytes);
    if actual_sha != expected_sha {
        bail!(
            "exact-pair identity drift for {}: expected {}, got {}",
            path.display(),
            expected_sha,
            actual_sha
        );
    }
    let diagnostic = build_story65_continuation_diagnostic(&bytes)
        .context("build source-safe post-Story65 diagnostic")?;
    if diagnostic.source_sha256 != expected_sha {
        bail!("diagnostic source identity drift for {}", path.display());
    }
    let (opened, reader) = reader_summary(&bytes);
    Ok((diagnostic, opened, reader))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let target_path = PathBuf::from(
        args.next()
            .context("usage: reader1050-trophy-sibling-ab TARGET.pub CONTROL.pub OUT_DIR")?,
    );
    let control_path = PathBuf::from(
        args.next()
            .context("usage: reader1050-trophy-sibling-ab TARGET.pub CONTROL.pub OUT_DIR")?,
    );
    let out_dir = PathBuf::from(
        args.next()
            .context("usage: reader1050-trophy-sibling-ab TARGET.pub CONTROL.pub OUT_DIR")?,
    );
    if args.next().is_some() {
        bail!("reader1050-trophy-sibling-ab accepts exactly TARGET.pub CONTROL.pub OUT_DIR");
    }

    let (target_diag, target_opened, target_reader) = inspect(&target_path, TARGET_SHA)?;
    let (control_diag, control_opened, control_reader) = inspect(&control_path, CONTROL_SHA)?;
    let decision = decision(
        &target_diag,
        target_opened,
        &control_diag,
        control_opened,
    );
    let target_quill_coarse_key = target_diag.quill.coarse_key();
    let control_quill_coarse_key = control_diag.quill.coarse_key();

    let payload = json!({
        "schema": SCHEMA,
        "target": {
            "source_sha256": TARGET_SHA,
            "byte_len": target_diag.byte_len,
            "reader": target_reader,
            "diagnostic": target_diag,
            "quill_coarse_key": target_quill_coarse_key,
        },
        "control": {
            "source_sha256": CONTROL_SHA,
            "byte_len": control_diag.byte_len,
            "reader": control_reader,
            "diagnostic": control_diag,
            "quill_coarse_key": control_quill_coarse_key,
        },
        "decision": decision,
        "evidence_boundary": "exact-SHA source-safe sibling A/B only; no document text, raw stream bytes, filenames/paths or repaired PUB materialization retained"
    });

    fs::create_dir_all(&out_dir).context("create sibling A/B output directory")?;
    fs::write(
        out_dir.join("decision.json"),
        serde_json::to_vec_pretty(&payload).context("serialize sibling A/B receipt")?,
    )
    .context("write sibling A/B JSON")?;
    fs::write(out_dir.join("decision.md"), render_markdown(&payload))
        .context("write sibling A/B Markdown")?;
    println!(
        "{}",
        serde_json::to_string_pretty(&payload["decision"])
            .context("serialize sibling A/B decision")?
    );
    Ok(())
}
