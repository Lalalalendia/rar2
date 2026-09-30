use anyhow::{Context, Result};
use pub_presentation_profile::{
    AUX4_PRESENTATION_INPUT_SCHEMA_V1, Aux4PageEvidenceV1, Aux4PresentationProfileInputV1,
    evaluate_aux4_presentation_profile_v1,
};
use pub_reader::analyze_mature_0x2c_page_roles;
use pub_viewer::{open_mature_0x2c_geometry, viewer_geometry_environment_v0_1};
use serde_json::json;
use std::{env, fs, io::Cursor, path::PathBuf};

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: page-projection-receipt SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: page-projection-receipt SOURCE.pub OUTPUT.json")?,
    );

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let visual = open_mature_0x2c_geometry(&bytes, viewer_geometry_environment_v0_1())
        .context("open PUB through current Viewer page projection")?;

    let aux4_profile = analyze_mature_0x2c_page_roles(Cursor::new(bytes.as_slice()))
        .ok()
        .map(|page_roles| {
            let scenario_evidence_list_count = page_roles
                .controlling
                .iter()
                .flat_map(|controlling| controlling.fields.iter())
                .filter(|field| field.id == 6 && !field.pgids.is_empty())
                .count();
            evaluate_aux4_presentation_profile_v1(Aux4PresentationProfileInputV1 {
                schema_version: AUX4_PRESENTATION_INPUT_SCHEMA_V1.to_owned(),
                scenario_evidence_list_count,
                pages: page_roles
                    .pages
                    .into_iter()
                    .map(|page| Aux4PageEvidenceV1 {
                        document_ordinal: page.document_ordinal,
                        contents_seq_num: page.contents_seq_num,
                        oid_dword0: page.oid_dword0,
                        oid_dword1: page.oid_dword1,
                        applied_master_seq_num: page.applied_master_seq_num,
                    })
                    .collect(),
            })
        });
    let page_projection_diagnostics = visual
        .document
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.starts_with("viewer.page_projection."))
        .map(|diagnostic| {
            json!({
                "code": diagnostic.code,
                "message": diagnostic.message,
            })
        })
        .collect::<Vec<_>>();

    let receipt = json!({
        "schema": "chaptera.viewer-page-projection-receipt.v1",
        "viewer_page_count": visual.document.pages.len(),
        "viewer_page_indices": visual.document.pages.iter().map(|page| page.index).collect::<Vec<_>>(),
        "scene_surface_count": visual.scene.surfaces.len(),
        "diagnostic_codes": visual.document.diagnostics.iter().map(|diagnostic| diagnostic.code.clone()).collect::<Vec<_>>(),
        "page_projection_diagnostics": page_projection_diagnostics,
        "aux4_profile": aux4_profile,
    });

    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize page projection receipt")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
