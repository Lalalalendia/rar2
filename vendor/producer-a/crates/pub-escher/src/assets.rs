use crate::{
    OfficeArtBody, OfficeArtReadError, OfficeArtRecord, ValidatedBlip, parse_officeart_stream,
    parse_record_at, validate_blip_record,
};
use pub_core::{RawSpan, StreamPath};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const OFFICE_ART_BSTORE_CONTAINER: u16 = 0xF001;
pub const OFFICE_ART_FBSE: u16 = 0xF007;
pub const OFFICE_ART_BLIP_EMF: u16 = 0xF01A;
pub const OFFICE_ART_BLIP_WMF: u16 = 0xF01B;
pub const OFFICE_ART_BLIP_PICT: u16 = 0xF01C;
pub const OFFICE_ART_BLIP_JPEG: u16 = 0xF01D;
pub const OFFICE_ART_BLIP_PNG: u16 = 0xF01E;
pub const OFFICE_ART_BLIP_DIB: u16 = 0xF01F;
pub const OFFICE_ART_BLIP_TIFF: u16 = 0xF029;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlipKind {
    Emf,
    Wmf,
    Pict,
    Jpeg,
    Png,
    Dib,
    Tiff,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BStoreSlot {
    /// One-based index used by OfficeArt pib/PXID.
    pub slot: u32,
    pub record_source: RawSpan,
    pub payload_source: RawSpan,
    pub bt_win32: u8,
    pub bt_macos: u8,
    pub uid: [u8; 16],
    pub tag: u16,
    pub size: u32,
    pub c_ref: u32,
    pub fo_delay: u32,
    pub cb_name: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub name_data: Vec<u8>,
    pub embedded_blip_source: Option<RawSpan>,
}

impl BStoreSlot {
    pub fn is_empty(&self) -> bool {
        self.c_ref == 0
    }

    pub fn has_delayed_blip(&self) -> bool {
        !self.is_empty() && self.fo_delay != u32::MAX && self.embedded_blip_source.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BStoreInventory {
    pub stream: StreamPath,
    pub slots: Vec<BStoreSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelayedBlip {
    /// Zero-based physical record ordinal inside OfficeArtBStoreDelay.
    pub ordinal: u32,
    pub record_source: RawSpan,
    pub payload_source: RawSpan,
    pub rec_type: u16,
    pub rec_instance: u16,
    pub kind: BlipKind,
    /// Exact standard image payload when it can be identified conservatively.
    ///
    /// Raw OfficeArt record/payload carriers remain available even when this is None.
    pub image_payload_source: Option<RawSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelayedBlipInventory {
    pub stream: StreamPath,
    pub records: Vec<DelayedBlip>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DelayedBlipPrefixGap {
    TruncatedHeader {
        offset: u64,
        available: u64,
    },
    DeclaredRecordOutOfBounds {
        offset: u64,
        rec_type: u16,
        declared_len: u32,
        available: u64,
    },
    RecordParseFailure {
        offset: u64,
    },
    NonAdvancingRecord {
        offset: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedDelayedBlipPrefixInventory {
    pub stream: StreamPath,
    /// Bytes physically supplied to this prefix parser. This is not the
    /// directory-declared stream length and must not be used as completeness
    /// evidence.
    pub available_prefix_len: u64,
    pub scanned_record_count: u32,
    pub rejected_complete_blip_count: u32,
    pub records: Vec<ValidatedBlip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_gap: Option<DelayedBlipPrefixGap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetReadError {
    OfficeArt(OfficeArtReadError),
    FbseTooShort {
        offset: u64,
        len: u64,
    },
    FbseNameOutOfBounds {
        offset: u64,
        cb_name: u8,
        payload_len: u64,
    },
    SpanOutOfBounds {
        offset: u64,
        len: u64,
        stream_len: usize,
    },
    OffsetTooLarge {
        offset: u64,
    },
    SlotNotFound {
        slot: u32,
    },
    DelayedRecordNotFound {
        slot: u32,
        fo_delay: u32,
    },
}

impl fmt::Display for AssetReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OfficeArt(error) => write!(formatter, "{error}"),
            Self::FbseTooShort { offset, len } => {
                write!(
                    formatter,
                    "OfficeArtFBSE at {offset} is shorter than 36 bytes: {len}"
                )
            }
            Self::FbseNameOutOfBounds {
                offset,
                cb_name,
                payload_len,
            } => write!(
                formatter,
                "OfficeArtFBSE at {offset} declares nameData {cb_name} bytes outside payload {payload_len}"
            ),
            Self::SpanOutOfBounds {
                offset,
                len,
                stream_len,
            } => write!(
                formatter,
                "raw span {offset}+{len} is outside stream length {stream_len}"
            ),
            Self::OffsetTooLarge { offset } => {
                write!(formatter, "offset {offset} does not fit this process")
            }
            Self::SlotNotFound { slot } => write!(formatter, "BStore slot {slot} not found"),
            Self::DelayedRecordNotFound { slot, fo_delay } => write!(
                formatter,
                "BStore slot {slot} points to missing delayed BLIP offset {fo_delay}"
            ),
        }
    }
}

impl std::error::Error for AssetReadError {}

impl From<OfficeArtReadError> for AssetReadError {
    fn from(value: OfficeArtReadError) -> Self {
        Self::OfficeArt(value)
    }
}

/// Reads all OfficeArtFBSE entries from OfficeArtBStoreContainer records.
///
/// Slot identity is preserved exactly and remains separate from any resolved
/// delayed-record ordinal.
pub fn inspect_bstore(stream: StreamPath, bytes: &[u8]) -> Result<BStoreInventory, AssetReadError> {
    let parsed = parse_officeart_stream(stream.clone(), bytes)?;
    let mut fbse_records = Vec::new();
    collect_fbse_records(&parsed.records, false, &mut fbse_records);

    let mut slots = Vec::with_capacity(fbse_records.len());
    for (index, record) in fbse_records.into_iter().enumerate() {
        slots.push(parse_fbse(bytes, record, index as u32 + 1)?);
    }

    Ok(BStoreInventory { stream, slots })
}

/// Reads the Publisher/host delay stream as an ordered sequence of OfficeArt BLIPs.
pub fn inspect_delayed_blips(
    stream: StreamPath,
    bytes: &[u8],
) -> Result<DelayedBlipInventory, AssetReadError> {
    let parsed = parse_officeart_stream(stream.clone(), bytes)?;
    let mut records = Vec::new();

    for (ordinal, record) in parsed.records.iter().enumerate() {
        if !(0xF018..=0xF117).contains(&record.header.rec_type) {
            continue;
        }

        records.push(DelayedBlip {
            ordinal: ordinal as u32,
            record_source: record.source.clone(),
            payload_source: record.payload_source.clone(),
            rec_type: record.header.rec_type,
            rec_instance: record.header.rec_instance,
            kind: blip_kind(record.header.rec_type),
            image_payload_source: detect_image_payload(bytes, record)?,
        });
    }

    Ok(DelayedBlipInventory { stream, records })
}

/**
Reads a physically available EscherDelay prefix without pretending the
available bytes form a complete stream.

Only complete OfficeArt records are traversed. A terminal short header or a
record whose declared payload crosses the available prefix becomes an explicit
terminal gap while already validated BLIPs remain available. V1 never scans or
resynchronizes beyond a broken record boundary.

Complete records that are outside the current strict BLIP validator (or fail
its integrity checks) are not promoted, but their exact declared record extent
may still be traversed to reach the next sequential record.
*/
pub fn inspect_validated_delayed_blips_prefix(
    stream: StreamPath,
    bytes: &[u8],
) -> ValidatedDelayedBlipPrefixInventory {
    let mut offset = 0usize;
    let mut scanned_record_count = 0u32;
    let mut rejected_complete_blip_count = 0u32;
    let mut records = Vec::new();
    let mut terminal_gap = None;

    while offset < bytes.len() {
        let available = bytes.len() - offset;
        if available < 8 {
            terminal_gap = Some(DelayedBlipPrefixGap::TruncatedHeader {
                offset: offset as u64,
                available: available as u64,
            });
            break;
        }

        let rec_type = read_u16(bytes, offset + 2);
        let rec_len = read_u32(bytes, offset + 4);
        let payload_len = match usize::try_from(rec_len) {
            Ok(value) => value,
            Err(_) => {
                terminal_gap = Some(DelayedBlipPrefixGap::DeclaredRecordOutOfBounds {
                    offset: offset as u64,
                    rec_type,
                    declared_len: rec_len,
                    available: available as u64,
                });
                break;
            }
        };
        let Some(record_end) = offset
            .checked_add(8)
            .and_then(|start| start.checked_add(payload_len))
        else {
            terminal_gap = Some(DelayedBlipPrefixGap::DeclaredRecordOutOfBounds {
                offset: offset as u64,
                rec_type,
                declared_len: rec_len,
                available: available as u64,
            });
            break;
        };
        if record_end > bytes.len() {
            terminal_gap = Some(DelayedBlipPrefixGap::DeclaredRecordOutOfBounds {
                offset: offset as u64,
                rec_type,
                declared_len: rec_len,
                available: available as u64,
            });
            break;
        }

        let record = match parse_record_at(bytes, &stream, offset, bytes.len()) {
            Ok(record) => record,
            Err(_) => {
                terminal_gap = Some(DelayedBlipPrefixGap::RecordParseFailure {
                    offset: offset as u64,
                });
                break;
            }
        };

        let next = record
            .sibling_tail_source
            .as_ref()
            .unwrap_or(&record.source)
            .end()
            .and_then(|value| usize::try_from(value).ok());
        let Some(next) = next else {
            terminal_gap = Some(DelayedBlipPrefixGap::RecordParseFailure {
                offset: offset as u64,
            });
            break;
        };
        if next <= offset {
            terminal_gap = Some(DelayedBlipPrefixGap::NonAdvancingRecord {
                offset: offset as u64,
            });
            break;
        }

        if (0xF018..=0xF117).contains(&rec_type) {
            match validate_blip_record(bytes, &record) {
                Ok(validated) => records.push(validated),
                Err(_) => {
                    rejected_complete_blip_count =
                        rejected_complete_blip_count.saturating_add(1);
                }
            }
        }

        scanned_record_count = scanned_record_count.saturating_add(1);
        offset = next;
    }

    ValidatedDelayedBlipPrefixInventory {
        stream,
        available_prefix_len: bytes.len() as u64,
        scanned_record_count,
        rejected_complete_blip_count,
        records,
        terminal_gap,
    }
}

/// Resolves a one-based BStore slot through its exact foDelay offset.
///
/// Empty or non-delayed slots resolve to None. No ordinal shortcut is used.
pub fn resolve_delayed_blip<'a>(
    bstore: &'a BStoreInventory,
    delayed: &'a DelayedBlipInventory,
    slot: u32,
) -> Result<Option<&'a DelayedBlip>, AssetReadError> {
    let entry = bstore
        .slots
        .iter()
        .find(|entry| entry.slot == slot)
        .ok_or(AssetReadError::SlotNotFound { slot })?;

    if !entry.has_delayed_blip() {
        return Ok(None);
    }

    delayed
        .records
        .iter()
        .find(|record| record.record_source.offset == u64::from(entry.fo_delay))
        .map(Some)
        .ok_or(AssetReadError::DelayedRecordNotFound {
            slot,
            fo_delay: entry.fo_delay,
        })
}

fn collect_fbse_records<'a>(
    records: &'a [OfficeArtRecord],
    inside_bstore: bool,
    output: &mut Vec<&'a OfficeArtRecord>,
) {
    for record in records {
        let now_inside = inside_bstore || record.header.rec_type == OFFICE_ART_BSTORE_CONTAINER;

        if inside_bstore && record.header.rec_type == OFFICE_ART_FBSE {
            output.push(record);
        }

        if let OfficeArtBody::Container { children } = &record.body {
            collect_fbse_records(children, now_inside, output);
        }
    }
}

fn parse_fbse(
    bytes: &[u8],
    record: &OfficeArtRecord,
    slot: u32,
) -> Result<BStoreSlot, AssetReadError> {
    let payload = span_slice(bytes, &record.payload_source)?;
    if payload.len() < 36 {
        return Err(AssetReadError::FbseTooShort {
            offset: record.payload_source.offset,
            len: record.payload_source.len,
        });
    }

    let mut uid = [0_u8; 16];
    uid.copy_from_slice(&payload[2..18]);

    let cb_name = payload[33];
    let name_end =
        36usize
            .checked_add(usize::from(cb_name))
            .ok_or(AssetReadError::FbseNameOutOfBounds {
                offset: record.payload_source.offset,
                cb_name,
                payload_len: record.payload_source.len,
            })?;
    if name_end > payload.len() {
        return Err(AssetReadError::FbseNameOutOfBounds {
            offset: record.payload_source.offset,
            cb_name,
            payload_len: record.payload_source.len,
        });
    }

    let embedded_blip_source = if name_end < payload.len() {
        Some(RawSpan {
            stream: record.payload_source.stream.clone(),
            offset: record.payload_source.offset + name_end as u64,
            len: (payload.len() - name_end) as u64,
        })
    } else {
        None
    };

    Ok(BStoreSlot {
        slot,
        record_source: record.source.clone(),
        payload_source: record.payload_source.clone(),
        bt_win32: payload[0],
        bt_macos: payload[1],
        uid,
        tag: read_u16(payload, 18),
        size: read_u32(payload, 20),
        c_ref: read_u32(payload, 24),
        fo_delay: read_u32(payload, 28),
        cb_name,
        name_data: payload[36..name_end].to_vec(),
        embedded_blip_source,
    })
}

fn detect_image_payload(
    bytes: &[u8],
    record: &OfficeArtRecord,
) -> Result<Option<RawSpan>, AssetReadError> {
    let payload = span_slice(bytes, &record.payload_source)?;
    let search_len = payload.len().min(64);
    let prefix = &payload[..search_len];

    let signature = match record.header.rec_type {
        OFFICE_ART_BLIP_PNG => {
            find_bytes(prefix, &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
        }
        OFFICE_ART_BLIP_JPEG => find_bytes(prefix, &[0xFF, 0xD8, 0xFF]),
        OFFICE_ART_BLIP_DIB => find_dib_header(prefix),
        _ => None,
    };

    Ok(signature.map(|relative| RawSpan {
        stream: record.payload_source.stream.clone(),
        offset: record.payload_source.offset + relative as u64,
        len: record.payload_source.len - relative as u64,
    }))
}

fn blip_kind(rec_type: u16) -> BlipKind {
    match rec_type {
        OFFICE_ART_BLIP_EMF => BlipKind::Emf,
        OFFICE_ART_BLIP_WMF => BlipKind::Wmf,
        OFFICE_ART_BLIP_PICT => BlipKind::Pict,
        OFFICE_ART_BLIP_JPEG => BlipKind::Jpeg,
        OFFICE_ART_BLIP_PNG => BlipKind::Png,
        OFFICE_ART_BLIP_DIB => BlipKind::Dib,
        OFFICE_ART_BLIP_TIFF => BlipKind::Tiff,
        _ => BlipKind::Unknown,
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn find_dib_header(bytes: &[u8]) -> Option<usize> {
    for offset in 0..=bytes.len().saturating_sub(16) {
        let header_size = read_u32(bytes, offset);
        if !matches!(header_size, 40 | 108 | 124) {
            continue;
        }
        let planes = read_u16(bytes, offset + 12);
        let bits_per_pixel = read_u16(bytes, offset + 14);
        if planes == 1 && matches!(bits_per_pixel, 1 | 4 | 8 | 16 | 24 | 32) {
            return Some(offset);
        }
    }
    None
}

fn span_slice<'a>(bytes: &'a [u8], span: &RawSpan) -> Result<&'a [u8], AssetReadError> {
    let start = usize::try_from(span.offset).map_err(|_| AssetReadError::OffsetTooLarge {
        offset: span.offset,
    })?;
    let len = usize::try_from(span.len)
        .map_err(|_| AssetReadError::OffsetTooLarge { offset: span.len })?;
    let end = start
        .checked_add(len)
        .ok_or(AssetReadError::SpanOutOfBounds {
            offset: span.offset,
            len: span.len,
            stream_len: bytes.len(),
        })?;
    bytes
        .get(start..end)
        .ok_or(AssetReadError::SpanOutOfBounds {
            offset: span.offset,
            len: span.len,
            stream_len: bytes.len(),
        })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(name: &str) -> StreamPath {
        StreamPath(name.into())
    }

    fn record(initial: u16, rec_type: u16, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&initial.to_le_bytes());
        bytes.extend_from_slice(&rec_type.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn strict_png_record(corrupt_uid: bool) -> Vec<u8> {
        use md4::{Digest, Md4};

        let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        data.extend_from_slice(b"prefix-salvage");
        let digest = Md4::digest(&data);
        let mut uid = [0u8; 16];
        uid.copy_from_slice(&digest);
        if corrupt_uid {
            uid[0] ^= 0x5A;
        }

        let mut payload = Vec::with_capacity(17 + data.len());
        payload.extend_from_slice(&uid);
        payload.push(0xFF);
        payload.extend_from_slice(&data);
        record(0x6E00, OFFICE_ART_BLIP_PNG, &payload)
    }

    #[test]
    fn reads_fbse_slot_without_collapsing_identity() {
        let mut fbse = vec![0_u8; 36];
        fbse[0] = 6;
        fbse[1] = 6;
        fbse[2..18].copy_from_slice(&[0xAB; 16]);
        fbse[18..20].copy_from_slice(&0x00FFu16.to_le_bytes());
        fbse[20..24].copy_from_slice(&104_174u32.to_le_bytes());
        fbse[24..28].copy_from_slice(&3u32.to_le_bytes());
        fbse[28..32].copy_from_slice(&0u32.to_le_bytes());

        let child = record((6 << 4) | 0x2, OFFICE_ART_FBSE, &fbse);
        let bstore = record(0x000F, OFFICE_ART_BSTORE_CONTAINER, &child);

        let inventory =
            inspect_bstore(stream("/Escher/EscherStm"), &bstore).expect("BStore should parse");
        assert_eq!(inventory.slots.len(), 1);
        let slot = &inventory.slots[0];
        assert_eq!(slot.slot, 1);
        assert_eq!(slot.bt_win32, 6);
        assert_eq!(slot.uid, [0xAB; 16]);
        assert_eq!(slot.c_ref, 3);
        assert_eq!(slot.fo_delay, 0);
        assert!(slot.has_delayed_blip());
    }

    #[test]
    fn empty_fbse_slot_does_not_resolve_to_delay_record() {
        let mut fbse = vec![0_u8; 36];
        fbse[24..28].copy_from_slice(&0u32.to_le_bytes());
        fbse[28..32].copy_from_slice(&u32::MAX.to_le_bytes());

        let child = record(0x0002, OFFICE_ART_FBSE, &fbse);
        let bstore = record(0x000F, OFFICE_ART_BSTORE_CONTAINER, &child);
        let inventory =
            inspect_bstore(stream("/Escher/EscherStm"), &bstore).expect("BStore should parse");

        let delayed = DelayedBlipInventory {
            stream: stream("/Escher/EscherDelayStm"),
            records: Vec::new(),
        };
        assert_eq!(
            resolve_delayed_blip(&inventory, &delayed, 1).expect("empty slot is valid"),
            None
        );
    }

    #[test]
    fn resolves_fo_delay_to_exact_png_record_and_payload() {
        let mut png_payload = vec![0_u8; 17];
        png_payload.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        png_payload.extend_from_slice(b"rest");
        let png = record(0x6E00, OFFICE_ART_BLIP_PNG, &png_payload);

        let delayed = inspect_delayed_blips(stream("/Escher/EscherDelayStm"), &png)
            .expect("delay stream should parse");
        assert_eq!(delayed.records.len(), 1);
        let blip = &delayed.records[0];
        assert_eq!(blip.record_source.offset, 0);
        assert_eq!(blip.kind, BlipKind::Png);
        assert_eq!(
            blip.image_payload_source.as_ref().map(|span| span.offset),
            Some(25)
        );

        let slot = BStoreSlot {
            slot: 1,
            record_source: RawSpan {
                stream: stream("/Escher/EscherStm"),
                offset: 0,
                len: 44,
            },
            payload_source: RawSpan {
                stream: stream("/Escher/EscherStm"),
                offset: 8,
                len: 36,
            },
            bt_win32: 6,
            bt_macos: 6,
            uid: [0; 16],
            tag: 0,
            size: png.len() as u32,
            c_ref: 1,
            fo_delay: 0,
            cb_name: 0,
            name_data: Vec::new(),
            embedded_blip_source: None,
        };
        let bstore = BStoreInventory {
            stream: stream("/Escher/EscherStm"),
            slots: vec![slot],
        };

        let resolved = resolve_delayed_blip(&bstore, &delayed, 1)
            .expect("slot resolution should succeed")
            .expect("non-empty delayed slot");
        assert_eq!(resolved.record_source.offset, 0);
        assert_eq!(resolved.kind, BlipKind::Png);
    }

    #[test]
    fn detects_grounded_one_bit_dib_payload_after_blip_prefix() {
        let mut dib_payload = vec![0_u8; 17];
        dib_payload.extend_from_slice(&40u32.to_le_bytes());
        dib_payload.extend_from_slice(&8i32.to_le_bytes());
        dib_payload.extend_from_slice(&8i32.to_le_bytes());
        dib_payload.extend_from_slice(&1u16.to_le_bytes());
        dib_payload.extend_from_slice(&1u16.to_le_bytes());
        dib_payload.extend_from_slice(&[0; 64]);

        let dib = record(0x7A80, OFFICE_ART_BLIP_DIB, &dib_payload);
        let delayed = inspect_delayed_blips(stream("/Escher/EscherDelayStm"), &dib)
            .expect("DIB delay record should parse");
        assert_eq!(delayed.records[0].kind, BlipKind::Dib);
        assert_eq!(
            delayed.records[0]
                .image_payload_source
                .as_ref()
                .map(|span| span.offset),
            Some(25)
        );
    }

 
    #[test]
    fn prefix_retains_complete_blip_before_truncated_terminal_header() {
        let first = strict_png_record(false);
        let mut bytes = first.clone();
        bytes.extend_from_slice(&[0x00, 0x6E, 0x1E, 0xF0]);

        let observed =
            inspect_validated_delayed_blips_prefix(stream("/Escher/EscherDelayStm"), &bytes);
        assert_eq!(observed.records.len(), 1);
        assert_eq!(observed.scanned_record_count, 1);
        assert_eq!(observed.rejected_complete_blip_count, 0);
        assert_eq!(observed.records[0].record_source.offset, 0);
        assert_eq!(observed.records[0].record_source.end(), Some(first.len() as u64));
        assert!(matches!(
            observed.terminal_gap,
            Some(DelayedBlipPrefixGap::TruncatedHeader {
                offset,
                available: 4
            }) if offset == first.len() as u64
        ));
    }

    #[test]
    fn prefix_retains_complete_blip_before_declared_record_oob() {
        let first = strict_png_record(false);
        let mut bytes = first.clone();
        bytes.extend_from_slice(&0x6E00u16.to_le_bytes());
        bytes.extend_from_slice(&OFFICE_ART_BLIP_PNG.to_le_bytes());
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 12]);

        let observed =
            inspect_validated_delayed_blips_prefix(stream("/Escher/EscherDelayStm"), &bytes);
        assert_eq!(observed.records.len(), 1);
        assert_eq!(observed.scanned_record_count, 1);
        assert!(matches!(
            observed.terminal_gap,
            Some(DelayedBlipPrefixGap::DeclaredRecordOutOfBounds {
                offset,
                rec_type: OFFICE_ART_BLIP_PNG,
                declared_len: 100,
                ..
            }) if offset == first.len() as u64
        ));
    }

    #[test]
    fn complete_bad_uid_record_is_rejected_without_resync_or_losing_neighbors() {
        let first = strict_png_record(false);
        let bad = strict_png_record(true);
        let third = strict_png_record(false);
        let mut bytes = first.clone();
        bytes.extend_from_slice(&bad);
        bytes.extend_from_slice(&third);

        let observed =
            inspect_validated_delayed_blips_prefix(stream("/Escher/EscherDelayStm"), &bytes);
        assert_eq!(observed.scanned_record_count, 3);
        assert_eq!(observed.rejected_complete_blip_count, 1);
        assert_eq!(observed.records.len(), 2);
        assert!(observed.terminal_gap.is_none());
        assert_eq!(observed.records[0].record_source.offset, 0);
        assert_eq!(
            observed.records[1].record_source.offset,
            (first.len() + bad.len()) as u64
        );
    }

    #[test]
    fn exact_record_boundary_does_not_claim_complete_stream() {
        let bytes = strict_png_record(false);
        let observed =
            inspect_validated_delayed_blips_prefix(stream("/Escher/EscherDelayStm"), &bytes);

        assert_eq!(observed.available_prefix_len, bytes.len() as u64);
        assert_eq!(observed.records.len(), 1);
        assert!(observed.terminal_gap.is_none());
        // Absence of a parser-visible terminal gap means only that the supplied
        // bytes end on a complete record boundary. Physical stream completeness
        // belongs to the CFB prefix evidence, not this parser.
    }

    #[test]
    fn complete_supported_prefix_matches_strict_record_boundaries() {
        let first = strict_png_record(false);
        let second = strict_png_record(false);
        let mut bytes = first.clone();
        bytes.extend_from_slice(&second);

        let strict = inspect_delayed_blips(stream("/Escher/EscherDelayStm"), &bytes)
            .expect("complete delay stream");
        let prefix =
            inspect_validated_delayed_blips_prefix(stream("/Escher/EscherDelayStm"), &bytes);

        assert_eq!(strict.records.len(), 2);
        assert_eq!(prefix.records.len(), 2);
        assert!(prefix.terminal_gap.is_none());
        for (strict_record, validated_record) in strict.records.iter().zip(&prefix.records) {
            assert_eq!(strict_record.record_source, validated_record.record_source);
        }
    }
}

