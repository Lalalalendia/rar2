//! Strictly bounded source-backed EMF solid-fill path preview.
//!
//! This is deliberately NOT a generic EMF renderer. Only the fully classified
//! MM_ANISOTROPIC + solid GDI brush + Begin/End/Close/FillPath + LineTo and
//! PolyBezierTo(16) profile is admitted. Unsupported state/painters reject the
//! entire vector candidate, rather than silently omitting visible operations.
//! The produced RGBA is a derived *Partial* preview, never source-exact.
use super::logical_emf_bytes;
use crate::{
    ESCHER_DELAY_STREAM_PATH, ESCHER_STREAM_PATH, PubAssetUse, PubSourceGraph,
    build_pub_asset_manifest,
};
use anyhow::{Context, Result, bail};
use pub_core::StreamPath;
use pub_escher::{BlipKind, inspect_validated_delayed_blips_prefix};
use std::collections::BTreeMap;
use std::io::Cursor;

const MAX_RECORDS: usize = 4_096;
const MAX_VECTOR_BYTES: usize = 8 * 1024 * 1024;
const MAX_POINTS: usize = 4_096;
const MAX_OBJECTS: usize = 256;
const MAX_SIDE: u32 = 1_024;
const MAX_PIXELS: u64 = 1_048_576;
const MAX_FILL_WORK: u64 = 16 * 1024 * 1024;
const MM_ANISOTROPIC: u32 = 8;
const ALTERNATE: u32 = 1;
const WINDING: u32 = 2;

pub const MATURE_EMF_VECTOR_SOLID_PATH_PARTIAL_V1: &str =
    "mature-officeart-emf-solid-fill-path-partial-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubMatureEmfVectorPreviewSource {
    pub slot: u32,
    pub uses: Vec<PubAssetUse>,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub filled_path_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PubMatureEmfVectorPreviewBundle {
    pub sources: Vec<PubMatureEmfVectorPreviewSource>,
    pub skipped_bitmap_count: usize,
    pub rejected_source_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pt {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Debug)]
struct Raster {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    fill_work: u64,
    painted: bool,
}

#[derive(Debug)]
struct State {
    window_origin: Pt,
    window_extent: Pt,
    viewport_origin: Pt,
    viewport_extent: Pt,
    map_mode: Option<u32>,
    fill_mode: u32,
    brushes: BTreeMap<u32, Color>,
    brush: Option<Color>,
    current: Option<Pt>,
    path_active: bool,
    path_ready: bool,
    subpaths: Vec<Vec<Pt>>,
    filled_paths: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            window_origin: Pt { x: 0.0, y: 0.0 },
            window_extent: Pt { x: 1.0, y: 1.0 },
            viewport_origin: Pt { x: 0.0, y: 0.0 },
            viewport_extent: Pt { x: 1.0, y: 1.0 },
            map_mode: None,
            fill_mode: ALTERNATE,
            brushes: BTreeMap::new(),
            brush: None,
            current: None,
            path_active: false,
            path_ready: false,
            subpaths: Vec::new(),
            filled_paths: 0,
        }
    }
}

fn span(bytes: &[u8], at: usize, count: usize) -> Result<&[u8]> {
    let end = at.checked_add(count).context("EMF vector record offset overflow")?;
    bytes.get(at..end).context("EMF vector record truncated")
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(span(bytes, at, 4)?.try_into()?))
}
fn i32_at(bytes: &[u8], at: usize) -> Result<i32> {
    Ok(u32_at(bytes, at)? as i32)
}
fn i16_at(bytes: &[u8], at: usize) -> Result<i16> {
    Ok(i16::from_le_bytes(span(bytes, at, 2)?.try_into()?))
}
fn pt_at(bytes: &[u8], at: usize) -> Result<Pt> {
    Ok(Pt {
        x: f64::from(i32_at(bytes, at)?),
        y: f64::from(i32_at(bytes, at + 4)?),
    })
}
fn color_ref(v: u32) -> Result<Color> {
    if v & 0xff00_0000 != 0 {
        bail!("unsupported EMF brush COLORREF type");
    }
    Ok(Color {
        r: v as u8,
        g: (v >> 8) as u8,
        b: (v >> 16) as u8,
    })
}
fn stock_brush(id: u32) -> Result<Option<Color>> {
    let shade = match id {
        0 => 255,
        1 => 192,
        2 => 128,
        3 => 64,
        4 => 0,
        5 => return Ok(None),
        _ => bail!("unsupported EMF stock brush"),
    };
    Ok(Some(Color {
        r: shade,
        g: shade,
        b: shade,
    }))
}

impl State {
    fn map(&self, logical: Pt, raster: &Raster, bounds: [i32; 4]) -> Result<Pt> {
        if self.map_mode != Some(MM_ANISOTROPIC)
            || self.window_extent.x == 0.0
            || self.window_extent.y == 0.0
        {
            bail!("unsupported EMF mapping mode or zero window extent");
        }
        let px = (logical.x - self.window_origin.x) * self.viewport_extent.x
            / self.window_extent.x
            + self.viewport_origin.x;
        let py = (logical.y - self.window_origin.y) * self.viewport_extent.y
            / self.window_extent.y
            + self.viewport_origin.y;
        let w = f64::from(bounds[2] - bounds[0]);
        let h = f64::from(bounds[3] - bounds[1]);
        let x = (px - f64::from(bounds[0])) * f64::from(raster.width) / w;
        let y = (py - f64::from(bounds[1])) * f64::from(raster.height) / h;
        if !x.is_finite()
            || !y.is_finite()
            || x.abs() > f64::from(MAX_SIDE) * 16.0
            || y.abs() > f64::from(MAX_SIDE) * 16.0
        {
            bail!("mapped EMF vector point exceeds bounded canvas");
        }
        Ok(Pt { x, y })
    }

    fn append(&mut self, point: Pt) -> Result<()> {
        if !self.path_active || self.path_ready {
            bail!("EMF vector paint command outside active path bracket");
        }
        if self.subpaths.is_empty() {
            let from = self.current.context("EMF missing initial MoveToEx")?;
            self.subpaths.push(vec![from]);
        }
        let total_points = self.subpaths.iter().map(Vec::len).sum::<usize>();
        if total_points >= MAX_POINTS {
            bail!("EMF vector path exceeds point budget");
        }
        self.subpaths.last_mut().context("missing EMF subpath")?.push(point);
        self.current = Some(point);
        Ok(())
    }

    fn start_figure(&mut self, point: Pt) -> Result<()> {
        if self.path_active {
            if self.subpaths.len() >= 128 {
                bail!("EMF path exceeds figure budget");
            }
            self.subpaths.push(vec![point]);
        }
        self.current = Some(point);
        Ok(())
    }

    fn close_figure(&mut self) -> Result<()> {
        if !self.path_active || self.path_ready {
            bail!("EMF close figure outside active path");
        }
        let path = self.subpaths.last_mut().context("no active EMF subpath")?;
        let first = *path.first().context("empty EMF subpath")?;
        if path.last().copied() != Some(first) {
            path.push(first);
        }
        self.current = Some(first);
        Ok(())
    }
}

impl Raster {
    fn new(bounds: [i32; 4]) -> Result<Self> {
        let w = i64::from(bounds[2]) - i64::from(bounds[0]);
        let h = i64::from(bounds[3]) - i64::from(bounds[1]);
        if w <= 0 || h <= 0 || w > 1_000_000 || h > 1_000_000 {
            bail!("invalid bounded EMF header bounds");
        }
        let scale = (f64::from(MAX_SIDE) / w as f64)
            .min(f64::from(MAX_SIDE) / h as f64)
            .min(1.0);
        let width = (w as f64 * scale).round().max(1.0) as u32;
        let height = (h as f64 * scale).round().max(1.0) as u32;
        let pixel_count = u64::from(width) * u64::from(height);
        if pixel_count > MAX_PIXELS || width == 0 || height == 0 {
            bail!("EMF vector raster exceeds output pixel budget");
        }
        Ok(Self {
            width,
            height,
            rgba: vec![0; usize::try_from(pixel_count * 4)?],
            fill_work: 0,
            painted: false,
        })
    }

    fn fill(&mut self, figures: &[Vec<Pt>], color: Color, fill_mode: u32) -> Result<()> {
        let point_count = figures.iter().map(Vec::len).sum::<usize>();
        if point_count > MAX_POINTS {
            bail!("EMF path segment budget exceeded");
        }
        let y_min = figures.iter().flatten().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let y_max = figures.iter().flatten().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        if !y_min.is_finite() || !y_max.is_finite() {
            bail!("EMF path has no finite fill geometry");
        }
        let top = (y_min.floor() as i32).max(0);
        let bottom = (y_max.ceil() as i32).min(self.height as i32);
        let work = u64::try_from(point_count)?
            .checked_mul(u64::try_from((bottom - top).max(0))?)
            .context("EMF vector raster work overflow")?;
        self.fill_work = self.fill_work.checked_add(work).context("EMF fill work overflow")?;
        if self.fill_work > MAX_FILL_WORK {
            bail!("EMF vector raster exceeds fill work budget");
        }
        for y in top..bottom {
            let yc = f64::from(y) + 0.5;
            let mut crossings = Vec::<(f64, i32)>::new();
            for path in figures {
                if path.len() < 3 {
                    continue;
                }
                for index in 0..path.len() {
                    let a = path[index];
                    let b = path[(index + 1) % path.len()];
                    if (a.y <= yc && yc < b.y) || (b.y <= yc && yc < a.y) {
                        let x = a.x + (yc - a.y) * (b.x - a.x) / (b.y - a.y);
                        if x.is_finite() {
                            crossings.push((x, if b.y > a.y { 1 } else { -1 }));
                        }
                    }
                }
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut previous = None;
            let mut winding = 0_i32;
            for (x, delta) in crossings {
                if let Some(left) = previous {
                    let should_fill = if fill_mode == ALTERNATE {
                        winding % 2 != 0
                    } else {
                        winding != 0
                    };
                    if should_fill {
                        let start = ((left - 0.5).ceil() as i32).max(0);
                        let end = ((x - 0.5).floor() as i32).min(self.width as i32 - 1);
                        for col in start..=end {
                            let at = ((y as usize * self.width as usize) + col as usize) * 4;
                            self.rgba[at..at + 4].copy_from_slice(&[color.r, color.g, color.b, 255]);
                            self.painted = true;
                        }
                    }
                }
                winding = if fill_mode == ALTERNATE {
                    winding + 1
                } else {
                    winding + delta
                };
                previous = Some(x);
            }
        }
        Ok(())
    }
}

fn parse_vector_emf(logical: &[u8]) -> Result<Option<(Raster, usize)>> {
    if logical.len() < 108 || logical.len() > MAX_VECTOR_BYTES {
        bail!("EMF vector file outside bounded record profile");
    }
    let mut state = State::default();
    let mut raster = None;
    let mut bounds = [0_i32; 4];
    let mut at = 0_usize;
    let mut count = 0_usize;
    let mut eof = false;
    let mut skipped_bitmap = false;
    while at < logical.len() {
        if count >= MAX_RECORDS {
            bail!("EMF vector record budget exceeded");
        }
        let kind = u32_at(logical, at)?;
        let size = usize::try_from(u32_at(logical, at + 4)?)?;
        if size < 8 || size % 4 != 0 {
            bail!("EMF vector record misframed");
        }
        let r = span(logical, at, size)?;
        if eof { bail!("EMF has trailing records after EOF"); }
        match kind {
            1 if count == 0 && size >= 88 => {
                bounds = [i32_at(r, 8)?, i32_at(r, 12)?, i32_at(r, 16)?, i32_at(r, 20)?];
                if u32_at(r, 40)? != 0x464d_4520 {
                    bail!("EMF header signature missing");
                }
                raster = Some(Raster::new(bounds)?);
            }
            14 if size >= 20 => eof = true,
            81 => skipped_bitmap = true,
            17 if size >= 12 => {
                let mode = u32_at(r, 8)?;
                if mode != MM_ANISOTROPIC { bail!("unsupported EMF map mode"); }
                state.map_mode = Some(mode);
            }
            19 if size >= 12 => {
                let mode = u32_at(r, 8)?;
                if mode != ALTERNATE && mode != WINDING {
                    bail!("unsupported EMF polygon fill mode");
                }
                state.fill_mode = mode;
            }
            9 | 11 if size >= 16 => {
                let extent = pt_at(r, 8)?;
                if extent.x <= 0.0 || extent.y <= 0.0 {
                    bail!("unsupported EMF negative/zero mapping extent");
                }
                if kind == 9 { state.window_extent = extent; }
                else { state.viewport_extent = extent; }
            }
            10 | 12 if size >= 16 => {
                let origin = pt_at(r, 8)?;
                if kind == 10 { state.window_origin = origin; }
                else { state.viewport_origin = origin; }
            }
            39 if size >= 24 => {
                let handle = u32_at(r, 8)?;
                if handle == 0 || handle & 0x8000_0000 != 0
                    || state.brushes.len() >= MAX_OBJECTS
                    || u32_at(r, 12)? != 0
                {
                    bail!("unsupported EMF brush object or style");
                }
                let color = color_ref(u32_at(r, 16)?)?;
                if state.brushes.insert(handle, color).is_some() {
                    bail!("EMF brush handle reused without delete");
                }
            }
            37 if size >= 12 => {
                let handle = u32_at(r, 8)?;
                if handle & 0x8000_0000 != 0 {
                    let stock = handle & 0x7fff_ffff;
                    if stock <= 5 {
                        state.brush = stock_brush(stock)?;
                    } else if !(6..=8).contains(&stock) {
                        bail!("unsupported selected EMF stock GDI object");
                    }
                } else {
                    state.brush = Some(*state.brushes.get(&handle)
                        .context("EMF selected undeclared brush")?);
                }
            }
            40 if size >= 12 => {
                if state.brushes.remove(&u32_at(r, 8)?).is_none() {
                    bail!("EMF deletes undeclared brush");
                }
            }
            59 if size == 8 => {
                if state.path_active || state.path_ready {
                    bail!("nested EMF path bracket");
                }
                state.path_active = true;
                state.subpaths.clear();
                state.current = None;
            }
            60 if size == 8 => {
                if !state.path_active { bail!("EMF EndPath without BeginPath"); }
                state.path_active = false;
                state.path_ready = true;
            }
            61 if size == 8 => state.close_figure()?,
            27 if size >= 16 => {
                let dst = state.map(pt_at(r, 8)?, raster.as_ref().context("missing EMF raster")?, bounds)?;
                state.start_figure(dst)?;
            }
            54 if size >= 16 => {
                let dst = state.map(pt_at(r, 8)?, raster.as_ref().context("missing EMF raster")?, bounds)?;
                state.append(dst)?;
            }
            5 | 88 if size >= 28 => {
                let n = usize::try_from(u32_at(r, 24)?)?;
                let step = if kind == 5 { 8 } else { 4 };
                if n == 0 || n > MAX_POINTS || n % 3 != 0
                    || n.checked_mul(step).and_then(|v| v.checked_add(28))
                        .is_none_or(|required| required > size)
                {
                    bail!("EMF Bezier segment count or record bounds invalid");
                }
                let mut start = state.current.context("EMF BezierTo lacks starting point")?;
                for segment in 0..(n / 3) {
                    let mut control = [Pt { x: 0.0, y: 0.0 }; 3];
                    for (point_index, dest) in control.iter_mut().enumerate() {
                        let base = 28 + (segment * 3 + point_index) * step;
                        let logical_pt = if kind == 5 {
                            pt_at(r, base)?
                        } else {
                            Pt { x: f64::from(i16_at(r, base)?),
                                y: f64::from(i16_at(r, base + 2)?) }
                        };
                        *dest = state.map(logical_pt, raster.as_ref().context("missing EMF raster")?, bounds)?;
                    }
                    let [a, b, end] = control;
                    // Piecewise cubic approximation, bounded and deterministic.
                    for k in 1..=16 {
                        let t = f64::from(k) / 16.0;
                        let q = 1.0 - t;
                        let p = Pt {
                            x: q*q*q*start.x + 3.0*q*q*t*a.x + 3.0*q*t*t*b.x + t*t*t*end.x,
                            y: q*q*q*start.y + 3.0*q*q*t*a.y + 3.0*q*t*t*b.y + t*t*t*end.y,
                        };
                        state.append(p)?;
                    }
                    start = end;
                }
            }
            62 if size >= 24 => {
                if !state.path_ready || state.path_active {
                    bail!("EMF FillPath without finished bracket");
                }
                if state.subpaths.iter().any(|p| p.len() < 3) {
                    bail!("EMF FillPath contains incomplete figure");
                }
                if let Some(color) = state.brush {
                    raster.as_mut().context("no EMF raster")?
                        .fill(&state.subpaths, color, state.fill_mode)?;
                }
                state.path_ready = false;
                state.subpaths.clear();
                state.filled_paths += 1;
            }
            _ => bail!("unsupported EMF record in source-backed vector preview"),
        }
        at = at.checked_add(size).context("EMF record cursor overflow")?;
        count += 1;
    }
    if !eof || at != logical.len() || state.path_active || state.path_ready {
        bail!("EMF vector stream incomplete");
    }
    if skipped_bitmap {
        return Ok(None);
    }
    let image = raster.context("EMF missing header")?;
    if state.filled_paths == 0 || !image.painted {
        bail!("EMF vector profile has no proven visible filled path");
    }
    Ok(Some((image, state.filled_paths)))
}

/// The strict envelope, BStore slot, UID, exact logical payload hash, and
/// source-use identity are retained. Nothing is exact-authority promoted.
pub fn build_mature_0x2c_emf_vector_preview_bundle_from_bytes(
    pub_bytes: &[u8],
    graph: &PubSourceGraph,
) -> Result<PubMatureEmfVectorPreviewBundle> {
    let inventory = pub_cfb::inspect_reader(Cursor::new(pub_bytes))?;
    if !inventory.entries.iter().any(|e| e.path == ESCHER_DELAY_STREAM_PATH) {
        return Ok(PubMatureEmfVectorPreviewBundle::default());
    }
    let escher = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_STREAM_PATH)?;
    let delayed = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_DELAY_STREAM_PATH)?;
    let manifest = build_pub_asset_manifest(graph, &escher, &delayed)?;
    if !manifest.assets.iter().any(|a| a.blip_kind == Some(BlipKind::Emf)) {
        return Ok(PubMatureEmfVectorPreviewBundle::default());
    }
    let validated = inspect_validated_delayed_blips_prefix(
        StreamPath(ESCHER_DELAY_STREAM_PATH.to_owned()), &delayed
    );
    if validated.terminal_gap.is_some() || !validated.rejected_complete_blips.is_empty() {
        bail!("EMF vector strict BLIP prefix contains rejected/trailing payload");
    }
    let mut out = PubMatureEmfVectorPreviewBundle::default();
    for asset in manifest.assets.iter().filter(|a| a.blip_kind == Some(BlipKind::Emf)) {
        let Some(source_span) = asset.blip_record_source.as_ref() else {
            out.rejected_source_count += 1; continue;
        };
        let Some(record) = validated.records.iter()
            .find(|r| &r.record_source == source_span)
        else { out.rejected_source_count += 1; continue; };
        if record.kind != BlipKind::Emf || record.effective_uid != asset.uid {
            out.rejected_source_count += 1; continue;
        }
        let decoded = logical_emf_bytes(&delayed, record)
            .and_then(|logical| parse_vector_emf(&logical));
        match decoded {
            Ok(Some((raster, filled_path_count))) => {
                let mut uses = asset.uses.clone();
                uses.sort();
                uses.dedup();
                out.sources.push(PubMatureEmfVectorPreviewSource {
                    slot: asset.slot,
                    uses,
                    width: raster.width,
                    height: raster.height,
                    rgba: raster.rgba,
                    filled_path_count,
                });
            }
            Ok(None) => out.skipped_bitmap_count += 1,
            Err(_) => out.rejected_source_count += 1,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: u32, body: &[u8]) -> Vec<u8> {
        let len = 8 + body.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&(len as u32).to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }
    fn vec2(x: i32, y: i32) -> Vec<u8> {
        [x.to_le_bytes(), y.to_le_bytes()].concat()
    }
    fn synthetic() -> Vec<u8> {
        let mut header = vec![0; 88];
        header[0..4].copy_from_slice(&1_u32.to_le_bytes());
        header[4..8].copy_from_slice(&88_u32.to_le_bytes());
        header[16..20].copy_from_slice(&32_u32.to_le_bytes());
        header[20..24].copy_from_slice(&32_u32.to_le_bytes());
        header[40..44].copy_from_slice(&0x464d_4520_u32.to_le_bytes());
        let mut bytes = header;
        bytes.extend(record(17, &MM_ANISOTROPIC.to_le_bytes()));
        bytes.extend(record(9, &vec2(32, 32)));
        bytes.extend(record(11, &vec2(32, 32)));
        let mut brush = [0_u8; 16];
        brush[0..4].copy_from_slice(&1_u32.to_le_bytes());
        brush[8..12].copy_from_slice(&0xff_u32.to_le_bytes());
        bytes.extend(record(39, &brush));
        bytes.extend(record(37, &1_u32.to_le_bytes()));
        bytes.extend(record(59, &[]));
        bytes.extend(record(27, &vec2(4, 4)));
        bytes.extend(record(54, &vec2(28, 4)));
        bytes.extend(record(54, &vec2(28, 28)));
        bytes.extend(record(54, &vec2(4, 28)));
        bytes.extend(record(61, &[]));
        bytes.extend(record(60, &[]));
        bytes.extend(record(62, &[0;16]));
        bytes.extend(record(14, &[0;12]));
        bytes
    }

    #[test]
    fn paints_only_the_proven_solid_path_and_preserves_transparency() {
        let (raster, n) = parse_vector_emf(&synthetic()).unwrap().unwrap();
        assert_eq!((raster.width, raster.height, n), (32,32,1));
        let inside = ((16*32+16)*4) as usize;
        assert_eq!(&raster.rgba[inside..inside+4], &[255,0,0,255]);
        assert_eq!(&raster.rgba[0..4], &[0,0,0,0]);
    }

    #[test]
    fn fails_closed_on_unmodeled_draw_operations_and_invalid_source() {
        let mut data = synthetic();
        let last = data.split_off(data.len()-20);
        data.extend(record(35, &[0;16]));
        data.extend(last);
        assert!(parse_vector_emf(&data).is_err());
        let mut truncated = synthetic();
        truncated.pop();
        assert!(parse_vector_emf(&truncated).is_err());
    }
}
