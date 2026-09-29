use anyhow::{Context, Result, bail};

const PLACEABLE_KEY: u32 = 0x9ac6_cdd7;
const PLACEABLE_HEADER_BYTES: usize = 22;
const META_HEADER_BYTES: usize = 18;
const META_HEADER_WORDS: u16 = 9;
const META_EOF_FUNCTION: u16 = 0x0000;
const MIN_RECORD_WORDS: u32 = 3;
const MAX_WMF_RECORD_COUNT: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WmfMetafileInfo {
    pub placeable: bool,
    pub metafile_type: u16,
    pub version: u16,
    pub declared_bytes: usize,
    pub object_count: u16,
    pub max_record_words: u32,
    pub record_count: usize,
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn validate_placeable_header(bytes: &[u8]) -> Result<()> {
    if bytes.len() < PLACEABLE_HEADER_BYTES {
        bail!("truncated placeable WMF header");
    }
    let reserved = read_u32(bytes, 16).context("missing placeable WMF Reserved field")?;
    if reserved != 0 {
        bail!("placeable WMF Reserved field must be zero");
    }

    let expected = read_u16(bytes, 20).context("missing placeable WMF checksum")?;
    let actual = (0..10)
        .map(|index| read_u16(bytes, index * 2).expect("placeable header bounds checked"))
        .fold(0u16, |checksum, word| checksum ^ word);
    if actual != expected {
        bail!("placeable WMF checksum mismatch");
    }
    Ok(())
}

pub fn validate_wmf_metafile(bytes: &[u8]) -> Result<WmfMetafileInfo> {
    let placeable = read_u32(bytes, 0) == Some(PLACEABLE_KEY);
    let header_offset = if placeable {
        validate_placeable_header(bytes)?;
        PLACEABLE_HEADER_BYTES
    } else {
        0
    };

    let header_end = header_offset
        .checked_add(META_HEADER_BYTES)
        .context("WMF header offset overflow")?;
    if header_end > bytes.len() {
        bail!("truncated WMF META_HEADER");
    }

    let metafile_type = read_u16(bytes, header_offset).context("missing WMF Type")?;
    if !matches!(metafile_type, 1 | 2) {
        bail!("unsupported WMF metafile type {metafile_type}");
    }

    let header_words =
        read_u16(bytes, header_offset + 2).context("missing WMF HeaderSize")?;
    if header_words != META_HEADER_WORDS {
        bail!("invalid WMF HeaderSize {header_words}");
    }

    let version = read_u16(bytes, header_offset + 4).context("missing WMF Version")?;
    if !matches!(version, 0x0100 | 0x0300) {
        bail!("unsupported WMF version {version:#06x}");
    }

    let declared_words =
        read_u32(bytes, header_offset + 6).context("missing WMF declared size")?;
    let declared_bytes = usize::try_from(declared_words)
        .context("WMF declared word count overflow")?
        .checked_mul(2)
        .context("WMF declared byte size overflow")?;
    if declared_bytes < META_HEADER_BYTES {
        bail!("WMF declared size is smaller than META_HEADER");
    }

    let metafile_end = header_offset
        .checked_add(declared_bytes)
        .context("WMF declared range overflow")?;
    if metafile_end != bytes.len() {
        bail!(
            "WMF declared size mismatch: declared {}, observed {}",
            metafile_end,
            bytes.len()
        );
    }

    let object_count =
        read_u16(bytes, header_offset + 10).context("missing WMF NumberOfObjects")?;
    let max_record_words =
        read_u32(bytes, header_offset + 12).context("missing WMF MaxRecord")?;
    if max_record_words < MIN_RECORD_WORDS {
        bail!("WMF MaxRecord is smaller than a record header");
    }

    let mut offset = header_end;
    let mut record_count = 0usize;
    let mut saw_eof = false;
    while offset < metafile_end {
        if record_count >= MAX_WMF_RECORD_COUNT {
            bail!("WMF record count exceeds bounded limit");
        }

        let record_words = read_u32(bytes, offset).context("truncated WMF record size")?;
        if record_words < MIN_RECORD_WORDS {
            bail!("invalid WMF record size {record_words} words");
        }
        if record_words > max_record_words {
            bail!("WMF record exceeds META_HEADER MaxRecord");
        }
        let record_bytes = usize::try_from(record_words)
            .context("WMF record word count overflow")?
            .checked_mul(2)
            .context("WMF record byte size overflow")?;
        let record_end = offset
            .checked_add(record_bytes)
            .context("WMF record range overflow")?;
        if record_end > metafile_end {
            bail!("WMF record exceeds declared metafile size");
        }

        let function =
            read_u16(bytes, offset + 4).context("truncated WMF RecordFunction")?;
        record_count += 1;
        offset = record_end;

        if function == META_EOF_FUNCTION {
            if offset != metafile_end {
                bail!("WMF META_EOF is not the final record");
            }
            saw_eof = true;
            break;
        }
    }

    if !saw_eof {
        bail!("WMF META_EOF record is missing");
    }

    Ok(WmfMetafileInfo {
        placeable,
        metafile_type,
        version,
        declared_bytes,
        object_count,
        max_record_words,
        record_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standard_wmf() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&META_HEADER_WORDS.to_le_bytes());
        bytes.extend_from_slice(&0x0300u16.to_le_bytes());
        bytes.extend_from_slice(&12u32.to_le_bytes()); // 9-word header + 3-word EOF
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&META_EOF_FUNCTION.to_le_bytes());
        bytes
    }

    #[test]
    fn validates_minimal_standard_wmf() {
        let info = validate_wmf_metafile(&standard_wmf()).expect("valid WMF");
        assert!(!info.placeable);
        assert_eq!(info.version, 0x0300);
        assert_eq!(info.declared_bytes, 24);
        assert_eq!(info.record_count, 1);
    }

    #[test]
    fn rejects_declared_size_mismatch_and_missing_eof() {
        let mut size_mismatch = standard_wmf();
        size_mismatch[6..10].copy_from_slice(&11u32.to_le_bytes());
        assert!(validate_wmf_metafile(&size_mismatch).is_err());

        let mut no_eof = standard_wmf();
        no_eof[22..24].copy_from_slice(&0x0103u16.to_le_bytes());
        assert!(validate_wmf_metafile(&no_eof).is_err());
    }

    #[test]
    fn validates_placeable_checksum_before_meta_header() {
        let standard = standard_wmf();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PLACEABLE_KEY.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // HWmf
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&100i16.to_le_bytes());
        bytes.extend_from_slice(&100i16.to_le_bytes());
        bytes.extend_from_slice(&1440u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let checksum = (0..10)
            .map(|index| read_u16(&bytes, index * 2).unwrap())
            .fold(0u16, |checksum, word| checksum ^ word);
        bytes.extend_from_slice(&checksum.to_le_bytes());
        bytes.extend_from_slice(&standard);

        let info = validate_wmf_metafile(&bytes).expect("valid placeable WMF");
        assert!(info.placeable);
        assert_eq!(info.declared_bytes, standard.len());
    }
}
