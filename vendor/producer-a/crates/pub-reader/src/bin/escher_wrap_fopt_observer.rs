use std::{env, fs, io::Cursor, path::PathBuf};

use anyhow::{bail, Context, Result};
use pub_core::StreamPath;
use pub_escher::{inspect_sp_containers, PUBLISHER_FIELD_SHAPE_ID};
use serde::Serialize;
use sha2::{Digest, Sha256};

const RECEIPT_SCHEMA: &str = "chaptera.escher-wrap-fopt-observer/v1";
const ESCHER_STREAM_PATH: &str = "/Escher/EscherStm";
const WRAP_PROPERTY_IDS: [u16; 4] = [900, 901, 902, 903];
const RECOLOR_PROPERTY_ID: u16 = 0x011A;

#[derive(Debug, Serialize)]
struct FoptProperty {
    property_id: u16,
    opid: u16,
    op: u32,
}

#[derive(Debug, Serialize)]
struct ShapeObservation {
    publisher_shape_id: Option<u32>,
    officeart_spid: Option<u32>,
    officeart_shape_type: Option<u16>,
    wrap_properties: Vec<FoptProperty>,
    recolor_properties: Vec<FoptProperty>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    shape_count: usize,
    observations: Vec<ShapeObservation>,
}

fn unique_publisher_shape_id(record: Option<&pub_escher::PublisherFieldRecord>) -> Option<u32> {
    let record = record?;
    let values = record.values(PUBLISHER_FIELD_SHAPE_ID).collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Some(*value),
        _ => None,
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .context("usage: escher_wrap_fopt_observer <input.pub> <output.json>")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: escher_wrap_fopt_observer <input.pub> <output.json>")?,
    );
    if args.next().is_some() {
        bail!("usage: escher_wrap_fopt_observer <input.pub> <output.json>");
    }

    let bytes = fs::read(&input).with_context(|| format!("read {}", input.display()))?;
    let source_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let escher = pub_cfb::read_stream_reader(Cursor::new(&bytes), ESCHER_STREAM_PATH)
        .with_context(|| format!("read {ESCHER_STREAM_PATH}"))?;
    let inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.into()), &escher)
        .context("inspect Escher SpContainers")?;

    let mut observations = Vec::with_capacity(inventory.shapes.len());
    for shape in &inventory.shapes {
        let mut wrap_properties = Vec::new();
        let mut recolor_properties = Vec::new();
        for fopt in &shape.fopts {
            for property in &fopt.properties {
                let property_id = property.opid & 0x3FFF;
                let observed = FoptProperty {
                    property_id,
                    opid: property.opid,
                    op: property.op,
                };
                if WRAP_PROPERTY_IDS.contains(&property_id) {
                    wrap_properties.push(observed);
                } else if property_id == RECOLOR_PROPERTY_ID {
                    recolor_properties.push(observed);
                }
            }
        }
        wrap_properties.sort_by_key(|property| (property.property_id, property.opid, property.op));
        recolor_properties
            .sort_by_key(|property| (property.property_id, property.opid, property.op));

        observations.push(ShapeObservation {
            publisher_shape_id: unique_publisher_shape_id(shape.client_data.as_ref()),
            officeart_spid: shape.fsp.as_ref().map(|fsp| fsp.spid),
            officeart_shape_type: shape.fsp.as_ref().map(|fsp| fsp.shape_type),
            wrap_properties,
            recolor_properties,
        });
    }

    let receipt = Receipt {
        schema: RECEIPT_SCHEMA,
        source_sha256,
        shape_count: observations.len(),
        observations,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize Escher wrap receipt")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
