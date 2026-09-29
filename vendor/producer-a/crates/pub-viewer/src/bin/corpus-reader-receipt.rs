use anyhow::{Context, Result};
use pub_viewer::{open_pub_geometry, viewer_geometry_environment_v0_1};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}


fn open_error_stage(error_text: &str) -> &'static str {
    if error_text.contains("unsupported PUB family/profile") {
        "family_dispatch"
    } else if error_text.contains("build mature-0x2C PUB source graph for Viewer") {
        "mature_source_graph"
    } else if error_text.contains("resolve PUB source graph for Viewer") {
        "mature_resolve_graph"
    } else if error_text.contains("build legacy-0x22 no-Quill PUB source graph for Viewer") {
        "legacy_noquill_source_graph"
    } else if error_text.contains("resolve legacy no-Quill PUB source graph for Viewer") {
        "legacy_noquill_resolve_graph"
    } else if error_text.contains("build legacy-0x22+Quill PUB source graph for Viewer") {
        "legacy_quill_source_graph"
    } else if error_text.contains("resolve legacy PUB source graph for Viewer") {
        "legacy_quill_resolve_graph"
    } else if error_text.contains("canonical paint bridge rejected Viewer node paint") {
        "paint_projection"
    } else if error_text.contains("Viewer text-flow resolution blocked by layout projection errors") {
        "text_flow"
    } else if error_text.contains("Viewer geometry resolution blocked by layout projection errors")
        || error_text.contains("legacy Viewer geometry resolution blocked by layout projection errors")
        || error_text.contains("legacy no-Quill Viewer geometry resolution blocked by layout projection errors")
    {
        "geometry_resolution"
    } else if error_text.contains("document references missing canonical page")
        || error_text.contains("layout projection missing document page")
    {
        "page_projection"
    } else {
        "other"
    }
}

fn open_error_domain(error_text: &str) -> &'static str {
    if error_text.contains("unsupported PUB family/profile") {
        "family"
    } else if error_text.contains("read /Contents") {
        "cfb_contents"
    } else if error_text.contains("read /Quill") {
        "cfb_quill"
    } else if error_text.contains("read /Escher") {
        "cfb_escher"
    } else if error_text.contains("Quill") {
        "quill"
    } else if error_text.contains("Escher") || error_text.contains("OfficeArt") {
        "escher"
    } else if error_text.contains("Contents") || error_text.contains("0x2C") || error_text.contains("0x22") {
        "contents"
    } else if error_text.contains("paint bridge") {
        "paint"
    } else if error_text.contains("text-flow") {
        "text_flow"
    } else if error_text.contains("layout projection") || error_text.contains("geometry resolution") {
        "layout"
    } else if error_text.contains("canonical page") || error_text.contains("document page") {
        "page_graph"
    } else {
        "other"
    }
}

fn error_chain_component_hashes(error: &anyhow::Error) -> Vec<String> {
    error
        .chain()
        .map(|component| sha256_hex(component.to_string().as_bytes()))
        .collect()
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: corpus-reader-receipt SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: corpus-reader-receipt SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("corpus-reader-receipt accepts exactly SOURCE.pub OUTPUT.json");
    }

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let source_sha256 = sha256_hex(&bytes);

    let receipt = match open_pub_geometry(&bytes, viewer_geometry_environment_v0_1()) {
        Ok(visual) => {
            let mut diagnostic_codes = visual
                .document
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect::<Vec<_>>();
            diagnostic_codes.sort();
            diagnostic_codes.dedup();

            let inherited_typography_run_count = visual
                .typography_runs
                .iter()
                .filter(|run| run.font_inherited || run.size_inherited)
                .count();
            let image_placement_count = visual
                .images
                .iter()
                .map(|image| image.node_ids.len())
                .sum::<usize>();

            json!({
                "schema": "chaptera.reader-corpus-structural-receipt.v1",
                "opened": true,
                "source_sha256": source_sha256,
                "byte_len": bytes.len(),
                "format": visual.document.source.format,
                "format_version": visual.document.source.format_version,
                "fidelity_status": visual.document.fidelity_status(),
                "viewer_page_count": visual.document.pages.len(),
                "scene_surface_count": visual.scene.surfaces.len(),
                "scene_node_count": visual.scene.nodes.len(),
                "story_count": visual.document.stories.len(),
                "story_frame_count": visual.story_frames.len(),
                "text_fragment_count": visual.text_fragments.len(),
                "typography_run_count": visual.typography_runs.len(),
                "inherited_typography_run_count": inherited_typography_run_count,
                "image_resource_count": visual.images.len(),
                "image_placement_count": image_placement_count,
                "paint_node_count": visual.paints.len(),
                "solid_fill_count": visual.paints.iter().filter(|paint| paint.solid_fill_rgb.is_some()).count(),
                "solid_line_count": visual.paints.iter().filter(|paint| paint.solid_line.is_some()).count(),
                "diagnostic_codes": diagnostic_codes,
                "visual_fidelity_proven": false,
            })
        }
        Err(error) => {
            let error_text = format!("{error:#}");
            json!({
                "schema": "chaptera.reader-corpus-structural-receipt.v1",
                "opened": false,
                "source_sha256": source_sha256,
                "byte_len": bytes.len(),
                "open_error_kind": "viewer_open_failed",
                "open_error_stage": open_error_stage(&error_text),
                "open_error_domain": open_error_domain(&error_text),
                "open_error_chain_depth": error.chain().count(),
                "open_error_chain_component_sha256": error_chain_component_hashes(&error),
                "open_error_signature_sha256": sha256_hex(error_text.as_bytes()),
                "visual_fidelity_proven": false,
            })
        }
    };

    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize Reader corpus receipt")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_stage_taxonomy_is_source_free_and_stable() {
        assert_eq!(
            open_error_stage(
                "build mature-0x2C PUB source graph for Viewer: parse mature-0x2C Contents header"
            ),
            "mature_source_graph"
        );
        assert_eq!(
            open_error_stage(
                "canonical paint bridge rejected Viewer node paint: unsupported source value"
            ),
            "paint_projection"
        );
        assert_eq!(
            open_error_stage(
                "Viewer text-flow resolution blocked by layout projection errors: story_cycle"
            ),
            "text_flow"
        );
        assert_eq!(
            open_error_stage(
                "unsupported PUB family/profile: family=Unknown, profile=unknown, route=unsupported"
            ),
            "family_dispatch"
        );
    }

    #[test]
    fn failure_domain_taxonomy_does_not_copy_error_payloads() {
        assert_eq!(
            open_error_domain("build mature-0x2C PUB source graph for Viewer: read /Quill/QuillSub/CONTENTS"),
            "cfb_quill"
        );
        assert_eq!(
            open_error_domain("Viewer geometry resolution blocked by layout projection errors: node_out_of_bounds"),
            "layout"
        );
    }
}
