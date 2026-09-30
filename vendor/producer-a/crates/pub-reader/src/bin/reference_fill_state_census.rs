use anyhow::{bail, Context, Result};
use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_escher::{inspect_dgg_default_options, inspect_sp_containers, Fopte};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const FILL_TYPE: u16 = 0x0180;
const FILL_COLOR: u16 = 0x0181;
const FILL_BOOLEANS: u16 = 0x01BF;
const FILL_USE_FILLED_BIT: u32 = 1 << 11;
const FILL_FILLED_BIT: u32 = 1 << 27;
const FSP_CONNECTOR_BIT: u32 = 1 << 8;
const SHAPE_TYPE_NOT_PRIMITIVE: u16 = 0x0000;
const SHAPE_TYPE_LINE: u16 = 0x0014;

#[derive(Debug, Default, Serialize)]
struct Counts {
    shape_containers: usize,
    fill_type_observations: usize,
    fill_color_observations: usize,
    fill_boolean_observations: usize,
    fill_boolean_use_zero: usize,
    fill_boolean_use_one: usize,
    fill_boolean_used_false: usize,
    fill_boolean_used_true: usize,
    effective_solid_visible: usize,
    effective_solid_hidden: usize,
    effective_non_solid: usize,
    effective_unresolved: usize,
    dgg_fill_type_observations: usize,
    dgg_fill_color_observations: usize,
    dgg_fill_boolean_observations: usize,
}

#[derive(Debug, Default, Serialize)]
struct Histograms {
    fill_type_raw_hex: BTreeMap<String, usize>,
    fill_color_class: BTreeMap<String, usize>,
    fill_scheme_ordinal: BTreeMap<String, usize>,
    fill_boolean_raw_hex: BTreeMap<String, usize>,
    cooccurrence: BTreeMap<String, usize>,
    dgg_fill_type_raw_hex: BTreeMap<String, usize>,
    dgg_fill_color_class: BTreeMap<String, usize>,
    dgg_fill_boolean_raw_hex: BTreeMap<String, usize>,
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
    (entry.property_id() == property_id && !entry.f_bid() && !entry.f_complex()).then_some(entry.op)
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn color_class(raw: u32) -> &'static str {
    match (raw >> 24) as u8 {
        0x00 => "direct_rgb",
        0x08 => "scheme",
        0x10 => "system_or_extended",
        _ => "other_flagged",
    }
}

fn shape_admits_normative_2d_defaults(shape: &pub_escher::SpContainerObservation) -> bool {
    shape.fsp.as_ref().is_some_and(|fsp| {
        fsp.shape_type != SHAPE_TYPE_NOT_PRIMITIVE
            && fsp.shape_type != SHAPE_TYPE_LINE
            && fsp.flags & FSP_CONNECTOR_BIT == 0
    })
}

fn unique_scalar(
    shape: &pub_escher::SpContainerObservation,
    property_id: u16,
) -> Option<Option<u32>> {
    let values = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter_map(|entry| scalar_property(entry, property_id))
        .collect::<Vec<_>>();
    match values.as_slice() {
        [] => Some(None),
        [value] => Some(Some(*value)),
        _ => None,
    }
}

fn effective_fill_state(shape: &pub_escher::SpContainerObservation) -> &'static str {
    if !shape_admits_normative_2d_defaults(shape) {
        return "unresolved";
    }
    let Some(fill_type) = unique_scalar(shape, FILL_TYPE) else {
        return "unresolved";
    };
    let Some(fill_bool) = unique_scalar(shape, FILL_BOOLEANS) else {
        return "unresolved";
    };
    if fill_type.unwrap_or(0) != 0 {
        return "non_solid";
    }
    let visible = match fill_bool {
        Some(raw) if raw & FILL_USE_FILLED_BIT != 0 => raw & FILL_FILLED_BIT != 0,
        _ => true,
    };
    if visible {
        "solid_visible"
    } else {
        "solid_hidden"
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: reference_fill_state_census SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: reference_fill_state_census SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("reference_fill_state_census accepts exactly SOURCE.pub OUTPUT.json");
    }

    let pub_bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let escher =
        read_stream_path(&source, "/Escher/EscherStm").context("read Publisher Escher stream")?;
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
        let mut fill_type = None;
        let mut fill_color = None;
        let mut fill_bool = None;
        for fopt in &shape.fopts {
            for entry in &fopt.properties {
                if let Some(raw) = scalar_property(entry, FILL_TYPE) {
                    counts.fill_type_observations += 1;
                    fill_type = Some(raw);
                    bump(&mut histograms.fill_type_raw_hex, format!("0x{raw:08X}"));
                }
                if let Some(raw) = scalar_property(entry, FILL_COLOR) {
                    counts.fill_color_observations += 1;
                    fill_color = Some(raw);
                    bump(&mut histograms.fill_color_class, color_class(raw));
                    if (raw >> 24) as u8 == 0x08 {
                        bump(
                            &mut histograms.fill_scheme_ordinal,
                            (raw & 0x00FF_FFFF).to_string(),
                        );
                    }
                }
                if let Some(raw) = scalar_property(entry, FILL_BOOLEANS) {
                    counts.fill_boolean_observations += 1;
                    fill_bool = Some(raw);
                    bump(&mut histograms.fill_boolean_raw_hex, format!("0x{raw:08X}"));
                    if raw & FILL_USE_FILLED_BIT == 0 {
                        counts.fill_boolean_use_zero += 1;
                    } else {
                        counts.fill_boolean_use_one += 1;
                        if raw & FILL_FILLED_BIT == 0 {
                            counts.fill_boolean_used_false += 1;
                        } else {
                            counts.fill_boolean_used_true += 1;
                        }
                    }
                }
            }
        }

        let type_bucket =
            fill_type.map_or("type_absent".to_owned(), |raw| format!("type_0x{raw:08X}"));
        let color_bucket = fill_color.map_or("color_absent", color_class);
        let bool_bucket = match fill_bool {
            None => "filled_absent",
            Some(raw) if raw & FILL_USE_FILLED_BIT == 0 => "filled_use0",
            Some(raw) if raw & FILL_FILLED_BIT == 0 => "filled_false",
            Some(_) => "filled_true",
        };
        bump(
            &mut histograms.cooccurrence,
            format!("{type_bucket}|{color_bucket}|{bool_bucket}"),
        );

        match effective_fill_state(shape) {
            "solid_visible" => counts.effective_solid_visible += 1,
            "solid_hidden" => counts.effective_solid_hidden += 1,
            "non_solid" => counts.effective_non_solid += 1,
            _ => counts.effective_unresolved += 1,
        }
    }

    for drawing_group in &dgg.drawing_groups {
        for fopt in drawing_group
            .primary_options
            .iter()
            .chain(drawing_group.tertiary_options.iter())
        {
            for entry in &fopt.properties {
                if let Some(raw) = scalar_property(entry, FILL_TYPE) {
                    counts.dgg_fill_type_observations += 1;
                    bump(
                        &mut histograms.dgg_fill_type_raw_hex,
                        format!("0x{raw:08X}"),
                    );
                }
                if let Some(raw) = scalar_property(entry, FILL_COLOR) {
                    counts.dgg_fill_color_observations += 1;
                    bump(&mut histograms.dgg_fill_color_class, color_class(raw));
                }
                if let Some(raw) = scalar_property(entry, FILL_BOOLEANS) {
                    counts.dgg_fill_boolean_observations += 1;
                    bump(
                        &mut histograms.dgg_fill_boolean_raw_hex,
                        format!("0x{raw:08X}"),
                    );
                }
            }
        }
    }

    let receipt = Receipt {
        schema: "chaptera.reference-fill-state-census.v1",
        source_sha256: sha256_hex(&pub_bytes),
        byte_len: pub_bytes.len(),
        counts,
        histograms,
        guardrails: vec![
            "Raw OfficeArt values are observations; this receipt does not infer Publisher authoring intent.",
            "The effective-state bucket mirrors only the current bounded solid/visibility admission law.",
            "No PDF pixels are used as parser or paint authority.",
            "No source text, object ids, paths, filenames, offsets, or raw bytes are emitted.",
        ],
    };
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize fill census")?,
    )
    .with_context(|| format!("write {}", output.display()))?;

    println!(
        "REFERENCE_FILL_STATE_CENSUS sha={} shapes={} solid_visible={} solid_hidden={} non_solid={} unresolved={} fill_types={} fill_booleans={}",
        receipt.source_sha256,
        receipt.counts.shape_containers,
        receipt.counts.effective_solid_visible,
        receipt.counts.effective_solid_hidden,
        receipt.counts.effective_non_solid,
        receipt.counts.effective_unresolved,
        receipt.counts.fill_type_observations,
        receipt.counts.fill_boolean_observations,
    );
    Ok(())
}
