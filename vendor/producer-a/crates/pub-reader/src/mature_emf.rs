//! Conservative source-backed *derived* EMF bitmap preview for mature Publisher.
//! This is not an EMF renderer: only one bounded EMR_STRETCHDIBITS BI_RGB layer
//! is decoded. World transforms, clipping, vector paint, and EMF transparency
//! remain unsupported; callers must expose Partial/Needs review.
use crate::{
    ESCHER_DELAY_STREAM_PATH, ESCHER_STREAM_PATH, PubAssetUse, PubSourceGraph,
    build_pub_asset_manifest,
};
use anyhow::{Context, Result, anyhow, bail};
use flate2::bufread::ZlibDecoder;
use pub_core::StreamPath;
use pub_escher::{
    BlipKind, BlipMetafileCompression, ValidatedBlip, inspect_validated_delayed_blips_prefix,
};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};

pub const MATURE_OFFICEART_EMF_BITMAP_PREVIEW_SOURCE_V1: &str =
    "mature-officeart-emf-birgb-partial-preview-v1";

const MAX_LOGICAL_EMF_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECORD_COUNT: usize = 4_096;
const MAX_OUTPUT_PIXELS: u64 = 1_048_576;
const MAX_OUTPUT_SIDE: u32 = 2_048;
const EMR_HEADER: u32 = 1;
const EMR_EOF: u32 = 14;
const EMR_STRETCHDIBITS: u32 = 81;
const SRCCOPY: u32 = 0x00cc_0020;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubMatureEmfBitmapPreviewSource {
    pub slot: u32,
    pub uses: Vec<PubAssetUse>,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// The rest of the EMF record stream is *not* painted by this preview.
    pub other_emr_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PubMatureEmfBitmapPreviewBundle {
    pub sources: Vec<PubMatureEmfBitmapPreviewSource>,
    pub vector_only_count: usize,
    pub rejected_source_count: usize,
}

fn slice_span(bytes: &[u8], offset: u64, len: u64) -> Result<&[u8]> {
    let start = usize::try_from(offset).context("EMF source offset overflow")?;
    let len = usize::try_from(len).context("EMF source length overflow")?;
    let end = start.checked_add(len).context("EMF source end overflow")?;
    bytes
        .get(start..end)
        .context("EMF source span outside stream")
}

fn bytes_at(bytes: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    let end = offset.checked_add(len).context("EMF field span overflow")?;
    bytes.get(offset..end).context("EMF field truncated")
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    let a: [u8; 4] = bytes_at(bytes, offset, 4)?
        .try_into()
        .map_err(|_| anyhow!("EMF u32 truncated"))?;
    Ok(u32::from_le_bytes(a))
}

fn i32_at(bytes: &[u8], offset: usize) -> Result<i32> {
    Ok(u32_at(bytes, offset)? as i32)
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    let a: [u8; 2] = bytes_at(bytes, offset, 2)?
        .try_into()
        .map_err(|_| anyhow!("EMF u16 truncated"))?;
    Ok(u16::from_le_bytes(a))
}

fn logical_emf_bytes(delayed: &[u8], record: &ValidatedBlip) -> Result<Vec<u8>> {
    if record.kind != BlipKind::Emf
        || !record.internal_uid_verified
        || !record.format_header_verified
    {
        bail!("not a strictly validated OfficeArt EMF");
    }
    let logical_len = usize::try_from(
        record
            .logical_payload_len
            .context("EMF missing logical length")?,
    )
    .context("EMF logical length overflow")?;
    if logical_len > MAX_LOGICAL_EMF_BYTES {
        bail!("EMF exceeds bounded bitmap-preview source limit");
    }
    let stored = slice_span(
        delayed,
        record.payload_source.offset,
        record.payload_source.len,
    )?;
    let logical = match record.metafile_compression {
        Some(BlipMetafileCompression::Uncompressed) => {
            if stored.len() != logical_len {
                bail!("uncompressed EMF logical length mismatch");
            }
            stored.to_vec()
        }
        Some(BlipMetafileCompression::Deflate) => {
            let mut decoder = ZlibDecoder::new(Cursor::new(stored));
            let mut logical = Vec::new();
            decoder
                .by_ref()
                .take(u64::try_from(logical_len)? + 1)
                .read_to_end(&mut logical)
                .context("bounded EMF inflate failed")?;
            if logical.len() != logical_len || decoder.total_in() != stored.len() as u64 {
                bail!("EMF inflated length/trailing-compressed-data mismatch");
            }
            logical
        }
        None => bail!("EMF compression profile unavailable"),
    };
    let digest = format!("{:x}", Sha256::digest(&logical));
    if record.logical_payload_sha256.as_deref() != Some(digest.as_str()) {
        bail!("EMF logical SHA-256 differs from strictly validated envelope");
    }
    Ok(logical)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BitmapLayer {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn decode_stretchdibits_layer(record: &[u8]) -> Result<BitmapLayer> {
    if record.len() < 80 || u32_at(record, 0)? != EMR_STRETCHDIBITS {
        bail!("EMR_STRETCHDIBITS fixed header unavailable");
    }
    let declared = usize::try_from(u32_at(record, 4)?)?;
    if declared != record.len() {
        bail!("EMR_STRETCHDIBITS record length mismatch");
    }
    let bmi_offset = usize::try_from(u32_at(record, 48)?)?;
    let bmi_len = usize::try_from(u32_at(record, 52)?)?;
    let bits_offset = usize::try_from(u32_at(record, 56)?)?;
    let bits_len = usize::try_from(u32_at(record, 60)?)?;
    let bmi = bytes_at(record, bmi_offset, bmi_len)?;
    let bits = bytes_at(record, bits_offset, bits_len)?;
    let bmi_end = bmi_offset
        .checked_add(bmi_len)
        .context("DIB header end overflow")?;
    if bmi_offset < 80 || bmi_len != 40 || bits_offset < bmi_end {
        bail!("unsupported EMF DIB offset/header layout");
    }
    if u32_at(record, 64)? != 0 || u32_at(record, 68)? != SRCCOPY {
        bail!("unsupported EMF DIB color usage or raster operation");
    }

    let header_size = u32_at(bmi, 0)?;
    let width_i = i32_at(bmi, 4)?;
    let height_i = i32_at(bmi, 8)?;
    let width = u32::try_from(width_i).context("nonpositive DIB width")?;
    let height = height_i.unsigned_abs();
    if header_size != 40
        || u16_at(bmi, 12)? != 1
        || u16_at(bmi, 14)? != 24
        || u32_at(bmi, 16)? != 0
        || width == 0
        || height == 0
        || width > MAX_OUTPUT_SIDE
        || height > MAX_OUTPUT_SIDE
    {
        bail!("unsupported EMF BI_RGB bitmap profile");
    }
    let pixel_count = u64::from(width)
        .checked_mul(u64::from(height))
        .context("EMF DIB pixel overflow")?;
    if pixel_count > MAX_OUTPUT_PIXELS {
        bail!("EMF DIB work exceeds preview budget");
    }
    let row_stride = u64::from(width)
        .checked_mul(3)
        .and_then(|n| n.checked_add(3))
        .map(|n| n & !3)
        .context("EMF DIB row stride overflow")?;
    let required = row_stride
        .checked_mul(u64::from(height))
        .context("EMF DIB payload size overflow")?;
    if u64::try_from(bits.len())? != required {
        bail!("EMF DIB payload length mismatch");
    }
    let image_size = u64::from(u32_at(bmi, 20)?);
    if image_size != 0 && image_size != required {
        bail!("EMF DIB declared image size mismatch");
    }

    if i32_at(record, 32)? != 0
        || i32_at(record, 36)? != 0
        || i32_at(record, 40)? != width_i
        || i32_at(record, 44)? != i32::try_from(height)?
        || i32_at(record, 72)? <= 0
        || i32_at(record, 76)? <= 0
    {
        bail!("EMF DIB does not cover the full positive source region");
    }
    let capacity = usize::try_from(
        pixel_count
            .checked_mul(4)
            .context("EMF preview buffer overflow")?,
    )?;
    let mut rgba = Vec::with_capacity(capacity);
    let stride = usize::try_from(row_stride)?;
    let h = usize::try_from(height)?;
    let w = usize::try_from(width)?;
    for y in 0..h {
        let input_y = if height_i > 0 { h - 1 - y } else { y };
        let row_start = input_y
            .checked_mul(stride)
            .context("EMF DIB row overflow")?;
        let row = bytes_at(bits, row_start, stride)?;
        for x in 0..w {
            let rgb = bytes_at(row, x * 3, 3)?;
            rgba.extend_from_slice(&[rgb[2], rgb[1], rgb[0], 255]);
        }
    }
    if rgba.len() != capacity {
        bail!("EMF preview RGBA length mismatch");
    }
    Ok(BitmapLayer {
        width,
        height,
        rgba,
    })
}

/// Bounded EMF framing: one bitmap layer only. All other paint/state records
/// are unmodeled, even when decoding succeeds. The Viewer must warn about them.
fn decode_emf_bitmap_layer(logical: &[u8]) -> Result<Option<(BitmapLayer, usize)>> {
    let mut offset = 0usize;
    let mut count = 0usize;
    let mut bitmap = None;
    let mut other_emr_count = 0usize;
    let mut saw_eof = false;

    while offset < logical.len() {
        if count >= MAX_RECORD_COUNT {
            bail!("EMF contains too many records");
        }
        let kind = u32_at(logical, offset)?;
        let record_len = usize::try_from(u32_at(logical, offset + 4)?)?;
        if record_len < 8 || record_len % 4 != 0 {
            bail!("invalid EMF record size");
        }
        let record = bytes_at(logical, offset, record_len)?;
        if count == 0 && (kind != EMR_HEADER || record_len < 88) {
            bail!("EMF header must be first and bounded");
        }
        if saw_eof {
            bail!("EMF data present after EOF");
        }
        match kind {
            EMR_HEADER => {
                if count != 0 {
                    bail!("duplicate EMF header");
                }
            }
            EMR_EOF => {
                if record_len < 20 {
                    bail!("short EMF EOF");
                }
                saw_eof = true;
            }
            EMR_STRETCHDIBITS => {
                if bitmap.is_some() {
                    bail!("multiple competing EMF bitmap records");
                }
                bitmap = Some(decode_stretchdibits_layer(record)?);
            }
            _ => other_emr_count += 1,
        }
        count += 1;
        offset = offset
            .checked_add(record_len)
            .context("EMF record offset overflow")?;
    }
    if !saw_eof || offset != logical.len() {
        bail!("EMF lacks complete terminal EOF");
    }
    Ok(bitmap.map(|b| (b, other_emr_count)))
}

/// Preserve exact source-graph node usages and a verified BLIP envelope;
/// derived RGBA is never an exact embedded image source.
pub fn build_mature_0x2c_emf_bitmap_preview_bundle_from_bytes(
    pub_bytes: &[u8],
    graph: &PubSourceGraph,
) -> Result<PubMatureEmfBitmapPreviewBundle> {
    let inventory = pub_cfb::inspect_reader(Cursor::new(pub_bytes))
        .context("inspect CFB for mature EMF bitmap preview")?;
    if !inventory
        .entries
        .iter()
        .any(|e| e.path == ESCHER_DELAY_STREAM_PATH)
    {
        return Ok(PubMatureEmfBitmapPreviewBundle::default());
    }
    let escher = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_STREAM_PATH)?;
    let delayed = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_DELAY_STREAM_PATH)?;
    let manifest = build_pub_asset_manifest(graph, &escher, &delayed)?;
    let validated = inspect_validated_delayed_blips_prefix(
        StreamPath(ESCHER_DELAY_STREAM_PATH.to_owned()),
        &delayed,
    );
    if validated.terminal_gap.is_some() || !validated.rejected_complete_blips.is_empty() {
        bail!("strict EMF delayed BLIP inventory contains rejected/trailing records");
    }
    let mut out = PubMatureEmfBitmapPreviewBundle::default();
    for asset in manifest
        .assets
        .iter()
        .filter(|asset| asset.blip_kind == Some(BlipKind::Emf))
    {
        let Some(source_span) = asset.blip_record_source.as_ref() else {
            out.rejected_source_count += 1;
            continue;
        };
        let Some(record) = validated
            .records
            .iter()
            .find(|r| &r.record_source == source_span)
        else {
            out.rejected_source_count += 1;
            continue;
        };
        if record.kind != BlipKind::Emf || record.effective_uid != asset.uid {
            out.rejected_source_count += 1;
            continue;
        }
        let maybe = logical_emf_bytes(&delayed, record)
            .and_then(|logical| decode_emf_bitmap_layer(&logical));
        match maybe {
            Ok(Some((bitmap, other_emr_count))) => {
                let mut uses = asset.uses.clone();
                uses.sort();
                uses.dedup();
                out.sources.push(PubMatureEmfBitmapPreviewSource {
                    slot: asset.slot,
                    uses,
                    width: bitmap.width,
                    height: bitmap.height,
                    rgba: bitmap.rgba,
                    other_emr_count,
                });
            }
            Ok(None) => out.vector_only_count += 1,
            Err(_) => out.rejected_source_count += 1,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u32(bytes: &mut [u8], at: usize, v: u32) {
        bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn synthetic_emf(height_signed: i32, rop: u32) -> Vec<u8> {
        // Two pixels per row; DIB rows padded to 8 bytes.
        let width = 2_u32;
        let height = 2_u32;
        let record_size = 80 + 40 + 16;
        let mut header = vec![0_u8; 88];
        push_u32(&mut header, 0, EMR_HEADER);
        push_u32(&mut header, 4, 88);
        let mut stretch = vec![0_u8; record_size];
        push_u32(&mut stretch, 0, EMR_STRETCHDIBITS);
        push_u32(&mut stretch, 4, record_size as u32);
        push_u32(&mut stretch, 40, width);
        push_u32(&mut stretch, 44, height);
        push_u32(&mut stretch, 48, 80);
        push_u32(&mut stretch, 52, 40);
        push_u32(&mut stretch, 56, 120);
        push_u32(&mut stretch, 60, 16);
        push_u32(&mut stretch, 68, rop);
        push_u32(&mut stretch, 72, width);
        push_u32(&mut stretch, 76, height);
        push_u32(&mut stretch, 80, 40);
        push_u32(&mut stretch, 84, width);
        push_u32(&mut stretch, 88, height_signed as u32);
        stretch[92..94].copy_from_slice(&1_u16.to_le_bytes());
        stretch[94..96].copy_from_slice(&24_u16.to_le_bytes());
        // Input row0 BGR: red, green; input row1: blue, white.
        stretch[120..136].copy_from_slice(&[
            0, 0, 255, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0,
        ]);
        let mut eof = vec![0_u8; 20];
        push_u32(&mut eof, 0, EMR_EOF);
        push_u32(&mut eof, 4, 20);
        let mut emf = header;
        emf.extend_from_slice(&stretch);
        emf.extend_from_slice(&eof);
        emf
    }

    #[test]
    fn decodes_bottom_up_and_top_down_bgr_without_guessing_alpha() {
        let a = decode_emf_bitmap_layer(&synthetic_emf(2, SRCCOPY))
            .expect("valid bottom-up")
            .expect("bitmap")
            .0;
        assert_eq!((a.width, a.height), (2, 2));
        assert_eq!(&a.rgba[0..4], &[255, 255, 255, 255]);
        let b = decode_emf_bitmap_layer(&synthetic_emf(-2, SRCCOPY))
            .expect("valid top-down")
            .expect("bitmap")
            .0;
        assert_eq!(&b.rgba[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn refuses_non_srccopy_truncated_bitmap_and_trailing_records() {
        assert!(decode_emf_bitmap_layer(&synthetic_emf(2, 0x0088_00c6)).is_err());
        let mut invalid = synthetic_emf(2, SRCCOPY);
        push_u32(&mut invalid, 88 + 56, 0xffff_fffc);
        assert!(decode_emf_bitmap_layer(&invalid).is_err());
        let mut trailing = synthetic_emf(2, SRCCOPY);
        trailing.extend_from_slice(&[0, 0, 0, 0]);
        assert!(decode_emf_bitmap_layer(&trailing).is_err());
    }

    #[test]
    fn refuses_competing_bitmap_and_unsupported_dib_profile() {
        let a = synthetic_emf(2, SRCCOPY);
        let mut doubled = a[..88 + 136].to_vec();
        doubled.extend_from_slice(&a[88..88 + 136]);
        doubled.extend_from_slice(&a[88 + 136..]);
        assert!(decode_emf_bitmap_layer(&doubled).is_err());
        let mut bad = synthetic_emf(2, SRCCOPY);
        bad[88 + 94..88 + 96].copy_from_slice(&32_u16.to_le_bytes());
        assert!(decode_emf_bitmap_layer(&bad).is_err());
    }
}
