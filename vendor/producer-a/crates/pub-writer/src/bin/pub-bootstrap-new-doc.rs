use pub_writer::{
    BootstrapNewDocTemplate, bootstrap_new_doc_report_json,
    materialize_bounded_bootstrap_new_doc_candidate,
};
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let seed = required_path(&mut args, "seed.pub")?;
    let contents = required_path(&mut args, "contents.bin")?;
    let escher = required_path(&mut args, "escher.bin")?;
    let quill = required_path(&mut args, "quill.bin")?;
    let output = required_path(&mut args, "output.pub")?;
    let receipt = required_path(&mut args, "receipt.json")?;
    if args.next().is_some() {
        return Err(usage().into());
    }

    let seed_bytes = fs::read(&seed)?;
    let contents_bytes = fs::read(&contents)?;
    let escher_bytes = fs::read(&escher)?;
    let quill_bytes = fs::read(&quill)?;

    let candidate = materialize_bounded_bootstrap_new_doc_candidate(
        &seed_bytes,
        BootstrapNewDocTemplate {
            contents_stream: &contents_bytes,
            escher_stream: &escher_bytes,
            quill_stream: &quill_bytes,
        },
    )?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = receipt.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, &candidate.bytes)?;
    fs::write(&receipt, bootstrap_new_doc_report_json(&candidate.report)?)?;

    println!("wrote {}", output.display());
    println!("receipt {}", receipt.display());
    println!("output_sha256 {}", candidate.report.output_sha256);
    Ok(())
}

fn required_path(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
    label: &str,
) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing {label}; {}", usage()))
}

fn usage() -> &'static str {
    "usage: pub-bootstrap-new-doc <seed.pub> <contents.bin> <escher.bin> <quill.bin> <output.pub> <receipt.json>"
}
