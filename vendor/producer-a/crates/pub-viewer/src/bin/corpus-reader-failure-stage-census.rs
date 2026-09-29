use anyhow::{Context, Result};
use pub_model::Sha256Digest;
use pub_reader::{
    build_legacy_0x22_noquill_source_graph, build_legacy_0x22_quill_source_graph,
    build_mature_0x2c_source_graph, classify_pub_family,
    probe_mature_0x2c_quill_story_error_kind, probe_mature_0x2c_quill_story_failure_stage,
    probe_mature_0x2c_source_graph_failure_stage, resolve_pub_source_graph, PubReaderRoute,
};
use pub_viewer::{open_pub_geometry, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

#[derive(Debug, Serialize)]
struct FailureStageRow {
    source_sha256: String,
    byte_len: usize,
    family: String,
    profile: String,
    route: String,
    stage: String,
    opened: bool,
    open_error_signature_sha256: Option<String>,
}

fn diagnose(bytes: &[u8]) -> FailureStageRow {
    let classification = classify_pub_family(bytes);
    let source_sha256 = sha256_hex(bytes);
    let mut stage = "family_route".to_owned();

    let lower_ok = match classification.route {
        PubReaderRoute::Mature2c => {
            stage = "mature.source_graph".to_owned();
            match build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash(bytes)) {
                Ok(source) => {
                    stage = "mature.resolve_graph".to_owned();
                    resolve_pub_source_graph(&source.graph).is_ok()
                }
                Err(_) => {
                    let substage =
                        probe_mature_0x2c_source_graph_failure_stage(bytes, source_hash(bytes))
                            .map(|value| value.as_str())
                            .unwrap_or("unclassified");
                    stage = if substage == "quill_story_catalog" {
                        let kind = probe_mature_0x2c_quill_story_error_kind(bytes)
                            .unwrap_or("unclassified");
                        let quill_stage = probe_mature_0x2c_quill_story_failure_stage(bytes)
                            .unwrap_or("unclassified");
                        format!("mature.source_graph.{substage}.{kind}.{quill_stage}")
                    } else {
                        format!("mature.source_graph.{substage}")
                    };
                    false
                }
            }
        }
        PubReaderRoute::Legacy22Quill => {
            stage = "legacy22_quill.source_graph".to_owned();
            match build_legacy_0x22_quill_source_graph(Cursor::new(bytes), source_hash(bytes)) {
                Ok(source) => {
                    stage = "legacy22_quill.resolve_graph".to_owned();
                    match resolve_pub_source_graph(&source.graph) {
                        Ok(_) => true,
                        Err(error) => {
                            stage_error_signature_sha256 = Some(sha256_hex(format!("{error:#}").as_bytes()));
                            false
                        }
                    }
                }
                Err(error) => {
                    stage_error_signature_sha256 = Some(sha256_hex(format!("{error:#}").as_bytes()));
                    false
                }
            }
        }
        PubReaderRoute::Legacy22LowText => {
            stage = "legacy22_noquill.source_graph".to_owned();
            match build_legacy_0x22_noquill_source_graph(Cursor::new(bytes), source_hash(bytes)) {
                Ok(source) => {
                    stage = "legacy22_noquill.resolve_graph".to_owned();
                    match resolve_pub_source_graph(&source.graph) {
                        Ok(_) => true,
                        Err(error) => {
                            stage_error_signature_sha256 = Some(sha256_hex(format!("{error:#}").as_bytes()));
                            false
                        }
                    }
                }
                Err(error) => {
                    stage_error_signature_sha256 = Some(sha256_hex(format!("{error:#}").as_bytes()));
                    false
                }
            }
        }
        PubReaderRoute::Unsupported => false,
    };

    if classification.route == PubReaderRoute::Unsupported {
        return FailureStageRow {
            source_sha256,
            byte_len: bytes.len(),
            family: format!("{:?}", classification.family),
            profile: classification.profile.as_str().to_owned(),
            route: classification.route.as_str().to_owned(),
            stage: "family.unsupported".to_owned(),
            opened: false,
            open_error_signature_sha256: None,
        };
    }

    if !lower_ok {
        return FailureStageRow {
            source_sha256,
            byte_len: bytes.len(),
            family: format!("{:?}", classification.family),
            profile: classification.profile.as_str().to_owned(),
            route: classification.route.as_str().to_owned(),
            stage,
            opened: false,
            open_error_signature_sha256: stage_error_signature_sha256,
        };
    }

    match open_pub_geometry(bytes, viewer_geometry_environment_v0_1()) {
        Ok(_) => FailureStageRow {
            source_sha256,
            byte_len: bytes.len(),
            family: format!("{:?}", classification.family),
            profile: classification.profile.as_str().to_owned(),
            route: classification.route.as_str().to_owned(),
            stage: "opened".to_owned(),
            opened: true,
            open_error_signature_sha256: None,
        },
        Err(error) => {
            let error_text = format!("{error:#}");
            FailureStageRow {
                source_sha256,
                byte_len: bytes.len(),
                family: format!("{:?}", classification.family),
                profile: classification.profile.as_str().to_owned(),
                route: classification.route.as_str().to_owned(),
                stage: "post_resolve.viewer_open".to_owned(),
                opened: false,
                open_error_signature_sha256: Some(sha256_hex(error_text.as_bytes())),
            }
        }
    }
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = fs::read_dir(root)
        .with_context(|| format!("read corpus dir {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    out.sort();
    Ok(out)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: corpus-reader-failure-stage-census CORPUS_DIR OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: corpus-reader-failure-stage-census CORPUS_DIR OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("corpus-reader-failure-stage-census accepts exactly CORPUS_DIR OUTPUT.json");
    }

    let paths = pub_paths(&root)?;
    let mut rows = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        rows.push(diagnose(&bytes));
    }

    let mut stage_counts = BTreeMap::<String, usize>::new();
    let mut signature_counts = BTreeMap::<String, usize>::new();
    for row in &rows {
        *stage_counts.entry(row.stage.clone()).or_default() += 1;
        if let Some(signature) = &row.open_error_signature_sha256 {
            *signature_counts.entry(signature.clone()).or_default() += 1;
        }
    }

    let failed = rows.iter().filter(|row| !row.opened).count();
    let report = serde_json::json!({
        "schema": "chaptera.reader-failure-stage-census.v1",
        "corpus_file_count": rows.len(),
        "opened_count": rows.len() - failed,
        "failed_count": failed,
        "stage_counts": stage_counts,
        "open_error_signature_counts": signature_counts,
        "rows": rows,
        "evidence_boundary": "source-free stage localization only; error text, document text, filenames, paths, raw streams and source bytes are not retained",
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report["stage_counts"])?);
    Ok(())
}
