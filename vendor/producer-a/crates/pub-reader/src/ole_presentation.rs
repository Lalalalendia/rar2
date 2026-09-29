use anyhow::{Result, bail};

const STANDARD_CLIPBOARD_MARKER_ANSI: u32 = 0xffff_ffff;
const STANDARD_CLIPBOARD_MARKER_UNICODE: u32 = 0xffff_fffe;
const CF_METAFILEPICT: u32 = 3;
const METAFILE_RESERVED2_LEN: usize = 18;
const MAX_PRESENTATION_BYTES: usize = 64 * 1024 * 1024;
const MAX_TARGET_DEVICE_BYTES: usize = 1024 * 1024;

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
    if data_end > bytes.len() {
        bail!("truncated CF_METAFILEPICT OLE presentation data");
    }
    let trailing_len = bytes.len() - data_end;
    if trailing_len != 0 && trailing_len < METAFILE_RESERVED2_LEN {
        bail!("truncated CF_METAFILEPICT OLE presentation trailer");
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

        let mut truncated_data = fixture(CF_METAFILEPICT, 4, &payload);
        truncated_data.truncate(truncated_data.len() - METAFILE_RESERVED2_LEN - 1);
        assert!(parse_cf_metafilepict_ole_presentation(&truncated_data).is_err());

        let mut no_trailer = fixture(CF_METAFILEPICT, 4, &payload);
        no_trailer.truncate(no_trailer.len() - METAFILE_RESERVED2_LEN);
        assert!(parse_cf_metafilepict_ole_presentation(&no_trailer).is_ok());

        let mut short_trailer = no_trailer.clone();
        short_trailer.extend_from_slice(&[0_u8; METAFILE_RESERVED2_LEN - 1]);
        assert!(parse_cf_metafilepict_ole_presentation(&short_trailer).is_err());
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
