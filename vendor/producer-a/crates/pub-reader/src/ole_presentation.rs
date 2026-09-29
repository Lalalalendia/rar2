use crate::wmf::{WmfMetafileInfo, validate_wmf_metafile};
use anyhow::{Context, Result, bail};
use std::io::{Read, Seek};

const STANDARD_CLIPBOARD_MARKER_ANSI: u32 = 0xffff_ffff;
const STANDARD_CLIPBOARD_MARKER_UNICODE: u32 = 0xffff_fffe;
const CF_METAFILEPICT: u32 = 3;
const METAFILE_RESERVED2_LEN: usize = 18;
const MAX_PRESENTATION_BYTES: usize = 64 * 1024 * 1024;
const MAX_TARGET_DEVICE_BYTES: usize = 1024 * 1024;
const MAX_OLE_PRESENTATION_COUNT: usize = 999;
const MAX_OLE_PRESENTATION_TOTAL_BYTES: usize = 64 * 1024 * 1024;
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
    pub stream_ordinal: u16,
    pub clipboard_format: u32,
    pub aspect: u32,
    pub lindex: u32,
    pub advf: u32,
    pub width: u32,
    pub height: u32,
    pub wmf: WmfMetafileInfo,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyOleCachedPresentationDiagnostic {
    pub stream_path: String,
    pub stream_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LegacyOleCachedPresentationScan {
    pub presentations: Vec<LegacyOleCachedPresentation>,
    pub diagnostics: Vec<LegacyOleCachedPresentationDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyOleCachedPresentationSelection {
    None,
    Selected {
        presentation: LegacyOleCachedPresentation,
        equivalent_candidate_count: usize,
    },
    Ambiguous {
        candidate_count: usize,
    },
}

fn equivalent_cached_presentation(
    left: &LegacyOleCachedPresentation,
    right: &LegacyOleCachedPresentation,
) -> bool {
    left.clipboard_format == right.clipboard_format
        && left.aspect == right.aspect
        && left.lindex == right.lindex
        && left.advf == right.advf
        && left.width == right.width
        && left.height == right.height
        && left.wmf == right.wmf
        && left.data == right.data
}

/// Selects a cached presentation only when the visual payload is unambiguous.
///
/// Stream names and ordinals are storage identities, not display priority. Multiple
/// valid candidates are admitted only when their parsed metadata and exact WMF bytes
/// are equivalent; otherwise callers must keep the OLE frame preview-less.
pub fn select_unambiguous_legacy_ole_cached_presentation(
    scan: &LegacyOleCachedPresentationScan,
) -> LegacyOleCachedPresentationSelection {
    let Some(first) = scan
        .presentations
        .iter()
        .min_by_key(|presentation| presentation.stream_ordinal)
    else {
        return LegacyOleCachedPresentationSelection::None;
    };

    if !scan
        .presentations
        .iter()
        .all(|candidate| equivalent_cached_presentation(first, candidate))
    {
        return LegacyOleCachedPresentationSelection::Ambiguous {
            candidate_count: scan.presentations.len(),
        };
    }

    LegacyOleCachedPresentationSelection::Selected {
        presentation: first.clone(),
        equivalent_candidate_count: scan.presentations.len(),
    }
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

fn ole_pres_stream_ordinal(name: &str) -> Option<u16> {
    let suffix = name.strip_prefix(OLE_PRES_STREAM_PREFIX)?;
    if suffix.len() != 3 || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    suffix.parse().ok()
}

fn parse_cached_presentation_blob(
    blob: pub_cfb::CfbStreamBlob,
) -> Result<LegacyOleCachedPresentation> {
    let stream_ordinal = ole_pres_stream_ordinal(&blob.name)
        .with_context(|| format!("invalid OLE presentation stream name {}", blob.name))?;
    let parsed = parse_cf_metafilepict_ole_presentation(&blob.bytes)
        .with_context(|| format!("parse bounded OLE presentation {}", blob.path))?;
    let wmf = validate_wmf_metafile(parsed.data)
        .with_context(|| format!("validate bounded WMF payload {}", blob.path))?;

    Ok(LegacyOleCachedPresentation {
        stream_path: blob.path,
        stream_name: blob.name,
        stream_ordinal,
        clipboard_format: parsed.clipboard_format,
        aspect: parsed.aspect,
        lindex: parsed.lindex,
        advf: parsed.advf,
        width: parsed.width,
        height: parsed.height,
        wmf,
        data: parsed.data.to_vec(),
    })
}

/// Scans persisted cached OLE presentations under the proven Object N storage.
///
/// CFB count/per-stream/aggregate byte limits are fail-closed. Individual malformed
/// OlePres siblings become diagnostics so a separate valid cached presentation can
/// remain available to a higher-layer preview policy.
pub fn scan_legacy_ole_cached_presentations<R: Read + Seek>(
    reader: R,
    storage_number: u16,
) -> Result<LegacyOleCachedPresentationScan> {
    let storage_path = format!("/Objects/Object {storage_number}");
    let blobs = pub_cfb::read_direct_child_streams_with_prefix_reader(
        reader,
        &storage_path,
        OLE_PRES_STREAM_PREFIX,
        MAX_OLE_PRESENTATION_COUNT,
        MAX_PRESENTATION_BYTES,
        MAX_OLE_PRESENTATION_TOTAL_BYTES,
    )
    .with_context(|| format!("read bounded cached presentations under {storage_path}"))?;

    let mut scan = LegacyOleCachedPresentationScan::default();
    for blob in blobs {
        let stream_path = blob.path.clone();
        let stream_name = blob.name.clone();
        match parse_cached_presentation_blob(blob) {
            Ok(presentation) => scan.presentations.push(presentation),
            Err(error) => scan
                .diagnostics
                .push(LegacyOleCachedPresentationDiagnostic {
                    stream_path,
                    stream_name,
                    reason: error.to_string(),
                }),
        }
    }
    Ok(scan)
}

/// Strict compatibility wrapper for callers that require every persisted OlePres
/// sibling to validate. Preview consumers should use the scan API and surface its
/// diagnostics while selecting only successfully validated candidates.
pub fn read_legacy_ole_cached_presentations<R: Read + Seek>(
    reader: R,
    storage_number: u16,
) -> Result<Vec<LegacyOleCachedPresentation>> {
    let scan = scan_legacy_ole_cached_presentations(reader, storage_number)?;
    if !scan.diagnostics.is_empty() {
        bail!(
            "{} cached OLE presentation stream(s) were rejected",
            scan.diagnostics.len()
        );
    }
    Ok(scan.presentations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn cached_presentation_cfb(streams: &[(&str, Vec<u8>)]) -> Cursor<Vec<u8>> {
        let mut compound =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("create test CFB");
        compound.create_storage("/Objects").expect("Objects storage");
        compound
            .create_storage("/Objects/Object 73")
            .expect("Object 73 storage");
        for (name, bytes) in streams {
            compound
                .create_stream(format!("/Objects/Object 73/{name}"))
                .expect("create OlePres stream")
                .write_all(bytes)
                .expect("write OlePres stream");
        }
        compound.flush().expect("flush test CFB");
        let mut cursor = compound.into_inner();
        cursor.set_position(0);
        cursor
    }

    fn valid_wmf_payload() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&9u16.to_le_bytes());
        bytes.extend_from_slice(&0x0300u16.to_le_bytes());
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

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
        let payload = valid_wmf_payload();
        let blob = pub_cfb::CfbStreamBlob {
            path: "/Objects/Object 73/\u{2}OlePres001".into(),
            name: "\u{2}OlePres001".into(),
            bytes: fixture(CF_METAFILEPICT, 4, &payload),
        };

        let parsed = parse_cached_presentation_blob(blob).expect("cached presentation");
        assert_eq!(parsed.stream_path, "/Objects/Object 73/\u{2}OlePres001");
        assert_eq!(parsed.stream_name, "\u{2}OlePres001");
        assert_eq!(parsed.stream_ordinal, 1);
        assert_eq!(parsed.clipboard_format, CF_METAFILEPICT);
        assert_eq!(parsed.wmf.record_count, 1);
        assert_eq!(parsed.data.as_slice(), payload.as_slice());
    }

    #[test]
    fn cached_presentation_rejects_malformed_wmf_payload() {
        let malformed = [0x06u8; 18];
        let blob = pub_cfb::CfbStreamBlob {
            path: "/Objects/Object 73/\u{2}OlePres001".into(),
            name: "\u{2}OlePres001".into(),
            bytes: fixture(CF_METAFILEPICT, 4, &malformed),
        };

        assert!(parse_cached_presentation_blob(blob).is_err());
    }

    #[test]
    fn scan_keeps_valid_sibling_and_reports_malformed_sibling() {
        let valid = fixture(CF_METAFILEPICT, 4, &valid_wmf_payload());
        let malformed = fixture(CF_METAFILEPICT, 4, &[0x06_u8; 18]);
        let scan = scan_legacy_ole_cached_presentations(
            cached_presentation_cfb(&[
                ("\u{2}OlePres001", valid),
                ("\u{2}OlePres002", malformed),
            ]),
            73,
        )
        .expect("bounded scan");

        assert_eq!(scan.presentations.len(), 1);
        assert_eq!(scan.presentations[0].stream_ordinal, 1);
        assert_eq!(scan.diagnostics.len(), 1);
        assert_eq!(scan.diagnostics[0].stream_name, "\u{2}OlePres002");
    }

    #[test]
    fn selection_is_unique_or_equivalent_only() {
        let valid = fixture(CF_METAFILEPICT, 4, &valid_wmf_payload());
        let scan = scan_legacy_ole_cached_presentations(
            cached_presentation_cfb(&[
                ("\u{2}OlePres002", valid.clone()),
                ("\u{2}OlePres001", valid),
            ]),
            73,
        )
        .expect("bounded scan");

        match select_unambiguous_legacy_ole_cached_presentation(&scan) {
            LegacyOleCachedPresentationSelection::Selected {
                presentation,
                equivalent_candidate_count,
            } => {
                assert_eq!(presentation.stream_ordinal, 1);
                assert_eq!(equivalent_candidate_count, 2);
            }
            other => panic!("expected equivalent selection, got {other:?}"),
        }

        let mut distinct = fixture(CF_METAFILEPICT, 4, &valid_wmf_payload());
        let width_offset = 8 + 4 + 16;
        distinct[width_offset..width_offset + 4].copy_from_slice(&641u32.to_le_bytes());
        let scan = scan_legacy_ole_cached_presentations(
            cached_presentation_cfb(&[
                ("\u{2}OlePres001", fixture(CF_METAFILEPICT, 4, &valid_wmf_payload())),
                ("\u{2}OlePres002", distinct),
            ]),
            73,
        )
        .expect("bounded scan");

        assert_eq!(
            select_unambiguous_legacy_ole_cached_presentation(&scan),
            LegacyOleCachedPresentationSelection::Ambiguous { candidate_count: 2 }
        );
    }

    #[test]
    fn cached_presentation_rejects_noncanonical_stream_suffix() {
        let payload = [0x05u8; 18];
        let blob = pub_cfb::CfbStreamBlob {
            path: "/Objects/Object 73/\u{2}OlePres01x".into(),
            name: "\u{2}OlePres01x".into(),
            bytes: fixture(CF_METAFILEPICT, 4, &payload),
        };

        assert!(parse_cached_presentation_blob(blob).is_err());
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
