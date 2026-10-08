use anyhow::{bail, Context, Result};
use pub_cfb::read_stream_path;
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference, parse_confirmed_mature_color_scheme,
    Contents0x2cChunkReference, MatureColorScheme,
};
use pub_core::StreamPath;
use pub_escher::inspect_sp_containers;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

const RAW_TYPE_COLOR_SCHEME: u16 = 0x5C;
const OFFICE_ART_FILL_COLOR: u16 = 0x0181;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn one_raw_type(reference: &Contents0x2cChunkReference) -> Option<u16> {
    match reference.raw_types.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn current_color_scheme(contents: &[u8]) -> Result<(usize, MatureColorScheme)> {
    let stream = StreamPath("/Contents".to_owned());
    let header =
        parse_0x2c_header(stream.clone(), contents).context("parse mature Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature Contents trailer")?;

    let mut matches = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse Contents reference seq {seq_num}"))?
        else {
            continue;
        };
        if one_raw_type(&reference) == Some(RAW_TYPE_COLOR_SCHEME) {
            matches.push(reference);
        }
    }

    let reference = match matches.as_slice() {
        [reference] => reference,
        [] => bail!("missing unique raw0x5C/OplSccm reference"),
        many => bail!("multiple raw0x5C/OplSccm references: {}", many.len()),
    };
    let offset = match reference.chunk_offsets.as_slice() {
        [offset] => offset.value,
        offsets => bail!(
            "raw0x5C/OplSccm seq {} has {} chunk offsets",
            reference.seq_num,
            offsets.len()
        ),
    };
    let chunk = parse_confirmed_0x2c_chunk(stream, contents, offset)
        .with_context(|| format!("parse raw0x5C/OplSccm seq {}", reference.seq_num))?;
    let scheme = parse_confirmed_mature_color_scheme(contents, &chunk)
        .with_context(|| format!("decode raw0x5C/OplSccm seq {}", reference.seq_num))?;
    Ok((reference.seq_num, scheme))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: scheme_color_oracle_receipt SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: scheme_color_oracle_receipt SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("scheme_color_oracle_receipt accepts exactly SOURCE.pub OUTPUT.json");
    }

    let pub_bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let contents =
        read_stream_path(&source, "/Contents").context("read Publisher /Contents stream")?;
    let escher =
        read_stream_path(&source, "/Escher/EscherStm").context("read Publisher Escher stream")?;

    let (scheme_seq_num, scheme) = current_color_scheme(&contents)?;
    let escher_inventory =
        inspect_sp_containers(StreamPath("/Escher/EscherStm".to_owned()), &escher)
            .context("inspect OfficeArt shape containers")?;

    let slots = scheme
        .slots
        .iter()
        .map(|slot| {
            json!({
                "ordinal": slot.ordinal,
                "rgb": slot.rgb,
                "source": {
                    "offset": slot.source.offset,
                    "len": slot.source.len,
                },
                "rgb_source": slot.rgb_source.as_ref().map(|source| json!({
                    "offset": source.offset,
                    "len": source.len,
                })),
            })
        })
        .collect::<Vec<_>>();

    let mut scheme_fills = Vec::new();
    for (shape_ordinal, shape) in escher_inventory.shapes.iter().enumerate() {
        for fopt in &shape.fopts {
            for property in &fopt.properties {
                if property.property_id() != OFFICE_ART_FILL_COLOR
                    || property.f_bid()
                    || property.f_complex()
                    || (property.op >> 24) as u8 != 0x08
                {
                    continue;
                }
                let ordinal = usize::try_from(property.op & 0x00FF_FFFF)
                    .context("scheme ordinal does not fit usize")?;
                let resolved_rgb = scheme.slots.get(ordinal).and_then(|slot| slot.rgb);
                scheme_fills.push(json!({
                    "shape_ordinal": shape_ordinal,
                    "spid": shape.fsp.as_ref().map(|fsp| fsp.spid),
                    "fopt_rec_type": fopt.rec_type,
                    "has_client_anchor": shape.client_anchor.is_some(),
                    "raw_colorref": property.op,
                    "scheme_ordinal": ordinal,
                    "resolved_rgb": resolved_rgb,
                    "property_source": {
                        "offset": property.source.offset,
                        "len": property.source.len,
                    },
                    "shape_source": {
                        "offset": shape.source.offset,
                        "len": shape.source.len,
                    },
                }));
            }
        }
    }

    let receipt = json!({
        "schema": "chaptera.viewer-scheme-color-raw-oracle.v1",
        "source_sha256": sha256_hex(&pub_bytes),
        "byte_len": pub_bytes.len(),
        "contents_sha256": sha256_hex(&contents),
        "escher_sha256": sha256_hex(&escher),
        "oplsccm": {
            "raw_type": RAW_TYPE_COLOR_SCHEME,
            "seq_num": scheme_seq_num,
            "declared_count": scheme.declared_count,
            "name": scheme.name,
            "source": {
                "offset": scheme.source.offset,
                "len": scheme.source.len,
            },
            "slots": slots,
        },
        "officeart": {
            "shape_container_count": escher_inventory.shapes.len(),
            "scheme_fill_count": scheme_fills.len(),
            "scheme_fills": scheme_fills,
        },
    });

    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize scheme-color oracle receipt")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
