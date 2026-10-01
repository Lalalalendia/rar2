use anyhow::{Context, Result};
use pub_core::StreamPath;
use pub_quill::{parse_bounded_typography, parse_confirmed_story_catalog};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Cursor};

const CONTENTS_STREAM_PATH: &str = "/Contents";
const QUILL_STREAM_PATH: &str = "/Quill/QuillSub/CONTENTS";
const ESCHER_STREAM_PATH: &str = "/Escher/EscherStm";
const ESCHER_DELAY_STREAM_PATH: &str = "/Escher/EscherDelayStm";
const MAX_DIFF_RUNS: usize = 64;
const MAX_HEX_BYTES_PER_SIDE: usize = 32;

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_stream(pub_bytes: &[u8], path: &str) -> Result<Vec<u8>> {
    pub_cfb::read_stream_reader(Cursor::new(pub_bytes), path)
        .with_context(|| format!("read CFB stream {path}"))
}

fn stream_digest(pub_bytes: &[u8], path: &str) -> Value {
    match read_stream(pub_bytes, path) {
        Ok(bytes) => json!({
            "present": true,
            "byte_len": bytes.len(),
            "sha256": sha256_hex(&bytes),
        }),
        Err(error) => json!({
            "present": false,
            "error_class": error.to_string(),
        }),
    }
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(MAX_HEX_BYTES_PER_SIDE)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn byte_diff_runs(left: &[u8], right: &[u8]) -> Vec<Value> {
    let common = left.len().min(right.len());
    let mut runs = Vec::new();
    let mut index = 0usize;

    while index < common && runs.len() < MAX_DIFF_RUNS {
        if left[index] == right[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < common && left[index] != right[index] {
            index += 1;
        }
        let end = index;
        let left_slice = &left[start..end];
        let right_slice = &right[start..end];
        runs.push(json!({
            "offset": start,
            "length": end - start,
            "left_hex_prefix": hex_prefix(left_slice),
            "right_hex_prefix": hex_prefix(right_slice),
            "hex_truncated": left_slice.len() > MAX_HEX_BYTES_PER_SIDE,
        }));
    }

    if runs.len() < MAX_DIFF_RUNS && left.len() != right.len() {
        let start = common;
        let left_tail = &left[start..];
        let right_tail = &right[start..];
        runs.push(json!({
            "offset": start,
            "length_left": left_tail.len(),
            "length_right": right_tail.len(),
            "left_hex_prefix": hex_prefix(left_tail),
            "right_hex_prefix": hex_prefix(right_tail),
            "hex_truncated": left_tail.len().max(right_tail.len()) > MAX_HEX_BYTES_PER_SIDE,
        }));
    }

    runs
}

fn typography_summary(quill: &[u8]) -> Result<Value> {
    let story_catalog = parse_confirmed_story_catalog(StreamPath(QUILL_STREAM_PATH.into()), quill)
        .context("parse confirmed Quill Story catalog")?;
    let typography = parse_bounded_typography(quill, &story_catalog)
        .context("parse bounded Quill typography")?;

    let stories = story_catalog
        .stories
        .iter()
        .map(|story| {
            json!({
                "story_index": story.index,
                "story_syid": story.syid.0,
                "utf16_code_units": story.utf16_code_units,
                "text_sha256": sha256_hex(&story.utf16le),
            })
        })
        .collect::<Vec<_>>();

    let ranges = typography
        .ranges
        .iter()
        .map(|range| {
            let intersections = range
                .story_intersections
                .iter()
                .map(|intersection| {
                    json!({
                        "story_index": intersection.story_index,
                        "story_syid": intersection.story_syid.0,
                        "global_start_utf16": intersection.global_start_utf16,
                        "global_end_utf16": intersection.global_end_utf16,
                        "story_start_utf16": intersection.story_start_utf16,
                        "story_end_utf16": intersection.story_end_utf16,
                    })
                })
                .collect::<Vec<_>>();
            let script_fonts = range
                .script_fonts
                .iter()
                .map(|entry| {
                    json!({
                        "script_slot": entry.script_slot,
                        "font_index": entry.font_index,
                        "disposition": entry.disposition,
                        "source": entry.source,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "global_start_utf16": range.global_start_utf16,
                "global_end_utf16": range.global_end_utf16,
                "fdpc_descriptor_ordinal": range.fdpc_descriptor_ordinal,
                "fdpc_style_ordinal": range.fdpc_style_ordinal,
                "fdpc_style_source": range.fdpc_style_source,
                "text_offset_source": range.text_offset_source,
                "font_indices": range.font_indices,
                "script_fonts": script_fonts,
                "text_sizes_emu": range.text_sizes_emu,
                "story_intersections": intersections,
            })
        })
        .collect::<Vec<_>>();

    let explicit_runs = typography
        .explicit_runs
        .iter()
        .map(|run| {
            json!({
                "story_index": run.story_index,
                "story_syid": run.story_syid.0,
                "story_start_utf16": run.story_start_utf16,
                "story_end_utf16": run.story_end_utf16,
                "font_index": run.font_index,
                "text_size_emu": run.text_size_emu,
                "fdpc_descriptor_ordinal": run.fdpc_descriptor_ordinal,
                "fdpc_style_ordinal": run.fdpc_style_ordinal,
                "fdpc_style_source": run.fdpc_style_source,
            })
        })
        .collect::<Vec<_>>();

    let effective_runs = typography
        .effective_runs
        .iter()
        .map(|run| {
            json!({
                "story_index": run.story_index,
                "story_syid": run.story_syid.0,
                "story_start_utf16": run.story_start_utf16,
                "story_end_utf16": run.story_end_utf16,
                "font_index": run.font_index,
                "font_source": run.font_source,
                "text_size_emu": run.text_size_emu,
                "text_size_source": run.text_size_source,
                "inherited_style_index": run.inherited_style_index,
                "inherited_selector_source": run.inherited_selector_source,
                "fdpc_descriptor_ordinal": run.fdpc_descriptor_ordinal,
                "fdpc_style_ordinal": run.fdpc_style_ordinal,
                "fdpc_style_source": run.fdpc_style_source,
                "fdpp_style_source": run.fdpp_style_source,
                "stsh_character_default_source": run.stsh_character_default_source,
            })
        })
        .collect::<Vec<_>>();

    let script_font_maps = typography
        .script_font_maps
        .iter()
        .map(|map| {
            let entries = map
                .entries
                .iter()
                .map(|entry| {
                    json!({
                        "script_slot": entry.script_slot,
                        "font_index": entry.font_index,
                        "disposition": entry.disposition,
                        "source": entry.source,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "story_index": map.story_index,
                "story_syid": map.story_syid.0,
                "story_start_utf16": map.story_start_utf16,
                "story_end_utf16": map.story_end_utf16,
                "fdpc_descriptor_ordinal": map.fdpc_descriptor_ordinal,
                "fdpc_style_ordinal": map.fdpc_style_ordinal,
                "fdpc_style_source": map.fdpc_style_source,
                "entries": entries,
            })
        })
        .collect::<Vec<_>>();

    let font_name_fingerprints = typography
        .font_names
        .iter()
        .map(|name| sha256_hex(name.as_bytes()))
        .collect::<Vec<_>>();

    Ok(json!({
        "story_count": stories.len(),
        "stories": stories,
        "font_catalog_count": typography.font_names.len(),
        "font_name_fingerprints_sha256": font_name_fingerprints,
        "ranges": ranges,
        "explicit_runs": explicit_runs,
        "effective_runs": effective_runs,
        "script_font_maps": script_font_maps,
        "unknown_block_types_assumed_zero_length": typography.unknown_block_types_assumed_zero_length,
        "inheritance_unknown_block_types_assumed_zero_length": typography.inheritance_unknown_block_types_assumed_zero_length,
        "effective_inheritance_unavailable_reason": typography.effective_inheritance_unavailable_reason,
    }))
}

fn analyze(pub_bytes: &[u8]) -> Result<Value> {
    let quill = read_stream(pub_bytes, QUILL_STREAM_PATH)?;
    Ok(json!({
        "source_sha256": sha256_hex(pub_bytes),
        "source_byte_len": pub_bytes.len(),
        "streams": {
            "contents": stream_digest(pub_bytes, CONTENTS_STREAM_PATH),
            "quill": stream_digest(pub_bytes, QUILL_STREAM_PATH),
            "escher": stream_digest(pub_bytes, ESCHER_STREAM_PATH),
            "escher_delay": stream_digest(pub_bytes, ESCHER_DELAY_STREAM_PATH),
        },
        "typography": typography_summary(&quill)?,
    }))
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let left_path = args.next().context("left PUB path missing")?;
    let right_path = args.next().context("right PUB path missing")?;
    if args.next().is_some() {
        anyhow::bail!("usage: cmo7_size_pair_probe LEFT.pub RIGHT.pub");
    }

    let left = fs::read(&left_path).context("read left PUB")?;
    let right = fs::read(&right_path).context("read right PUB")?;
    let left_quill = read_stream(&left, QUILL_STREAM_PATH)?;
    let right_quill = read_stream(&right, QUILL_STREAM_PATH)?;

    let left_analysis = analyze(&left)?;
    let right_analysis = analyze(&right)?;

    let changed_control_streams = [
        ("contents", CONTENTS_STREAM_PATH),
        ("quill", QUILL_STREAM_PATH),
        ("escher", ESCHER_STREAM_PATH),
        ("escher_delay", ESCHER_DELAY_STREAM_PATH),
    ]
    .into_iter()
    .filter_map(|(name, path)| {
        let left_stream = read_stream(&left, path).ok();
        let right_stream = read_stream(&right, path).ok();
        if left_stream == right_stream {
            None
        } else {
            Some(name)
        }
    })
    .collect::<Vec<_>>();

    let receipt = json!({
        "schema": "chaptera.cmo7-size-pair-probe.v1",
        "scope": "measurement_only",
        "left": left_analysis,
        "right": right_analysis,
        "changed_control_streams": changed_control_streams,
        "quill_byte_diff_runs": byte_diff_runs(&left_quill, &right_quill),
        "quill_diff_run_limit": MAX_DIFF_RUNS,
        "hex_prefix_byte_limit_per_side": MAX_HEX_BYTES_PER_SIDE,
        "interpretation_fence": [
            "This receipt localizes persisted differences; it does not assign semantics by itself.",
            "Only a Publisher-authored one-factor pair may support a causal text-size claim.",
            "Do not infer text size from PDF pixels, browser geometry, fallback font metrics, or ScriptFonts family state."
        ],
    });

    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
