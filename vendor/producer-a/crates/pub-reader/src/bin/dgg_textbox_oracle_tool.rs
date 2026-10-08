use std::{collections::BTreeSet, env, fs, io::Cursor, path::PathBuf};

use anyhow::{Context, Result};
use pub_cfb::{read_stream_reader, replace_stream_reader};
use pub_core::StreamPath;
use pub_escher::{inspect_dgg_default_options, inspect_sp_containers, PUBLISHER_FIELD_SHAPE_ID};
use serde::Serialize;
use sha2::{Digest, Sha256};

const ESCHER_STREAM: &str = "/Escher/EscherStm";
const SHAPE_TEXTBOX: u16 = 0x00CA;
const FILL_COLOR: u16 = 0x0181;
const FILL_BOOLEANS: u16 = 0x01BF;
const FILL_USE_FILLED_BIT: u32 = 1 << 20;
const FILL_FILLED_BIT: u32 = 1 << 4;

#[derive(Debug, Serialize)]
struct SparseTextboxCandidate {
    publisher_shape_id: u32,
    officeart_spid: u32,
    local_fill_color_absent: bool,
    local_filled_participates_true: bool,
}

#[derive(Debug, Serialize)]
struct ProfileReceipt {
    schema: &'static str,
    source_sha256: String,
    dgg_primary_fill_color_count: usize,
    dgg_primary_scalar_fill_color_count: usize,
    dgg_primary_fill_color_op: Option<u32>,
    sparse_textbox_candidates: Vec<SparseTextboxCandidate>,
}

#[derive(Debug, Serialize)]
struct PatchReceipt {
    schema: &'static str,
    source_sha256: String,
    output_sha256: String,
    target_property_id: &'static str,
    changed_stream: &'static str,
    changed_byte_count: usize,
    old_op: u32,
    new_op: u32,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn scalar_fill_color_properties(
    dgg: &pub_escher::DggDefaultOptionsObservation,
) -> Vec<&pub_escher::Fopte> {
    dgg.primary_options
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_COLOR)
        .filter(|property| !property.f_bid() && !property.f_complex())
        .collect()
}

fn profile(input: &PathBuf, output: &PathBuf) -> Result<()> {
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let escher = read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_STREAM)
        .context("read EscherStm")?;

    let dgg_inventory = inspect_dgg_default_options(StreamPath(ESCHER_STREAM.into()), &escher)
        .context("inspect DGG defaults")?;
    anyhow::ensure!(
        dgg_inventory.drawing_groups.len() == 1,
        "expected exactly one DGG, got {}",
        dgg_inventory.drawing_groups.len()
    );
    let dgg = &dgg_inventory.drawing_groups[0];
    let fill_color_all = dgg
        .primary_options
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_COLOR)
        .collect::<Vec<_>>();
    let fill_color_scalar = scalar_fill_color_properties(dgg);

    let shapes = inspect_sp_containers(StreamPath(ESCHER_STREAM.into()), &escher)
        .context("inspect shape containers")?;
    let mut candidates = Vec::new();
    for shape in &shapes.shapes {
        let Some(fsp) = shape.fsp.as_ref() else {
            continue;
        };
        if fsp.shape_type != SHAPE_TEXTBOX {
            continue;
        }

        let local_fill_color_count = shape
            .fopts
            .iter()
            .flat_map(|record| record.properties.iter())
            .filter(|property| property.property_id() == FILL_COLOR)
            .count();
        if local_fill_color_count != 0 {
            continue;
        }

        let mut participating_true = 0usize;
        let mut participating_false = 0usize;
        let mut malformed = 0usize;
        for property in shape
            .fopts
            .iter()
            .flat_map(|record| record.properties.iter())
            .filter(|property| property.property_id() == FILL_BOOLEANS)
        {
            if property.f_bid() || property.f_complex() {
                malformed += 1;
                continue;
            }
            if property.op & FILL_USE_FILLED_BIT == 0 {
                continue;
            }
            if property.op & FILL_FILLED_BIT != 0 {
                participating_true += 1;
            } else {
                participating_false += 1;
            }
        }
        let filled_true = participating_true == 1 && participating_false == 0 && malformed == 0;
        if !filled_true {
            continue;
        }

        let shape_ids = shape
            .client_data
            .as_ref()
            .map(|record| {
                record
                    .values(PUBLISHER_FIELD_SHAPE_ID)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        if shape_ids.len() != 1 {
            continue;
        }

        candidates.push(SparseTextboxCandidate {
            publisher_shape_id: *shape_ids.iter().next().expect("one shape id"),
            officeart_spid: fsp.spid,
            local_fill_color_absent: true,
            local_filled_participates_true: true,
        });
    }
    candidates.sort_by_key(|candidate| candidate.publisher_shape_id);

    let receipt = ProfileReceipt {
        schema: "chaptera.publisher-dgg-textbox-profile.v1",
        source_sha256: sha256_hex(&bytes),
        dgg_primary_fill_color_count: fill_color_all.len(),
        dgg_primary_scalar_fill_color_count: fill_color_scalar.len(),
        dgg_primary_fill_color_op: (fill_color_scalar.len() == 1)
            .then_some(fill_color_scalar[0].op),
        sparse_textbox_candidates: candidates,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}

fn patch_dgg_fill_color(input: &PathBuf, output: &PathBuf, receipt_path: &PathBuf) -> Result<()> {
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let escher = read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_STREAM)
        .context("read EscherStm")?;
    let inventory = inspect_dgg_default_options(StreamPath(ESCHER_STREAM.into()), &escher)
        .context("inspect DGG defaults")?;
    anyhow::ensure!(
        inventory.drawing_groups.len() == 1,
        "expected exactly one DGG, got {}",
        inventory.drawing_groups.len()
    );
    let dgg = &inventory.drawing_groups[0];
    let candidates = scalar_fill_color_properties(dgg);
    anyhow::ensure!(
        candidates.len() == 1,
        "expected exactly one scalar DGG-primary fillColor, got {}",
        candidates.len()
    );
    let property = candidates[0];
    anyhow::ensure!(property.source.stream.0 == ESCHER_STREAM);
    anyhow::ensure!(property.source.len == 6);

    let old_op = property.op;
    let new_op: u32 = if old_op == 0x0000_00FF {
        0x0000_FF00
    } else {
        0x0000_00FF
    };

    let mut patched_escher = escher.clone();
    let op_start = usize::try_from(property.source.offset)
        .context("FOPTE offset does not fit usize")?
        .checked_add(2)
        .context("FOPTE op offset overflow")?;
    let op_end = op_start.checked_add(4).context("FOPTE op end overflow")?;
    anyhow::ensure!(op_end <= patched_escher.len());
    patched_escher[op_start..op_end].copy_from_slice(&new_op.to_le_bytes());

    let changed_byte_count = escher
        .iter()
        .zip(&patched_escher)
        .filter(|(before, after)| before != after)
        .count();
    anyhow::ensure!(
        (1..=4).contains(&changed_byte_count),
        "DGG patch changed unexpected byte count {changed_byte_count}"
    );

    let reparsed = inspect_dgg_default_options(StreamPath(ESCHER_STREAM.into()), &patched_escher)
        .context("reparse patched DGG defaults")?;
    anyhow::ensure!(reparsed.drawing_groups.len() == 1);
    let after = scalar_fill_color_properties(&reparsed.drawing_groups[0]);
    anyhow::ensure!(after.len() == 1 && after[0].op == new_op);

    let output_bytes = replace_stream_reader(
        Cursor::new(bytes.as_slice()),
        ESCHER_STREAM,
        &patched_escher,
    )
    .context("replace EscherStm in bounded CFB copy")?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(output, &output_bytes).with_context(|| format!("write {}", output.display()))?;

    let receipt = PatchReceipt {
        schema: "chaptera.publisher-dgg-fill-color-patch.v1",
        source_sha256: sha256_hex(&bytes),
        output_sha256: sha256_hex(&output_bytes),
        target_property_id: "0x0181",
        changed_stream: ESCHER_STREAM,
        changed_byte_count,
        old_op,
        new_op,
    };
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", receipt_path.display()))?;
    Ok(())
}

fn main() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    match args.as_slice() {
        [_, command, input, output] if command == "profile" => {
            profile(&PathBuf::from(input), &PathBuf::from(output))
        }
        [_, command, input, output, receipt] if command == "patch-dgg-fill-color" => {
            patch_dgg_fill_color(
                &PathBuf::from(input),
                &PathBuf::from(output),
                &PathBuf::from(receipt),
            )
        }
        _ => anyhow::bail!(
            "usage: dgg_textbox_oracle_tool profile INPUT.pub PROFILE.json | patch-dgg-fill-color INPUT.pub OUTPUT.pub RECEIPT.json"
        ),
    }
}
