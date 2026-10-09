//! T352: bounded in-place CFB stream patching, without touching the source PUB.
//!
//! Usage:
//!   cargo run --bin val_xproj_patch -- prepare BASE.pub STRUCTURAL.json OUT_DIR
//!   cargo run --bin val_xproj_patch -- fingerprint FILE.pub OUT.json
//!
//! The structural JSON is produced by the existing structural_base_manifest
//! binary on this *same exact* Publisher-normalized base. No semantic
//! classification is inferred here: this tool creates controls for the real
//! Publisher Open -> Save -> fresh Reopen experiment.
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const DELTA_EMU: i64 = 127_000; // exactly +10 pt
const BASE_WIDTH_EMU: i64 = 5_076_000;
const BASE_XE_EMU: i64 = 2_376_000;

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn require_i64<'a>(value: &'a Value, name: &str) -> Result<i64> {
    value
        .get(name)
        .and_then(Value::as_i64)
        .with_context(|| format!("missing signed integer {name}"))
}

fn require_str<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string {name}"))
}

fn uint32_bytes(value: i64) -> Result<[u8; 4]> {
    Ok(u32::try_from(value)
        .context("U32 value outside bounds")?
        .to_le_bytes())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Span {
    stream: String,
    offset: u64,
    len: u64,
}

fn span(value: &Value) -> Result<Span> {
    let stream = require_str(value, "stream")?.to_owned();
    let offset = value
        .get("offset")
        .and_then(Value::as_u64)
        .context("span missing offset")?;
    let len = value
        .get("len")
        .and_then(Value::as_u64)
        .context("span missing len")?;
    if !stream.starts_with('/') || stream.contains("..") {
        bail!("span stream must be an absolute CFB logical path");
    }
    Ok(Span {
        stream,
        offset,
        len,
    })
}

#[derive(Debug)]
struct Patch {
    span: Span, // the exact 4-byte value, not the surrounding tag
    expected: [u8; 4],
    replacement: [u8; 4],
}

fn plan(receipt: &Value) -> Result<(Patch, Patch)> {
    if require_str(receipt, "schema")? != "chaptera.modern-structural-base/v1" {
        bail!("unsupported structural receipt schema");
    }
    if require_i64(receipt, "stream_count")? != 10 || require_i64(receipt, "candidate_count")? != 6
    {
        bail!("T370 semantic contract: unexpected stream/candidate counts");
    }
    let target = receipt
        .get("selected_target")
        .context("missing selected_target")?;
    for (name, expected) in [
        ("contents_seq_num", 293),
        ("publisher_shape_id", 293),
        ("officeart_spid", 1025),
        ("officeart_shape_type", 202),
        ("contents_width_emu", BASE_WIDTH_EMU),
        ("contents_height_emu", 972_000),
        ("anchor_xs", -2_700_000),
        ("anchor_ys", -4_446_000),
        ("anchor_xe", BASE_XE_EMU),
        ("anchor_ye", -3_474_000),
    ] {
        if require_i64(target, name)? != expected {
            bail!("T370 target semantic contract mismatch: {name}");
        }
    }
    if target
        .get("contents_anchor_extent_exact")
        .and_then(Value::as_bool)
        != Some(true)
    {
        bail!("T370 Contents/Escher invariant absent");
    }
    if require_i64(target, "contents_width_wire_type")? != 0x20 {
        // The U32 tag's block_type is defined by pub-contents; a changed
        // encoding requires explicit review, never guessing a 4-byte patch.
        bail!("Contents width is not a known U32 wire type");
    }

    let cw = span(
        target
            .get("contents_width_source")
            .context("missing Contents width span")?,
    )?;
    if cw.stream != "/Contents" || cw.len != 4 {
        bail!("Contents width must be an exact four-byte /Contents value");
    }
    let candidates = receipt
        .pointer("/manifest/candidates")
        .and_then(Value::as_array)
        .context("missing structural candidates")?;
    let matched: Vec<_> = candidates
        .iter()
        .filter(|v| {
            v.get("contents_seq_num").and_then(Value::as_i64) == Some(293)
                && v.get("officeart_spid").and_then(Value::as_i64) == Some(1025)
        })
        .collect();
    if matched.len() != 1 {
        bail!("selected shape must have exactly one Contents/Escher joined candidate");
    }
    let fields = matched[0]
        .pointer("/escher_shape/client_anchor/fields")
        .and_then(Value::as_array)
        .context("missing selected ClientAnchor fields")?;
    let xe: Vec<_> = fields
        .iter()
        .filter(|v| v.get("id").and_then(Value::as_u64) == Some(0x2003))
        .collect();
    if xe.len() != 1 || xe[0].get("value").and_then(Value::as_u64) != Some(BASE_XE_EMU as u64) {
        bail!("Escher ClientAnchor XE must be present exactly once with expected value");
    }
    let tagged = span(xe[0].get("source").context("missing XE source")?)?;
    if tagged.len != 6 {
        bail!("Escher PublisherField must be exactly 2-byte ID + 4-byte value");
    }
    let value_offset = tagged.offset.checked_add(2).context("XE offset overflow")?;
    let ex = Span {
        stream: tagged.stream,
        offset: value_offset,
        len: 4,
    };
    if ex.stream == cw.stream {
        bail!("Contents and Escher patch streams unexpectedly coincide");
    }

    Ok((
        Patch {
            span: cw,
            expected: uint32_bytes(BASE_WIDTH_EMU)?,
            replacement: uint32_bytes(BASE_WIDTH_EMU + DELTA_EMU)?,
        },
        Patch {
            span: ex,
            expected: uint32_bytes(BASE_XE_EMU)?,
            replacement: uint32_bytes(BASE_XE_EMU + DELTA_EMU)?,
        },
    ))
}

fn all_streams(path: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut comp = cfb::open(path).with_context(|| format!("open CFB {}", path.display()))?;
    let names: Vec<String> = comp
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    let mut streams = BTreeMap::new();
    for name in names {
        let mut stream = comp.open_stream(&name)?;
        let mut content = Vec::new();
        stream.read_to_end(&mut content)?;
        if streams.insert(name, content).is_some() {
            bail!("duplicate CFB stream");
        }
    }
    Ok(streams)
}

fn patch_one(path: &Path, plan: &Patch) -> Result<()> {
    let mut comp = cfb::open_rw(path).context("open copied PUB for exact-range patch")?;
    let mut stream = comp
        .open_stream(&plan.span.stream)
        .with_context(|| format!("missing planned stream {}", plan.span.stream))?;
    if plan.span.len != 4
        || plan
            .span
            .offset
            .checked_add(4)
            .is_none_or(|n| n > stream.len())
    {
        bail!("planned four-byte patch lies outside logical CFB stream");
    }
    stream.seek(SeekFrom::Start(plan.span.offset))?;
    let mut actual = [0u8; 4];
    stream.read_exact(&mut actual)?;
    if actual != plan.expected {
        bail!("expected original four-byte value does not match pinned structural receipt");
    }
    stream.seek(SeekFrom::Start(plan.span.offset))?;
    stream.write_all(&plan.replacement)?;
    stream.flush()?;
    Ok(())
}

fn changed_offsets(before: &[u8], after: &[u8]) -> Result<Vec<usize>> {
    if before.len() != after.len() {
        bail!("CFB logical stream size changed under four-byte patch");
    }
    Ok(before
        .iter()
        .zip(after)
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some(i))
        .collect())
}

#[derive(Serialize)]
struct StreamFingerprint {
    path: String,
    len: usize,
    sha256: String,
}

#[derive(Serialize)]
struct FileFingerprint {
    schema: &'static str,
    whole_file_sha256: String,
    whole_file_len: usize,
    streams: Vec<StreamFingerprint>,
}

fn fingerprint(path: &Path) -> Result<FileFingerprint> {
    let raw = fs::read(path)?;
    let streams = all_streams(path)?
        .into_iter()
        .map(|(path, data)| StreamFingerprint {
            path,
            len: data.len(),
            sha256: sha(&data),
        })
        .collect();
    Ok(FileFingerprint {
        schema: "chaptera.t352-cfb-fingerprint.v1",
        whole_file_sha256: sha(&raw),
        whole_file_len: raw.len(),
        streams,
    })
}

fn write_json<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let mut f = File::create(path)?;
    serde_json::to_writer_pretty(&mut f, data)?;
    f.write_all(b"\n")?;
    Ok(())
}

fn prepare(base: &Path, receipt_path: &Path, output: &Path) -> Result<()> {
    if output.exists() {
        bail!("output directory already exists: refusing to reuse or overwrite arms");
    }
    let base_raw = fs::read(base).context("read normalized PUB")?;
    if base_raw.len() < 50_000 || base_raw.len() > 10_000_000 {
        bail!("base PUB outside bounded size gate");
    }
    let manifest: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
    let base_sha = sha(&base_raw);
    if require_str(&manifest, "source_sha256")? != base_sha {
        bail!("structural receipt is not bound to the exact normalized PUB bytes");
    }
    let (contents, escher) = plan(&manifest)?;
    let baseline_streams = all_streams(base)?;
    for patch in [&contents, &escher] {
        let bytes = baseline_streams
            .get(&patch.span.stream)
            .context("planned stream absent")?;
        let start = usize::try_from(patch.span.offset)?;
        if bytes.get(start..start + 4) != Some(patch.expected.as_slice()) {
            bail!("planned original range failed stream-byte check");
        }
    }

    fs::create_dir_all(output)?;
    let mut summaries = Vec::new();
    for (arm, use_contents, use_escher) in [
        ("control", false, false),
        ("both_consistent", true, true),
        ("contents_only", true, false),
        ("escher_only", false, true),
    ] {
        let filename = format!("{arm}.pub");
        let target = output.join(&filename);
        fs::copy(base, &target)?;
        if use_contents {
            patch_one(&target, &contents)?;
        }
        if use_escher {
            patch_one(&target, &escher)?;
        }

        let after = all_streams(&target)?;
        if after.keys().collect::<Vec<_>>() != baseline_streams.keys().collect::<Vec<_>>() {
            bail!("CFB stream topology changed while patching {arm}");
        }
        let mut changed = Vec::new();
        for (path, before_bytes) in &baseline_streams {
            let after_bytes = after.get(path).context("missing CFB stream after patch")?;
            let offsets = changed_offsets(before_bytes, after_bytes)?;
            if !offsets.is_empty() {
                let expected_span = if use_contents && *path == contents.span.stream {
                    Some(&contents.span)
                } else if use_escher && *path == escher.span.stream {
                    Some(&escher.span)
                } else {
                    None
                };
                let allowed = expected_span.context("unplanned CFB stream was changed")?;
                let start = usize::try_from(allowed.offset)?;
                if offsets.iter().any(|i| *i < start || *i >= start + 4) {
                    bail!("CFB stream changed outside exact planned four-byte range");
                }
                changed.push(json!({
                    "stream": path,
                    "before_sha256": sha(before_bytes),
                    "after_sha256": sha(after_bytes),
                    "changed_byte_count": offsets.len(),
                    "offset": start
                }));
            }
        }
        if changed.len() != usize::from(use_contents) + usize::from(use_escher) {
            bail!("patch did not modify exactly the expected logical streams in {arm}");
        }
        summaries.push(json!({
            "arm": arm, "private_file": filename,
            "file_sha256": sha(&fs::read(&target)?),
            "changed_streams": changed
        }));
    }
    if sha(&fs::read(base)?) != base_sha {
        bail!("immutable source base changed during patch preparation");
    }
    write_json(
        &output.join("t352-patch-receipt.json"),
        &json!({
            "schema": "chaptera.t352-exact-four-arm-patch.v1",
            "base_sha256": base_sha,
            "base_file_len": base_raw.len(),
            "selected_contents_seq_num": 293,
            "selected_spid": 1025,
            "width_before_emu": BASE_WIDTH_EMU,
            "width_after_emu": BASE_WIDTH_EMU + DELTA_EMU,
            "delta_emu": DELTA_EMU,
            "source_mutated": false,
            "arms": summaries
        }),
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let words: Vec<String> = args
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    match words.as_slice() {
        [command, base, receipt, output] if command == "prepare" => {
            prepare(Path::new(base), Path::new(receipt), Path::new(output))
        }
        [command, source, output] if command == "fingerprint" => {
            write_json(Path::new(output), &fingerprint(Path::new(source))?)
        }
        _ => bail!(
            "usage: val_xproj_patch prepare BASE.pub STRUCTURAL.json NEW_DIR | fingerprint FILE.pub OUT.json"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_width_and_anchor_delta() {
        assert_eq!(BASE_WIDTH_EMU + DELTA_EMU, 5_203_000);
        assert_eq!(BASE_XE_EMU + DELTA_EMU, 2_503_000);
        assert_eq!(
            uint32_bytes(BASE_WIDTH_EMU).unwrap(),
            5_076_000u32.to_le_bytes()
        );
    }
    #[test]
    fn tampered_structural_contract_fails_closed() {
        let r = json!({"schema": "chaptera.modern-structural-base/v1",
            "stream_count": 9, "candidate_count": 6,
            "selected_target": {}});
        assert!(plan(&r).is_err());
    }
}
