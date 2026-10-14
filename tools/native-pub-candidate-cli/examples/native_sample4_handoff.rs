//! Exact pinned Sample4 PUB handoff for the licensed Publisher 2019 oracle.
//! This CLI produces *private* control/candidate bundles from one public
//! Apache POI source; neither a hosted Writer PASS nor this generator grants
//! product Download PUB permission.
use anyhow::{Context, Result, bail, ensure};
use pub_core::StreamPath;
use pub_editor::{Sha256Digest, open_mature_0x2c_editor};
use pub_reader::{QUILL_STREAM_PATH, derive_pub_story_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, io::{Cursor, Write}, path::Path};

const SOURCE_SHA: &str =
    "42195f7ad23d911219fea3ec88e66e867e9b9a6821a16dd1b535e2aa9d57a11b";
const ACCEPTED_HOSTED_CANDIDATE_SHA: &str =
    "2f7795a3c4307716565c7accf4636a80b7947f8921c60a87b51e1a48ee529509";
const SOURCE_LEN: usize = 72_192;
const BEFORE_UNITS: usize = 86;
const AFTER_UNITS: usize = 85;
const MAX_EDITABLE_STORIES: usize = 8;

const SAMPLE4_B64: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/Sample4.pub.b64"
));

fn sha256(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut hash = [0_u8; 32];
    hash.copy_from_slice(&digest);
    Sha256Digest::from_bytes(hash)
}

fn text_sha256(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

// Public pinned fixture only. No file paths or arbitrary input PUB are accepted.
fn decode_fixture(encoded: &str) -> Result<Vec<u8>> {
    let cleaned = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    ensure!(!cleaned.is_empty() && cleaned.len() % 4 == 0, "invalid fixture base64 length");
    let value = |byte: u8| -> Result<u8> {
        match byte {
            b'A'..=b'Z' => Ok(byte - b'A'),
            b'a'..=b'z' => Ok(byte - b'a' + 26),
            b'0'..=b'9' => Ok(byte - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => bail!("invalid fixture base64 character"),
        }
    };
    let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
    for (i, quartet) in cleaned.chunks_exact(4).enumerate() {
        let final_quartet = i + 1 == cleaned.len() / 4;
        ensure!(final_quartet || (quartet[2] != b'=' && quartet[3] != b'='),
            "invalid fixture base64 early padding");
        ensure!(quartet[2] != b'=' || quartet[3] == b'=', "invalid fixture base64 padding");
        let a = value(quartet[0])?;
        let b = value(quartet[1])?;
        let c = if quartet[2] == b'=' { 0 } else { value(quartet[2])? };
        let d = if quartet[3] == b'=' { 0 } else { value(quartet[3])? };
        out.push((a << 2) | (b >> 4));
        if quartet[2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if quartet[3] != b'=' {
            out.push((c << 6) | d);
        }
    }
    Ok(out)
}

fn write_new(path: &Path, content: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create unused private handoff {}", path.display()))?;
    file.write_all(content).context("write private bundle input")?;
    file.sync_all().context("sync private bundle input")?;
    Ok(())
}

fn write_handoff(
    root: &Path,
    part: &str,
    source: &[u8],
    candidate: &[u8],
    manifest: Value,
) -> Result<()> {
    let dest = root.join(part);
    fs::create_dir(&dest).context("create exact private handoff arm")?;
    write_new(&dest.join("source.pub"), source)?;
    write_new(&dest.join("candidate.pub"), candidate)?;
    let encoded = serde_json::to_vec_pretty(&manifest)?;
    write_new(&dest.join("handoff.json"), &encoded)?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let flag = args.next().context("expected --output-dir")?;
    ensure!(flag == "--output-dir", "only --output-dir is supported");
    let out_dir = args.next().context("expected unused private output directory")?;
    ensure!(args.next().is_none(), "unexpected command arguments");
    let out_dir = Path::new(&out_dir);
    ensure!(!out_dir.exists(), "private native handoff destination already exists");

    let source = decode_fixture(SAMPLE4_B64)?;
    let source_hash = sha256(&source);
    ensure!(source.len() == SOURCE_LEN && source_hash.to_string() == SOURCE_SHA,
        "pinned Sample4 source identity mismatch");
    let opened = open_mature_0x2c_editor(&source, source_hash)
        .context("Sample4 cannot open in current Editor")?;
    let stories = opened
        .graph()
        .stories
        .iter()
        .filter(|(story_id, text)| {
            opened.can_replace_story_text(**story_id).is_ok()
                && text.text.chars().any(|ch| ch.is_ascii_alphanumeric())
        })
        .take(MAX_EDITABLE_STORIES)
        .map(|(id, story)| (*id, story.text.clone()))
        .collect::<Vec<_>>();

    let quill = pub_cfb::read_stream_reader(Cursor::new(&source), QUILL_STREAM_PATH)
        .context("read source Quill stream")?;
    let catalog = pub_quill::parse_confirmed_story_catalog(
        StreamPath(QUILL_STREAM_PATH.into()),
        &quill,
    )
    .context("source Quill Story catalog")?;

    for (id, before) in stories {
        let Some((index, _)) = before
            .char_indices()
            .find(|(_, letter)| letter.is_ascii_alphanumeric())
        else {
            continue;
        };
        let mut after = before.clone();
        after.remove(index); // precisely one UTF-16 ASCII unit, never terminal CR
        let mut editor = open_mature_0x2c_editor(&source, source_hash)
            .context("fresh Editor cannot reopen pinned Sample4 source")?;
        if editor.replace_story_text(id, after.clone()).is_err() {
            continue;
        }
        let Ok(candidate) = editor.materialize_mature_0x2c_native_pub_candidate(&source)
        else {
            continue;
        };
        if candidate.output_hash.to_string() != ACCEPTED_HOSTED_CANDIDATE_SHA {
            continue; // never substitute a merely Reader-openable new candidate
        }

        ensure!(candidate.bytes.len() == SOURCE_LEN, "candidate CFB size drift");
        ensure!(candidate.output_hash == sha256(&candidate.bytes), "candidate output SHA drift");
        ensure!(candidate.source_hash == source_hash, "candidate source SHA drift");
        ensure!(before.encode_utf16().count() == BEFORE_UNITS, "source Story length drift");
        ensure!(after.encode_utf16().count() == AFTER_UNITS, "edited Story length drift");
        let mut replay = open_mature_0x2c_editor(&source, source_hash)
            .context("fresh EditorProject source reopen")?;
        replay.apply_project(&editor.project())
            .context("pinned EditorProject replay failed")?;
        let again = replay.materialize_mature_0x2c_native_pub_candidate(&source)
            .context("pinned EditorProject Writer replay blocked")?;
        ensure!(again.bytes == candidate.bytes, "EditorProject replay produced a different candidate");

        let matching = catalog.stories.iter().filter(|story| {
            derive_pub_story_id(&source_hash, story.syid.0)
                .ok()
                .is_some_and(|derived| derived == id)
        }).collect::<Vec<_>>();
        ensure!(matching.len() == 1, "source StoryId-to-Quill SYID is not unique");
        let syid = matching[0].syid.0;
        let before_sha = text_sha256(&before);
        let after_sha = text_sha256(&after);

        let original_manifest = json!({
            "schema": "chaptera.pub-native-story-handoff.v1",
            "fixture": "Sample4.pub (public Apache POI pinned)",
            "research_variant": "matched_native_control_original_source",
            "source_sha256": SOURCE_SHA,
            "candidate_sha256": SOURCE_SHA,
            "story_syid": syid,
            "after_utf16_len": BEFORE_UNITS,
            "after_text_sha256": before_sha,
            "mutation": { "kind": "no_op_native_saveas_source_control", "removed_utf16_units": 0 },
            "native_target": { "publisher_family": "Publisher 2019", "version": "16.0", "build_prefix": "12527" }
        });
        let edited_manifest = json!({
            "schema": "chaptera.pub-native-story-handoff.v1",
            "fixture": "Sample4.pub (public Apache POI pinned)",
            "research_variant": "one_ascii_utf16_unit_removed_from_ordinary_story",
            "source_sha256": SOURCE_SHA,
            "candidate_sha256": ACCEPTED_HOSTED_CANDIDATE_SHA,
            "story_syid": syid,
            "before_utf16_len": BEFORE_UNITS,
            "after_utf16_len": AFTER_UNITS,
            "before_text_sha256": before_sha,
            "after_text_sha256": after_sha,
            "mutation": { "kind": "bounded_story_text_delete", "removed_utf16_units": 1 },
            "native_target": { "publisher_family": "Publisher 2019", "version": "16.0", "build_prefix": "12527" }
        });

        fs::create_dir(out_dir).context("create unused private native root")?;
        write_handoff(out_dir, "control", &source, &source, original_manifest)?;
        write_handoff(out_dir, "candidate", &source, &candidate.bytes, edited_manifest)?;
        // Browser acceptance consumes the same real Rust EditorProject baseline.
        // It stays private inside the hosted work directory and is never uploaded.
        write_new(
            &out_dir.join("baseline-project.json"),
            &serde_json::to_vec_pretty(&opened.project())?,
        )?;
        let receipt = json!({
            "schema": "chaptera.sample4.native-oracle-handoff.v1",
            "fixture": "Sample4",
            "source_sha256": SOURCE_SHA,
            "candidate_sha256": ACCEPTED_HOSTED_CANDIDATE_SHA,
            "story_syid": syid,
            "source_byte_len": SOURCE_LEN,
            "candidate_byte_len": candidate.bytes.len(),
            "before_utf16_len": BEFORE_UNITS,
            "after_utf16_len": AFTER_UNITS,
            "writer_reader_reopen": true,
            "editor_project_replay_byte_equal": true,
            "publisher_native_acceptance": "not_evaluated",
            "product_pub_download_authority": false
        });
        write_new(&out_dir.join("source-safe-receipt.json"), &serde_json::to_vec_pretty(&receipt)?)?;
        println!("{}", serde_json::to_string(&receipt)?);
        return Ok(());
    }
    bail!("no exact-SHA-matched Sample4 one-unit Story Writer candidate; STOP before Publisher")
}
