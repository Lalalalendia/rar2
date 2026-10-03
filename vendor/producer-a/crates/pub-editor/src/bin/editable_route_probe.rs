use pub_editor::{EditorEditableTarget, EditorSession, Sha256Digest, open_mature_0x2c_editor};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "chaptera.migration-editable-route-probe.v1";

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn target_name(target: EditorEditableTarget) -> &'static str {
    match target {
        EditorEditableTarget::Idml => "idml",
        EditorEditableTarget::Odg => "odg",
    }
}

fn probe_target(
    session: &EditorSession,
    target: EditorEditableTarget,
    source_label: &str,
    materialize_dir: Option<&Path>,
) -> Result<Value, Box<dyn Error>> {
    let name = target_name(target);
    let preview = match session.preview_editable_export(target, source_label.to_owned()) {
        Ok(preview) => preview,
        Err(_) => {
            return Ok(json!({
                "state": "not_verified",
                "reason_code": "preview_failed",
                "materialized": false,
            }));
        }
    };

    let state = if preview.report.can_serialize {
        "available_with_declared_losses"
    } else {
        "unavailable"
    };

    let mut result = json!({
        "state": state,
        "reason_code": if preview.report.can_serialize {
            Value::Null
        } else {
            Value::String("canonical_export_blocker".to_owned())
        },
        "counts": preview.report.counts,
        "materialized": false,
    });

    if preview.report.can_serialize {
        if let Some(dir) = materialize_dir {
            fs::create_dir_all(dir)?;
            match session.export_editable(target, source_label.to_owned()) {
                Ok(export) => {
                    if export.report != preview.report {
                        result["state"] = Value::String("not_verified".to_owned());
                        result["reason_code"] =
                            Value::String("preview_materialization_report_mismatch".to_owned());
                        result["materialized"] = Value::Bool(false);
                        return Ok(result);
                    }

                    let artifact = dir.join(format!("output.{}", target.extension()));
                    fs::write(&artifact, &export.bytes)?;
                    fs::write(
                        dir.join(format!("{name}.report.json")),
                        serde_json::to_vec_pretty(&export.report)?,
                    )?;
                    fs::write(
                        dir.join(format!("{name}.report.txt")),
                        export.human_summary.as_bytes(),
                    )?;
                    result["materialized"] = Value::Bool(true);
                    result["artifact_bytes"] = json!(export.bytes.len());
                }
                Err(error) => {
                    result["state"] = Value::String("not_verified".to_owned());
                    result["reason_code"] = Value::String("materialization_failed".to_owned());
                    result["materialization_error"] = Value::String(error.to_string());
                    result["materialized"] = Value::Bool(false);
                }
            }
        }
    }

    Ok(result)
}

fn parse_args() -> Result<(PathBuf, String, Option<PathBuf>), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: editable_route_probe INPUT [--label LABEL] [--materialize-dir DIR]")?,
    );
    let mut label = input
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source.pub")
        .to_owned();
    let mut materialize_dir = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--label" => {
                label = args.next().ok_or("--label requires a value")?;
            }
            "--materialize-dir" => {
                materialize_dir = Some(PathBuf::from(
                    args.next().ok_or("--materialize-dir requires a value")?,
                ));
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    Ok((input, label, materialize_dir))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (input, label, materialize_dir) = parse_args()?;
    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);

    let session = match open_mature_0x2c_editor(&bytes, hash) {
        Ok(session) => session,
        Err(_) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "schema": SCHEMA,
                    "source_label": label,
                    "source_sha256": hash.to_string(),
                    "source_bytes": bytes.len(),
                    "open_state": "not_admitted",
                    "reason_code": "editor_source_unsupported",
                    "targets": {
                        "idml": {
                            "state": "not_verified",
                            "reason_code": "source_not_admitted",
                            "materialized": false
                        },
                        "odg": {
                            "state": "not_verified",
                            "reason_code": "source_not_admitted",
                            "materialized": false
                        }
                    }
                }))?
            );
            return Ok(());
        }
    };

    let idml = probe_target(
        &session,
        EditorEditableTarget::Idml,
        &label,
        materialize_dir.as_deref(),
    )?;
    let odg = probe_target(
        &session,
        EditorEditableTarget::Odg,
        &label,
        materialize_dir.as_deref(),
    )?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": SCHEMA,
            "source_label": label,
            "source_sha256": hash.to_string(),
            "source_bytes": bytes.len(),
            "open_state": "admitted",
            "reason_code": Value::Null,
            "source_image_count": session.source_image_count(),
            "targets": {
                "idml": idml,
                "odg": odg
            }
        }))?
    );
    Ok(())
}
