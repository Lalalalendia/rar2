use anyhow::{Result, anyhow, bail};
use std::collections::BTreeMap;

const PLACEABLE_KEY: u32 = 0x9ac6_cdd7;
const PLACEABLE_HEADER_LEN: usize = 22;
const META_HEADER_LEN: usize = 18;
const META_HEADER_WORDS: u16 = 9;
const MAX_WMF_BYTES: usize = 64 * 1024 * 1024;
const MAX_WMF_RECORDS: usize = 100_000;
const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WmfProfile {
    pub placeable: bool,
    pub file_type: u16,
    pub header_size_words: u16,
    pub version: u16,
    pub declared_size_words: u32,
    pub object_count: u16,
    pub max_record_words: u32,
    pub parameter_count: u16,
    pub record_count: usize,
    pub saw_eof: bool,
    pub functions: BTreeMap<u16, u32>,
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn checked_words_to_bytes(words: u32) -> Result<usize> {
    usize::try_from(words)
        .map_err(|_| anyhow!("WMF word count overflow"))?
        .checked_mul(2)
        .ok_or_else(|| anyhow!("WMF byte count overflow"))
}

pub fn profile_wmf(bytes: &[u8]) -> Result<WmfProfile> {
    if bytes.len() > MAX_WMF_BYTES {
        bail!("WMF exceeds bounded size");
    }
    if bytes.len() < META_HEADER_LEN {
        bail!("WMF is shorter than METAHEADER");
    }

    let (placeable, header_offset) = if read_u32(bytes, 0) == Some(PLACEABLE_KEY) {
        if bytes.len() < PLACEABLE_HEADER_LEN + META_HEADER_LEN {
            bail!("truncated placeable WMF header");
        }
        (true, PLACEABLE_HEADER_LEN)
    } else {
        (false, 0)
    };

    let file_type = read_u16(bytes, header_offset).ok_or_else(|| anyhow!("missing mtType"))?;
    if !matches!(file_type, 0 | 1) {
        bail!("unsupported WMF mtType {file_type}");
    }

    let header_size_words =
        read_u16(bytes, header_offset + 2).ok_or_else(|| anyhow!("missing mtHeaderSize"))?;
    if header_size_words != META_HEADER_WORDS {
        bail!("invalid WMF mtHeaderSize {header_size_words}");
    }

    let version = read_u16(bytes, header_offset + 4).ok_or_else(|| anyhow!("missing mtVersion"))?;
    if !matches!(version, 0x0100 | 0x0300) {
        bail!("unsupported WMF version {version:#06x}");
    }

    let declared_size_words =
        read_u32(bytes, header_offset + 6).ok_or_else(|| anyhow!("missing mtSize"))?;
    let object_count =
        read_u16(bytes, header_offset + 10).ok_or_else(|| anyhow!("missing mtNoObjects"))?;
    let max_record_words =
        read_u32(bytes, header_offset + 12).ok_or_else(|| anyhow!("missing mtMaxRecord"))?;
    let parameter_count =
        read_u16(bytes, header_offset + 16).ok_or_else(|| anyhow!("missing mtNoParameters"))?;
    if parameter_count != 0 {
        bail!("unsupported nonzero WMF mtNoParameters {parameter_count}");
    }

    let declared_bytes = checked_words_to_bytes(declared_size_words)?;
    let meta_end = header_offset
        .checked_add(declared_bytes)
        .ok_or_else(|| anyhow!("WMF declared range overflow"))?;
    if declared_bytes < META_HEADER_LEN || meta_end > bytes.len() {
        bail!("truncated WMF declared file range");
    }

    let mut offset = header_offset + META_HEADER_LEN;
    let mut record_count = 0usize;
    let mut saw_eof = false;
    let mut functions = BTreeMap::<u16, u32>::new();

    while offset < meta_end {
        if record_count >= MAX_WMF_RECORDS {
            bail!("WMF record count exceeds bounded limit");
        }
        if offset + 6 > meta_end {
            bail!("truncated WMF record header");
        }

        let size_words = read_u32(bytes, offset).ok_or_else(|| anyhow!("missing record size"))?;
        if size_words < 3 {
            bail!("invalid WMF record size {size_words} words");
        }
        let size_bytes = checked_words_to_bytes(size_words)?;
        if size_bytes > MAX_RECORD_BYTES {
            bail!("WMF record exceeds bounded size");
        }

        let end = offset
            .checked_add(size_bytes)
            .ok_or_else(|| anyhow!("WMF record range overflow"))?;
        if end > meta_end {
            bail!("WMF record exceeds declared file range");
        }

        let function =
            read_u16(bytes, offset + 4).ok_or_else(|| anyhow!("missing WMF record function"))?;
        *functions.entry(function).or_default() += 1;
        record_count += 1;

        if function == 0x0000 {
            if size_words != 3 {
                bail!("invalid META_EOF record size {size_words}");
            }
            saw_eof = true;
            if end != meta_end {
                let trailing = &bytes[end..meta_end];
                if trailing.iter().any(|byte| *byte != 0) {
                    bail!("nonzero bytes after META_EOF");
                }
            }
            break;
        }

        offset = end;
    }

    if !saw_eof {
        bail!("WMF missing META_EOF");
    }

    Ok(WmfProfile {
        placeable,
        file_type,
        header_size_words,
        version,
        declared_size_words,
        object_count,
        max_record_words,
        parameter_count,
        record_count,
        saw_eof,
        functions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_wmf(extra_records: &[(u16, &[u16])]) -> Vec<u8> {
        let mut records = Vec::<u8>::new();
        for (function, params) in extra_records {
            let size_words = 3u32 + params.len() as u32;
            records.extend_from_slice(&size_words.to_le_bytes());
            records.extend_from_slice(&function.to_le_bytes());
            for param in *params {
                records.extend_from_slice(&param.to_le_bytes());
            }
        }
        records.extend_from_slice(&3u32.to_le_bytes());
        records.extend_from_slice(&0u16.to_le_bytes());

        let total_words = ((META_HEADER_LEN + records.len()) / 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&META_HEADER_WORDS.to_le_bytes());
        bytes.extend_from_slice(&0x0300u16.to_le_bytes());
        bytes.extend_from_slice(&total_words.to_le_bytes());
        bytes.extend_from_slice(&8u16.to_le_bytes());
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&records);
        bytes
    }

    #[test]
    fn profiles_bounded_records() {
        let bytes = minimal_wmf(&[(0x0103, &[8]), (0x020b, &[10, 20])]);
        let profile = profile_wmf(&bytes).expect("wmf");
        assert_eq!(profile.version, 0x0300);
        assert_eq!(profile.record_count, 3);
        assert_eq!(profile.functions.get(&0x0103), Some(&1));
        assert_eq!(profile.functions.get(&0x020b), Some(&1));
        assert_eq!(profile.functions.get(&0x0000), Some(&1));
        assert!(profile.saw_eof);
    }

    #[test]
    fn rejects_record_past_declared_end() {
        let mut bytes = minimal_wmf(&[(0x0103, &[8])]);
        let record_offset = META_HEADER_LEN;
        bytes[record_offset..record_offset + 4].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
        assert!(profile_wmf(&bytes).is_err());
    }

    #[test]
    fn accepts_placeable_prefix() {
        let base = minimal_wmf(&[]);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PLACEABLE_KEY.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 16]);
        bytes.extend_from_slice(&base);
        let profile = profile_wmf(&bytes).expect("placeable wmf");
        assert!(profile.placeable);
    }
}
