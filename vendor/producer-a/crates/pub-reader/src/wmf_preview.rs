use crate::wmf::validate_wmf_metafile;
use anyhow::{Result, anyhow, bail};

const MAX_WMF_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;
const MAX_OBJECTS: usize = 4096;
const MAX_POINTS_PER_RECORD: usize = 4096;
const MAX_OUTPUT_SIDE: u32 = 1024;
const MAX_OUTPUT_PIXELS: u64 = 1024 * 1024;

const META_EOF: u16 = 0x0000;
const META_SAVEDC: u16 = 0x001e;
const META_SETBKMODE: u16 = 0x0102;
const META_SETMAPMODE: u16 = 0x0103;
const META_SETROP2: u16 = 0x0104;
const META_SETRELABS: u16 = 0x0105;
const META_SETPOLYFILLMODE: u16 = 0x0106;
const META_SETSTRETCHBLTMODE: u16 = 0x0107;
const META_RESTOREDC: u16 = 0x0127;
const META_SELECTOBJECT: u16 = 0x012d;
const META_SETTEXTALIGN: u16 = 0x012e;
const META_DIBCREATEPATTERNBRUSH: u16 = 0x0142;
const META_DELETEOBJECT: u16 = 0x01f0;
const META_SETBKCOLOR: u16 = 0x0201;
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

const PS_SOLID: u16 = 0;
const PS_NULL: u16 = 5;
const PS_INSIDEFRAME: u16 = 6;
const BS_SOLID: u16 = 0;
const BS_NULL: u16 = 1;

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
struct Brush {
    style: u16,
    color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GdiObject {
    Pen(Pen),
    Brush(Brush),
    Unsupported,
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
            },
        }
    }
}

struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
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
        })
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

    fn span(&mut self, clip: RectPx, y: i32, x0: i32, x1: i32, color: Color) {
        if y < clip.top || y >= clip.bottom || y < 0 || y >= self.height as i32 {
            return;
        }
        let start = x0.min(x1).max(clip.left).max(0);
        let end = x0.max(x1).min(clip.right - 1).min(self.width as i32 - 1);
        if start > end {
            return;
        }
        for x in start..=end {
            self.set(clip, x, y, color);
        }
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    read_u16(bytes, offset).map(|value| i16::from_le_bytes(value.to_le_bytes()))
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

fn draw_disk(canvas: &mut Canvas, clip: RectPx, x: i32, y: i32, radius: i32, color: Color) {
    if radius <= 0 {
        canvas.set(clip, x, y, color);
        return;
    }
    let rr = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= rr {
                canvas.set(clip, x + dx, y + dy, color);
            }
        }
    }
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
            let dy = i64::from(y1) - i64::from(y0);
            if dy == 0 {
                return None;
            }
            let x = i64::from(x0)
                + (i64::from(x1) - i64::from(x0)) * (i64::from(bottom) - i64::from(y0)) / dy;
            (i32::try_from(x).ok()?, bottom)
        } else if code & 4 != 0 {
            let dy = i64::from(y1) - i64::from(y0);
            if dy == 0 {
                return None;
            }
            let x = i64::from(x0)
                + (i64::from(x1) - i64::from(x0)) * (i64::from(rect.top) - i64::from(y0)) / dy;
            (i32::try_from(x).ok()?, rect.top)
        } else if code & 2 != 0 {
            let dx = i64::from(x1) - i64::from(x0);
            if dx == 0 {
                return None;
            }
            let y = i64::from(y0)
                + (i64::from(y1) - i64::from(y0)) * (i64::from(right) - i64::from(x0)) / dx;
            (right, i32::try_from(y).ok()?)
        } else {
            let dx = i64::from(x1) - i64::from(x0);
            if dx == 0 {
                return None;
            }
            let y = i64::from(y0)
                + (i64::from(y1) - i64::from(y0)) * (i64::from(rect.left) - i64::from(x0)) / dx;
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
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    width: u32,
    color: Color,
) {
    let Some((cx0, cy0, cx1, cy1)) = clip_line_to_rect(clip, x0, y0, x1, y1) else {
        return;
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
        draw_disk(canvas, clip, x0, y0, radius, color);
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
}

fn draw_polyline(
    canvas: &mut Canvas,
    state: &PlaybackState,
    points: &[(i32, i32)],
    closed: bool,
) {
    if state.pen.style == PS_NULL || points.len() < 2 {
        return;
    }
    let width = mapped_pen_width(state, canvas);
    for pair in points.windows(2) {
        draw_line(
            canvas,
            state.clip,
            pair[0].0,
            pair[0].1,
            pair[1].0,
            pair[1].1,
            width,
            state.pen.color,
        );
    }
    if closed {
        let first = points[0];
        let last = points[points.len() - 1];
        draw_line(
            canvas,
            state.clip,
            last.0,
            last.1,
            first.0,
            first.1,
            width,
            state.pen.color,
        );
    }
}

fn fill_rings(
    canvas: &mut Canvas,
    state: &PlaybackState,
    rings: &[Vec<(i32, i32)>],
    color: Color,
) {
    let Some(min_y) = rings.iter().flat_map(|ring| ring.iter().map(|point| point.1)).min() else {
        return;
    };
    let Some(max_y) = rings.iter().flat_map(|ring| ring.iter().map(|point| point.1)).max() else {
        return;
    };
    let start_y = min_y.max(state.clip.top).max(0);
    let end_y = max_y
        .min(state.clip.bottom.saturating_sub(1))
        .min(canvas.height as i32 - 1);

    for y in start_y..=end_y {
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
                    canvas.span(state.clip, y, x0, x1, color);
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
                            canvas.span(
                                state.clip,
                                y,
                                from.ceil() as i32,
                                x.floor() as i32,
                                color,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn allocate_object(objects: &mut [Option<GdiObject>], object: GdiObject) -> Result<()> {
    let Some(slot) = objects.iter_mut().find(|slot| slot.is_none()) else {
        bail!("WMF object table is full");
    };
    *slot = Some(object);
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

fn validate_enhanced_metafile_escape(params: &[u8]) -> Result<()> {
    if params.len() < 4 {
        bail!("WMF META_ESCAPE record is truncated");
    }
    let escape =
        read_u16(params, 0).ok_or_else(|| anyhow!("WMF escape function is truncated"))?;
    if escape != META_ESCAPE_ENHANCED_METAFILE {
        bail!("unsupported WMF escape function 0x{escape:04x}");
    }
    let byte_count = usize::from(
        read_u16(params, 2).ok_or_else(|| anyhow!("WMF escape byte count is truncated"))?,
    );
    if byte_count > params.len().saturating_sub(4) {
        bail!("WMF enhanced-metafile escape payload is truncated");
    }
    if byte_count < 34 {
        bail!("WMF enhanced-metafile escape payload is too short");
    }
    let body = &params[4..4 + byte_count];
    let identifier =
        read_u32(body, 0).ok_or_else(|| anyhow!("WMF EMF comment identifier is truncated"))?;
    let comment_type =
        read_u32(body, 4).ok_or_else(|| anyhow!("WMF EMF comment type is truncated"))?;
    if identifier != 0x4346_4d57 || comment_type != 1 {
        bail!("unsupported WMF escape comment payload");
    }
    let current_record_size = usize::try_from(
        read_u32(body, 22).ok_or_else(|| anyhow!("WMF EMF current record size is truncated"))?,
    )
    .map_err(|_| anyhow!("WMF EMF current record size overflow"))?;
    if current_record_size > 8192 {
        bail!("WMF embedded EMF segment exceeds bounded record size");
    }
    let segment_size = usize::try_from(
        read_u32(body, 30).ok_or_else(|| anyhow!("WMF EMF segment size is truncated"))?,
    )
    .map_err(|_| anyhow!("WMF EMF segment size overflow"))?;
    if segment_size != current_record_size || 34 + segment_size > body.len() {
        bail!("WMF embedded EMF segment length mismatch");
    }
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
    let mut objects = vec![None; object_count.max(1)];

    let mut offset = 18usize;
    let mut records = 0usize;
    let mut eof_seen = false;

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
        let function =
            read_u16(bytes, offset + 4).ok_or_else(|| anyhow!("WMF record function is truncated"))?;
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
                let mode = read_u16(params, 0).ok_or_else(|| anyhow!("WMF SETROP2 is truncated"))?;
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
            META_SETWINDOWORG => {
                state.window_org_y = i32::from(
                    read_i16(params, 0).ok_or_else(|| anyhow!("WMF SETWINDOWORG y is truncated"))?,
                );
                state.window_org_x = i32::from(
                    read_i16(params, 2).ok_or_else(|| anyhow!("WMF SETWINDOWORG x is truncated"))?,
                );
            }
            META_SETWINDOWEXT => {
                state.window_ext_y = i32::from(
                    read_i16(params, 0).ok_or_else(|| anyhow!("WMF SETWINDOWEXT y is truncated"))?,
                );
                state.window_ext_x = i32::from(
                    read_i16(params, 2).ok_or_else(|| anyhow!("WMF SETWINDOWEXT x is truncated"))?,
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
                };
                allocate_object(&mut objects, GdiObject::Brush(brush))?;
            }
            META_DIBCREATEPATTERNBRUSH | META_CREATEREGION => {
                allocate_object(&mut objects, GdiObject::Unsupported)?;
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
                    GdiObject::Unsupported => {
                        bail!("WMF selects an unsupported graphics object");
                    }
                }
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
                        fill_rings(
                            &mut canvas,
                            &state,
                            std::slice::from_ref(&points),
                            state.brush.color,
                        );
                    }
                    draw_polyline(&mut canvas, &state, &points, true);
                } else {
                    draw_polyline(&mut canvas, &state, &points, false);
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
                    fill_rings(&mut canvas, &state, &rings, state.brush.color);
                }
                for ring in &rings {
                    draw_polyline(&mut canvas, &state, ring, true);
                }
            }
            META_RECTANGLE => {
                let bottom = read_i16(params, 0)
                    .ok_or_else(|| anyhow!("WMF RECTANGLE bottom is truncated"))?;
                let right =
                    read_i16(params, 2).ok_or_else(|| anyhow!("WMF RECTANGLE right is truncated"))?;
                let top =
                    read_i16(params, 4).ok_or_else(|| anyhow!("WMF RECTANGLE top is truncated"))?;
                let left =
                    read_i16(params, 6).ok_or_else(|| anyhow!("WMF RECTANGLE left is truncated"))?;
                let logical = [(left, top), (right, top), (right, bottom), (left, bottom)];
                let points = map_points(&state, &canvas, &logical)?;
                if state.brush.style != BS_NULL {
                    fill_rings(
                        &mut canvas,
                        &state,
                        std::slice::from_ref(&points),
                        state.brush.color,
                    );
                }
                draw_polyline(&mut canvas, &state, &points, true);
            }
            META_ESCAPE => validate_enhanced_metafile_escape(params)?,
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
    fn bounds_requested_output_size() {
        let image = rasterize_wmf_preview(&synthetic_polygon(), 10_000, 5_000).expect("preview");
        assert!(image.width <= MAX_OUTPUT_SIDE);
        assert!(image.height <= MAX_OUTPUT_SIDE);
        assert!(
            u64::from(image.width) * u64::from(image.height) <= MAX_OUTPUT_PIXELS
        );
    }
}
