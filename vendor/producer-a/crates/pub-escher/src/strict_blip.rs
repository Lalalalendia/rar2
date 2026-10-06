use crate::{
    BlipKind, OFFICE_ART_BLIP_DIB, OFFICE_ART_BLIP_JPEG, OFFICE_ART_BLIP_PNG, OFFICE_ART_BLIP_TIFF,
    OfficeArtRecord,
};
use md4::{Digest, Md4};
use pub_core::RawSpan;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlipUidRule {
    SingleUid,
    SecondUid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedBlip {
    pub record_source: RawSpan,
    pub payload_source: RawSpan,
    pub rec_type: u16,
    pub rec_instance: u16,
    pub kind: BlipKind,
    pub effective_uid: [u8; 16],
    pub uid_rule: BlipUidRule,
    pub internal_uid_verified: bool,
    pub format_header_verified: bool,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlipValidationError {
    UnsupportedType {
        rec_type: u16,
    },
    InvalidRecVer {
        rec_ver: u8,
    },
    UnsupportedInstance {
        rec_type: u16,
        rec_instance: u16,
    },
    PayloadTooShort {
        rec_type: u16,
        rec_instance: u16,
        required_prefix: usize,
        available: usize,
    },
    SpanOutOfBounds {
        offset: u64,
        len: u64,
        stream_len: usize,
    },
    InconsistentRecordEnvelope,
    UidMismatch {
        rec_type: u16,
        rec_instance: u16,
    },
    InvalidFormatHeader {
        kind: BlipKind,
    },
}

impl fmt::Display for BlipValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedType { rec_type } => {
                write!(f, "unsupported OfficeArt BLIP type 0x{rec_type:04X}")
            }
            Self::InvalidRecVer { rec_ver } => {
                write!(f, "OfficeArt BLIP recVer must be 0, got 0x{rec_ver:X}")
            }
            Self::UnsupportedInstance {
                rec_type,
                rec_instance,
            } => write!(
                f,
                "unsupported OfficeArt BLIP instance 0x{rec_instance:X} for type 0x{rec_type:04X}"
            ),
            Self::PayloadTooShort {
                rec_type,
                rec_instance,
                required_prefix,
                available,
            } => write!(
                f,
                "OfficeArt BLIP 0x{rec_type:04X}/0x{rec_instance:X} needs {required_prefix} prefix bytes, only {available} available"
            ),
            Self::SpanOutOfBounds {
                offset,
                len,
                stream_len,
            } => write!(
                f,
                "BLIP span {offset}+{len} lies outside stream length {stream_len}"
            ),
            Self::InconsistentRecordEnvelope => {
                write!(
                    f,
                    "OfficeArt BLIP record/header/payload envelope is inconsistent with source bytes"
                )
            }
            Self::UidMismatch {
                rec_type,
                rec_instance,
            } => write!(
                f,
                "OfficeArt BLIP internal UID mismatch for 0x{rec_type:04X}/0x{rec_instance:X}"
            ),
            Self::InvalidFormatHeader { kind } => {
                write!(f, "invalid bounded image header for {kind:?} BLIP")
            }
        }
    }
}

impl std::error::Error for BlipValidationError {}

#[derive(Debug, Clone, Copy)]
struct RasterGrammar {
    kind: BlipKind,
    uid_count: usize,
}

pub fn validate_blip_record(
    bytes: &[u8],
    record: &OfficeArtRecord,
) -> Result<ValidatedBlip, BlipValidationError> {
    validate_record_envelope(bytes, record)?;
    if record.header.rec_ver != 0 {
        return Err(BlipValidationError::InvalidRecVer {
            rec_ver: record.header.rec_ver,
        });
    }

    let grammar = raster_grammar(record.header.rec_type, record.header.rec_instance)?;
    let prefix_len = match grammar.uid_count {
        1 => 17usize,
        2 => 33usize,
        _ => unreachable!("bounded raster grammar uses one or two UIDs"),
    };

    let payload = span_slice(bytes, &record.payload_source)?;
    if payload.len() < prefix_len {
        return Err(BlipValidationError::PayloadTooShort {
            rec_type: record.header.rec_type,
            rec_instance: record.header.rec_instance,
            required_prefix: prefix_len,
            available: payload.len(),
        });
    }

    let uid_offset = if grammar.uid_count == 2 { 16 } else { 0 };
    let mut effective_uid = [0u8; 16];
    effective_uid.copy_from_slice(&payload[uid_offset..uid_offset + 16]);

    let blip_file_data = &payload[prefix_len..];
    let digest = Md4::digest(blip_file_data);
    let mut observed_uid = [0u8; 16];
    observed_uid.copy_from_slice(&digest);
    if observed_uid != effective_uid {
        return Err(BlipValidationError::UidMismatch {
            rec_type: record.header.rec_type,
            rec_instance: record.header.rec_instance,
        });
    }

    let validated_kind = validated_raster_kind(grammar.kind, blip_file_data);
    if !valid_format_header(validated_kind, blip_file_data) {
        return Err(BlipValidationError::InvalidFormatHeader {
            kind: validated_kind,
        });
    }

    let data_offset = record.payload_source.offset + prefix_len as u64;
    let data_len = blip_file_data.len() as u64;
    let payload_source = RawSpan {
        stream: record.payload_source.stream.clone(),
        offset: data_offset,
        len: data_len,
    };

    Ok(ValidatedBlip {
        record_source: record.source.clone(),
        payload_source,
        rec_type: record.header.rec_type,
        rec_instance: record.header.rec_instance,
        kind: validated_kind,
        effective_uid,
        uid_rule: if grammar.uid_count == 2 {
            BlipUidRule::SecondUid
        } else {
            BlipUidRule::SingleUid
        },
        internal_uid_verified: true,
        format_header_verified: true,
        payload_sha256: hex_lower(&Sha256::digest(blip_file_data)),
    })
}

fn validate_record_envelope(
    bytes: &[u8],
    record: &OfficeArtRecord,
) -> Result<(), BlipValidationError> {
    let Some(payload_offset) = record.source.offset.checked_add(8) else {
        return Err(BlipValidationError::InconsistentRecordEnvelope);
    };
    let Some(record_len) = 8u64.checked_add(u64::from(record.header.rec_len)) else {
        return Err(BlipValidationError::InconsistentRecordEnvelope);
    };

    if record.header.source.stream != record.source.stream
        || record.payload_source.stream != record.source.stream
        || record.header.source.offset != record.source.offset
        || record.header.source.len != 8
        || record.payload_source.offset != payload_offset
        || record.payload_source.len != u64::from(record.header.rec_len)
        || record.source.len != record_len
        || record.header.source.end() != Some(record.payload_source.offset)
        || record.source.end() != record.payload_source.end()
    {
        return Err(BlipValidationError::InconsistentRecordEnvelope);
    }

    let header = span_slice(bytes, &record.header.source)?;
    if header.len() != 8 {
        return Err(BlipValidationError::InconsistentRecordEnvelope);
    }
    let initial = u16::from_le_bytes([header[0], header[1]]);
    let rec_ver = (initial & 0x000f) as u8;
    let rec_instance = initial >> 4;
    let rec_type = u16::from_le_bytes([header[2], header[3]]);
    let rec_len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if rec_ver != record.header.rec_ver
        || rec_instance != record.header.rec_instance
        || rec_type != record.header.rec_type
        || rec_len != record.header.rec_len
    {
        return Err(BlipValidationError::InconsistentRecordEnvelope);
    }

    Ok(())
}

fn raster_grammar(rec_type: u16, rec_instance: u16) -> Result<RasterGrammar, BlipValidationError> {
    let grammar = match (rec_type, rec_instance) {
        (OFFICE_ART_BLIP_JPEG, 0x46A | 0x6E2) => RasterGrammar {
            kind: BlipKind::Jpeg,
            uid_count: 1,
        },
        (OFFICE_ART_BLIP_JPEG, 0x46B | 0x6E3) => RasterGrammar {
            kind: BlipKind::Jpeg,
            uid_count: 2,
        },
        (OFFICE_ART_BLIP_PNG, 0x6E0) => RasterGrammar {
            kind: BlipKind::Png,
            uid_count: 1,
        },
        (OFFICE_ART_BLIP_PNG, 0x6E1) => RasterGrammar {
            kind: BlipKind::Png,
            uid_count: 2,
        },
        (OFFICE_ART_BLIP_DIB, 0x7A8) => RasterGrammar {
            kind: BlipKind::Dib,
            uid_count: 1,
        },
        (OFFICE_ART_BLIP_DIB, 0x7A9) => RasterGrammar {
            kind: BlipKind::Dib,
            uid_count: 2,
        },
        (OFFICE_ART_BLIP_TIFF, 0x6E4) => RasterGrammar {
            kind: BlipKind::Tiff,
            uid_count: 1,
        },
        (OFFICE_ART_BLIP_TIFF, 0x6E5) => RasterGrammar {
            kind: BlipKind::Tiff,
            uid_count: 2,
        },
        (
            OFFICE_ART_BLIP_JPEG | OFFICE_ART_BLIP_PNG | OFFICE_ART_BLIP_DIB | OFFICE_ART_BLIP_TIFF,
            _,
        ) => {
            return Err(BlipValidationError::UnsupportedInstance {
                rec_type,
                rec_instance,
            });
        }
        _ => return Err(BlipValidationError::UnsupportedType { rec_type }),
    };
    Ok(grammar)
}

fn validated_raster_kind(declared_kind: BlipKind, data: &[u8]) -> BlipKind {
    if declared_kind == BlipKind::Png
        && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"))
    {
        BlipKind::Gif
    } else {
        declared_kind
    }
}

fn valid_format_header(kind: BlipKind, data: &[u8]) -> bool {
    match kind {
        BlipKind::Png => data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        BlipKind::Gif => data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"),
        BlipKind::Jpeg => data.starts_with(&[0xFF, 0xD8, 0xFF]),
        BlipKind::Dib => valid_dib_header(data),
        BlipKind::Tiff => {
            data.starts_with(&[b'I', b'I', 0x2A, 0x00])
                || data.starts_with(&[b'M', b'M', 0x00, 0x2A])
        }
        _ => false,
    }
}

fn valid_dib_header(data: &[u8]) -> bool {
    if data.len() < 16 {
        return false;
    }
    let header_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if !matches!(header_size, 40 | 108 | 124) {
        return false;
    }
    let planes = u16::from_le_bytes([data[12], data[13]]);
    let bits_per_pixel = u16::from_le_bytes([data[14], data[15]]);
    planes == 1 && matches!(bits_per_pixel, 1 | 4 | 8 | 16 | 24 | 32)
}

fn span_slice<'a>(bytes: &'a [u8], span: &RawSpan) -> Result<&'a [u8], BlipValidationError> {
    let start = usize::try_from(span.offset).map_err(|_| BlipValidationError::SpanOutOfBounds {
        offset: span.offset,
        len: span.len,
        stream_len: bytes.len(),
    })?;
    let len = usize::try_from(span.len).map_err(|_| BlipValidationError::SpanOutOfBounds {
        offset: span.offset,
        len: span.len,
        stream_len: bytes.len(),
    })?;
    let end = start
        .checked_add(len)
        .ok_or(BlipValidationError::SpanOutOfBounds {
            offset: span.offset,
            len: span.len,
            stream_len: bytes.len(),
        })?;
    bytes
        .get(start..end)
        .ok_or(BlipValidationError::SpanOutOfBounds {
            offset: span.offset,
            len: span.len,
            stream_len: bytes.len(),
        })
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_officeart_stream;
    use pub_core::StreamPath;

    fn stream() -> StreamPath {
        StreamPath("/Escher/EscherDelayStm".into())
    }

    fn image_data(kind: BlipKind) -> Vec<u8> {
        match kind {
            BlipKind::Png => {
                let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
                data.extend_from_slice(b"strict-png");
                data
            }
            BlipKind::Gif => {
                let mut data = b"GIF89a".to_vec();
                data.extend_from_slice(&616u16.to_le_bytes());
                data.extend_from_slice(&354u16.to_le_bytes());
                data.extend_from_slice(b"strict-gif");
                data
            }
            BlipKind::Jpeg => vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F'],
            BlipKind::Dib => {
                let mut data = vec![0u8; 40];
                data[0..4].copy_from_slice(&40u32.to_le_bytes());
                data[4..8].copy_from_slice(&2i32.to_le_bytes());
                data[8..12].copy_from_slice(&2i32.to_le_bytes());
                data[12..14].copy_from_slice(&1u16.to_le_bytes());
                data[14..16].copy_from_slice(&24u16.to_le_bytes());
                data
            }
            BlipKind::Tiff => vec![b'I', b'I', 0x2A, 0x00, 8, 0, 0, 0],
            other => panic!("unsupported synthetic raster kind {other:?}"),
        }
    }

    fn record_bytes(
        rec_type: u16,
        rec_instance: u16,
        kind: BlipKind,
        two_uid: bool,
        corrupt_effective_uid: bool,
    ) -> Vec<u8> {
        let data = image_data(kind);
        let digest = Md4::digest(&data);
        let mut payload = Vec::new();
        if two_uid {
            payload.extend_from_slice(&[0xA5; 16]);
        }
        let mut effective = [0u8; 16];
        effective.copy_from_slice(&digest);
        if corrupt_effective_uid {
            effective[0] ^= 0xFF;
        }
        payload.extend_from_slice(&effective);
        payload.push(0xFF);
        payload.extend_from_slice(&data);

        let initial = rec_instance << 4;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&initial.to_le_bytes());
        bytes.extend_from_slice(&rec_type.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn first_record(bytes: &[u8]) -> OfficeArtRecord {
        parse_officeart_stream(stream(), bytes)
            .expect("synthetic OfficeArt record")
            .records
            .into_iter()
            .next()
            .expect("one OfficeArt record")
    }

    #[test]
    fn md4_rfc1320_known_answers() {
        assert_eq!(
            hex_lower(&Md4::digest(b"")),
            "31d6cfe0d16ae931b73c59d7e0c089c0"
        );
        assert_eq!(
            hex_lower(&Md4::digest(b"a")),
            "bde52cb31de33e46245e05fbdbd6fb24"
        );
        assert_eq!(
            hex_lower(&Md4::digest(b"abc")),
            "a448017aaf21d8525fc10ae87aa6729d"
        );
    }

    #[test]
    fn validates_supported_one_uid_raster_families() {
        let cases = [
            (OFFICE_ART_BLIP_PNG, 0x6E0, BlipKind::Png),
            (OFFICE_ART_BLIP_JPEG, 0x46A, BlipKind::Jpeg),
            (OFFICE_ART_BLIP_JPEG, 0x6E2, BlipKind::Jpeg),
            (OFFICE_ART_BLIP_DIB, 0x7A8, BlipKind::Dib),
            (OFFICE_ART_BLIP_TIFF, 0x6E4, BlipKind::Tiff),
        ];

        for (rec_type, rec_instance, kind) in cases {
            let bytes = record_bytes(rec_type, rec_instance, kind, false, false);
            let record = first_record(&bytes);
            let validated = validate_blip_record(&bytes, &record).expect("strict raster BLIP");
            assert_eq!(validated.kind, kind);
            assert_eq!(validated.uid_rule, BlipUidRule::SingleUid);
            assert!(validated.internal_uid_verified);
            assert!(validated.format_header_verified);
            assert_eq!(validated.payload_source.end(), record.source.end());
        }
    }

    #[test]
    fn two_uid_variant_uses_second_uid_and_ignores_first() {
        let bytes = record_bytes(OFFICE_ART_BLIP_PNG, 0x6E1, BlipKind::Png, true, false);
        let record = first_record(&bytes);
        let validated = validate_blip_record(&bytes, &record).expect("two UID PNG");
        assert_eq!(validated.uid_rule, BlipUidRule::SecondUid);

        let bad = record_bytes(OFFICE_ART_BLIP_PNG, 0x6E1, BlipKind::Png, true, true);
        let bad_record = first_record(&bad);
        assert!(matches!(
            validate_blip_record(&bad, &bad_record),
            Err(BlipValidationError::UidMismatch { .. })
        ));
    }

    #[test]
    fn two_uid_authority_is_shared_across_raster_families() {
        let cases = [
            (OFFICE_ART_BLIP_JPEG, 0x6E3, BlipKind::Jpeg),
            (OFFICE_ART_BLIP_DIB, 0x7A9, BlipKind::Dib),
            (OFFICE_ART_BLIP_TIFF, 0x6E5, BlipKind::Tiff),
        ];

        for (rec_type, rec_instance, kind) in cases {
            let bytes = record_bytes(rec_type, rec_instance, kind, true, false);
            let record = first_record(&bytes);
            let validated = validate_blip_record(&bytes, &record).expect("two-UID raster BLIP");
            assert_eq!(validated.kind, kind);
            assert_eq!(validated.uid_rule, BlipUidRule::SecondUid);

            let bad = record_bytes(rec_type, rec_instance, kind, true, true);
            let bad_record = first_record(&bad);
            assert!(matches!(
                validate_blip_record(&bad, &bad_record),
                Err(BlipValidationError::UidMismatch { .. })
            ));
        }
    }

    #[test]
    fn uid_layout_must_match_rec_instance() {
        let one_instance_two_uid_layout =
            record_bytes(OFFICE_ART_BLIP_PNG, 0x6E0, BlipKind::Png, true, false);
        let one_record = first_record(&one_instance_two_uid_layout);
        assert!(validate_blip_record(&one_instance_two_uid_layout, &one_record).is_err());

        let two_instance_one_uid_layout =
            record_bytes(OFFICE_ART_BLIP_PNG, 0x6E1, BlipKind::Png, false, false);
        let two_record = first_record(&two_instance_one_uid_layout);
        assert!(validate_blip_record(&two_instance_one_uid_layout, &two_record).is_err());
    }

    #[test]
    fn forged_record_envelope_is_rejected() {
        let bytes = record_bytes(OFFICE_ART_BLIP_PNG, 0x6E0, BlipKind::Png, false, false);
        let mut record = first_record(&bytes);
        record.header.rec_len = record.header.rec_len.saturating_add(1);

        assert!(matches!(
            validate_blip_record(&bytes, &record),
            Err(BlipValidationError::InconsistentRecordEnvelope)
        ));
    }

    #[test]
    fn png_typed_gif_preserves_strict_uid_authority() {
        let bytes = record_bytes(OFFICE_ART_BLIP_PNG, 0x6E0, BlipKind::Gif, false, false);
        let record = first_record(&bytes);
        let validated = validate_blip_record(&bytes, &record).expect("strict GIF-under-PNG");
        assert_eq!(validated.rec_type, OFFICE_ART_BLIP_PNG);
        assert_eq!(validated.rec_instance, 0x6E0);
        assert_eq!(validated.kind, BlipKind::Gif);
        assert_eq!(validated.uid_rule, BlipUidRule::SingleUid);
        assert!(validated.internal_uid_verified);
        assert!(validated.format_header_verified);
    }

    #[test]
    fn png_typed_gif_two_uid_uses_second_uid() {
        let bytes = record_bytes(OFFICE_ART_BLIP_PNG, 0x6E1, BlipKind::Gif, true, false);
        let record = first_record(&bytes);
        let validated = validate_blip_record(&bytes, &record).expect("two-UID GIF-under-PNG");
        assert_eq!(validated.kind, BlipKind::Gif);
        assert_eq!(validated.uid_rule, BlipUidRule::SecondUid);
    }

    #[test]
    fn png_typed_gif_with_bad_effective_uid_is_rejected() {
        let bytes = record_bytes(OFFICE_ART_BLIP_PNG, 0x6E0, BlipKind::Gif, false, true);
        let record = first_record(&bytes);
        assert!(matches!(
            validate_blip_record(&bytes, &record),
            Err(BlipValidationError::UidMismatch { .. })
        ));
    }

    #[test]
    fn signature_with_bad_uid_is_rejected() {
        let bytes = record_bytes(OFFICE_ART_BLIP_JPEG, 0x46A, BlipKind::Jpeg, false, true);
        let record = first_record(&bytes);
        assert!(matches!(
            validate_blip_record(&bytes, &record),
            Err(BlipValidationError::UidMismatch { .. })
        ));
    }

    #[test]
    fn wrong_instance_is_rejected() {
        let bytes = record_bytes(OFFICE_ART_BLIP_PNG, 0x6E0, BlipKind::Png, false, false);
        let mut wrong = bytes.clone();
        wrong[0..2].copy_from_slice(&(0x777u16 << 4).to_le_bytes());
        let record = first_record(&wrong);
        assert!(matches!(
            validate_blip_record(&wrong, &record),
            Err(BlipValidationError::UnsupportedInstance { .. })
        ));
    }

    #[test]
    fn first_record_payload_does_not_absorb_next_record() {
        let first = record_bytes(OFFICE_ART_BLIP_PNG, 0x6E0, BlipKind::Png, false, false);
        let second = record_bytes(OFFICE_ART_BLIP_JPEG, 0x46A, BlipKind::Jpeg, false, false);
        let mut combined = first.clone();
        combined.extend_from_slice(&second);

        let parsed = parse_officeart_stream(stream(), &combined).expect("two records");
        assert_eq!(parsed.records.len(), 2);
        let a = validate_blip_record(&combined, &parsed.records[0]).expect("first strict BLIP");
        assert_eq!(a.record_source.end(), Some(first.len() as u64));
        assert_eq!(a.payload_source.end(), Some(first.len() as u64));

        let original_hash = a.payload_sha256.clone();
        let last = combined.len() - 1;
        combined[last] ^= 0x55;
        let parsed_mutated =
            parse_officeart_stream(stream(), &combined).expect("mutated two records");
        let a_after =
            validate_blip_record(&combined, &parsed_mutated.records[0]).expect("first unchanged");
        assert_eq!(a_after.payload_sha256, original_hash);
    }
}
