use anyhow::{bail, Context, Result};
use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_quill::parse_confirmed_story_catalog;
use std::{collections::BTreeSet, env, path::PathBuf};

const FDPC: [u8; 4] = *b"FDPC";
const PL: [u8; 4] = *b"PL  ";
const VARIABLE_BLOCK_TYPES: [u8; 6] = [0xC0, 0x80, 0x88, 0x90, 0x98, 0xA0];

fn u16le(bytes: &[u8], at: usize, end: usize) -> Result<u16> {
    if at + 2 > end || at + 2 > bytes.len() {
        bail!("u16 out of bounds at 0x{at:x}");
    }
    Ok(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

fn u32le(bytes: &[u8], at: usize, end: usize) -> Result<u32> {
    if at + 4 > end || at + 4 > bytes.len() {
        bail!("u32 out of bounds at 0x{at:x}");
    }
    Ok(u32::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
    ]))
}

fn decode_tag(raw: [u8; 2]) -> (u16, u8) {
    let raw_type = raw[1];
    if raw_type & 0x07 == 0x02 {
        (
            u16::from(raw[0]) | (u16::from(raw_type & 0x07) << 8),
            raw_type & 0xF8,
        )
    } else {
        (u16::from(raw[0]), raw_type)
    }
}

fn block(bytes: &[u8], start: usize, limit: usize) -> Result<(u16, u8, Option<u32>, usize)> {
    if start + 2 > limit {
        bail!("block header out of bounds");
    }
    let (id, ty) = decode_tag([bytes[start], bytes[start + 1]]);
    let data = start + 2;
    if VARIABLE_BLOCK_TYPES.contains(&ty) {
        let declared = u32le(bytes, data, limit)? as usize;
        if declared < 4 {
            bail!("variable block length < 4");
        }
        let end = data.checked_add(declared).context("variable block overflow")?;
        if end > limit {
            bail!("variable block exceeds limit");
        }
        return Ok((id, ty, None, end));
    }
    let len = match ty {
        0x78 | 0x05 | 0x08 => 0,
        0x10 | 0x18 | 0x07 => 2,
        0x20 | 0x58 | 0x68 | 0x70 | 0xB8 => 4,
        0x28 => 8,
        0x38 => 16,
        0x48 => 24,
        _ => 0,
    };
    let end = data + len;
    if end > limit {
        bail!("fixed block exceeds limit");
    }
    let value = match len {
        2 => Some(u16le(bytes, data, limit)? as u32),
        4 => Some(u32le(bytes, data, limit)?),
        _ => None,
    };
    Ok((id, ty, value, end))
}

fn decode_utf16le(raw: &[u8]) -> String {
    let units = raw
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

fn main() -> Result<()> {
    let source = PathBuf::from(
        env::args_os()
            .nth(1)
            .context("usage: reference_text_color_discriminator SOURCE.pub")?,
    );
    if env::args_os().nth(2).is_some() {
        bail!("accepts exactly one source");
    }

    let quill =
        read_stream_path(&source, "/Quill/QuillSub/CONTENTS").context("read Quill stream")?;
    let catalog = parse_confirmed_story_catalog(
        StreamPath("/Quill/QuillSub/CONTENTS".into()),
        &quill,
    )
    .context("parse Quill story catalog")?;
    let descriptors = catalog
        .descriptor_nodes
        .iter()
        .flat_map(|node| node.descriptors.iter())
        .collect::<Vec<_>>();

    let targets = [
        "Together We Take Small Steps",
        "Reception",
        "Year 1",
        "Year 2",
    ];
    let mut target_story_indices = BTreeSet::new();
    for story in &catalog.stories {
        let text = decode_utf16le(&story.utf16le);
        if targets.iter().any(|target| text.contains(target)) {
            println!(
                "TEXT_COLOR_TARGET_STORY index={} syid={:?} utf16={} text={:?}",
                story.index, story.syid, story.utf16_code_units, text
            );
            target_story_indices.insert(story.index);
        }
    }

    let mut refs = Vec::new();
    for descriptor in descriptors
        .iter()
        .copied()
        .filter(|descriptor| descriptor.name.value == PL)
    {
        let start = descriptor.data_offset.value as usize;
        let end = start + descriptor.data_length.value as usize;
        let count = u32le(&quill, start, end)? as usize;
        let mut cursor = start + 12;
        println!(
            "TEXT_COLOR_PL descriptor_offset=0x{start:x} len={} count={count}",
            descriptor.data_length.value
        );
        for ordinal in 0..count {
            if cursor + 4 > end {
                bail!("PL record {ordinal} header exceeds chunk");
            }
            let record_start = cursor;
            let record_len = u32le(&quill, cursor, end)? as usize;
            let record_end = record_start
                .checked_add(record_len)
                .context("PL record overflow")?;
            if record_len < 4 || record_end > end {
                bail!("PL record {ordinal} invalid length {record_len}");
            }
            cursor += 4;
            while cursor < record_end {
                let (id, ty, value, next) = block(&quill, cursor, record_end)?;
                if id == 1 {
                    if let Some(raw) = value {
                        refs.push(raw);
                        println!(
                            "TEXT_COLOR_PL_REF ordinal={ordinal} type=0x{ty:02X} raw=0x{raw:08X} high=0x{:02X}",
                            raw >> 24
                        );
                    }
                }
                if next <= cursor {
                    bail!("PL parser did not advance");
                }
                cursor = next;
            }
            if cursor != record_end {
                bail!("PL record {ordinal} did not close");
            }
        }
    }
    println!("TEXT_COLOR_PL_TOTAL refs={} values={:X?}", refs.len(), refs);

    let text_start = catalog.text.source.offset as u32;
    let mut story_global_starts = Vec::new();
    let mut global = 0_u32;
    for story in &catalog.stories {
        story_global_starts.push((
            story.index,
            global,
            global + story.utf16_code_units,
        ));
        global += story.utf16_code_units;
    }

    for descriptor in descriptors
        .iter()
        .copied()
        .filter(|descriptor| descriptor.name.value == FDPC)
    {
        let start = descriptor.data_offset.value as usize;
        let end = start + descriptor.data_length.value as usize;
        let count = u16le(&quill, start, end)? as usize;
        let offsets_start = start + 8;
        let style_offsets_start = offsets_start + count * 4;
        let body_start = style_offsets_start + count * 2;
        let mut previous = 0_u32;

        for ordinal in 0..count {
            let absolute_end = u32le(&quill, offsets_start + ordinal * 4, end)?;
            if absolute_end < text_start || (absolute_end - text_start) % 2 != 0 {
                continue;
            }
            let global_end = (absolute_end - text_start) / 2;
            let relative = u16le(&quill, style_offsets_start + ordinal * 2, end)? as usize;
            let style_start = start + relative;
            if style_start < body_start || style_start + 4 > end {
                bail!("FDPC style offset invalid");
            }
            let style_len = u32le(&quill, style_start, end)? as usize;
            let style_end = style_start + style_len;
            if style_len < 4 || style_end > end {
                bail!("FDPC style length invalid");
            }
            let intersects_target = story_global_starts.iter().any(|(index, story_start, story_end)| {
                target_story_indices.contains(index)
                    && previous < *story_end
                    && global_end > *story_start
            });
            if intersects_target {
                println!(
                    "TEXT_COLOR_FDPC_RANGE ordinal={ordinal} global={previous}..{global_end} style=0x{style_start:x}..0x{style_end:x}"
                );
                let mut cursor = style_start + 4;
                while cursor < style_end {
                    let (id, ty, value, next) = block(&quill, cursor, style_end)?;
                    println!(
                        "TEXT_COLOR_FDPC_BLOCK ordinal={ordinal} at=0x{cursor:x} id=0x{id:03X} type=0x{ty:02X} value={}",
                        value
                            .map(|raw| format!("0x{raw:08X}"))
                            .unwrap_or_else(|| "-".into())
                    );
                    if next <= cursor {
                        bail!("FDPC parser did not advance");
                    }
                    cursor = next;
                }
            }
            previous = global_end;
        }
    }

    Ok(())
}
