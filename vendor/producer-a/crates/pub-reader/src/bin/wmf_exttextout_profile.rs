//! Measurement-only: inspect mature Publisher WMF EXTTEXTOUT wire without
//! exposing any source text, font family name, handle ID, or image bytes.
use pub_model::Sha256Digest;
use pub_reader::{
    build_mature_0x2c_source_graph, build_mature_0x2c_wmf_preview_bundle_from_bytes,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, error::Error, fs, io::Cursor};

const EXTTEXTOUT: u16 = 0x0a32;
const MAX_RECORDS: usize = 100_000;
const MAX_WMF_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
struct Font {
    params_len: usize,
    height: i16,
    width: i16,
    escapement: i16,
    orientation: i16,
    weight: u16,
    charset: u8,
    out_precision: u8,
    clip_precision: u8,
    quality: u8,
    pitch_family: u8,
}
#[derive(Clone, Copy, Debug)]
enum Object {
    Font(Font),
    Other,
}
#[derive(Default)]
struct Graphics {
    selected_font: Option<Font>,
    text_color: Option<u32>,
    background_color: Option<u32>,
    background_mode: Option<u16>,
    text_align: Option<u16>,
    window_origin: Option<(i16, i16)>,
    window_extent: Option<(i16, i16)>,
    map_mode: Option<u16>,
}

fn u16_at(b: &[u8], i: usize) -> Result<u16, Box<dyn Error>> {
    Ok(u16::from_le_bytes(b.get(i..i + 2).ok_or("short u16")?.try_into()?))
}
fn i16_at(b: &[u8], i: usize) -> Result<i16, Box<dyn Error>> {
    Ok(u16_at(b, i)? as i16)
}
fn u32_at(b: &[u8], i: usize) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_le_bytes(b.get(i..i + 4).ok_or("short u32")?.try_into()?))
}

fn font_profile(params: &[u8]) -> Result<Font, Box<dyn Error>> {
    if params.len() < 18 {
        return Err("short LOGFONT profile".into());
    }
    Ok(Font {
        params_len: params.len(),
        height: i16_at(params, 0)?,
        width: i16_at(params, 2)?,
        escapement: i16_at(params, 4)?,
        orientation: i16_at(params, 6)?,
        weight: u16_at(params, 8)?,
        charset: params[13],
        out_precision: params[14],
        clip_precision: params[15],
        quality: params[16],
        pitch_family: params[17],
    })
}
fn classify_exttextout(params: &[u8], g: &Graphics) -> serde_json::Value {
    if params.len() < 8 {
        return json!({"wire_state":"truncated_fixed_fields","param_len":params.len()});
    }
    let y = i16_at(params, 0).unwrap_or_default();
    let x = i16_at(params, 2).unwrap_or_default();
    let string_len_signed = i16_at(params, 4).unwrap_or_default();
    let flags = u16_at(params, 6).unwrap_or_default();
    let has_rect = flags & 0x0006 != 0;
    let rect_bytes = if has_rect { 8 } else { 0 };
    let start = 8 + rect_bytes;
    let mut wire_state = "valid";
    let mut padding = None;
    let mut dx_count = None;
    let mut glyph_span_complete = false;
    let mut high_byte_count = None;
    let mut zero_byte_count = None;
    let mut rect_extent = None;
    if has_rect && params.len() >= 16 {
        let l = i16_at(params, 8).unwrap_or_default();
        let t = i16_at(params, 10).unwrap_or_default();
        let r = i16_at(params, 12).unwrap_or_default();
        let b = i16_at(params, 14).unwrap_or_default();
        rect_extent = Some((i32::from(r) - i32::from(l), i32::from(b) - i32::from(t)));
    }
    if string_len_signed < 0 || start > params.len() {
        wire_state = "invalid_length_or_rectangle";
    } else {
        let n = string_len_signed as usize;
        let padded = n + (n & 1);
        if let Some(bytes) = params.get(start..start.saturating_add(n)) {
            glyph_span_complete = true;
            high_byte_count = Some(bytes.iter().filter(|c| **c >= 0x80).count());
            zero_byte_count = Some(bytes.iter().filter(|c| **c == 0).count());
            if let Some(end) = start.checked_add(padded) {
                if end <= params.len() {
                    padding = Some(padded - n);
                    let remaining = params.len() - end;
                    if remaining % 2 == 0 {
                        dx_count = Some(remaining / 2);
                        if remaining != 0 && remaining != 2 * n {
                            wire_state = "unexpected_dx_length";
                        }
                    } else {
                        wire_state = "odd_dx_payload";
                    }
                } else {
                    wire_state = "missing_alignment_padding";
                }
            } else {
                wire_state = "string_end_overflow";
            }
        } else {
            wire_state = "truncated_string";
        }
    }
    let f = g.selected_font;
    json!({
        "wire_state":wire_state,
        "param_len": params.len(),
        "text_length":string_len_signed,
        "opts":flags,
        "has_clip_or_opaque_rect":has_rect,
        "rectangle_extent":rect_extent,
        "string_start":start,
        "string_span_complete":glyph_span_complete,
        "padding_bytes":padding,
        "dx_count":dx_count,
        "high_byte_count":high_byte_count,
        "zero_byte_count":zero_byte_count,
        "logical_x":x,
        "logical_y":y,
        "font_selected":f.is_some(),
        "font_profile":f.map(|f| json!({
            "len":f.params_len,"height":f.height,"width":f.width,
            "escapement":f.escapement,"orientation":f.orientation,"weight":f.weight,
            "charset":f.charset,"out_precision":f.out_precision,
            "clip_precision":f.clip_precision,"quality":f.quality,"pitch_family":f.pitch_family,
        })),
        "map_mode":g.map_mode,
        "window_origin":g.window_origin,
        "window_extent":g.window_extent,
        "text_color_set":g.text_color.is_some(),
        "text_color_is_black":g.text_color == Some(0),
        "background_color_set":g.background_color.is_some(),
        "background_mode":g.background_mode,
        "text_align":g.text_align
    })
}
fn census(wmf: &[u8]) -> Result<serde_json::Value, Box<dyn Error>> {
    if wmf.len() > MAX_WMF_BYTES || wmf.len() < 18 {
        return Err("WMF input exceeds bounded probe".into());
    }
    if u16_at(wmf, 0)? != 1 || u16_at(wmf, 2)? != 9 || u16_at(wmf, 4)? != 0x0300
        || usize::try_from(u32_at(wmf, 6)?)?.checked_mul(2) != Some(wmf.len())
    {
        return Err("WMF header is not the bounded native profile".into());
    }
    let object_count = usize::from(u16_at(wmf, 10)?);
    if object_count > 4096 {
        return Err("WMF object table too large".into());
    }
    let mut objects = vec![None; object_count];
    let mut graphics = Graphics::default();
    let mut font_lengths = BTreeMap::<usize, usize>::new();
    let mut functions = BTreeMap::<u16, usize>::new();
    let mut text_rows = Vec::new();
    let mut offset = 18usize;
    let mut saw_eof = false;
    let mut count = 0usize;
    while offset < wmf.len() {
        count += 1;
        if count > MAX_RECORDS {
            return Err("WMF record cap exceeded".into());
        }
        let words = usize::try_from(u32_at(wmf, offset)?)?;
        if words < 3 { return Err("WMF short record".into()); }
        let len = words.checked_mul(2).ok_or("WMF record size overflow")?;
        let end = offset.checked_add(len).ok_or("WMF record end overflow")?;
        let params = wmf.get(offset + 6..end).ok_or("WMF record truncated")?;
        let kind = u16_at(wmf, offset + 4)?;
        *functions.entry(kind).or_default() += 1;
        if saw_eof { return Err("record after EOF".into()); }
        match kind {
            0x02fb => {
                let f = font_profile(params)?;
                *font_lengths.entry(f.params_len).or_default() += 1;
                let target = objects.iter_mut().find(|x| x.is_none()).ok_or("WMF object table full")?;
                *target = Some(Object::Font(f));
            }
            0x02fa | 0x02fc | 0x00f7 | 0x0142 | 0x06ff => {
                let target = objects.iter_mut().find(|x| x.is_none()).ok_or("WMF object table full")?;
                *target = Some(Object::Other);
            }
            0x012d if params.len() >= 2 => {
                let index = usize::from(u16_at(params, 0)?);
                if let Some(Object::Font(f)) = objects.get(index).and_then(|v| *v) {
                    graphics.selected_font = Some(f);
                }
            }
            0x01f0 if params.len() >= 2 => {
                let index = usize::from(u16_at(params, 0)?);
                if let Some(value) = objects.get_mut(index) { *value = None; }
            }
            0x0103 if params.len() >= 2 => graphics.map_mode = Some(u16_at(params, 0)?),
            0x0102 if params.len() >= 2 => graphics.background_mode = Some(u16_at(params, 0)?),
            0x012e if params.len() >= 2 => graphics.text_align = Some(u16_at(params, 0)?),
            0x0201 if params.len() >= 4 => graphics.background_color = Some(u32_at(params, 0)?),
            0x0209 if params.len() >= 4 => graphics.text_color = Some(u32_at(params, 0)?),
            0x020b if params.len() >= 4 => graphics.window_origin = Some((i16_at(params, 2)?, i16_at(params, 0)?)),
            0x020c if params.len() >= 4 => graphics.window_extent = Some((i16_at(params, 2)?, i16_at(params, 0)?)),
            EXTTEXTOUT => text_rows.push(classify_exttextout(params, &graphics)),
            0 if len == 6 => saw_eof = true,
            _ => {}
        }
        offset = end;
    }
    if !saw_eof || offset != wmf.len() { return Err("WMF EOF not final".into()); }
    Ok(json!({
        "record_count":count,
        "function_histogram":functions,
        "font_lengths":font_lengths,
        "text_records":text_rows,
        "full_consumption":true,
    }))
}
fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let sha = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&sha);
    Sha256Digest::from_bytes(raw)
}
fn main() -> Result<(), Box<dyn Error>> {
    let input = env::args().nth(1).ok_or("usage: wmf_exttextout_profile INPUT.pub")?;
    let bytes = fs::read(&input)?;
    if bytes.len() > 16 * 1024 * 1024 { return Err("PUB input too large".into()); }
    let hash = source_hash(&bytes);
    let graph = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), hash)?;
    let bundle = build_mature_0x2c_wmf_preview_bundle_from_bytes(&bytes, &graph.graph)?;
    let mut rows = Vec::new();
    for item in &bundle.sources {
        let details = census(&item.wmf_bytes)?;
        rows.push(json!({
            "uses":item.uses.len(),
            "record_len":item.wmf_bytes.len(),
            "census":details,
        }));
    }
    println!("WMF_EXTTEXTOUT_PROFILE {}", serde_json::to_string(&json!({
        "schema":"chaptera.wmf-exttextout-wire.v1",
        "pub_sha256":hash.to_string(),
        "wmf_source_count":bundle.sources.len(),
        "wmf_use_count":bundle.sources.iter().map(|r|r.uses.len()).sum::<usize>(),
        "rejected_source_count":bundle.rejected_source_count,
        "physical_wmf_record_count":bundle.physical_wmf_record_count,
        "rows":rows,
    }))?);
    Ok(())
}
