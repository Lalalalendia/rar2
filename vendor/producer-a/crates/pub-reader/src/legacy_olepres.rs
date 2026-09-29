#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PubLegacyOlePresClipboardFormat {
    None,
    Standard(u32),
    Registered { byte_len: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubLegacyOlePresEnvelope {
    pub clipboard_format: PubLegacyOlePresClipboardFormat,
    pub target_device_size: u32,
    pub aspect: u32,
    pub lindex: u32,
    pub advf: u32,
    pub width: u32,
    pub height: u32,
    pub data_size: u32,
    pub data_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PubLegacyOlePresParseError {
    StreamTooLarge { len: usize },
    Truncated { offset: usize, need: usize, len: usize },
    RegisteredClipboardNameTooLarge { byte_len: u32 },
    InvalidTargetDeviceSize { byte_len: u32 },
    TargetDeviceTooLarge { byte_len: u32 },
    PresentationDataTooLarge { byte_len: u32 },
}

pub const LEGACY_OLEPRES_MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;
pub const LEGACY_OLEPRES_MAX_REGISTERED_CLIPBOARD_NAME_BYTES: u32 = 4096;
pub const LEGACY_OLEPRES_MAX_TARGET_DEVICE_BYTES: u32 = 1024 * 1024;
pub const LEGACY_OLEPRES_MAX_PRESENTATION_DATA_BYTES: u32 = 64 * 1024 * 1024;

pub fn parse_legacy_olepres_envelope(
    bytes: &[u8],
) -> Result<PubLegacyOlePresEnvelope, PubLegacyOlePresParseError> {
    if bytes.len() > LEGACY_OLEPRES_MAX_STREAM_BYTES {
        return Err(PubLegacyOlePresParseError::StreamTooLarge { len: bytes.len() });
    }

    let (clipboard_format, mut offset) = parse_clipboard_format(bytes, 0)?;

    let target_device_size = read_u32(bytes, offset)?;
    if target_device_size < 4 {
        return Err(PubLegacyOlePresParseError::InvalidTargetDeviceSize {
            byte_len: target_device_size,
        });
    }
    if target_device_size > LEGACY_OLEPRES_MAX_TARGET_DEVICE_BYTES {
        return Err(PubLegacyOlePresParseError::TargetDeviceTooLarge {
            byte_len: target_device_size,
        });
    }

    offset = offset
        .checked_add(target_device_size as usize)
        .ok_or(PubLegacyOlePresParseError::TargetDeviceTooLarge {
            byte_len: target_device_size,
        })?;

    let aspect = read_u32(bytes, offset)?;
    let lindex = read_u32(bytes, offset + 4)?;
    let advf = read_u32(bytes, offset + 8)?;
    let width = read_u32(bytes, offset + 16)?;
    let height = read_u32(bytes, offset + 20)?;
    let data_size = read_u32(bytes, offset + 24)?;
    if data_size > LEGACY_OLEPRES_MAX_PRESENTATION_DATA_BYTES {
        return Err(PubLegacyOlePresParseError::PresentationDataTooLarge {
            byte_len: data_size,
        });
    }

    let data_offset = offset + 28;
    require(bytes, data_offset, data_size as usize)?;

    Ok(PubLegacyOlePresEnvelope {
        clipboard_format,
        target_device_size,
        aspect,
        lindex,
        advf,
        width,
        height,
        data_size,
        data_offset,
    })
}

fn parse_clipboard_format(
    bytes: &[u8],
    offset: usize,
) -> Result<(PubLegacyOlePresClipboardFormat, usize), PubLegacyOlePresParseError> {
    let marker = read_u32(bytes, offset)?;
    let offset = offset + 4;

    match marker {
        0 => Ok((PubLegacyOlePresClipboardFormat::None, offset)),
        0xffff_ffff | 0xffff_fffe => {
            let format = read_u32(bytes, offset)?;
            Ok((PubLegacyOlePresClipboardFormat::Standard(format), offset + 4))
        }
        byte_len => {
            if byte_len > LEGACY_OLEPRES_MAX_REGISTERED_CLIPBOARD_NAME_BYTES {
                return Err(
                    PubLegacyOlePresParseError::RegisteredClipboardNameTooLarge { byte_len },
                );
            }
            require(bytes, offset, byte_len as usize)?;
            Ok((
                PubLegacyOlePresClipboardFormat::Registered { byte_len },
                offset + byte_len as usize,
            ))
        }
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, PubLegacyOlePresParseError> {
    require(bytes, offset, 4)?;
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn require(
    bytes: &[u8],
    offset: usize,
    need: usize,
) -> Result<(), PubLegacyOlePresParseError> {
    let end = offset
        .checked_add(need)
        .ok_or(PubLegacyOlePresParseError::Truncated {
            offset,
            need,
            len: bytes.len(),
        })?;
    if end > bytes.len() {
        return Err(PubLegacyOlePresParseError::Truncated {
            offset,
            need,
            len: bytes.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn standard_metafile_fixture(data_size: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 0xffff_ffff);
        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 4);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 320);
        push_u32(&mut bytes, 200);
        push_u32(&mut bytes, data_size);
        bytes.extend(std::iter::repeat_n(0x5a, data_size as usize));
        bytes
    }

    #[test]
    fn parses_standard_cf_metafilepict_envelope_without_touching_payload() {
        let bytes = standard_metafile_fixture(3);
        let parsed = parse_legacy_olepres_envelope(&bytes).expect("valid OlePres envelope");

        assert_eq!(
            parsed.clipboard_format,
            PubLegacyOlePresClipboardFormat::Standard(3)
        );
        assert_eq!(parsed.target_device_size, 4);
        assert_eq!(parsed.aspect, 1);
        assert_eq!(parsed.lindex, 0);
        assert_eq!(parsed.advf, 2);
        assert_eq!(parsed.width, 320);
        assert_eq!(parsed.height, 200);
        assert_eq!(parsed.data_size, 3);
        assert_eq!(&bytes[parsed.data_offset..], &[0x5a, 0x5a, 0x5a]);
    }

    #[test]
    fn parses_registered_clipboard_name_as_bounded_length_only() {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 4);
        bytes.extend_from_slice(b"ABC\0");
        push_u32(&mut bytes, 4);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 10);
        push_u32(&mut bytes, 10);
        push_u32(&mut bytes, 0);

        let parsed = parse_legacy_olepres_envelope(&bytes).expect("registered format");
        assert_eq!(
            parsed.clipboard_format,
            PubLegacyOlePresClipboardFormat::Registered { byte_len: 4 }
        );
        assert_eq!(parsed.data_size, 0);
    }

    #[test]
    fn rejects_target_device_smaller_than_size_field() {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 3);

        assert_eq!(
            parse_legacy_olepres_envelope(&bytes),
            Err(PubLegacyOlePresParseError::InvalidTargetDeviceSize { byte_len: 3 })
        );
    }

    #[test]
    fn rejects_truncated_presentation_payload() {
        let mut bytes = standard_metafile_fixture(3);
        bytes.pop();

        assert!(matches!(
            parse_legacy_olepres_envelope(&bytes),
            Err(PubLegacyOlePresParseError::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_oversized_presentation_data_before_allocation_or_decode() {
        let mut bytes = standard_metafile_fixture(0);
        let data_size_offset = bytes.len() - 4;
        bytes[data_size_offset..].copy_from_slice(
            &(LEGACY_OLEPRES_MAX_PRESENTATION_DATA_BYTES + 1).to_le_bytes(),
        );

        assert_eq!(
            parse_legacy_olepres_envelope(&bytes),
            Err(PubLegacyOlePresParseError::PresentationDataTooLarge {
                byte_len: LEGACY_OLEPRES_MAX_PRESENTATION_DATA_BYTES + 1,
            })
        );
    }
}
