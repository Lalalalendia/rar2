use std::{env, fs, io::Cursor, path::PathBuf};

use anyhow::{bail, Context, Result};
use pub_core::StreamPath;
use pub_escher::{inspect_sp_containers, PUBLISHER_FIELD_SHAPE_ID};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "chaptera.escher-fopt-census/v1";
const ESCHER_STREAM_PATH: &str = "/Escher/EscherStm";

#[derive(Debug, Serialize)]
struct PropertyObservation {
    rec_type: u16,
    property_id: u16,
    opid: u16,
    op: u32,
    f_bid: bool,
    f_complex: bool,
    op_is_blip_id: bool,
    complex_len: Option<usize>,
    complex_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
struct ShapeObservation {
    publisher_shape_id: Option<u32>,
    officeart_spid: Option<u32>,
    officeart_shape_type: Option<u16>,
    fopt_record_count: usize,
    property_count: usize,
    properties: Vec<PropertyObservation>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    shape_count: usize,
    shapes: Vec<ShapeObservation>,
}

fn unique_publisher_shape_id(record: Option<&pub_escher::PublisherFieldRecord>) -> Option<u32> {
    let record = record?;
    let values = record.values(PUBLISHER_FIELD_SHAPE_ID).collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Some(*value),
        _ => None,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .context("usage: escher_fopt_census <input.pub> <output.json>")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: escher_fopt_census <input.pub> <output.json>")?,
    );
    if args.next().is_some() {
        bail!("usage: escher_fopt_census <input.pub> <output.json>");
    }

    let bytes = fs::read(&input).with_context(|| format!("read {}", input.display()))?;
    let source_sha256 = sha256_hex(&bytes);
    let escher = pub_cfb::read_stream_reader(Cursor::new(&bytes), ESCHER_STREAM_PATH)
        .with_context(|| format!("read {ESCHER_STREAM_PATH}"))?;
    let inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.into()), &escher)
        .context("inspect Escher SpContainers")?;

    let mut shapes = Vec::with_capacity(inventory.shapes.len());
    for shape in &inventory.shapes {
        let mut properties = Vec::new();
        for fopt in &shape.fopts {
            for property in &fopt.properties {
                properties.push(PropertyObservation {
                    rec_type: fopt.rec_type,
                    property_id: property.property_id(),
                    opid: property.opid,
                    op: property.op,
                    f_bid: property.f_bid(),
                    f_complex: property.f_complex(),
                    op_is_blip_id: property.op_is_blip_id(),
                    complex_len: property.complex_data.as_ref().map(Vec::len),
                    complex_sha256: property
                        .complex_data
                        .as_deref()
                        .map(sha256_hex),
                });
            }
        }
        properties.sort_by_key(|property| {
            (
                property.rec_type,
                property.property_id,
                property.opid,
                property.op,
            )
        });

        shapes.push(ShapeObservation {
            publisher_shape_id: unique_publisher_shape_id(shape.client_data.as_ref()),
            officeart_spid: shape.fsp.as_ref().map(|fsp| fsp.spid),
            officeart_shape_type: shape.fsp.as_ref().map(|fsp| fsp.shape_type),
            fopt_record_count: shape.fopts.len(),
            property_count: properties.len(),
            properties,
        });
    }

    shapes.sort_by_key(|shape| {
        (
            shape.publisher_shape_id.unwrap_or(u32::MAX),
            shape.officeart_spid.unwrap_or(u32::MAX),
        )
    });

    let receipt = Receipt {
        schema: SCHEMA,
        source_sha256,
        shape_count: shapes.len(),
        shapes,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize FOPT census")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
