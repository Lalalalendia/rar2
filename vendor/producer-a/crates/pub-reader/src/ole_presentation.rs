use anyhow::{Context, Result, bail};
use std::io::{Read, Seek};

const STANDARD_CLIPBOARD_MARKER_ANSI: u32 = 0xffff_ffff;
const STANDARD_CLIPBOARD_MARKER_UNICODE: u32 = 0xffff_fffe;
const CF_METAFILEPICT: u32 = 3;
const METAFILE_RESERVED2_LEN: usize = 18;
const MAX_PRESENTATION_BYTES: usize = 64 * 1024 * 1024;
const MAX_TARGET_DEVICE_BYTES: usize = 1024 * 1024;
const MAX_OLE_PRESENTATION_COUNT: usize = 999;
const OLE_PRES_STREAM_PREFIX: &str = "\u{2}OlePres";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OlePresentation<'a> {
    pub clipboard_format: u32,
    pub aspect: u32,
    pub lindex: u32,
    pub advf: u32,
    pub width: u32,
    pub height: u32,
    pub data: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyOleCachedPresentation {
    pub stream_path: String,
    pub stream_name: String,
    pub clipboard_format: u32,
    pub aspect: u32,
    pub lindex: u32,
    pub advf: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

pub fn parse_cf_metafilepict_ole_presentation(bytes: &[u8]) -> Result<OlePresentation<'_>> {
    if bytes.len() > MAX_PRESENTATION_BYTES {
        bail!("OLE presentation stream exceeds bounded size");
    }

    let marker = read_u32(bytes, 0).ok_or_else(|| anyhow::anyhow!("missing clipboard marker"))?;
    if !matches!(
        marker,
        STANDARD_CLIPBOARD_MARKER_ANSI | STANDARD_CLIPBOARD_MARKER_UNICODE
    ) {
        bail!("unsupported OLE presentation clipboard-format encoding");
    }

    let clipboard_format =
        read_u32(bytes, 4).ok_or_else(|| anyhow::anyhow!("missing clipboard format"))?;
    if clipboard_format != CF_METAFILEPICT {
        bail!("unsupported OLE presentation clipboard format {clipboard_format}");
    }

    let target_device_size = usize::try_from(
        read_u32(bytes, 8).ok_or_else(|| anyhow::anyhow!("missing target device size"))?,
    )
    .map_err(|_| anyhow::anyhow!("target device size overflow"))?;
    if target_device_size < 4 {
        bail!("invalid OLE target device size {target_device_size}");
    }
    if target_device_size > MAX_TARGET_DEVICE_BYTES {
        bail!("OLE target device exceeds bounded size");
    }

    let fields = 8usize
        .checked_add(target_device_size)
        .ok_or_else(|| anyhow::anyhow!("OLE presentation offset overflow"))?;
    let data_size_offset = fields
        .checked_add(24)
        .ok_or_else(|| anyhow::anyhow!("OLE presentation offset overflow"))?;
    let data_offset = data_size_offset
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("OLE presentation offset overflow"))?;

    let aspect = read_u32(bytes, fields).ok_or_else(|| anyhow::anyhow!("missing Aspect"))?;
    let lindex = read_u32(bytes, fields + 4).ok_or_else(|| anyhow::anyhow!("missing Lindex"))?;
    let advf = read_u32(bytes, fields + 8).ok_or_else(|| anyhow::anyhow!("missing Advf"))?;
    let width = read_u32(bytes, fields + 16).ok_or_else(|| anyhow::anyhow!("missing Width"))?;
    let height = read_u32(bytes, fields + 20).ok_or_else(|| anyhow::anyhow!("missing Height"))?;
    let data_size = usize::try_from(
        read_u32(bytes, data_size_offset).ok_or_else(|| anyhow::anyhow!("missing Data size"))?,
    )
    .map_err(|_| anyhow::anyhow!("OLE presentation data size overflow"))?;

    let data_end = data_offset
        .checked_add(data_size)
        .ok_or_else(|| anyhow::anyhow!("OLE presentation data range overflow"))?;
    let reserved2_end = data_end
        .checked_add(METAFILE_RESERVED2_LEN)
        .ok_or_else(|| anyhow::anyhow!("OLE presentation trailer overflow"))?;
    if reserved2_end > bytes.len() {
        bail!("truncated CF_METAFILEPICT OLE presentation");
    }

    let data = &bytes[data_offset..data_end];
    if data.len() < 18 {
        bail!("WMF presentation payload is too short");
    }

    Ok(OlePresentation {
        clipboard_format,
        aspect,
        lindex,
        advf,
        width,
        height,
        data,
    })
}

fn parse_cached_presentation_blob(
    blob: pub_cfb::CfbStreamBlob,
) -> Result<LegacyOleCachedPresentation> {
    let parsed = parse_cf_metafilepict_ole_presentation(&blob.bytes)
        .with_context(|| format!("parse bounded OLE presentation {}", blob.path))?;

    Ok(LegacyOleCachedPresentation {
        stream_path: blob.path,
        stream_name: blob.name,
        clipboard_format: parsed.clipboard_format,
        aspect: parsed.aspect,
        lindex: parsed.lindex,
        advf: parsed.advf,
        width: parsed.width,
        height: parsed.height,
        data: parsed.data.to_vec(),
    })
}

/// Reads only persisted cached OLE presentations under the proven Object N storage.
///
/// This function never activates OLE/COM servers. It only reads bounded direct-child
/// OlePres streams and parses the MS-OLEDS presentation envelope.
pub fn read_legacy_ole_cached_presentations<R: Read + Seek>(
    reader: R,
    storage_number: u16,
) -> Result<Vec<LegacyOleCachedPresentation>> {
    let storage_path = format!("/Objects/Object {storage_number}");
    let blobs = pub_cfb::read_direct_child_streams_with_prefix_reader(
        reader,
        &storage_path,
        OLE_PRES_STREAM_PREFIX,
        MAX_OLE_PRESENTATION_COUNT,
        MAX_PRESENTATION_BYTES,
    )
    .with_context(|| format!("read bounded cached presentations under {storage_path}"))?;

    blobs
        .into_iter()
        .map(parse_cached_presentation_blob)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(format: u32, target_device_size: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&STANDARD_CLIPBOARD_MARKER_ANSI.to_le_bytes());
        bytes.extend_from_slice(&format.to_le_bytes());
        bytes.extend_from_slice(&target_device_size.to_le_bytes());
        if target_device_size > 4 {
            bytes.resize(8 + target_device_size as usize, 0);
        }
        bytes.extend_from_slice(&1u32.to_le_bytes()); // Aspect
        bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // Lindex
        bytes.extend_from_slice(&2u32.to_le_bytes()); // Advf
        bytes.extend_from_slice(&0u32.to_le_bytes()); // Reserved1
        bytes.extend_from_slice(&640u32.to_le_bytes()); // Width
        bytes.extend_from_slice(&480u32.to_le_bytes()); // Height
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&[0u8; METAFILE_RESERVED2_LEN]);
        bytes
    }

    #[test]
    fn owned_cached_presentation_preserves_stream_identity_and_payload() {
        let payload = [0x04u8; 18];
        let blob = pub_cfb::CfbStreamBlob {
            path: "/Objects/Object 73/\u{2}OlePres001".into(),
            name: "\u{2}OlePres001".into(),
            bytes: fixture(CF_METAFILEPICT, 4, &payload),
        };

        let parsed = parse_cached_presentation_blob(blob).expect("cached presentation");
        assert_eq!(parsed.stream_path, "/Objects/Object 73/\u{2}OlePres001");
        assert_eq!(parsed.stream_name, "\u{2}OlePres001");
        assert_eq!(parsed.clipboard_format, CF_METAFILEPICT);
        assert_eq!(parsed.data, payload);
    }

    #[test]
    fn parses_bounded_standard_metafile_presentation() {
        let payload = [0x01u8; 18];
        let bytes = fixture(CF_METAFILEPICT, 4, &payload);
        let parsed = parse_cf_metafilepict_ole_presentation(&bytes).expect("presentation");
        assert_eq!(parsed.clipboard_format, CF_METAFILEPICT);
        assert_eq!(parsed.aspect, 1);
        assert_eq!(parsed.lindex, u32::MAX);
        assert_eq!(parsed.advf, 2);
        assert_eq!(parsed.width, 640);
        assert_eq!(parsed.height, 480);
        assert_eq!(parsed.data, payload);
    }

    #[test]
    fn rejects_other_clipboard_formats_and_truncation() {
        let payload = [0x01u8; 18];
        assert!(parse_cf_metafilepict_ole_presentation(&fixture(8, 4, &payload)).is_err());

        let mut bytes = fixture(CF_METAFILEPICT, 4, &payload);
        bytes.truncate(bytes.len() - 1);
        assert!(parse_cf_metafilepict_ole_presentation(&bytes).is_err());
    }

    #[test]
    fn skips_bounded_target_device_bytes() {
        let payload = [0x02u8; 18];
        let bytes = fixture(CF_METAFILEPICT, 12, &payload);
        let parsed = parse_cf_metafilepict_ole_presentation(&bytes).expect("presentation");
        assert_eq!(parsed.data, payload);
    }

    #[test]
    fn accepts_both_standard_clipboard_markers() {
        let payload = [0x03u8; 18];
        let mut bytes = fixture(CF_METAFILEPICT, 4, &payload);
        bytes[0..4].copy_from_slice(&STANDARD_CLIPBOARD_MARKER_UNICODE.to_le_bytes());
        let parsed = parse_cf_metafilepict_ole_presentation(&bytes).expect("presentation");
        assert_eq!(parsed.clipboard_format, CF_METAFILEPICT);
    }
}
