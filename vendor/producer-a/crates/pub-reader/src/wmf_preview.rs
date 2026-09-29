use crate::wmf::validate_wmf_metafile;
use anyhow::{Result, anyhow, bail};

pub const LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1: &str = "legacy-ole-wmf-preview-rasterizer-v1";

const MAX_WMF_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;
const MAX_OBJECTS: usize = 4096;
const MAX_POINTS_PER_RECORD: usize = 4096;
const MAX_OUTPUT_SIDE: u32 = 1024;
const MAX_OUTPUT_PIXELS: u64 = 1024 * 1024;
const MAX_RASTER_WORK_UNITS: u64 = 64 * 1024 * 1024;

const META_EOF: u16 = 0x0000;
const META_SAVEDC: u16 = 0x001e;
const META_SETBKMODE: u16 = 0x0102;
const META_SETMAPMODE: u16 = 0x0103;
const META_SETROP2: u16 = 0x0104;
const META_SETRELABS: u16 = 0x0105;
const META_SETPOLYFILLMODE: u16 = 0x0106;
const META_SETSTRETCHBLTMODE: u16 = 0x0107;
const META_RESTOREDC: u16 = 0x0127;
const META_SELECTCLIPREGION: u16 = 0x012c;
const META_SELECTOBJECT: u16 = 0x012d;
const META_SETTEXTALIGN: u16 = 0x012e;
const META_DIBCREATEPATTERNBRUSH: u16 = 0x0142;
const META_DELETEOBJECT: u16 = 0x01f0;
const META_SETBKCOLOR: u16 = 0x0201;
const META_SETTEXTCOLOR: u16 = 0x0209;
const META_SETWINDOWORG: u16 = 0x020b;
const META_SETWINDOWEXT: u16 = 0x020c;
const META_CREATEPENINDIRECT: u16 = 0x02fa;
const META_CREATEBRUSHINDIRECT: u16 = 0x02fc;
const META_POLYGON: u16 = 0x0324;
const META_POLYLINE: u16 = 0x0325;
const META_INTERSECTCLIPRECT: u16 = 0x0416;
const META_RECTANGLE: u16 = 0x041b;
const META_POLYPOLYGON: u16 = 0x0538;
const META_ESCAPE: u16 = 0x0626;
const META_CREATEREGION: u16 = 0x06ff;

const MM_ANISOTROPIC: u16 = 8;
const R2_COPYPEN: u16 = 13;
const STRETCH_DELETESCANS: u16 = 3;
const ABSOLUTE: u16 = 1;
const ALTERNATE: u16 = 1;
const WINDING: u16 = 2;
const META_ESCAPE_ENHANCED_METAFILE: u16 = 0x000f;
const META_ESCAPE_POSTSCRIPT_IGNORE: u16 = 0x0026;
const WMFC_COMMENT_IDENTIFIER: u32 = 0x4346_4d57;
const WMFC_COMMENT_TYPE: u32 = 1;
const MAX_EMBEDDED_EMF_BYTES: usize = MAX_WMF_BYTES;
const MAX_EMBEDDED_EMF_CHUNK_BYTES: usize = 8192;

const PS_SOLID: u16 = 0;
const PS_NULL: u16 = 5;
const PS_INSIDEFRAME: u16 = 6;
const BS_SOLID: u16 = 0;
const BS_NULL: u16 = 1;
const BS_DIBPATTERNPT: u16 = 5;
const DIB_RGB_COLORS: u16 = 0;
const BI_RGB: u32 = 0;
const LEGACY_PATTERN_DIB_BYTES: usize = 96;
const LEGACY_PATTERN_SIDE: usize = 8;
const LEGACY_PATTERN_ROW_BYTES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WmfPreviewRgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pen {
    style: u16,
    width_x: i16,
    color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PatternBrush8x8 {
    colors: [Color; 2],
    rows: [u8; LEGACY_PATTERN_SIDE],
}

impl PatternBrush8x8 {
    fn color_at(self, x: i32, y: i32) -> Color {
        let x = usize::try_from(x).unwrap_or(0) % LEGACY_PATTERN_SIDE;
        let y = usize::try_from(y).unwrap_or(0) % LEGACY_PATTERN_SIDE;
        let bit = (self.rows[y] >> (7 - x)) & 1;
        self.colors[usize::from(bit)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Brush {
    style: u16,
    color: Color,
    pattern: Option<PatternBrush8x8>,
}

impl Brush {
    fn color_at(self, x: i32, y: i32) -> Color {
        self.pattern
            .map(|pattern| pattern.color_at(x, y))
            .unwrap_or(self.color)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GdiObject {
    Pen(Pen),
    Brush(Brush),
    RegionCompatibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RectPx {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl RectPx {
    fn full(width: u32, height: u32) -> Self {
        Self {
            left: 0,
            top: 0,
            right: i32::try_from(width).unwrap_or(i32::MAX),
            bottom: i32::try_from(height).unwrap_or(i32::MAX),
        }
    }

    fn intersect(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaybackState {
    window_org_x: i32,
    window_org_y: i32,
    window_ext_x: i32,
    window_ext_y: i32,
    clip: RectPx,
    polygon_fill_mode: u16,
    pen: Pen,
    brush: Brush,
}

impl PlaybackState {
    fn new(width: u32, height: u32) -> Self {
        Self {
            window_org_x: 0,
            window_org_y: 0,
            window_ext_x: i32::try_from(width).unwrap_or(1).max(1),
            window_ext_y: i32::try_from(height).unwrap_or(1).max(1),
            clip: RectPx::full(width, height),
            polygon_fill_mode: ALTERNATE,
            pen: Pen {
                style: PS_SOLID,
                width_x: 1,
                color: Color { r: 0, g: 0, b: 0 },
            },
            brush: Brush {
                style: BS_SOLID,
                color: Color {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                pattern: None,
            },
        }
    }
}

struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    work_units: u64,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Result<Self> {
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or_else(|| anyhow!("WMF output pixel count overflow"))?;
        if pixels == 0 || pixels > MAX_OUTPUT_PIXELS {
            bail!("WMF output dimensions exceed bounded pixel count");
        }
        let byte_len = usize::try_from(
            pixels
                .checked_mul(4)
                .ok_or_else(|| anyhow!("WMF output byte count overflow"))?,
        )
        .map_err(|_| anyhow!("WMF output byte count does not fit address space"))?;
        Ok(Self {
            width,
            height,
            rgba: vec![0; byte_len],
            work_units: 0,
        })
    }

    fn charge_work(&mut self, units: u64) -> Result<()> {
        self.work_units = self
            .work_units
            .checked_add(units)
            .ok_or_else(|| anyhow!("WMF raster work counter overflow"))?;
        if self.work_units > MAX_RASTER_WORK_UNITS {
            bail!("WMF raster work exceeds bounded limit");
        }
        Ok(())
    }

    fn set(&mut self, clip: RectPx, x: i32, y: i32, color: Color) {
        if !clip.contains(x, y) || x < 0 || y < 0 {
            return;
        }
        let Ok(x) = u32::try_from(x) else {
            return;
        };
        let Ok(y) = u32::try_from(y) else {
            return;
        };
        if x >= self.width || y >= self.height {
            return;
        }
        let index = (u64::from(y) * u64::from(self.width) + u64::from(x)) * 4;
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        self.rgba[index..index + 4].copy_from_slice(&[color.r, color.g, color.b, 255]);
    }

    fn span_brush(&mut self, clip: RectPx, y: i32, x0: i32, x1: i32, brush: Brush) -> Result<()> {
        if y < clip.top || y >= clip.bottom || y < 0 || y >= self.height as i32 {
            return Ok(());
        }
        let start = x0.min(x1).max(clip.left).max(0);
        let end = x0.max(x1).min(clip.right - 1).min(self.width as i32 - 1);
        if start > end {
            return Ok(());
        }
        let pixel_count = u64::try_from(end - start + 1)
            .map_err(|_| anyhow!("WMF span work does not fit u64"))?;
        self.charge_work(pixel_count)?;
        for x in start..=end {
            self.set(clip, x, y, brush.color_at(x, y));
        }
        Ok(())
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    read_u16(bytes, offset).map(|value| i16::from_le_bytes(value.to_le_bytes()))
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn color_ref(value: u32) -> Color {
    Color {
        r: (value & 0xff) as u8,
        g: ((value >> 8) & 0xff) as u8,
        b: ((value >> 16) & 0xff) as u8,
    }
}

fn rgb_quad(bytes: &[u8], offset: usize) -> Result<Color> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow!("WMF pattern brush RGBQUAD is truncated"))?;
    if raw[3] != 0 {
        bail!("WMF pattern brush RGBQUAD reserved byte is nonzero");
    }
    Ok(Color {
        r: raw[2],
        g: raw[1],
        b: raw[0],
    })
}

fn parse_legacy_pattern_brush(params: &[u8]) -> Result<Brush> {
    if params.len() != 4 + LEGACY_PATTERN_DIB_BYTES {
        bail!("unsupported WMF DIB pattern brush payload length");
    }
    let style =
        read_u16(params, 0).ok_or_else(|| anyhow!("WMF DIB pattern brush style is truncated"))?;
    let color_usage = read_u16(params, 2)
        .ok_or_else(|| anyhow!("WMF DIB pattern brush color usage is truncated"))?;
    if style != BS_DIBPATTERNPT || color_usage != DIB_RGB_COLORS {
        bail!("unsupported WMF DIB pattern brush profile");
    }

    let dib = &params[4..];
    let header_size =
        read_u32(dib, 0).ok_or_else(|| anyhow!("WMF DIB pattern brush header is truncated"))?;
    let width =
        read_i32(dib, 4).ok_or_else(|| anyhow!("WMF DIB pattern brush width is truncated"))?;
    let height =
        read_i32(dib, 8).ok_or_else(|| anyhow!("WMF DIB pattern brush height is truncated"))?;
    let planes =
        read_u16(dib, 12).ok_or_else(|| anyhow!("WMF DIB pattern brush planes are truncated"))?;
    let bit_count =
        read_u16(dib, 14).ok_or_else(|| anyhow!("WMF DIB pattern brush bit count is truncated"))?;
    let compression = read_u32(dib, 16)
        .ok_or_else(|| anyhow!("WMF DIB pattern brush compression is truncated"))?;
    let image_size = read_u32(dib, 20)
        .ok_or_else(|| anyhow!("WMF DIB pattern brush image size is truncated"))?;
    let colors_used = read_u32(dib, 32)
        .ok_or_else(|| anyhow!("WMF DIB pattern brush color count is truncated"))?;

    if header_size != 40
        || width != 8
        || height != 8
        || planes != 1
        || bit_count != 1
        || compression != BI_RGB
        || !matches!(image_size, 0 | 32)
        || colors_used != 2
    {
        bail!("unsupported WMF DIB pattern brush bitmap profile");
    }

    let colors = [rgb_quad(dib, 40)?, rgb_quad(dib, 44)?];
    let bitmap_bytes = LEGACY_PATTERN_SIDE * LEGACY_PATTERN_ROW_BYTES;
    let bitmap_offset = dib
        .len()
        .checked_sub(bitmap_bytes)
        .ok_or_else(|| anyhow!("WMF DIB pattern brush bitmap is truncated"))?;
    if bitmap_offset < 48 {
        bail!("WMF DIB pattern brush overlaps its color table");
    }

    let mut rows = [0_u8; LEGACY_PATTERN_SIDE];
    for (top_row, row) in rows.iter_mut().enumerate() {
        let source_row = LEGACY_PATTERN_SIDE - 1 - top_row;
        let offset = bitmap_offset + source_row * LEGACY_PATTERN_ROW_BYTES;
        *row = *dib
            .get(offset)
            .ok_or_else(|| anyhow!("WMF DIB pattern brush scanline is truncated"))?;
    }

    Ok(Brush {
        style,
        color: colors[0],
        pattern: Some(PatternBrush8x8 { colors, rows }),
    })
}

fn bounded_output_size(width_hint: u32, height_hint: u32) -> Result<(u32, u32)> {
    if width_hint == 0 || height_hint == 0 {
        bail!("WMF presentation has zero output extent");
    }
    let mut width = width_hint;
    let mut height = height_hint;

    let side_scale = (MAX_OUTPUT_SIDE as f64 / f64::from(width.max(height))).min(1.0);
    let pixel_count = u64::from(width) * u64::from(height);
    let pixel_scale = if pixel_count > MAX_OUTPUT_PIXELS {
        (MAX_OUTPUT_PIXELS as f64 / pixel_count as f64).sqrt()
    } else {
        1.0
    };
    let scale = side_scale.min(pixel_scale);
    if scale < 1.0 {
        width = (f64::from(width) * scale).round().max(1.0) as u32;
        height = (f64::from(height) * scale).round().max(1.0) as u32;
    }
    Ok((width.max(1), height.max(1)))
}

fn map_point(state: &PlaybackState, canvas: &Canvas, x: i16, y: i16) -> Result<(i32, i32)> {
    if state.window_ext_x == 0 || state.window_ext_y == 0 {
        bail!("WMF window extent is zero");
    }
    let px = (f64::from(i32::from(x) - state.window_org_x) * f64::from(canvas.width)
        / f64::from(state.window_ext_x))
    .round();
    let py = (f64::from(i32::from(y) - state.window_org_y) * f64::from(canvas.height)
        / f64::from(state.window_ext_y))
    .round();
    if !px.is_finite() || !py.is_finite() {
        bail!("WMF coordinate transform produced a non-finite value");
    }
    if px < f64::from(i32::MIN)
        || px > f64::from(i32::MAX)
        || py < f64::from(i32::MIN)
        || py > f64::from(i32::MAX)
    {
        bail!("WMF coordinate transform overflow");
    }
    Ok((px as i32, py as i32))
}

fn mapped_pen_width(state: &PlaybackState, canvas: &Canvas) -> u32 {
    if state.pen.width_x == 0 {
        return 1;
    }
    let logical = f64::from(i32::from(state.pen.width_x).abs());
    let scale = f64::from(canvas.width) / f64::from(state.window_ext_x.abs().max(1));
    (logical * scale).round().clamp(1.0, 64.0) as u32
}

fn draw_disk(
    canvas: &mut Canvas,
    clip: RectPx,
    x: i32,
    y: i32,
    radius: i32,
    color: Color,
) -> Result<()> {
    if radius <= 0 {
        canvas.charge_work(1)?;
        canvas.set(clip, x, y, color);
        return Ok(());
    }
    let diameter = u64::try_from(radius.saturating_mul(2).saturating_add(1))
        .map_err(|_| anyhow!("WMF stroke work does not fit u64"))?;
    canvas.charge_work(
        diameter
            .checked_mul(diameter)
            .ok_or_else(|| anyhow!("WMF stroke work overflow"))?,
    )?;
    let rr = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= rr {
                canvas.set(clip, x + dx, y + dy, color);
            }
        }
    }
    Ok(())
}

fn out_code(rect: RectPx, x: i32, y: i32) -> u8 {
    let mut code = 0u8;
    if x < rect.left {
        code |= 1;
    } else if x >= rect.right {
        code |= 2;
    }
    if y < rect.top {
        code |= 4;
    } else if y >= rect.bottom {
        code |= 8;
    }
    code
}

fn clip_line_to_rect(
    rect: RectPx,
    mut x0: i32,
    mut y0: i32,
    mut x1: i32,
    mut y1: i32,
) -> Option<(i32, i32, i32, i32)> {
    if rect.left >= rect.right || rect.top >= rect.bottom {
        return None;
    }
    let right = rect.right.saturating_sub(1);
    let bottom = rect.bottom.saturating_sub(1);

    loop {
        let c0 = out_code(rect, x0, y0);
        let c1 = out_code(rect, x1, y1);
        if c0 | c1 == 0 {
            return Some((x0, y0, x1, y1));
        }
        if c0 & c1 != 0 {
            return None;
        }

        let code = if c0 != 0 { c0 } else { c1 };
        let (nx, ny) = if code & 8 != 0 {
            let dy = i128::from(y1) - i128::from(y0);
            if dy == 0 {
                return None;
            }
            let x = i128::from(x0)
                + (i128::from(x1) - i128::from(x0)) * (i128::from(bottom) - i128::from(y0)) / dy;
            (i32::try_from(x).ok()?, bottom)
        } else if code & 4 != 0 {
            let dy = i128::from(y1) - i128::from(y0);
            if dy == 0 {
                return None;
            }
            let x = i128::from(x0)
                + (i128::from(x1) - i128::from(x0)) * (i128::from(rect.top) - i128::from(y0)) / dy;
            (i32::try_from(x).ok()?, rect.top)
        } else if code & 2 != 0 {
            let dx = i128::from(x1) - i128::from(x0);
            if dx == 0 {
                return None;
            }
            let y = i128::from(y0)
                + (i128::from(y1) - i128::from(y0)) * (i128::from(right) - i128::from(x0)) / dx;
            (right, i32::try_from(y).ok()?)
        } else {
            let dx = i128::from(x1) - i128::from(x0);
            if dx == 0 {
                return None;
            }
            let y = i128::from(y0)
                + (i128::from(y1) - i128::from(y0)) * (i128::from(rect.left) - i128::from(x0)) / dx;
            (rect.left, i32::try_from(y).ok()?)
        };

        if code == c0 {
            x0 = nx;
            y0 = ny;
        } else {
            x1 = nx;
            y1 = ny;
        }
    }
}

fn draw_line(
    canvas: &mut Canvas,
    clip: RectPx,
    start: (i32, i32),
    end: (i32, i32),
    width: u32,
    color: Color,
) -> Result<()> {
    let (mut x0, mut y0) = start;
    let (x1, y1) = end;
    let Some((cx0, cy0, cx1, cy1)) = clip_line_to_rect(clip, x0, y0, x1, y1) else {
        return Ok(());
    };
    x0 = cx0;
    y0 = cy0;
    let x1 = cx1;
    let y1 = cy1;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let radius = i32::try_from(width.saturating_sub(1) / 2).unwrap_or(0);

    loop {
        draw_disk(canvas, clip, x0, y0, radius, color)?;
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = err.saturating_mul(2);
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    Ok(())
}

fn draw_polyline(
    canvas: &mut Canvas,
    state: &PlaybackState,
    points: &[(i32, i32)],
    closed: bool,
) -> Result<()> {
    if state.pen.style == PS_NULL || points.len() < 2 {
        return Ok(());
    }
    let width = mapped_pen_width(state, canvas);
    for pair in points.windows(2) {
        draw_line(canvas, state.clip, pair[0], pair[1], width, state.pen.color)?;
    }
    if closed {
        let first = points[0];
        let last = points[points.len() - 1];
        draw_line(canvas, state.clip, last, first, width, state.pen.color)?;
    }
    Ok(())
}

fn fill_rings(canvas: &mut Canvas, state: &PlaybackState, rings: &[Vec<(i32, i32)>]) -> Result<()> {
    let Some(min_y) = rings
        .iter()
        .flat_map(|ring| ring.iter().map(|point| point.1))
        .min()
    else {
        return Ok(());
    };
    let Some(max_y) = rings
        .iter()
        .flat_map(|ring| ring.iter().map(|point| point.1))
        .max()
    else {
        return Ok(());
    };
    let edge_count = rings.iter().try_fold(0_u64, |total, ring| {
        total
            .checked_add(u64::try_from(ring.len()).map_err(|_| anyhow!("WMF edge count overflow"))?)
            .ok_or_else(|| anyhow!("WMF edge count overflow"))
    })?;
    let start_y = min_y.max(state.clip.top).max(0);
    let end_y = max_y
        .min(state.clip.bottom.saturating_sub(1))
        .min(canvas.height as i32 - 1);

    for y in start_y..=end_y {
        canvas.charge_work(edge_count)?;
        let scan_y = f64::from(y) + 0.5;
        let mut crossings = Vec::<(f64, i32)>::new();
        for ring in rings {
            if ring.len() < 3 {
                continue;
            }
            for index in 0..ring.len() {
                let (x0, y0) = ring[index];
                let (x1, y1) = ring[(index + 1) % ring.len()];
                let y0f = f64::from(y0);
                let y1f = f64::from(y1);
                if (y0f <= scan_y && y1f > scan_y) || (y1f <= scan_y && y0f > scan_y) {
                    let t = (scan_y - y0f) / (y1f - y0f);
                    let x = f64::from(x0) + t * f64::from(x1 - x0);
                    let winding = if y1 > y0 { 1 } else { -1 };
                    crossings.push((x, winding));
                }
            }
        }
        crossings.sort_by(|left, right| left.0.total_cmp(&right.0));

        match state.polygon_fill_mode {
            ALTERNATE => {
                for pair in crossings.chunks_exact(2) {
                    let x0 = pair[0].0.ceil() as i32;
                    let x1 = pair[1].0.floor() as i32;
                    canvas.span_brush(state.clip, y, x0, x1, state.brush)?;
                }
            }
            WINDING => {
                let mut winding = 0_i32;
                let mut start = None::<f64>;
                for (x, delta) in crossings {
                    let before = winding;
                    winding += delta;
                    if before == 0 && winding != 0 {
                        start = Some(x);
                    } else if before != 0 && winding == 0 {
                        if let Some(from) = start.take() {
                            canvas.span_brush(
                                state.clip,
                                y,
                                from.ceil() as i32,
                                x.floor() as i32,
                                state.brush,
                            )?;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn allocate_object(objects: &mut [Option<GdiObject>], object: GdiObject) -> Result<()> {
    let Some(slot) = objects.iter_mut().find(|slot| slot.is_none()) else {
        bail!("WMF object table is full");
    };
    *slot = Some(object);
    Ok(())
}

fn validate_region_compatibility_object(params: &[u8]) -> Result<()> {
    // Exact-1050 evidence for the seven Publisher Region compatibility objects:
    // 42-byte payload, ObjectType=6, RegionSize=42, one scan, maxScan=2.
    // The one scan is structurally valid (Count=Count2=2) and the corpus profile
    // carries an additional bounded eight-byte tail that is playback-inert here.
    if params.len() != 42 {
        bail!(
            "unsupported WMF Region compatibility payload length {}",
            params.len()
        );
    }

    let object_type =
        read_i16(params, 2).ok_or_else(|| anyhow!("WMF Region ObjectType is truncated"))?;
    let region_size = read_i16(params, 8).ok_or_else(|| anyhow!("WMF RegionSize is truncated"))?;
    let scan_count =
        read_i16(params, 10).ok_or_else(|| anyhow!("WMF Region ScanCount is truncated"))?;
    let max_scan =
        read_i16(params, 12).ok_or_else(|| anyhow!("WMF Region maxScan is truncated"))?;
    if object_type != 6 || region_size != 42 || scan_count != 1 || max_scan != 2 {
        bail!(
            "unsupported WMF Region compatibility profile type={object_type} size={region_size} scans={scan_count} max_scan={max_scan}"
        );
    }

    // BoundingRectangle is present in the fixed Region header. Values are not
    // interpreted because this compatibility object never becomes the active
    // clipping region in the admitted sequence.
    for offset in [14usize, 16, 18, 20] {
        read_i16(params, offset)
            .ok_or_else(|| anyhow!("WMF Region bounding rectangle is truncated"))?;
    }

    let count =
        read_u16(params, 22).ok_or_else(|| anyhow!("WMF Region Scan Count is truncated"))?;
    let top = read_u16(params, 24).ok_or_else(|| anyhow!("WMF Region Scan Top is truncated"))?;
    let bottom =
        read_u16(params, 26).ok_or_else(|| anyhow!("WMF Region Scan Bottom is truncated"))?;
    if count != 2 {
        bail!("unsupported WMF Region scan coordinate count {count}");
    }
    let _left = read_u16(params, 28).ok_or_else(|| anyhow!("WMF Region Scan left is truncated"))?;
    let _right =
        read_u16(params, 30).ok_or_else(|| anyhow!("WMF Region Scan right is truncated"))?;
    let count2 =
        read_u16(params, 32).ok_or_else(|| anyhow!("WMF Region Scan Count2 is truncated"))?;
    if count2 != count {
        bail!("WMF Region Scan Count2 does not match Count");
    }
    if bottom < top {
        bail!("WMF Region scan vertical bounds are inverted");
    }

    // Exact corpus profile has an eight-byte compatibility tail. Keep it
    // bounded but opaque because Region geometry is not activated in this slice.
    if params.len() - 34 != 8 {
        bail!("unsupported WMF Region compatibility tail length");
    }
    Ok(())
}

fn parse_points(params: &[u8], count: usize, offset: usize) -> Result<Vec<(i16, i16)>> {
    if count > MAX_POINTS_PER_RECORD {
        bail!("WMF point count exceeds bounded limit");
    }
    let byte_len = count
        .checked_mul(4)
        .ok_or_else(|| anyhow!("WMF point byte count overflow"))?;
    let end = offset
        .checked_add(byte_len)
        .ok_or_else(|| anyhow!("WMF point range overflow"))?;
    if end > params.len() {
        bail!("WMF point array is truncated");
    }
    let mut points = Vec::with_capacity(count);
    for index in 0..count {
        let base = offset + index * 4;
        points.push((
            read_i16(params, base).ok_or_else(|| anyhow!("WMF point x is truncated"))?,
            read_i16(params, base + 2).ok_or_else(|| anyhow!("WMF point y is truncated"))?,
        ));
    }
    Ok(points)
}

fn map_points(
    state: &PlaybackState,
    canvas: &Canvas,
    points: &[(i16, i16)],
) -> Result<Vec<(i32, i32)>> {
    points
        .iter()
        .map(|&(x, y)| map_point(state, canvas, x, y))
        .collect()
}

fn validate_meta_escape(params: &[u8]) -> Result<()> {
    if params.len() < 4 {
        bail!("WMF META_ESCAPE record is truncated");
    }
    let escape = read_u16(params, 0).ok_or_else(|| anyhow!("WMF escape function is truncated"))?;
    let byte_count = usize::from(
        read_u16(params, 2).ok_or_else(|| anyhow!("WMF escape byte count is truncated"))?,
    );

    if escape == META_ESCAPE_POSTSCRIPT_IGNORE {
        if params.len() != 6 || byte_count != 2 {
            bail!("unsupported WMF POSTSCRIPT_IGNORE profile");
        }
        let _opaque_control = params
            .get(4..6)
            .ok_or_else(|| anyhow!("WMF POSTSCRIPT_IGNORE payload is truncated"))?;
        return Ok(());
    }

    if escape != META_ESCAPE_ENHANCED_METAFILE {
        bail!("unsupported WMF escape function 0x{escape:04x}");
    }
    if byte_count > params.len().saturating_sub(4) {
        bail!("WMF MFCOMMENT payload is truncated");
    }

    let body = &params[4..4 + byte_count];
    let Some(identifier) = read_u32(body, 0) else {
        // MFCOMMENT is also a carrier for opaque private comment data. A payload
        // shorter than the standard WMFC magic has no rendering semantics here.
        return Ok(());
    };
    if identifier != WMFC_COMMENT_IDENTIFIER {
        // Non-WMFC MFCOMMENT payloads are opaque private data. They are bounded
        // by the enclosing WMF record and ByteCount and are never executed.
        return Ok(());
    }

    if body.len() < 34 {
        bail!("WMF embedded-EMF comment header is truncated");
    }
    let comment_type =
        read_u32(body, 4).ok_or_else(|| anyhow!("WMF EMF comment type is truncated"))?;
    if comment_type != WMFC_COMMENT_TYPE {
        bail!("unsupported WMF WMFC comment type {comment_type}");
    }

    let flags = read_u32(body, 14).ok_or_else(|| anyhow!("WMF EMF flags are truncated"))?;
    if flags != 0 {
        bail!("WMF embedded-EMF comment flags must be zero");
    }
    let comment_record_count = usize::try_from(
        read_u32(body, 18).ok_or_else(|| anyhow!("WMF EMF comment record count is truncated"))?,
    )
    .map_err(|_| anyhow!("WMF EMF comment record count overflow"))?;
    if comment_record_count == 0 || comment_record_count > MAX_RECORDS {
        bail!("WMF embedded-EMF comment record count is out of bounds");
    }

    let current_record_size = usize::try_from(
        read_u32(body, 22).ok_or_else(|| anyhow!("WMF EMF current record size is truncated"))?,
    )
    .map_err(|_| anyhow!("WMF EMF current record size overflow"))?;
    if current_record_size > MAX_EMBEDDED_EMF_CHUNK_BYTES {
        bail!("WMF embedded EMF segment exceeds bounded record size");
    }
    let remaining_bytes = usize::try_from(
        read_u32(body, 26).ok_or_else(|| anyhow!("WMF EMF remaining size is truncated"))?,
    )
    .map_err(|_| anyhow!("WMF EMF remaining size overflow"))?;
    let total_emf_size = usize::try_from(
        read_u32(body, 30).ok_or_else(|| anyhow!("WMF EMF total size is truncated"))?,
    )
    .map_err(|_| anyhow!("WMF EMF total size overflow"))?;
    if total_emf_size > MAX_EMBEDDED_EMF_BYTES {
        bail!("WMF embedded EMF total size exceeds bounded limit");
    }

    let local_body_size = 34usize
        .checked_add(current_record_size)
        .ok_or_else(|| anyhow!("WMF embedded EMF local size overflow"))?;
    if local_body_size != body.len() {
        bail!("WMF embedded EMF segment length mismatch");
    }
    let current_plus_remaining = current_record_size
        .checked_add(remaining_bytes)
        .ok_or_else(|| anyhow!("WMF embedded EMF remaining-size overflow"))?;
    if current_record_size > total_emf_size
        || remaining_bytes > total_emf_size
        || current_plus_remaining > total_emf_size
    {
        bail!("WMF embedded EMF chunk accounting is inconsistent");
    }

    // The embedded EMF data stays opaque. WMF fallback records are the only
    // drawing instructions executed by this bounded rasterizer.
    Ok(())
}

pub fn rasterize_wmf_preview(
    bytes: &[u8],
    width_hint: u32,
    height_hint: u32,
) -> Result<WmfPreviewRgba> {
    if bytes.len() > MAX_WMF_BYTES {
        bail!("WMF payload exceeds bounded size");
    }
    if bytes.len() < 18 {
        bail!("WMF header is truncated");
    }

    let info = validate_wmf_metafile(bytes)?;
    if info.placeable || info.metafile_type != 1 || info.version != 0x0300 {
        bail!(
            "unsupported WMF raster profile placeable={} type={} version=0x{:04x}",
            info.placeable,
            info.metafile_type,
            info.version
        );
    }

    let meta_type = read_u16(bytes, 0).ok_or_else(|| anyhow!("missing WMF type"))?;
    let header_words = read_u16(bytes, 2).ok_or_else(|| anyhow!("missing WMF header size"))?;
    let version = read_u16(bytes, 4).ok_or_else(|| anyhow!("missing WMF version"))?;
    let size_words = read_u32(bytes, 6).ok_or_else(|| anyhow!("missing WMF size"))?;
    let object_count =
        usize::from(read_u16(bytes, 10).ok_or_else(|| anyhow!("missing WMF object count"))?);

    if meta_type != 1 || header_words != 9 || version != 0x0300 {
        bail!(
            "unsupported WMF header type={meta_type} header_words={header_words} version=0x{version:04x}"
        );
    }
    if object_count > MAX_OBJECTS || object_count != usize::from(info.object_count) {
        bail!("WMF object table exceeds bounded size or disagrees with validated header");
    }

    let declared_len = bytes.len();
    let expected_words = u32::try_from(declared_len / 2)
        .map_err(|_| anyhow!("WMF declared size does not fit u32 words"))?;
    if size_words != expected_words {
        bail!("WMF declared size disagrees with validated payload");
    }

    let (width, height) = bounded_output_size(width_hint, height_hint)?;
    let mut canvas = Canvas::new(width, height)?;
    let mut state = PlaybackState::new(width, height);
    let mut state_stack = Vec::<PlaybackState>::new();
    let mut objects = vec![None; object_count];

    let mut offset = 18usize;
    let mut records = 0usize;
    let mut eof_seen = false;
    let mut pending_region_compat_cliprect = false;

    while offset < declared_len {
        records += 1;
        if records > MAX_RECORDS {
            bail!("WMF record count exceeds bounded limit");
        }
        if offset + 6 > declared_len {
            bail!("WMF record header is truncated");
        }
        let record_words =
            read_u32(bytes, offset).ok_or_else(|| anyhow!("WMF record size is truncated"))?;
        let function = read_u16(bytes, offset + 4)
            .ok_or_else(|| anyhow!("WMF record function is truncated"))?;
        if record_words < 3 {
            bail!("WMF record size {record_words} is smaller than three words");
        }
        let record_len = usize::try_from(
            record_words
                .checked_mul(2)
                .ok_or_else(|| anyhow!("WMF record size overflow"))?,
        )
        .map_err(|_| anyhow!("WMF record size does not fit address space"))?;
        let next = offset
            .checked_add(record_len)
            .ok_or_else(|| anyhow!("WMF record range overflow"))?;
        if next > declared_len {
            bail!("WMF record 0x{function:04x} exceeds declared metafile size");
        }
        let params = &bytes[offset + 6..next];

        if pending_region_compat_cliprect {
            if function != META_INTERSECTCLIPRECT {
                bail!(
                    "WMF Region compatibility SELECTOBJECT is not followed by META_INTERSECTCLIPRECT"
                );
            }
            pending_region_compat_cliprect = false;
        }

        match function {
            META_EOF => {
                if next != declared_len {
                    bail!("WMF META_EOF is not the final record");
                }
                eof_seen = true;
                break;
            }
            META_SAVEDC => state_stack.push(state.clone()),
            META_RESTOREDC => {
                let relative =
                    read_i16(params, 0).ok_or_else(|| anyhow!("WMF RESTOREDC is truncated"))?;
                if relative != -1 {
                    bail!("unsupported WMF RESTOREDC value {relative}");
                }
                state = state_stack
                    .pop()
                    .ok_or_else(|| anyhow!("WMF RESTOREDC without matching SAVEDC"))?;
            }
            META_SETBKMODE => {
                read_u16(params, 0).ok_or_else(|| anyhow!("WMF SETBKMODE is truncated"))?;
            }
            META_SETMAPMODE => {
                let mode =
                    read_u16(params, 0).ok_or_else(|| anyhow!("WMF SETMAPMODE is truncated"))?;
                if mode != MM_ANISOTROPIC {
                    bail!("unsupported WMF map mode {mode}");
                }
            }
            META_SETROP2 => {
                let mode =
                    read_u16(params, 0).ok_or_else(|| anyhow!("WMF SETROP2 is truncated"))?;
                if mode != R2_COPYPEN {
                    bail!("unsupported WMF ROP2 mode {mode}");
                }
            }
            META_SETRELABS => {
                let mode =
                    read_u16(params, 0).ok_or_else(|| anyhow!("WMF SETRELABS is truncated"))?;
                if mode != ABSOLUTE {
                    bail!("unsupported WMF relative/absolute mode {mode}");
                }
            }
            META_SETPOLYFILLMODE => {
                let mode = read_u16(params, 0)
                    .ok_or_else(|| anyhow!("WMF SETPOLYFILLMODE is truncated"))?;
                if !matches!(mode, ALTERNATE | WINDING) {
                    bail!("unsupported WMF polygon fill mode {mode}");
                }
                state.polygon_fill_mode = mode;
            }
            META_SETSTRETCHBLTMODE => {
                let mode = read_u16(params, 0)
                    .ok_or_else(|| anyhow!("WMF SETSTRETCHBLTMODE is truncated"))?;
                if mode != STRETCH_DELETESCANS {
                    bail!("unsupported WMF stretch mode {mode}");
                }
            }
            META_SETTEXTALIGN => {
                if params.len() < 2 {
                    bail!("WMF SETTEXTALIGN is truncated");
                }
                let _ =
                    read_u16(params, 0).ok_or_else(|| anyhow!("WMF SETTEXTALIGN is truncated"))?;
            }
            META_SETBKCOLOR => {
                if params.len() < 4 {
                    bail!("WMF SETBKCOLOR is truncated");
                }
                let _ =
                    read_u32(params, 0).ok_or_else(|| anyhow!("WMF SETBKCOLOR is truncated"))?;
            }
            META_SETTEXTCOLOR => {
                if params.len() != 4 {
                    bail!("WMF SETTEXTCOLOR parameter length is not 4 bytes");
                }
                let _ = read_u32(params, 0)
                    .ok_or_else(|| anyhow!("WMF SETTEXTCOLOR parameter is truncated"))?;
            }
            META_SETWINDOWORG => {
                state.window_org_y = i32::from(
                    read_i16(params, 0)
                        .ok_or_else(|| anyhow!("WMF SETWINDOWORG y is truncated"))?,
                );
                state.window_org_x = i32::from(
                    read_i16(params, 2)
                        .ok_or_else(|| anyhow!("WMF SETWINDOWORG x is truncated"))?,
                );
            }
            META_SETWINDOWEXT => {
                state.window_ext_y = i32::from(
                    read_i16(params, 0)
                        .ok_or_else(|| anyhow!("WMF SETWINDOWEXT y is truncated"))?,
                );
                state.window_ext_x = i32::from(
                    read_i16(params, 2)
                        .ok_or_else(|| anyhow!("WMF SETWINDOWEXT x is truncated"))?,
                );
                if state.window_ext_x == 0 || state.window_ext_y == 0 {
                    bail!("WMF SETWINDOWEXT contains a zero extent");
                }
            }
            META_INTERSECTCLIPRECT => {
                let bottom = read_i16(params, 0)
                    .ok_or_else(|| anyhow!("WMF INTERSECTCLIPRECT bottom is truncated"))?;
                let right = read_i16(params, 2)
                    .ok_or_else(|| anyhow!("WMF INTERSECTCLIPRECT right is truncated"))?;
                let top = read_i16(params, 4)
                    .ok_or_else(|| anyhow!("WMF INTERSECTCLIPRECT top is truncated"))?;
                let left = read_i16(params, 6)
                    .ok_or_else(|| anyhow!("WMF INTERSECTCLIPRECT left is truncated"))?;
                let (x0, y0) = map_point(&state, &canvas, left, top)?;
                let (x1, y1) = map_point(&state, &canvas, right, bottom)?;
                state.clip = state.clip.intersect(RectPx {
                    left: x0.min(x1),
                    top: y0.min(y1),
                    right: x0.max(x1),
                    bottom: y0.max(y1),
                });
            }
            META_CREATEPENINDIRECT => {
                if params.len() < 10 {
                    bail!("WMF CREATEPENINDIRECT is truncated");
                }
                let style = read_u16(params, 0).unwrap();
                if !matches!(style, PS_SOLID | PS_NULL | PS_INSIDEFRAME) {
                    bail!("unsupported WMF pen style {style}");
                }
                let pen = Pen {
                    style,
                    width_x: read_i16(params, 2).unwrap(),
                    color: color_ref(read_u32(params, 6).unwrap()),
                };
                allocate_object(&mut objects, GdiObject::Pen(pen))?;
            }
            META_CREATEBRUSHINDIRECT => {
                if params.len() < 8 {
                    bail!("WMF CREATEBRUSHINDIRECT is truncated");
                }
                let style = read_u16(params, 0).unwrap();
                if !matches!(style, BS_SOLID | BS_NULL) {
                    bail!("unsupported WMF brush style {style}");
                }
                let brush = Brush {
                    style,
                    color: color_ref(read_u32(params, 2).unwrap()),
                    pattern: None,
                };
                allocate_object(&mut objects, GdiObject::Brush(brush))?;
            }
            META_DIBCREATEPATTERNBRUSH => {
                let brush = parse_legacy_pattern_brush(params)?;
                allocate_object(&mut objects, GdiObject::Brush(brush))?;
            }
            META_CREATEREGION => {
                validate_region_compatibility_object(params)?;
                allocate_object(&mut objects, GdiObject::RegionCompatibility)?;
            }
            META_DELETEOBJECT => {
                let index = usize::from(
                    read_u16(params, 0).ok_or_else(|| anyhow!("WMF DELETEOBJECT is truncated"))?,
                );
                let slot = objects
                    .get_mut(index)
                    .ok_or_else(|| anyhow!("WMF DELETEOBJECT index is out of bounds"))?;
                *slot = None;
            }
            META_SELECTOBJECT => {
                let index = usize::from(
                    read_u16(params, 0).ok_or_else(|| anyhow!("WMF SELECTOBJECT is truncated"))?,
                );
                let object = objects
                    .get(index)
                    .and_then(|object| *object)
                    .ok_or_else(|| anyhow!("WMF SELECTOBJECT refers to an empty slot"))?;
                match object {
                    GdiObject::Pen(pen) => state.pen = pen,
                    GdiObject::Brush(brush) => state.brush = brush,
                    GdiObject::RegionCompatibility => {
                        pending_region_compat_cliprect = true;
                    }
                }
            }
            META_SELECTCLIPREGION => {
                bail!("WMF META_SELECTCLIPREGION remains unsupported");
            }
            META_POLYGON | META_POLYLINE => {
                let count = usize::from(
                    read_u16(params, 0).ok_or_else(|| anyhow!("WMF polygon count is truncated"))?,
                );
                if count < 2 {
                    bail!("WMF polygon has fewer than two points");
                }
                let logical = parse_points(params, count, 2)?;
                let points = map_points(&state, &canvas, &logical)?;
                if function == META_POLYGON {
                    if state.brush.style != BS_NULL && points.len() >= 3 {
                        fill_rings(&mut canvas, &state, std::slice::from_ref(&points))?;
                    }
                    draw_polyline(&mut canvas, &state, &points, true)?;
                } else {
                    draw_polyline(&mut canvas, &state, &points, false)?;
                }
            }
            META_POLYPOLYGON => {
                let polygon_count = usize::from(
                    read_u16(params, 0)
                        .ok_or_else(|| anyhow!("WMF POLYPOLYGON count is truncated"))?,
                );
                if polygon_count == 0 || polygon_count > MAX_POINTS_PER_RECORD {
                    bail!("WMF POLYPOLYGON polygon count is out of bounds");
                }
                let counts_end = 2usize
                    .checked_add(
                        polygon_count
                            .checked_mul(2)
                            .ok_or_else(|| anyhow!("WMF POLYPOLYGON count overflow"))?,
                    )
                    .ok_or_else(|| anyhow!("WMF POLYPOLYGON count range overflow"))?;
                if counts_end > params.len() {
                    bail!("WMF POLYPOLYGON counts are truncated");
                }
                let mut counts = Vec::with_capacity(polygon_count);
                let mut total_points = 0usize;
                for index in 0..polygon_count {
                    let count = usize::from(read_u16(params, 2 + index * 2).unwrap());
                    if count < 2 {
                        bail!("WMF POLYPOLYGON contains a short polygon");
                    }
                    total_points = total_points
                        .checked_add(count)
                        .ok_or_else(|| anyhow!("WMF POLYPOLYGON point count overflow"))?;
                    counts.push(count);
                }
                if total_points > MAX_POINTS_PER_RECORD {
                    bail!("WMF POLYPOLYGON point count exceeds bounded limit");
                }
                let logical = parse_points(params, total_points, counts_end)?;
                let mapped = map_points(&state, &canvas, &logical)?;
                let mut rings = Vec::with_capacity(polygon_count);
                let mut cursor = 0usize;
                for count in counts {
                    let end = cursor + count;
                    rings.push(mapped[cursor..end].to_vec());
                    cursor = end;
                }
                if state.brush.style != BS_NULL {
                    fill_rings(&mut canvas, &state, &rings)?;
                }
                for ring in &rings {
                    draw_polyline(&mut canvas, &state, ring, true)?;
                }
            }
            META_RECTANGLE => {
                let bottom = read_i16(params, 0)
                    .ok_or_else(|| anyhow!("WMF RECTANGLE bottom is truncated"))?;
                let right = read_i16(params, 2)
                    .ok_or_else(|| anyhow!("WMF RECTANGLE right is truncated"))?;
                let top =
                    read_i16(params, 4).ok_or_else(|| anyhow!("WMF RECTANGLE top is truncated"))?;
                let left = read_i16(params, 6)
                    .ok_or_else(|| anyhow!("WMF RECTANGLE left is truncated"))?;
                let logical = [(left, top), (right, top), (right, bottom), (left, bottom)];
                let points = map_points(&state, &canvas, &logical)?;
                if state.brush.style != BS_NULL {
                    fill_rings(&mut canvas, &state, std::slice::from_ref(&points))?;
                }
                draw_polyline(&mut canvas, &state, &points, true)?;
            }
            META_ESCAPE => validate_meta_escape(params)?,
            other => bail!("unsupported WMF record function 0x{other:04x}"),
        }

        offset = next;
    }

    if !eof_seen {
        bail!("WMF payload does not contain META_EOF");
    }

    Ok(WmfPreviewRgba {
        width,
        height,
        rgba: canvas.rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(function: u16, params: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let record_len = 6 + params.len();
        assert_eq!(record_len % 2, 0);
        bytes.extend_from_slice(&u32::try_from(record_len / 2).unwrap().to_le_bytes());
        bytes.extend_from_slice(&function.to_le_bytes());
        bytes.extend_from_slice(params);
        bytes
    }

    fn insert_record_before_eof(bytes: &mut Vec<u8>, extra: Vec<u8>) {
        let eof = bytes.len() - 6;
        let extra_words = u32::try_from(extra.len() / 2).expect("record words");
        bytes.splice(eof..eof, extra);
        let words = u32::try_from(bytes.len() / 2).expect("WMF words");
        bytes[6..10].copy_from_slice(&words.to_le_bytes());
        let max_record_words = read_u32(bytes, 12).expect("MaxRecord");
        if extra_words > max_record_words {
            bytes[12..16].copy_from_slice(&extra_words.to_le_bytes());
        }
    }

    fn insert_record_before_function(bytes: &mut Vec<u8>, target: u16, extra: Vec<u8>) {
        let mut offset = 18usize;
        let mut insert_at = None;
        while offset + 6 <= bytes.len() {
            let words = read_u32(bytes, offset).expect("record size");
            let function = read_u16(bytes, offset + 4).expect("record function");
            if function == target {
                insert_at = Some(offset);
                break;
            }
            offset += usize::try_from(words * 2).expect("record bytes");
        }
        let insert_at = insert_at.expect("target record");
        let extra_words = u32::try_from(extra.len() / 2).expect("record words");
        bytes.splice(insert_at..insert_at, extra);
        let words = u32::try_from(bytes.len() / 2).expect("WMF words");
        bytes[6..10].copy_from_slice(&words.to_le_bytes());
        let max_record_words = read_u32(bytes, 12).expect("MaxRecord");
        if extra_words > max_record_words {
            bytes[12..16].copy_from_slice(&extra_words.to_le_bytes());
        }
    }

    fn region_compatibility_params() -> Vec<u8> {
        let mut params = Vec::new();
        params.extend_from_slice(&0_i16.to_le_bytes()); // nextInChain: ignored
        params.extend_from_slice(&6_i16.to_le_bytes()); // ObjectType
        params.extend_from_slice(&0x02f6_u32.to_le_bytes()); // ObjectCount: ignored
        params.extend_from_slice(&42_i16.to_le_bytes()); // RegionSize
        params.extend_from_slice(&1_i16.to_le_bytes()); // ScanCount
        params.extend_from_slice(&2_i16.to_le_bytes()); // maxScan
        for value in [25_i16, 25, 75, 75] {
            params.extend_from_slice(&value.to_le_bytes());
        }
        params.extend_from_slice(&2_u16.to_le_bytes()); // Count
        params.extend_from_slice(&25_u16.to_le_bytes()); // Top
        params.extend_from_slice(&75_u16.to_le_bytes()); // Bottom
        params.extend_from_slice(&25_u16.to_le_bytes()); // Left
        params.extend_from_slice(&75_u16.to_le_bytes()); // Right
        params.extend_from_slice(&2_u16.to_le_bytes()); // Count2
        params.extend_from_slice(&[0_u8; 8]); // corpus-proven bounded compatibility tail
        assert_eq!(params.len(), 42);
        params
    }

    fn synthetic_polygon() -> Vec<u8> {
        let mut records = Vec::<u8>::new();
        let mut window = Vec::new();
        window.extend_from_slice(&100_i16.to_le_bytes());
        window.extend_from_slice(&100_i16.to_le_bytes());
        records.extend(record(META_SETWINDOWEXT, &window));

        let mut brush = Vec::new();
        brush.extend_from_slice(&BS_SOLID.to_le_bytes());
        brush.extend_from_slice(&0x0000_00ff_u32.to_le_bytes());
        brush.extend_from_slice(&0_u16.to_le_bytes());
        records.extend(record(META_CREATEBRUSHINDIRECT, &brush));

        let mut pen = Vec::new();
        pen.extend_from_slice(&PS_NULL.to_le_bytes());
        pen.extend_from_slice(&1_i16.to_le_bytes());
        pen.extend_from_slice(&0_i16.to_le_bytes());
        pen.extend_from_slice(&0_u32.to_le_bytes());
        records.extend(record(META_CREATEPENINDIRECT, &pen));

        records.extend(record(META_SELECTOBJECT, &0_u16.to_le_bytes()));
        records.extend(record(META_SELECTOBJECT, &1_u16.to_le_bytes()));

        let mut polygon = Vec::new();
        polygon.extend_from_slice(&4_u16.to_le_bytes());
        for (x, y) in [(20_i16, 20_i16), (80, 20), (80, 80), (20, 80)] {
            polygon.extend_from_slice(&x.to_le_bytes());
            polygon.extend_from_slice(&y.to_le_bytes());
        }
        records.extend(record(META_POLYGON, &polygon));
        records.extend(record(META_EOF, &[]));

        let total_len = 18 + records.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&9_u16.to_le_bytes());
        bytes.extend_from_slice(&0x0300_u16.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(total_len / 2).unwrap().to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&20_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend(records);
        bytes
    }

    fn synthetic_pattern_polygon() -> Vec<u8> {
        let mut records = Vec::<u8>::new();
        let mut window = Vec::new();
        window.extend_from_slice(&100_i16.to_le_bytes());
        window.extend_from_slice(&100_i16.to_le_bytes());
        records.extend(record(META_SETWINDOWEXT, &window));

        let mut pattern = Vec::new();
        pattern.extend_from_slice(&BS_DIBPATTERNPT.to_le_bytes());
        pattern.extend_from_slice(&DIB_RGB_COLORS.to_le_bytes());
        pattern.extend_from_slice(&40_u32.to_le_bytes());
        pattern.extend_from_slice(&8_i32.to_le_bytes());
        pattern.extend_from_slice(&8_i32.to_le_bytes());
        pattern.extend_from_slice(&1_u16.to_le_bytes());
        pattern.extend_from_slice(&1_u16.to_le_bytes());
        pattern.extend_from_slice(&BI_RGB.to_le_bytes());
        pattern.extend_from_slice(&0_u32.to_le_bytes());
        pattern.extend_from_slice(&0_i32.to_le_bytes());
        pattern.extend_from_slice(&0_i32.to_le_bytes());
        pattern.extend_from_slice(&2_u32.to_le_bytes());
        pattern.extend_from_slice(&0_u32.to_le_bytes());
        pattern.extend_from_slice(&[0, 0, 0, 0]);
        pattern.extend_from_slice(&[0, 0, 255, 0]);
        pattern.extend_from_slice(&[0_u8; 16]);
        for _ in 0..8 {
            pattern.extend_from_slice(&[0b1010_1010, 0, 0, 0]);
        }
        assert_eq!(pattern.len(), 100);
        records.extend(record(META_DIBCREATEPATTERNBRUSH, &pattern));

        let mut pen = Vec::new();
        pen.extend_from_slice(&PS_NULL.to_le_bytes());
        pen.extend_from_slice(&1_i16.to_le_bytes());
        pen.extend_from_slice(&0_i16.to_le_bytes());
        pen.extend_from_slice(&0_u32.to_le_bytes());
        records.extend(record(META_CREATEPENINDIRECT, &pen));

        records.extend(record(META_SELECTOBJECT, &0_u16.to_le_bytes()));
        records.extend(record(META_SELECTOBJECT, &1_u16.to_le_bytes()));

        let mut polygon = Vec::new();
        polygon.extend_from_slice(&4_u16.to_le_bytes());
        for (x, y) in [(20_i16, 20_i16), (80, 20), (80, 80), (20, 80)] {
            polygon.extend_from_slice(&x.to_le_bytes());
            polygon.extend_from_slice(&y.to_le_bytes());
        }
        records.extend(record(META_POLYGON, &polygon));
        records.extend(record(META_EOF, &[]));

        let total_len = 18 + records.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&9_u16.to_le_bytes());
        bytes.extend_from_slice(&0x0300_u16.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(total_len / 2).unwrap().to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&53_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend(records);
        bytes
    }

    #[test]
    fn rasterizes_bounded_8x8_monochrome_dib_pattern_brush() {
        let image = rasterize_wmf_preview(&synthetic_pattern_polygon(), 100, 100)
            .expect("bounded pattern brush");
        let red = ((50 * 100 + 50) * 4) as usize;
        let black = ((50 * 100 + 51) * 4) as usize;
        assert_eq!(&image.rgba[red..red + 4], &[255, 0, 0, 255]);
        assert_eq!(&image.rgba[black..black + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn normalizes_positive_height_pattern_rows_from_bottom_up_dib() {
        let mut params = Vec::new();
        params.extend_from_slice(&BS_DIBPATTERNPT.to_le_bytes());
        params.extend_from_slice(&DIB_RGB_COLORS.to_le_bytes());
        params.extend_from_slice(&40_u32.to_le_bytes());
        params.extend_from_slice(&8_i32.to_le_bytes());
        params.extend_from_slice(&8_i32.to_le_bytes());
        params.extend_from_slice(&1_u16.to_le_bytes());
        params.extend_from_slice(&1_u16.to_le_bytes());
        params.extend_from_slice(&BI_RGB.to_le_bytes());
        params.extend_from_slice(&32_u32.to_le_bytes());
        params.extend_from_slice(&0_i32.to_le_bytes());
        params.extend_from_slice(&0_i32.to_le_bytes());
        params.extend_from_slice(&2_u32.to_le_bytes());
        params.extend_from_slice(&0_u32.to_le_bytes());
        params.extend_from_slice(&[0, 0, 0, 0]);
        params.extend_from_slice(&[0, 0, 255, 0]);
        params.extend_from_slice(&[0_u8; 16]);
        for row in [0x01_u8, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80] {
            params.extend_from_slice(&[row, 0, 0, 0]);
        }

        let brush = parse_legacy_pattern_brush(&params).expect("bottom-up pattern brush");
        assert_eq!(
            brush.pattern.expect("pattern").rows,
            [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01]
        );
    }

    #[test]
    fn rejects_pattern_brush_outside_proven_dib_profile() {
        let mut params = vec![0_u8; 100];
        params[0..2].copy_from_slice(&BS_DIBPATTERNPT.to_le_bytes());
        params[2..4].copy_from_slice(&DIB_RGB_COLORS.to_le_bytes());
        params[4..8].copy_from_slice(&40_u32.to_le_bytes());
        params[8..12].copy_from_slice(&9_i32.to_le_bytes());
        params[12..16].copy_from_slice(&8_i32.to_le_bytes());
        params[16..18].copy_from_slice(&1_u16.to_le_bytes());
        params[18..20].copy_from_slice(&1_u16.to_le_bytes());
        params[20..24].copy_from_slice(&BI_RGB.to_le_bytes());
        params[36..40].copy_from_slice(&2_u32.to_le_bytes());
        assert!(parse_legacy_pattern_brush(&params).is_err());
    }

    #[test]
    fn rasterizes_bounded_polygon_without_platform_gdi() {
        let image = rasterize_wmf_preview(&synthetic_polygon(), 100, 100).expect("preview");
        assert_eq!((image.width, image.height), (100, 100));
        let center = ((50 * 100 + 50) * 4) as usize;
        assert_eq!(&image.rgba[center..center + 4], &[255, 0, 0, 255]);
        let corner = ((5 * 100 + 5) * 4) as usize;
        assert_eq!(&image.rgba[corner..corner + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn accepts_bounded_region_select_only_before_explicit_clip_rect() {
        let mut bytes = synthetic_polygon();
        bytes[10..12].copy_from_slice(&3_u16.to_le_bytes());

        insert_record_before_function(
            &mut bytes,
            META_POLYGON,
            record(META_CREATEREGION, &region_compatibility_params()),
        );
        insert_record_before_function(
            &mut bytes,
            META_POLYGON,
            record(META_SELECTOBJECT, &2_u16.to_le_bytes()),
        );

        let mut clip = Vec::new();
        for value in [60_i16, 60, 40, 40] {
            clip.extend_from_slice(&value.to_le_bytes());
        }
        insert_record_before_function(
            &mut bytes,
            META_POLYGON,
            record(META_INTERSECTCLIPRECT, &clip),
        );

        let image = rasterize_wmf_preview(&bytes, 100, 100).expect("region compatibility");
        let center = ((50 * 100 + 50) * 4) as usize;
        assert_eq!(&image.rgba[center..center + 4], &[255, 0, 0, 255]);
        let clipped = ((30 * 100 + 30) * 4) as usize;
        assert_eq!(
            &image.rgba[clipped..clipped + 4],
            &[0, 0, 0, 0],
            "explicit INTERSECTCLIPRECT, not Region SELECTOBJECT, supplies clipping"
        );
    }

    #[test]
    fn rejects_region_select_without_immediate_explicit_clip_rect() {
        let mut bytes = synthetic_polygon();
        bytes[10..12].copy_from_slice(&3_u16.to_le_bytes());
        insert_record_before_function(
            &mut bytes,
            META_POLYGON,
            record(META_CREATEREGION, &region_compatibility_params()),
        );
        insert_record_before_function(
            &mut bytes,
            META_POLYGON,
            record(META_SELECTOBJECT, &2_u16.to_le_bytes()),
        );
        assert!(rasterize_wmf_preview(&bytes, 100, 100).is_err());
    }

    #[test]
    fn rejects_region_outside_exact_bounded_compatibility_profile() {
        let mut params = region_compatibility_params();
        params[2..4].copy_from_slice(&5_i16.to_le_bytes());

        let mut bytes = synthetic_polygon();
        bytes[10..12].copy_from_slice(&3_u16.to_le_bytes());
        insert_record_before_function(&mut bytes, META_POLYGON, record(META_CREATEREGION, &params));
        assert!(rasterize_wmf_preview(&bytes, 100, 100).is_err());
    }

    #[test]
    fn keeps_selectclipregion_fail_closed() {
        let mut bytes = synthetic_polygon();
        bytes[10..12].copy_from_slice(&3_u16.to_le_bytes());
        insert_record_before_function(
            &mut bytes,
            META_POLYGON,
            record(META_CREATEREGION, &region_compatibility_params()),
        );
        insert_record_before_function(
            &mut bytes,
            META_POLYGON,
            record(META_SELECTCLIPREGION, &2_u16.to_le_bytes()),
        );
        assert!(rasterize_wmf_preview(&bytes, 100, 100).is_err());
    }

    #[test]
    fn rejects_object_creation_when_header_declares_zero_slots() {
        let mut bytes = synthetic_polygon();
        bytes[10..12].copy_from_slice(&0_u16.to_le_bytes());
        assert!(rasterize_wmf_preview(&bytes, 100, 100).is_err());
    }

    #[test]
    fn ignores_bounded_private_mfcomment_and_uses_wmf_fallback() {
        let mut bytes = synthetic_polygon();
        let mut params = Vec::new();
        params.extend_from_slice(&META_ESCAPE_ENHANCED_METAFILE.to_le_bytes());
        params.extend_from_slice(&6_u16.to_le_bytes());
        params.extend_from_slice(b"PUB123");
        insert_record_before_eof(&mut bytes, record(META_ESCAPE, &params));

        let image = rasterize_wmf_preview(&bytes, 100, 100).expect("private comment ignored");
        let center = ((50 * 100 + 50) * 4) as usize;
        assert_eq!(&image.rgba[center..center + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn accepts_bounded_multichunk_wmfc_metadata_without_emf_execution() {
        let mut bytes = synthetic_polygon();
        let current = [1_u8, 2, 3, 4];
        let mut body = Vec::new();
        body.extend_from_slice(&WMFC_COMMENT_IDENTIFIER.to_le_bytes());
        body.extend_from_slice(&WMFC_COMMENT_TYPE.to_le_bytes());
        body.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        body.extend_from_slice(&0_u16.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&2_u32.to_le_bytes());
        body.extend_from_slice(&u32::try_from(current.len()).unwrap().to_le_bytes());
        body.extend_from_slice(&8_u32.to_le_bytes());
        body.extend_from_slice(&12_u32.to_le_bytes());
        body.extend_from_slice(&current);

        let mut params = Vec::new();
        params.extend_from_slice(&META_ESCAPE_ENHANCED_METAFILE.to_le_bytes());
        params.extend_from_slice(&u16::try_from(body.len()).unwrap().to_le_bytes());
        params.extend_from_slice(&body);
        insert_record_before_eof(&mut bytes, record(META_ESCAPE, &params));

        assert!(rasterize_wmf_preview(&bytes, 100, 100).is_ok());
    }

    #[test]
    fn rejects_malformed_wmfc_comment_metadata() {
        let mut body = Vec::new();
        body.extend_from_slice(&WMFC_COMMENT_IDENTIFIER.to_le_bytes());
        body.extend_from_slice(&WMFC_COMMENT_TYPE.to_le_bytes());
        body.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        body.extend_from_slice(&0_u16.to_le_bytes());
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());

        let mut params = Vec::new();
        params.extend_from_slice(&META_ESCAPE_ENHANCED_METAFILE.to_le_bytes());
        params.extend_from_slice(&u16::try_from(body.len()).unwrap().to_le_bytes());
        params.extend_from_slice(&body);

        assert!(validate_meta_escape(&params).is_err());
    }

    #[test]
    fn accepts_bounded_postscript_ignore_without_postscript_execution() {
        let mut bytes = synthetic_polygon();
        let mut params = Vec::new();
        params.extend_from_slice(&META_ESCAPE_POSTSCRIPT_IGNORE.to_le_bytes());
        params.extend_from_slice(&2_u16.to_le_bytes());
        params.extend_from_slice(&[0x01, 0x00]);
        insert_record_before_eof(&mut bytes, record(META_ESCAPE, &params));

        let image = rasterize_wmf_preview(&bytes, 100, 100).expect("POSTSCRIPT_IGNORE no-op");
        let center = ((50 * 100 + 50) * 4) as usize;
        assert_eq!(&image.rgba[center..center + 4], &[255, 0, 0, 255]);

        let mut wrong_count = params.clone();
        wrong_count[2..4].copy_from_slice(&1_u16.to_le_bytes());
        assert!(validate_meta_escape(&wrong_count).is_err());

        let mut oversized = params;
        oversized.extend_from_slice(&[0, 0]);
        assert!(validate_meta_escape(&oversized).is_err());
    }

    #[test]
    fn accepts_bounded_text_color_setter_without_text_playback() {
        let mut bytes = synthetic_polygon();
        insert_record_before_eof(
            &mut bytes,
            record(META_SETTEXTCOLOR, &0x0000_00ff_u32.to_le_bytes()),
        );
        let image = rasterize_wmf_preview(&bytes, 100, 100).expect("text color setter ignored");
        let center = ((50 * 100 + 50) * 4) as usize;
        assert_eq!(&image.rgba[center..center + 4], &[255, 0, 0, 255]);

        let mut truncated = synthetic_polygon();
        insert_record_before_eof(&mut truncated, record(META_SETTEXTCOLOR, &[0, 0]));
        assert!(rasterize_wmf_preview(&truncated, 100, 100).is_err());

        let mut oversized = synthetic_polygon();
        insert_record_before_eof(
            &mut oversized,
            record(META_SETTEXTCOLOR, &[0, 0, 0, 0, 0, 0]),
        );
        assert!(rasterize_wmf_preview(&oversized, 100, 100).is_err());
    }

    #[test]
    fn rejects_unknown_record_instead_of_executing_it() {
        let mut bytes = synthetic_polygon();
        let eof = bytes.len() - 6;
        let unknown = record(0x7777, &[]);
        bytes.splice(eof..eof, unknown.clone());
        let words = u32::try_from(bytes.len() / 2).unwrap();
        bytes[6..10].copy_from_slice(&words.to_le_bytes());
        assert!(rasterize_wmf_preview(&bytes, 100, 100).is_err());
    }

    #[test]
    fn rejects_trailing_bytes_and_records_after_eof() {
        let mut trailing = synthetic_polygon();
        trailing.extend_from_slice(&[0, 0]);
        assert!(rasterize_wmf_preview(&trailing, 100, 100).is_err());

        let mut after_eof = synthetic_polygon();
        let extra = record(META_SETBKMODE, &1u16.to_le_bytes());
        after_eof.extend_from_slice(&extra);
        let words = u32::try_from(after_eof.len() / 2).unwrap();
        after_eof[6..10].copy_from_slice(&words.to_le_bytes());
        assert!(rasterize_wmf_preview(&after_eof, 100, 100).is_err());
    }

    #[test]
    fn clips_far_off_canvas_line_before_playback() {
        let rect = RectPx::full(100, 100);
        assert_eq!(
            clip_line_to_rect(rect, i32::MIN / 2, 50, i32::MAX / 2, 50),
            Some((0, 50, 99, 50))
        );
    }

    #[test]
    fn clips_extreme_diagonal_without_intersection_overflow() {
        let rect = RectPx::full(100, 100);
        let clipped = clip_line_to_rect(rect, i32::MIN, i32::MIN, i32::MAX, i32::MAX)
            .expect("extreme diagonal intersects bounded canvas");
        assert_eq!(clipped, (0, 0, 99, 99));
    }

    #[test]
    fn raster_work_budget_fails_closed() {
        let mut canvas = Canvas::new(1, 1).expect("bounded canvas");
        canvas
            .charge_work(MAX_RASTER_WORK_UNITS)
            .expect("exact work limit");
        assert!(canvas.charge_work(1).is_err());
    }

    #[test]
    fn bounds_requested_output_size() {
        let image = rasterize_wmf_preview(&synthetic_polygon(), 10_000, 5_000).expect("preview");
        assert!(image.width <= MAX_OUTPUT_SIDE);
        assert!(image.height <= MAX_OUTPUT_SIDE);
        assert!(u64::from(image.width) * u64::from(image.height) <= MAX_OUTPUT_PIXELS);
    }
}
