use std::{env, fs, io::Cursor, path::PathBuf};

use anyhow::{Context, Result, bail};
use pub_core::StreamPath;
use pub_escher::inspect_sp_containers;
use serde::Serialize;

const CONTENTS_STREAM: &str = "/Contents";
const ESCHER_STREAM: &str = "/Escher/EscherStm";

#[derive(Debug, Serialize)]
struct ContentsReferenceDump {
    seq_num: usize,
    raw_types: Vec<u16>,
    chunk_hex: String,
}

#[derive(Debug, Serialize)]
struct ContentsDump {
    schema: &'static str,
    family: &'static str,
    serialization_revision: u16,
    references: Vec<ContentsReferenceDump>,
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}

fn span_bytes<'a>(bytes: &'a [u8], offset: u64, len: u64) -> Result<&'a [u8]> {
    let start = usize::try_from(offset).context("span offset does not fit usize")?;
    let len = usize::try_from(len).context("span length does not fit usize")?;
    let end = start
        .checked_add(len)
        .filter(|end| *end <= bytes.len())
        .context("span out of bounds")?;
    Ok(&bytes[start..end])
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .context("usage: false_omission_dump <input.pub> <contents.json> <escher.json>")?,
    );
    let contents_out = PathBuf::from(
        args.next()
            .context("usage: false_omission_dump <input.pub> <contents.json> <escher.json>")?,
    );
    let escher_out = PathBuf::from(
        args.next()
            .context("usage: false_omission_dump <input.pub> <contents.json> <escher.json>")?,
    );
    if args.next().is_some() {
        bail!("usage: false_omission_dump <input.pub> <contents.json> <escher.json>");
    }

    let pub_bytes = fs::read(&input).with_context(|| format!("read {}", input.display()))?;

    let contents = pub_cfb::read_stream_reader(Cursor::new(&pub_bytes), CONTENTS_STREAM)
        .with_context(|| format!("read {CONTENTS_STREAM}"))?;
    let contents_stream = StreamPath(CONTENTS_STREAM.into());
    let header = pub_contents::parse_0x2c_header(contents_stream.clone(), &contents)
        .context("parse mature 0x2C Contents header")?;
    let trailer = pub_contents::parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse mature 0x2C Contents trailer")?;

    let mut references = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            pub_contents::parse_confirmed_chunk_reference(&contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse Contents reference seq {seq_num}"))?
        else {
            continue;
        };

        let [offset] = reference.chunk_offsets.as_slice() else {
            continue;
        };
        let chunk = pub_contents::parse_confirmed_0x2c_chunk(
            contents_stream.clone(),
            &contents,
            offset.value,
        )
        .with_context(|| format!("parse Contents chunk seq {seq_num}"))?;
        let chunk_bytes = span_bytes(&contents, chunk.source.offset, chunk.source.len)?;

        references.push(ContentsReferenceDump {
            seq_num,
            raw_types: reference.raw_types.iter().map(|item| item.value).collect(),
            chunk_hex: hex(chunk_bytes),
        });
    }

    let contents_dump = ContentsDump {
        schema: "pub-false-omission-01/contents-dump/v1",
        family: "Family0x2c",
        serialization_revision: header.preamble.serialization_revision,
        references,
    };

    let escher = pub_cfb::read_stream_reader(Cursor::new(&pub_bytes), ESCHER_STREAM)
        .with_context(|| format!("read {ESCHER_STREAM}"))?;
    let escher_dump = inspect_sp_containers(StreamPath(ESCHER_STREAM.into()), &escher)
        .context("inspect Escher SpContainers")?;

    for output in [&contents_out, &escher_out] {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create output directory {}", parent.display()))?;
        }
    }

    fs::write(
        &contents_out,
        serde_json::to_vec_pretty(&contents_dump).context("serialize Contents dump")?,
    )
    .with_context(|| format!("write {}", contents_out.display()))?;
    fs::write(
        &escher_out,
        serde_json::to_vec_pretty(&escher_dump).context("serialize Escher dump")?,
    )
    .with_context(|| format!("write {}", escher_out.display()))?;

    Ok(())
}
