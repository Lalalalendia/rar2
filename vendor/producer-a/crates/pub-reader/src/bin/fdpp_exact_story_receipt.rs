use anyhow::{Context, Result, bail};
use pub_model::Sha256Digest;
use pub_reader::{PubBridgeDiagnostic, build_mature_0x2c_source_graph};
use std::io::Cursor;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(args.next().context("missing PUB path")?);
    let expected_story_count: usize = args
        .next()
        .context("missing expected Story count")?
        .to_string_lossy()
        .parse()
        .context("parse expected Story count")?;
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }

    let stem = path
        .file_stem()
        .context("PUB path has no file stem")?
        .to_string_lossy();
    let source_hash: Sha256Digest = stem.parse().context("parse SHA-256 file stem")?;
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let build = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash)
        .with_context(|| format!("build SourceGraph for {}", path.display()))?;

    if build.graph.stories.len() != expected_story_count {
        bail!(
            "Story count mismatch: expected {}, got {}",
            expected_story_count,
            build.graph.stories.len()
        );
    }

    let fallback_counts = build
        .diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            PubBridgeDiagnostic::FdppExactStoryFallback { story_count } => Some(*story_count),
            _ => None,
        })
        .collect::<Vec<_>>();
    if fallback_counts != vec![expected_story_count] {
        bail!("unexpected FDPP fallback receipt: {fallback_counts:?}");
    }

    for story in build.graph.stories.values() {
        let labels = story
            .source_refs
            .iter()
            .filter_map(|source_ref| source_ref.path.as_deref())
            .collect::<Vec<_>>();
        if !labels.contains(&"Contents/0x65/textId")
            || !labels.contains(&"FDPP/storyEnd")
            || !labels.contains(&"TEXT")
        {
            bail!("Story is missing exact FDPP fallback provenance: {labels:?}");
        }
    }

    println!(
        "fdpp_exact_story_acceptance=pass stories={}",
        build.graph.stories.len()
    );
    Ok(())
}
