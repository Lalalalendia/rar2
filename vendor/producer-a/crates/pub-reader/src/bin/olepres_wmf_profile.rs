use anyhow::{Context, Result};
use pub_reader::{parse_cf_metafilepict_ole_presentation, profile_wmf};
use serde_json::json;
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: olepres_wmf_profile OLEPRES.bin OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: olepres_wmf_profile OLEPRES.bin OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("olepres_wmf_profile accepts exactly OLEPRES.bin OUTPUT.json");
    }

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let presentation = parse_cf_metafilepict_ole_presentation(&bytes)
        .with_context(|| format!("parse OLE presentation {}", source.display()))?;
    let wmf = profile_wmf(presentation.data)
        .with_context(|| format!("profile WMF {}", source.display()))?;

    let functions = wmf
        .functions
        .iter()
        .map(|(function, count)| (format!("0x{function:04x}"), json!(count)))
        .collect::<serde_json::Map<_, _>>();

    let receipt = json!({
        "schema": "chaptera.olepres-wmf-profile.v1",
        "clipboard_format": presentation.clipboard_format,
        "aspect": presentation.aspect,
        "lindex": presentation.lindex,
        "advf": presentation.advf,
        "width": presentation.width,
        "height": presentation.height,
        "wmf": {
            "placeable": wmf.placeable,
            "file_type": wmf.file_type,
            "header_size_words": wmf.header_size_words,
            "version": wmf.version,
            "declared_size_words": wmf.declared_size_words,
            "object_count": wmf.object_count,
            "max_record_words": wmf.max_record_words,
            "parameter_count": wmf.parameter_count,
            "record_count": wmf.record_count,
            "saw_eof": wmf.saw_eof,
            "functions": functions,
        },
    });

    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
