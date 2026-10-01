use crate::{
    ESCHER_DELAY_STREAM_PATH, ESCHER_STREAM_PATH, PubAssetUse, PubSourceGraph,
    build_pub_asset_manifest, bounded_wmf_metafile,
};
use anyhow::{Context, Result, bail};
use flate2::read::ZlibDecoder;
use pub_escher::{BlipKind, OFFICE_ART_BLIP_WMF, inspect_delayed_blips};
use std::io::{Cursor, Read};

pub const MATURE_OFFICEART_WMF_PREVIEW_SOURCE_V1: &str =
    "mature-officeart-wmf-preview-source-v1";

const OFFICEART_WMF_ONE_UID_INSTANCE: u16 = 0x0216;
const OFFICEART_WMF_ONE_UID_PREFIX_BYTES: usize = 50;
const OFFICEART_WMF_COMPRESSION_DEFLATE: u8 = 0x00;
const OFFICEART_WMF_FILTER_NONE: u8 = 0xfe;
const MAX_OFFICEART_WMF_INFLATED_BYTES: u32 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubMatureOfficeArtWmfPreviewSource {
    pub slot: u32,
    pub uses: Vec<PubAssetUse>,
    pub width_hint: u32,
    pub height_hint: u32,
    /// Derived bounded WMF bytes suitable for the existing pure-Rust preview
    /// rasterizer. Source OfficeArt bytes remain immutable.
    pub wmf_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubMatureOfficeArtWmfPreviewBundle {
    pub sources: Vec<PubMatureOfficeArtWmfPreviewSource>,
    /// Count only; callers must not promote rejected carrier details into the
    /// source-neutral Viewer contract.
    pub rejected_source_count: usize,
}

pub fn build_mature_0x2c_wmf_preview_bundle_from_bytes(
    pub_bytes: &[u8],
    graph: &PubSourceGraph,
) -> Result<PubMatureOfficeArtWmfPreviewBundle> {
    let escher = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_STREAM_PATH)
        .context("read Escher stream for mature WMF preview")?;
    let inventory = pub_cfb::inspect_reader(Cursor::new(pub_bytes))
        .context("inspect CFB for mature WMF preview")?;
    let has_delayed_stream = inventory
        .entries
        .iter()
        .any(|entry| entry.path == ESCHER_DELAY_STREAM_PATH);
    if !has_delayed_stream {
        return Ok(PubMatureOfficeArtWmfPreviewBundle {
            sources: Vec::new(),
            rejected_source_count: 0,
        });
    }
    let delayed = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_DELAY_STREAM_PATH)
        .context("read delayed Escher stream for mature WMF preview")?;
    let manifest = build_pub_asset_manifest(graph, &escher, &delayed)
        .context("build mature asset manifest for WMF preview")?;
    let delayed_inventory = inspect_delayed_blips(
        pub_core::StreamPath(ESCHER_DELAY_STREAM_PATH.into()),
        &delayed,
    )
    .context("inspect mature delayed BLIPs for WMF preview")?;

    let mut sources = Vec::new();
    let mut rejected_source_count = 0usize;

    for asset in manifest
        .assets
        .iter()
        .filter(|asset| asset.blip_kind == Some(BlipKind::Wmf))
    {
        let Some(record_source) = asset.blip_record_source.as_ref() else {
            rejected_source_count += 1;
            continue;
        };
        let Some(record) = delayed_inventory
            .records
            .iter()
            .find(|record| &record.record_source == record_source)
        else {
            rejected_source_count += 1;
            continue;
        };

        let payload = match slice_span(&delayed, &record.payload_source)
            .and_then(|payload| decode_officeart_wmf_payload(payload, record.rec_type, record.rec_instance))
        {
            Ok(payload) => payload,
            Err(_) => {
                rejected_source_count += 1;
                continue;
            }
        };

        let mut uses = asset.uses.clone();
        uses.sort();
        uses.dedup();
        sources.push(PubMatureOfficeArtWmfPreviewSource {
            slot: asset.slot,
            uses,
            width_hint: payload.width_hint,
            height_hint: payload.height_hint,
            wmf_bytes: payload.wmf_bytes,
        });
    }

    Ok(PubMatureOfficeArtWmfPreviewBundle {
        sources,
        rejected_source_count,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DecodedOfficeArtWmf {
    width_hint: u32,
    height_hint: u32,
    wmf_bytes: Vec<u8>,
}

fn decode_officeart_wmf_payload(
    payload: &[u8],
    rec_type: u16,
    rec_instance: u16,
) -> Result<DecodedOfficeArtWmf> {
    if rec_type != OFFICE_ART_BLIP_WMF {
        bail!("OfficeArt BLIP is not WMF");
    }
    if rec_instance != OFFICEART_WMF_ONE_UID_INSTANCE {
        bail!("unsupported OfficeArt WMF UID/profile instance");
    }
    if payload.len() < OFFICEART_WMF_ONE_UID_PREFIX_BYTES {
        bail!("OfficeArt WMF payload is shorter than one-UID MetafileHeader");
    }

    let cb_size = read_u32(payload, 16).context("missing OfficeArt WMF cbSize")?;
    if cb_size == 0 || cb_size > MAX_OFFICEART_WMF_INFLATED_BYTES {
        bail!("OfficeArt WMF cbSize exceeds bounded profile");
    }
    let width_hint = read_i32(payload, 36).context("missing OfficeArt WMF ptSize.x")?;
    let height_hint = read_i32(payload, 40).context("missing OfficeArt WMF ptSize.y")?;
    let width_hint =
        u32::try_from(width_hint).context("OfficeArt WMF ptSize.x must be positive")?;
    let height_hint =
        u32::try_from(height_hint).context("OfficeArt WMF ptSize.y must be positive")?;
    if width_hint == 0 || height_hint == 0 {
        bail!("OfficeArt WMF ptSize must be positive");
    }

    let cb_save = read_u32(payload, 44).context("missing OfficeArt WMF cbSave")?;
    let compression = payload[48];
    let filter = payload[49];
    if compression != OFFICEART_WMF_COMPRESSION_DEFLATE
        || filter != OFFICEART_WMF_FILTER_NONE
    {
        bail!("unsupported OfficeArt WMF compression/filter profile");
    }
    let compressed = &payload[OFFICEART_WMF_ONE_UID_PREFIX_BYTES..];
    if usize::try_from(cb_save).ok() != Some(compressed.len()) {
        bail!("OfficeArt WMF cbSave differs from bounded compressed payload");
    }

    let mut decoder = ZlibDecoder::new(compressed);
    let mut inflated = Vec::with_capacity(usize::try_from(cb_size).unwrap_or(0));
    decoder
        .by_ref()
        .take(u64::from(cb_size) + 1)
        .read_to_end(&mut inflated)
        .context("inflate OfficeArt WMF BLIPFileData")?;
    if inflated.len() != usize::try_from(cb_size).context("OfficeArt WMF cbSize overflow")? {
        bail!("OfficeArt WMF inflated byte length differs from cbSize");
    }

    let bounded = bounded_wmf_metafile(&inflated)
        .context("validate bounded OfficeArt WMF metafile")?;
    if bounded.source_len != inflated.len() {
        bail!("OfficeArt WMF contains bytes after the first bounded META_EOF");
    }

    Ok(DecodedOfficeArtWmf {
        width_hint,
        height_hint,
        wmf_bytes: bounded.normalized_bytes,
    })
}

fn slice_span<'a>(bytes: &'a [u8], span: &pub_core::RawSpan) -> Result<&'a [u8]> {
    let start = usize::try_from(span.offset).context("span offset does not fit usize")?;
    let len = usize::try_from(span.len).context("span length does not fit usize")?;
    let end = start.checked_add(len).context("span end overflow")?;
    bytes
        .get(start..end)
        .context("span is outside source stream")
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn minimal_wmf() -> Vec<u8> {
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

    fn one_uid_wmf_payload(wmf: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(wmf).expect("compress synthetic WMF");
        let compressed = encoder.finish().expect("finish synthetic WMF compression");

        let mut payload = vec![0x11; 16];
        payload.extend_from_slice(&(wmf.len() as u32).to_le_bytes());
        payload.extend_from_slice(&0i32.to_le_bytes());
        payload.extend_from_slice(&0i32.to_le_bytes());
        payload.extend_from_slice(&100i32.to_le_bytes());
        payload.extend_from_slice(&100i32.to_le_bytes());
        payload.extend_from_slice(&1_270_000i32.to_le_bytes());
        payload.extend_from_slice(&635_000i32.to_le_bytes());
        payload.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        payload.push(OFFICEART_WMF_COMPRESSION_DEFLATE);
        payload.push(OFFICEART_WMF_FILTER_NONE);
        payload.extend_from_slice(&compressed);
        payload
    }

    #[test]
    fn decodes_one_uid_deflated_officeart_wmf_into_bounded_metafile() {
        let wmf = minimal_wmf();
        let payload = one_uid_wmf_payload(&wmf);
        let decoded = decode_officeart_wmf_payload(
            &payload,
            OFFICE_ART_BLIP_WMF,
            OFFICEART_WMF_ONE_UID_INSTANCE,
        )
        .expect("bounded OfficeArt WMF");
        assert_eq!(decoded.width_hint, 1_270_000);
        assert_eq!(decoded.height_hint, 635_000);
        assert_eq!(decoded.wmf_bytes, wmf);
    }

    #[test]
    fn bounded_wmf_normalization_is_derived_not_source_mutation() {
        let mut wmf = minimal_wmf();
        wmf[6..10].copy_from_slice(&11u32.to_le_bytes());
        let payload = one_uid_wmf_payload(&wmf);
        let source = payload.clone();

        let decoded = decode_officeart_wmf_payload(
            &payload,
            OFFICE_ART_BLIP_WMF,
            OFFICEART_WMF_ONE_UID_INSTANCE,
        )
        .expect("bounded stale-size WMF");
        assert_eq!(payload, source);
        assert_eq!(&decoded.wmf_bytes[6..10], &12u32.to_le_bytes());
    }

    #[test]
    fn rejects_wrong_instance_filter_and_declared_compressed_length() {
        let wmf = minimal_wmf();
        let payload = one_uid_wmf_payload(&wmf);
        assert!(
            decode_officeart_wmf_payload(&payload, OFFICE_ART_BLIP_WMF, 0x0217).is_err()
        );

        let mut wrong_filter = payload.clone();
        wrong_filter[49] = 0;
        assert!(
            decode_officeart_wmf_payload(
                &wrong_filter,
                OFFICE_ART_BLIP_WMF,
                OFFICEART_WMF_ONE_UID_INSTANCE,
            )
            .is_err()
        );

        let mut wrong_size = payload;
        wrong_size[44..48].copy_from_slice(&1u32.to_le_bytes());
        assert!(
            decode_officeart_wmf_payload(
                &wrong_size,
                OFFICE_ART_BLIP_WMF,
                OFFICEART_WMF_ONE_UID_INSTANCE,
            )
            .is_err()
        );
    }
}
