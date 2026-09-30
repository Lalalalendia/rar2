use anyhow::{bail, Context, Result};
use pub_model::Sha256Digest;
use pub_reader::{build_mature_0x2c_source_graph, PubBridgeDiagnostic};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
struct ReceiptRow {
    source_sha256: String,
    success: bool,
    story_count: Option<usize>,
    fdpp_recovered_story_count: Option<usize>,
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(root)
        .with_context(|| format!("read witness directory {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "pub"))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn source_hash(bytes: &[u8]) -> (String, Sha256Digest) {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let source_hash = Sha256Digest::from_bytes(digest);
    (source_hash.to_string(), source_hash)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = args
        .next()
        .context("usage: quill-fdpp-story-recovery-receipt WITNESS_DIR OUTPUT.json")?;
    let output = args
        .next()
        .context("usage: quill-fdpp-story-recovery-receipt WITNESS_DIR OUTPUT.json")?;
    if args.next().is_some() {
        bail!("unexpected extra argument");
    }

    let paths = pub_paths(Path::new(&root))?;
    let mut rows = Vec::with_capacity(paths.len());

    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let (source_sha256, source_hash) = source_hash(&bytes);

        match build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash) {
            Ok(build) => {
                let fdpp_recovered_story_count = build.diagnostics.iter().find_map(|diagnostic| {
                    if let PubBridgeDiagnostic::QuillStoriesRecoveredFromFdpp { story_count } =
                        diagnostic
                    {
                        Some(*story_count)
                    } else {
                        None
                    }
                });
                rows.push(ReceiptRow {
                    source_sha256,
                    success: true,
                    story_count: Some(build.graph.stories.len()),
                    fdpp_recovered_story_count,
                });
            }
            Err(_) => rows.push(ReceiptRow {
                source_sha256,
                success: false,
                story_count: None,
                fdpp_recovered_story_count: None,
            }),
        }
    }

    rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));
    let report = serde_json::json!({
        "schema": "chaptera.quill-fdpp-story-recovery.v1",
        "witness_count": rows.len(),
        "success_count": rows.iter().filter(|row| row.success).count(),
        "failure_count": rows.iter().filter(|row| !row.success).count(),
        "rows": rows,
        "evidence_boundary": "exact SHA-addressed targeted witnesses only; no filenames, paths, document text, Story IDs, raw bytes, offsets, or parser error text",
    });

    if let Some(parent) = Path::new(&output).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
