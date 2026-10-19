//! Derive a private source-font plan from a local .pub without writing Viewer
//! Story text to disk. This is NOT physical-font admission or PDF authority.
//!
//! The verified Reader produces the in-memory Viewer model, piped directly to
//! the existing strict Python planner on stdin; only its source-safe plan may
//! be written with create_new. Never write full Viewer JSON or font bytes.
use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_PUB_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SAFE_PLAN_BYTES: usize = 8 * 1024 * 1024;

fn require_cli() -> Result<(PathBuf, PathBuf)> {
    let argv: Vec<_> = env::args_os().skip(1).collect();
    if argv.len() != 4
        || argv[0].to_str() != Some("--pub")
        || argv[2].to_str() != Some("--output")
    {
        bail!("usage: pub-private-source-font-plan-v1 --pub LOCAL.pub --output NEW_PRIVATE_PLAN.json");
    }
    Ok((PathBuf::from(&argv[1]), PathBuf::from(&argv[3])))
}

fn source_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn make_private_plan(pub_path: &Path, output_path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(pub_path).context("cannot inspect local PUB input")?;
    if !meta.file_type().is_file() || meta.len() == 0 || meta.len() > MAX_PUB_BYTES {
        bail!("PUB must be a regular nonempty file no larger than 32 MiB");
    }
    // No implicit overwrite, even if the destination is an input symlink.
    if output_path.exists() {
        bail!("private source-font plan already exists; choose a new output");
    }
    let bytes = fs::read(pub_path).context("cannot read local Publisher source")?;
    if bytes.len() as u64 != meta.len() {
        bail!("PUB changed while reading; refuse unpinned source");
    }
    let digest = source_hash(&bytes);
    let viewer = pub_viewer::open_pub_geometry(
        &bytes,
        pub_viewer::viewer_geometry_environment_v0_1(),
    )
    .context("current Reader rejected Publisher source")?;

    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tools/pub_source_font_requirements_v1.py");
    if !script.is_file() {
        bail!("trusted private source-font planner is missing from checkout");
    }
    let mut planner = Command::new("python")
        .arg(&script)
        .args(["--viewer", "-", "--output", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("cannot start trusted source-font planner")?;
    {
        let mut input = planner.stdin.take().context("planner stdin unavailable")?;
        serde_json::to_writer(&mut input, &viewer)
            .context("could not stream in-memory Viewer to private planner")?;
    }
    let completed = planner
        .wait_with_output()
        .context("could not receive source-safe font requirements")?;
    if !completed.status.success()
        || completed.stdout.is_empty()
        || completed.stdout.len() > MAX_SAFE_PLAN_BYTES
    {
        bail!("source-font planner refused the private document");
    }
    let plan: Value = serde_json::from_slice(&completed.stdout)
        .context("planner returned invalid source-safe JSON")?;
    if plan["document_source_sha256"].as_str() != Some(digest.as_str())
        || plan["direct_source_binding_count"].as_u64().unwrap_or(0) == 0
        || plan["direct_source_bindings_only_source_identity"] != true
        || plan["physical_font_bytes_included"] != false
        || plan["original_publisher_layout_authoritative"] != false
        || plan["fixed_pdf_allowed"] != false
        || plan.get("document").is_some()
        || plan.get("scene").is_some()
    {
        bail!("private font plan identity, provenance or output fence mismatch");
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options
        .open(output_path)
        .context("cannot create new private requirements file")?;
    out.write_all(&completed.stdout)
        .context("cannot write private requirements")?;
    out.flush().context("cannot flush private requirements")?;

    println!(
        "PRIVATE_REAL_PUB_FONT_PLAN_OK source_sha256={} direct_bindings={} families={} \
         physical_font_files=false editor_authoring_admitted=false fixed_pdf_allowed=false",
        digest,
        plan["direct_source_binding_count"].as_u64().unwrap_or(0),
        plan["families"].as_array().map_or(0, Vec::len)
    );
    Ok(())
}

fn main() -> Result<()> {
    let (pub_path, output_path) = require_cli()?;
    make_private_plan(&pub_path, &output_path)
}
