use anyhow::{Context, Result, bail};
use pub_core::StreamPath;
use pub_quill::{inspect_raw_fdpp_styles, parse_confirmed_story_catalog};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Cursor};

const STREAM: &str = "/Quill/QuillSub/CONTENTS";
const SCHEMA: &str = "chaptera.paragraph-metrics.quill-snapshot.v1";

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn span_bytes<'a>(bytes: &'a [u8], span: &pub_core::RawSpan) -> Result<&'a [u8]> {
    let start = usize::try_from(span.offset)?;
    let end = start
        .checked_add(usize::try_from(span.len)?)
        .context("span overflow")?;
    bytes.get(start..end).context("snapshot span out of bounds")
}

fn quill_snapshot(quill: &[u8]) -> Result<Value> {
    let catalog = parse_confirmed_story_catalog(StreamPath(STREAM.into()), quill)
        .context("confirmed Story catalog")?;
    let observations = inspect_raw_fdpp_styles(quill, &catalog).context("strict raw FDPP")?;
    let mut chunks = Vec::new();
    for (ordinal, descriptor) in catalog
        .descriptor_nodes
        .iter()
        .flat_map(|node| &node.descriptors)
        .enumerate()
    {
        let name = descriptor.name.value;
        if name != *b"FDPP" && name != *b"STSH" {
            continue;
        }
        let start = usize::try_from(descriptor.data_offset.value)?;
        let end = start
            .checked_add(usize::try_from(descriptor.data_length.value)?)
            .context("chunk overflow")?;
        let payload = quill.get(start..end).context("chunk out of bounds")?;
        chunks.push(json!({
            "name": if name == *b"FDPP" { "FDPP" } else { "STSH" },
            "descriptor_ordinal": ordinal,
            "byte_len": payload.len(),
            "sha256": sha256(payload),
        }));
    }
    let mut styles = Vec::new();
    for style in observations {
        let mut properties = Vec::new();
        for property in style.properties {
            let mut value = json!({
                "field_id": property.field_id,
                "wire_type": property.block_type,
                "raw_tag": property.raw_tag,
                "byte_len": property.source.len,
                "sha256": sha256(span_bytes(quill, &property.source)?),
            });
            // Only the declared candidate scalar is retained. Opaque property
            // payloads, text and style names never reach the snapshot.
            if matches!(property.field_id, 0x34 | 0x234) {
                value["raw_value"] = json!(property.scalar_value);
            }
            properties.push(value);
        }
        styles.push(json!({
            "descriptor_ordinal": style.descriptor_ordinal,
            "style_ordinal": style.style_ordinal,
            "start_utf16": style.global_start_utf16,
            "end_utf16": style.global_end_utf16,
            "sha256": sha256(span_bytes(quill, &style.source)?),
            "properties": properties,
        }));
    }
    Ok(json!({
        "schema": SCHEMA,
        "text": { "sha256": sha256(&catalog.text.bytes), "byte_len": catalog.text.bytes.len() },
        "stories": catalog.stories.iter().map(|story| json!({
            "index": story.index, "syid": story.syid.0, "utf16_len": story.utf16_code_units,
        })).collect::<Vec<_>>(),
        "chunks": chunks,
        "fdpp_styles": styles,
        "authority": "raw_observation_only",
    }))
}

fn snapshot(bytes: &[u8]) -> Result<Value> {
    let quill =
        pub_cfb::read_stream_reader(Cursor::new(bytes), STREAM).context("read Quill stream")?;
    let mut out = quill_snapshot(&quill)?;
    out["source"] = json!({ "sha256": sha256(bytes), "byte_len": bytes.len() });
    Ok(out)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let path = args
        .next()
        .context("usage: paragraph-metrics-probe <file.pub>")?;
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }
    let bytes = fs::read(path).context("read snapshot input")?;
    println!("{}", serde_json::to_string_pretty(&snapshot(&bytes)?)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn w16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn w32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture(raw_tag: [u8; 2], raw_value: u32) -> Vec<u8> {
        let mut bytes = vec![0; 0x180];
        w16(&mut bytes, 0x18, 0x18);
        w16(&mut bytes, 0x1a, 4);
        w32(&mut bytes, 0x1c, u32::MAX);
        for (index, (name, offset, length)) in [
            (*b"SYID", 0x100, 12),
            (*b"STRS", 0x110, 12),
            (*b"TEXT", 0x130, 6),
            (*b"FDPP", 0x140, 24),
        ]
        .into_iter()
        .enumerate()
        {
            let start = 0x20 + index * 24;
            w16(&mut bytes, start, 0x18);
            bytes[start + 2..start + 6].copy_from_slice(&name);
            bytes[start + 12..start + 16].copy_from_slice(&name);
            w32(&mut bytes, start + 16, offset);
            w32(&mut bytes, start + 20, length);
        }
        w32(&mut bytes, 0x104, 1);
        w32(&mut bytes, 0x108, 7);
        w32(&mut bytes, 0x110, 1);
        w32(&mut bytes, 0x114, 4);
        w32(&mut bytes, 0x118, 3);
        bytes[0x130..0x136].copy_from_slice(&[65, 0, 13, 0, 66, 0]);
        w16(&mut bytes, 0x140, 1);
        w32(&mut bytes, 0x148, 0x136);
        w16(&mut bytes, 0x14c, 14);
        w32(&mut bytes, 0x14e, 10);
        bytes[0x152..0x154].copy_from_slice(&raw_tag);
        w32(&mut bytes, 0x154, raw_value);
        bytes
    }

    #[test]
    fn reads_packed_candidate_without_promoting_units_or_text() {
        let quill = fixture([0x34, 0x22], 2_438_401);
        let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        compound.create_storage("/Quill").unwrap();
        compound.create_storage("/Quill/QuillSub").unwrap();
        compound
            .create_stream(STREAM)
            .unwrap()
            .write_all(&quill)
            .unwrap();
        let bytes = compound.into_inner().into_inner();
        let out = snapshot(&bytes).unwrap();
        let property = &out["fdpp_styles"][0]["properties"][0];
        assert_eq!(property["field_id"], 0x234);
        assert_eq!(property["wire_type"], 0x20);
        assert_eq!(property["raw_value"], 2_438_401);
        assert_eq!(out["text"]["sha256"], sha256(&quill[0x130..0x136]));
        assert_eq!(out["fdpp_styles"][0]["end_utf16"], 3);
        assert_eq!(out["authority"], "raw_observation_only");
        assert!(!out.to_string().contains("absolute_points"));
        assert!(!out.to_string().contains("utf16le"));
    }

    #[test]
    fn rejects_unknown_framing_truncation_and_nonclosing_text_range() {
        assert!(quill_snapshot(&fixture([0x34, 0x32], 1)).is_err());
        let mut bytes = fixture([0x34, 0x22], 1);
        w32(&mut bytes, 0x14e, 9);
        assert!(quill_snapshot(&bytes).is_err());
        let mut bytes = fixture([0x34, 0x22], 1);
        w32(&mut bytes, 0x148, 0x134);
        assert!(quill_snapshot(&bytes).is_err());
        let mut bytes = fixture([0x34, 0x22], 1);
        w32(&mut bytes, 0x148, 0x135);
        assert!(quill_snapshot(&bytes).is_err());
    }

    #[test]
    fn equal_length_text_mutation_changes_digest() {
        let a = fixture([0x34, 0x22], 1);
        let mut b = a.clone();
        b[0x130] = 67;
        let a = quill_snapshot(&a).unwrap();
        let b = quill_snapshot(&b).unwrap();
        assert_eq!(a["text"]["byte_len"], b["text"]["byte_len"]);
        assert_ne!(a["text"]["sha256"], b["text"]["sha256"]);
    }
}
