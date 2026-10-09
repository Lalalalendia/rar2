//! Source-pinned test input for the existing Publisher-accepted Story edit.
//! Produces EditorProject files only. Never grants product Save PUB authority.
use anyhow::{Context, Result, bail};
use pub_editor::{Sha256Digest, open_mature_0x2c_editor};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::Path};

const SOURCE_SHA: &str = "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc";
const MARKER: &str = "345678";

fn write_new(path: &str, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(path))
        .with_context(|| format!("create unused test input {}", path))?;
    file.write_all(bytes).context("write EditorProject")?;
    file.sync_all().context("sync EditorProject")?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let source_path = args.next().context("expected SOURCE.pub")?;
    let baseline_path = args.next().context("expected BASELINE.json")?;
    let edited_path = args.next().context("expected EDITED.json")?;
    if args.next().is_some() || baseline_path == edited_path {
        bail!("expected three arguments with distinct output paths");
    }

    let source = fs::read(&source_path).context("read pinned Sample3")?;
    let digest = Sha256::digest(&source);
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    let source_hash = Sha256Digest::from_bytes(bytes);
    if source.len() != 72_192 || source_hash.to_string() != SOURCE_SHA {
        bail!("Sample3 source identity mismatch; no Publisher authority");
    }

    let mut editor = open_mature_0x2c_editor(&source, source_hash)
        .map_err(|error| anyhow::anyhow!("open bounded Editor: {error:?}"))?;
    let baseline = serde_json::to_vec_pretty(&editor.project())?;
    let matches = editor
        .graph()
        .stories
        .iter()
        .filter(|(_, story)| story.text.contains(MARKER))
        .map(|(story_id, story)| (*story_id, story.text.clone()))
        .collect::<Vec<_>>();
    if matches.len() != 1 || matches[0].1.matches(MARKER).count() != 1 {
        bail!("expected exactly one controlled ordinary Story marker");
    }
    let (story_id, before) = matches.into_iter().next().context("missing Story")?;
    editor
        .replace_story_text(story_id, before.replacen(MARKER, "", 1))
        .map_err(|error| anyhow::anyhow!("bounded Story edit: {error:?}"))?;
    let edited = serde_json::to_vec_pretty(&editor.project())?;

    write_new(&baseline_path, &baseline)?;
    write_new(&edited_path, &edited)?;
    println!(
        "{}",
        serde_json::json!({
            "source_sha256": SOURCE_SHA,
            "input_kind": "pinned_sample3_six_unit_story_delete",
            "project_files_created": true,
            "publisher_authority": "external_exact_byte_receipt_only"
        })
    );
    Ok(())
}
