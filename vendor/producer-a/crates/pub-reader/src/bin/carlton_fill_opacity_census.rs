use anyhow::{Context, Result, bail};
use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_escher::{
    Fopte, inspect_dgg_default_options, inspect_sp_containers,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
};

const FILL_COLOR: u16 = 0x0181;
const FILL_OPACITY: u16 = 0x0182;
const LINE_OPACITY: u16 = 0x01C1;
const SCHEME_COLOR_TAG: u8 = 0x08;
const ACCENT1_ORDINAL: u32 = 1;

#[derive(Debug, Default, Serialize)]
struct Counts {
    shape_containers: usize,
    fill_color_observations: usize,
    accent1_shapes: usize,
    fill_opacity_observations: usize,
    fill_opacity_shapes: usize,
    line_opacity_observations: usize,
    line_opacity_shapes: usize,
    accent1_with_fill_opacity_shapes: usize,
    accent1_without_fill_opacity_shapes: usize,
    dgg_fill_opacity_observations: usize,
    dgg_line_opacity_observations: usize,
}

#[derive(Debug, Default, Serialize)]
struct Histograms {
    fill_opacity_raw_hex: BTreeMap<String, usize>,
    accent1_fill_opacity_raw_hex: BTreeMap<String, usize>,
    line_opacity_raw_hex: BTreeMap<String, usize>,
    dgg_fill_opacity_raw_hex: BTreeMap<String, usize>,
    dgg_line_opacity_raw_hex: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    byte_len: usize,
    counts: Counts,
    histograms: Histograms,
    guardrails: Vec<&'static str>,
}

fn scalar_property(entry: &Fopte, property_id: u16) -> Option<u32> {
    (entry.property_id() == property_id && !entry.f_bid() && !entry.f_complex())
        .then_some(entry.op)
}

fn accent1_fill(entry: &Fopte) -> bool {
    scalar_property(entry, FILL_COLOR).is_some_and(|raw| {
        (raw >> 24) as u8 == SCHEME_COLOR_TAG
            && (raw & 0x00FF_FFFF) == ACCENT1_ORDINAL
    })
}

fn bump(histogram: &mut BTreeMap<String, usize>, raw: u32) {
    *histogram.entry(format!("0x{raw:08X}")).or_default() += 1;
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: carlton_fill_opacity_census SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: carlton_fill_opacity_census SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("carlton_fill_opacity_census accepts exactly SOURCE.pub OUTPUT.json");
    }

    let pub_bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let escher = read_stream_path(&source, "/Escher/EscherStm")
        .context("read Publisher Escher stream")?;

    let shapes = inspect_sp_containers(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .context("inspect OfficeArt shape containers")?;
    let dgg = inspect_dgg_default_options(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .context("inspect OfficeArt DGG defaults")?;

    let mut counts = Counts {
        shape_containers: shapes.shapes.len(),
        ..Counts::default()
    };
    let mut histograms = Histograms::default();

    for shape in &shapes.shapes {
        let mut accent1 = false;
        let mut fill_opacity_values = Vec::new();
        let mut line_opacity_values = Vec::new();

        for fopt in &shape.fopts {
            for entry in &fopt.properties {
                if scalar_property(entry, FILL_COLOR).is_some() {
                    counts.fill_color_observations += 1;
                    accent1 |= accent1_fill(entry);
                }
                if let Some(raw) = scalar_property(entry, FILL_OPACITY) {
                    counts.fill_opacity_observations += 1;
                    fill_opacity_values.push(raw);
                    bump(&mut histograms.fill_opacity_raw_hex, raw);
                }
                if let Some(raw) = scalar_property(entry, LINE_OPACITY) {
                    counts.line_opacity_observations += 1;
                    line_opacity_values.push(raw);
                    bump(&mut histograms.line_opacity_raw_hex, raw);
                }
            }
        }

        if accent1 {
            counts.accent1_shapes += 1;
            if fill_opacity_values.is_empty() {
                counts.accent1_without_fill_opacity_shapes += 1;
            } else {
                counts.accent1_with_fill_opacity_shapes += 1;
                for raw in &fill_opacity_values {
                    bump(&mut histograms.accent1_fill_opacity_raw_hex, *raw);
                }
            }
        }
        if !fill_opacity_values.is_empty() {
            counts.fill_opacity_shapes += 1;
        }
        if !line_opacity_values.is_empty() {
            counts.line_opacity_shapes += 1;
        }
    }

    for drawing_group in &dgg.drawing_groups {
        for fopt in drawing_group
            .primary_options
            .iter()
            .chain(drawing_group.tertiary_options.iter())
        {
            for entry in &fopt.properties {
                if let Some(raw) = scalar_property(entry, FILL_OPACITY) {
                    counts.dgg_fill_opacity_observations += 1;
                    bump(&mut histograms.dgg_fill_opacity_raw_hex, raw);
                }
                if let Some(raw) = scalar_property(entry, LINE_OPACITY) {
                    counts.dgg_line_opacity_observations += 1;
                    bump(&mut histograms.dgg_line_opacity_raw_hex, raw);
                }
            }
        }
    }

    let receipt = Receipt {
        schema: "chaptera.carlton-fill-opacity-census.v1",
        source_sha256: sha256_hex(&pub_bytes),
        byte_len: pub_bytes.len(),
        counts,
        histograms,
        guardrails: vec![
            "Raw 0x0182 and 0x01C1 values are observations, not semantic opacity claims.",
            "No opacity/transparency inversion, quantization, or omission law is inferred here.",
            "No source text, filenames, paths, object ids, stream offsets, or raw bytes are emitted.",
            "Product promotion remains gated by SHAPE-TRANSPARENCY-AUTH-01.",
        ],
    };

    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize opacity census")?,
    )
    .with_context(|| format!("write {}", output.display()))?;

    println!(
        "CARLTON_FILL_OPACITY_CENSUS shapes={} accent1={} accent1_with_fill_opacity={} fill_opacity_shapes={} line_opacity_shapes={} dgg_fill_opacity={}",
        receipt.counts.shape_containers,
        receipt.counts.accent1_shapes,
        receipt.counts.accent1_with_fill_opacity_shapes,
        receipt.counts.fill_opacity_shapes,
        receipt.counts.line_opacity_shapes,
        receipt.counts.dgg_fill_opacity_observations,
    );
    Ok(())
}
