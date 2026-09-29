use anyhow::{Context, Result};
use pub_contents::parse_legacy_0x22_formatting_runs;
use pub_core::StreamPath;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, env, fs, path::PathBuf};

fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let corpus = PathBuf::from(args.next().context("usage: legacy22_font_signal_profile CORPUS_DIR OUTPUT.json")?);
    let output = PathBuf::from(args.next().context("usage: legacy22_font_signal_profile CORPUS_DIR OUTPUT.json")?);
    if args.next().is_some() { anyhow::bail!("too many arguments"); }

    let mut paths = fs::read_dir(&corpus)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("pub")).unwrap_or(false))
        .collect::<Vec<_>>();
    paths.sort();

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    let mut high_files = 0usize;
    let mut high_with_font = 0usize;
    let mut high_without_font = 0usize;
    let mut font_presence = BTreeMap::<String, usize>::new();

    for path in paths {
        let bytes = fs::read(&path)?;
        let source_sha256 = sha256_hex(&bytes);
        let inventory = match pub_cfb::inspect_path(&path) {
            Ok(v) => v,
            Err(e) => {
                errors.push(json!({"sha256":source_sha256,"stage":"cfb","error":format!("{e:#}")}));
                continue;
            }
        };
        if inventory.entries.iter().any(|e| e.path.starts_with("/Quill/")) {
            continue;
        }
        let contents = match pub_cfb::read_stream_path(&path, "/Contents") {
            Ok(v) => v,
            Err(_) => continue,
        };
        if contents.len() < 3 || contents[2] != 0x22 { continue; }

        let formatting = match parse_legacy_0x22_formatting_runs(StreamPath("/Contents".into()), &contents) {
            Ok(v) => v,
            Err(e) => {
                errors.push(json!({"sha256":source_sha256,"stage":"formatting","error":e.to_string()}));
                continue;
            }
        };
        let start = usize::try_from(formatting.descriptor.text_start)?;
        let end = usize::try_from(formatting.descriptor.text_end)?;
        let Some(text) = contents.get(start..end) else { continue; };

        let mut high_count = 0usize;
        let mut uncovered = 0usize;
        let mut font_indexes = BTreeSet::<u8>::new();
        let mut byte_to_fonts = BTreeMap::<String, BTreeSet<u8>>::new();

        for (rel, byte) in text.iter().copied().enumerate() {
            if byte < 0x80 { continue; }
            high_count += 1;
            let absolute = u32::try_from(start + rel)?;
            let run = formatting.character_runs.iter().find(|run| run.fc_first <= absolute && absolute < run.fc_lim);
            let font_index = run.and_then(|run| run.style.as_ref()).and_then(|style| style.font_index);
            if let Some(index) = font_index {
                font_indexes.insert(index);
                byte_to_fonts.entry(format!("0x{byte:02x}")).or_default().insert(index);
            } else {
                uncovered += 1;
            }
        }

        if high_count == 0 { continue; }
        high_files += 1;
        if font_indexes.is_empty() {
            high_without_font += 1;
        } else {
            high_with_font += 1;
            for index in &font_indexes {
                *font_presence.entry(index.to_string()).or_insert(0) += 1;
            }
        }

        let byte_to_fonts = byte_to_fonts.into_iter().map(|(k,v)| (k, v.into_iter().collect::<Vec<_>>())).collect::<BTreeMap<_,_>>();
        rows.push(json!({
            "sha256": source_sha256,
            "high_byte_count": high_count,
            "high_byte_font_indexes": font_indexes.into_iter().collect::<Vec<_>>(),
            "high_byte_uncovered_count": uncovered,
            "byte_to_font_indexes": byte_to_fonts,
            "character_run_count": formatting.character_runs.len()
        }));
    }

    let receipt = json!({
        "schema":"chaptera.legacy22-high-byte-font-signal.v1",
        "high_byte_file_count":high_files,
        "high_byte_files_with_font_index":high_with_font,
        "high_byte_files_without_font_index":high_without_font,
        "font_index_presence_counts":font_presence,
        "row_count":rows.len(),
        "error_count":errors.len(),
        "rows":rows,
        "errors":errors,
        "evidence_boundary":"font_index is table-relative persisted CHPX state; this receipt does not assign font names, charsets, or Unicode semantics."
    });
    if let Some(parent)=output.parent() { fs::create_dir_all(parent)?; }
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
