//! Internal source-hash-bound PUB candidate probe, not a public Save PUB action.
use anyhow::{Context, Result};
use pub_editor::{EditorProject, Sha256Digest};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::Path};

fn source_sha256(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut value = [0_u8; 32];
    value.copy_from_slice(&digest);
    Sha256Digest::from_bytes(value)
}

fn require_exact_source(bytes: &[u8], expected: Sha256Digest) -> Result<()> {
    if source_sha256(bytes) != expected {
        anyhow::bail!("native PUB candidate source SHA-256 does not match EditorProject");
    }
    Ok(())
}

fn require_unused_candidate_paths(output_path: &Path, report_path: &Path) -> Result<()> {
    if output_path == report_path || output_path.exists() || report_path.exists() {
        anyhow::bail!("native PUB candidate requires two distinct, unused output paths");
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create unused output {}", path.display()))?;
    file.write_all(bytes).context("write new output bytes")?;
    file.sync_all().context("flush new output bytes")?;
    Ok(())
}

/// Internal research/materialization CLI only. Never reports Publisher-native acceptance.
fn emit_native_pub_candidate(
    source_path: &str,
    project_path: &str,
    output_path: &str,
    report_path: &str,
) -> Result<()> {
    let output_path = Path::new(output_path);
    let report_path = Path::new(report_path);
    require_unused_candidate_paths(output_path, report_path)?;

    let source_bytes = fs::read(source_path).context("read original source PUB")?;
    let project: EditorProject =
        serde_json::from_slice(&fs::read(project_path).context("read canonical EditorProject")?)
            .context("parse canonical EditorProject")?;
    require_exact_source(&source_bytes, project.source_hash)?;

    let mut session = pub_editor::open_mature_0x2c_editor(&source_bytes, project.source_hash)
        .context("open bounded native candidate Editor")?;
    session
        .apply_project(&project)
        .context("replay complete canonical EditorProject")?;

    let candidate = match session.materialize_mature_0x2c_native_pub_candidate(&source_bytes) {
        Ok(candidate) => candidate,
        Err(error) => {
            let report = serde_json::json!({
                "protocol_version": "chaptera.native-pub-candidate.v1",
                "source_hash": project.source_hash,
                "can_materialize": false,
                "blocker_code": error.code(),
                "chaptera_reader_reopen_verified": false,
                "native_publisher_acceptance": "not_evaluated"
            });
            write_new(report_path, &serde_json::to_vec_pretty(&report)?)
                .context("write source-safe blocked candidate report")?;
            println!("{}", serde_json::to_string(&report)?);
            return Ok(());
        }
    };

    let report = serde_json::json!({
        "protocol_version": "chaptera.native-pub-candidate.v1",
        "source_hash": candidate.source_hash,
        "output_hash": candidate.output_hash,
        "source_story_id": candidate.source_story_id,
        "output_story_id": candidate.output_story_id,
        "byte_len": candidate.bytes.len(),
        "can_materialize": true,
        "blocker_code": serde_json::Value::Null,
        "chaptera_reader_reopen_verified": true,
        "native_publisher_acceptance": "not_evaluated"
    });
    write_new(output_path, &candidate.bytes).context("write unapproved native PUB candidate")?;
    if let Err(error) = write_new(report_path, &serde_json::to_vec_pretty(&report)?) {
        // Do not leave an unreceipted candidate when the second exclusive
        // creation fails (including a preexisting/racing report path).
        let _ = fs::remove_file(output_path);
        return Err(error).context("write source-safe candidate report");
    }
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

#[cfg(test)]
mod native_candidate_cli_tests {
    use super::*;

    #[test]
    fn exact_source_hash_must_match_project() {
        let expected: Sha256Digest =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                .parse()
                .expect("known SHA-256");
        require_exact_source(b"abc", expected).expect("exact source");
        assert!(require_exact_source(b"abd", expected).is_err());
    }

    fn sample3_source() -> Vec<u8> {
        let encoded = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/Sample3.pub.b64"
        ));
        let cleaned = encoded
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect::<Vec<_>>();
        assert_eq!(cleaned.len() % 4, 0);
        fn base64_value(byte: u8) -> u8 {
            match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                other => panic!("invalid base64 byte {other:#x}"),
            }
        }
        let mut output = Vec::with_capacity(cleaned.len() / 4 * 3);
        for quartet in cleaned.chunks_exact(4) {
            let a = base64_value(quartet[0]);
            let b = base64_value(quartet[1]);
            let c = if quartet[2] == b'=' {
                0
            } else {
                base64_value(quartet[2])
            };
            let d = if quartet[3] == b'=' {
                0
            } else {
                base64_value(quartet[3])
            };
            output.push((a << 2) | (b >> 4));
            if quartet[2] != b'=' {
                output.push((b << 4) | (c >> 2));
            }
            if quartet[3] != b'=' {
                output.push((c << 6) | d);
            }
        }
        output
    }

    #[test]
    fn exact_editorproject_story_edit_produces_candidate_but_noop_and_mismatch_do_not() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "chaptera-native-cli-e2e-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("private temp root");

        let source = sample3_source();
        let source_hash = source_sha256(&source);
        let source_file = root.join("source.pub");
        fs::write(&source_file, &source).expect("write private source");

        let baseline = pub_editor::open_mature_0x2c_editor(&source, source_hash)
            .expect("open pinned Sample3 in current Editor");
        let baseline_project = root.join("baseline-project.json");
        fs::write(
            &baseline_project,
            serde_json::to_vec(&baseline.project()).expect("serialize baseline"),
        )
        .expect("save baseline project");
        let blocked_output = root.join("blocked.pub");
        let blocked_report = root.join("blocked.json");
        emit_native_pub_candidate(
            source_file.to_str().expect("source path"),
            baseline_project.to_str().expect("project path"),
            blocked_output.to_str().expect("output path"),
            blocked_report.to_str().expect("report path"),
        )
        .expect("no-op must emit bounded blocked report");
        assert!(!blocked_output.exists(), "no-op must not create PUB");
        let blocked: serde_json::Value =
            serde_json::from_slice(&fs::read(&blocked_report).expect("blocked report"))
                .expect("bounded JSON");
        assert_eq!(blocked["can_materialize"], false);
        assert_eq!(blocked["blocker_code"], "editor_pub_story_mutation_count");
        assert_eq!(blocked["native_publisher_acceptance"], "not_evaluated");

        let mut edited = pub_editor::open_mature_0x2c_editor(&source, source_hash)
            .expect("reopen immutable Sample3");
        let (story_id, before) = edited
            .graph()
            .stories
            .iter()
            .find(|(_, story)| story.text.contains("345678"))
            .map(|(id, story)| (*id, story.text.clone()))
            .expect("controlled ordinary Story marker");
        edited
            .replace_story_text(story_id, before.replacen("345678", "", 1))
            .expect("bounded ordinary Story edit");
        let edited_project = root.join("edited-project.json");
        fs::write(
            &edited_project,
            serde_json::to_vec(&edited.project()).expect("serialize edited project"),
        )
        .expect("save edited project");
        let candidate_file = root.join("candidate.pub");
        let accepted_report = root.join("candidate.json");
        emit_native_pub_candidate(
            source_file.to_str().expect("source path"),
            edited_project.to_str().expect("project path"),
            candidate_file.to_str().expect("candidate path"),
            accepted_report.to_str().expect("report path"),
        )
        .expect("bounded EditorProject candidate");
        let candidate = fs::read(&candidate_file).expect("materialized PUB candidate");
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(&accepted_report).expect("report"))
                .expect("source-safe receipt");
        assert_eq!(receipt["can_materialize"], true);
        assert_eq!(receipt["chaptera_reader_reopen_verified"], true);
        assert_eq!(receipt["native_publisher_acceptance"], "not_evaluated");
        assert_eq!(
            receipt["output_hash"],
            serde_json::to_value(source_sha256(&candidate)).expect("candidate SHA")
        );
        assert_eq!(fs::read(&source_file).expect("source immutable"), source);
        assert_ne!(candidate, source, "candidate must represent changed Story");

        // A project for the original source must reject altered input bytes
        // before any candidate or receipt file is created.
        let corrupted_source = root.join("corrupted-source.pub");
        fs::write(&corrupted_source, b"not the pinned Sample3 bytes").expect("corrupted source");
        let mismatch_candidate = root.join("mismatch.pub");
        let mismatch_report = root.join("mismatch.json");
        assert!(
            emit_native_pub_candidate(
                corrupted_source.to_str().expect("corrupted path"),
                edited_project.to_str().expect("edited project path"),
                mismatch_candidate.to_str().expect("mismatch output path"),
                mismatch_report.to_str().expect("mismatch report path"),
            )
            .is_err()
        );
        assert!(!mismatch_candidate.exists());
        assert!(!mismatch_report.exists());
        fs::remove_dir_all(root).expect("cleanup temp source and candidate");
    }

    #[test]
    fn candidate_output_and_report_must_be_distinct_and_unoccupied() {
        let root = std::env::temp_dir().join(format!(
            "chaptera-candidate-cli-test-{}",
            std::process::id()
        ));
        let output = root.join("candidate.pub");
        let report = root.join("report.json");
        require_unused_candidate_paths(&output, &report).expect("distinct unused paths");
        assert!(require_unused_candidate_paths(&output, &output).is_err());
        // Atomic create_new is the final guard against replacement after
        // initial path preflight, including a preexisting output.
        fs::create_dir_all(&root).expect("private path root");
        write_new(&output, b"existing").expect("first output");
        assert!(require_unused_candidate_paths(&output, &report).is_err());
        assert!(write_new(&output, b"replacement").is_err());
        assert_eq!(fs::read(&output).expect("read existing"), b"existing");
        fs::remove_dir_all(root).expect("clean up");
    }
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let source = args.next().context("source PUB path missing")?;
    let project = args.next().context("EditorProject path missing")?;
    let output = args.next().context("candidate output path missing")?;
    let report = args.next().context("source-safe report path missing")?;
    if args.next().is_some() {
        anyhow::bail!("unexpected extra arguments");
    }
    emit_native_pub_candidate(&source, &project, &output, &report)
}
