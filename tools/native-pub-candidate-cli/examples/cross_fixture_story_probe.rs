//! Cross-fixture, offline-only Story writer discriminator.
//!
//! All inputs are pinned public Apache POI fixture blobs already stored in the
//! repository. Never exports PUB bytes or grants Publisher-native save authority.
//! The positive Sample3 control catches broken harness/Writer plumbing.
use anyhow::{Context, Result, bail, ensure};
use pub_editor::{EditorSession, Sha256Digest, open_mature_0x2c_editor};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::Path};

const ACCEPTED_CONTROL_SHA: &str =
    "b9b789f35a34e016faceb27acf50bf0621273a7d612762ecc17c56ca450fe715";
const SAMPLE3_SOURCE_SHA: &str =
    "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc";
const POSITIVE_MARKER: &str = "345678";
const MAX_STORIES_PER_FIXTURE: usize = 8;

const SAMPLE3_B64: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/Sample3.pub.b64"
));
const SAMPLE4_B64: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/Sample4.pub.b64"
));
const LINK_AT_10_B64: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/LinkAt10.pub.b64"
));
const PUB_60685_B64: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/60685.pub.b64"
));
const SAMPLE_B64: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/producer-a/crates/pub-reader/tests/fixtures/Sample.pub.b64"
));

fn sha256(bytes: &[u8]) -> Sha256Digest {
    let hash = Sha256::digest(bytes);
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&hash);
    Sha256Digest::from_bytes(digest)
}

// Strict, dependency-free decoding of checked-in fixture bytes, no external I/O.
fn decode_fixture(encoded: &str) -> Result<Vec<u8>> {
    let cleaned = encoded
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect::<Vec<_>>();
    ensure!(!cleaned.is_empty() && cleaned.len() % 4 == 0, "invalid fixture base64 framing");
    let value = |b: u8| -> Result<u8> {
        match b {
            b'A'..=b'Z' => Ok(b - b'A'),
            b'a'..=b'z' => Ok(b - b'a' + 26),
            b'0'..=b'9' => Ok(b - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => bail!("invalid fixture base64 alphabet"),
        }
    };
    let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
    for (index, chunk) in cleaned.chunks_exact(4).enumerate() {
        let last = index == cleaned.len() / 4 - 1;
        ensure!(last || (chunk[2] != b'=' && chunk[3] != b'='), "early fixture base64 padding");
        ensure!(chunk[2] != b'=' || chunk[3] == b'=', "invalid fixture base64 padding");
        let a = value(chunk[0])?;
        let b = value(chunk[1])?;
        let c = if chunk[2] == b'=' { 0 } else { value(chunk[2])? };
        let d = if chunk[3] == b'=' { 0 } else { value(chunk[3])? };
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            out.push((c << 6) | d);
        }
    }
    Ok(out)
}

// This checks an actual EditorProject replay as well as the Writer's own Reader
// reopen, and does not conflate either with Microsoft Publisher SaveAs.
fn verify_candidate(
    edited: &EditorSession,
    source: &[u8],
    expected_after: &str,
) -> Result<Value> {
    let source_hash = sha256(source);
    let project = edited.project();
    let candidate = edited
        .materialize_mature_0x2c_native_pub_candidate(source)
        .map_err(|error| anyhow::anyhow!("writer_blocked:{}", error.code()))?;
    ensure!(candidate.source_hash == source_hash, "Writer changed source identity");
    ensure!(candidate.output_hash == sha256(&candidate.bytes), "candidate hash mismatch");
    ensure!(candidate.bytes != source, "candidate did not differ from source");

    let reopened = open_mature_0x2c_editor(&candidate.bytes, candidate.output_hash)
        .context("Editor cannot reopen Writer candidate")?;
    let persisted_text = reopened
        .graph()
        .stories
        .get(&candidate.output_story_id)
        .context("Reader reopened candidate without target Story")?;
    ensure!(persisted_text.text == expected_after, "reopened Story semantic mismatch");

    let mut replay = open_mature_0x2c_editor(source, source_hash)
        .context("fresh Editor cannot reopen immutable original")?;
    replay
        .apply_project(&project)
        .context("EditorProject history replay failed")?;
    let replay_candidate = replay
        .materialize_mature_0x2c_native_pub_candidate(source)
        .map_err(|error| anyhow::anyhow!("replay_writer_blocked:{}", error.code()))?;
    ensure!(replay_candidate.bytes == candidate.bytes, "replayed EditorProject changed candidate bytes");
    ensure!(sha256(source) == source_hash, "immutable source changed");
    Ok(json!({
        "output_sha256": candidate.output_hash.to_string(),
        "output_byte_len": candidate.bytes.len(),
        "reader_semantic_reopen": true,
        "editor_project_replay_byte_equal": true,
        "source_immutable": true
    }))
}

fn positive_control() -> Result<Value> {
    let source = decode_fixture(SAMPLE3_B64)?;
    let source_hash = sha256(&source);
    ensure!(source_hash.to_string() == SAMPLE3_SOURCE_SHA, "control source fixture changed");
    let mut editor = open_mature_0x2c_editor(&source, source_hash)
        .context("positive control Editor open failed")?;
    let eligible = editor
        .graph()
        .stories
        .iter()
        .filter(|(id, story)| {
            story.text.matches(POSITIVE_MARKER).count() == 1
                && editor.can_replace_story_text(**id).is_ok()
        })
        .map(|(id, story)| (*id, story.text.clone()))
        .collect::<Vec<_>>();
    ensure!(eligible.len() == 1, "positive control Story identity/eligibility ambiguous");
    let (id, before) = &eligible[0];
    let after = before.replacen(POSITIVE_MARKER, "34567", 1);
    editor
        .replace_story_text(*id, after.clone())
        .context("positive control edit failed")?;
    let accepted = verify_candidate(&editor, &source, &after)?;
    ensure!(accepted["output_sha256"] == ACCEPTED_CONTROL_SHA, "Publisher-approved positive control SHA drift");
    Ok(json!({
        "fixture": "Sample3",
        "source_sha256": source_hash.to_string(),
        "source_byte_len": source.len(),
        "result": "positive_control_exact_native_publisher_accepted_pair",
        "receipt": accepted
    }))
}

fn safe_error_class(error: &str) -> &'static str {
    if error.starts_with("writer_blocked:") {
        "writer_blocked"
    } else if error.starts_with("Editor cannot reopen") || error.starts_with("Reader reopened") {
        "reader_or_editor_reopen_blocked"
    } else if error.starts_with("EditorProject history replay")
        || error.starts_with("replay_writer_blocked:")
        || error.starts_with("replayed EditorProject")
    {
        "project_replay_blocked"
    } else {
        "bounded_candidate_validation_failed"
    }
}

fn probe_fixture(label: &str, encoded: &str) -> Result<Value> {
    let source = decode_fixture(encoded)?;
    let source_hash = sha256(&source);
    let mut result = json!({
        "fixture": label,
        "source_sha256": source_hash.to_string(),
        "source_byte_len": source.len(),
        "result": "not_evaluated",
        "publisher_native_accepted": false,
        "product_download_authorized": false
    });
    if source_hash.to_string() == SAMPLE3_SOURCE_SHA {
        result["result"] = json!("not_independent_source");
        return Ok(result);
    }
    let editor = match open_mature_0x2c_editor(&source, source_hash) {
        Ok(editor) => editor,
        Err(error) => {
            let class = format!("{error:?}");
            result["result"] = json!("source_editor_open_blocked");
            result["blocker_class"] = json!(class.split(['{', '(']).next().unwrap_or("unknown"));
            return Ok(result);
        }
    };
    result["source_story_count"] = json!(editor.graph().stories.len());
    let options = editor
        .graph()
        .stories
        .iter()
        .filter(|(id, story)| {
            editor.can_replace_story_text(**id).is_ok()
                && story.text.chars().any(|c| c.is_ascii_alphanumeric())
        })
        .take(MAX_STORIES_PER_FIXTURE)
        .map(|(id, story)| (*id, story.text.clone()))
        .collect::<Vec<_>>();
    result["eligible_story_attempt_limit"] = json!(MAX_STORIES_PER_FIXTURE);
    result["eligible_story_attempts"] = json!(options.len());
    if options.is_empty() {
        result["result"] = json!("no_plain_editable_story");
        return Ok(result);
    }

    let mut last_blocker = "not_evaluated";
    for (story_id, before) in options {
        let Some((index, _)) = before.char_indices().find(|(_, c)| c.is_ascii_alphanumeric())
        else {
            continue;
        };
        let mut after = before.clone();
        after.remove(index); // exact one ASCII UTF-16 unit, never terminal-CR deletion
        let mut edited = open_mature_0x2c_editor(&source, source_hash)
            .context("known-openable source became unavailable")?;
        if edited.replace_story_text(story_id, after.clone()).is_err() {
            last_blocker = "canonical_story_edit_rejected";
            continue;
        }
        match verify_candidate(&edited, &source, &after) {
            Ok(receipt) => {
                result["result"] = json!("hosted_writer_reopen_replay_pass");
                result["selected_story_id"] = json!(format!("{story_id:?}"));
                result["before_utf16_units"] = json!(before.encode_utf16().count());
                result["after_utf16_units"] = json!(after.encode_utf16().count());
                result["candidate"] = receipt;
                return Ok(result);
            }
            Err(error) => last_blocker = safe_error_class(&error.to_string()),
        }
    }
    result["result"] = json!("hosted_candidate_blocked");
    result["blocker_class"] = json!(last_blocker);
    Ok(result)
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let destination = args.next().context("expected output receipt path")?;
    if args.next().is_some() {
        bail!("expected only source-safe receipt output path");
    }
    let control = positive_control()?;
    let inputs = [
        ("Sample4", SAMPLE4_B64),
        ("LinkAt10", LINK_AT_10_B64),
        ("60685", PUB_60685_B64),
        ("Sample", SAMPLE_B64),
    ];
    let mut candidates = Vec::with_capacity(inputs.len());
    for (label, encoded) in inputs {
        candidates.push(probe_fixture(label, encoded)?);
    }
    let qualified = candidates
        .iter()
        .filter(|row| row["result"] == "hosted_writer_reopen_replay_pass")
        .count();
    let report = json!({
        "schema": "chaptera.cross-fixture-story-preflight.v1",
        "positive_control": control,
        "candidate_count": candidates.len(),
        "hosted_qualified_candidate_count": qualified,
        "candidates": candidates,
        "native_publisher_acceptance": "not_evaluated_for_any_cross_fixture_candidate",
        "product_pub_download_authority": false,
        "fence": "Publisher Open-SaveAs-fresh-Reopen exact candidate required on a separate host"
    });
    let encoded = serde_json::to_vec_pretty(&report)?;
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(&destination))
        .with_context(|| format!("create unused source-safe receipt {destination}"))?;
    out.write_all(&encoded)?;
    out.write_all(b"\n")?;
    out.sync_all()?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
