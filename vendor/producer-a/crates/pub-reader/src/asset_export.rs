use crate::{
    ESCHER_DELAY_STREAM_PATH, ESCHER_STREAM_PATH, PubAssetManifest, PubAssetUse,
    PubImageResourceCatalog, PubSourceGraph, build_pub_asset_manifest,
    build_pub_image_resource_catalog,
};
use anyhow::{Context, Result, bail};
use flate2::read::ZlibDecoder;
use pub_core::{RawPublication, RawSpan};
use pub_escher::BlipKind;
use pub_model::{ResourceId, Sha256Digest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

pub const PUB_ASSET_EXPORT_SCHEMA_V0_1: &str = "pub-assets-v0.1";
pub const PUB_ASSET_MANIFEST_FILENAME: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubAssetExportFile {
    pub resource_id: ResourceId,
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubAssetExportManifestEntry {
    pub resource_id: ResourceId,
    pub filename: String,
    pub mime: String,
    pub sha256: Sha256Digest,
    pub byte_len: u64,
    pub source: RawSpan,
    pub uses: Vec<PubAssetUse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PubAssetExportDiagnostic {
    AssetNotPromoted { slot: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubAssetExportManifest {
    pub schema_version: String,
    pub assets: Vec<PubAssetExportManifestEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PubAssetExportDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubAssetExportBundle {
    pub files: Vec<PubAssetExportFile>,
    pub manifest: PubAssetExportManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubMaterializedWmfAsset {
    pub slot: u32,
    pub uses: Vec<PubAssetUse>,
    pub sha256: Sha256Digest,
    pub bytes: Vec<u8>,
}

/// Materializes the bounded delayed-WMF subset used by mature Publisher image frames.
///
/// OfficeArt metafile BLIPs carry compressed WMF bytes rather than a direct source span,
/// so they intentionally remain outside the exact-byte asset-export contract above.
/// This helper keeps the transformation format-local: one-UID WMF BLIPs only, no
/// guessed offsets, exact declared lengths, no metafile filter, and a validated WMF
/// payload after decompression.
pub fn materialize_mature_0x2c_wmf_assets_from_bytes(
    pub_bytes: &[u8],
    graph: &PubSourceGraph,
) -> Result<Vec<PubMaterializedWmfAsset>> {
    let escher = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_STREAM_PATH)
        .context("read Escher stream for WMF materialization")?;

    let inventory = pub_cfb::inspect_reader(Cursor::new(pub_bytes))
        .context("inspect CFB for delayed WMF stream")?;
    let has_delayed_stream = inventory
        .entries
        .iter()
        .any(|entry| entry.path == ESCHER_DELAY_STREAM_PATH);
    if !has_delayed_stream {
        return Ok(Vec::new());
    }

    let delayed = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_DELAY_STREAM_PATH)
        .context("read delayed Escher stream for WMF materialization")?;
    let manifest = build_pub_asset_manifest(graph, &escher, &delayed)
        .context("build PUB asset manifest for WMF materialization")?;

    let mut output = Vec::new();
    for asset in manifest.assets {
        if asset.blip_kind != Some(BlipKind::Wmf) {
            continue;
        }
        let Some(record_source) = asset.blip_record_source.as_ref() else {
            continue;
        };
        if record_source.stream.0 != ESCHER_DELAY_STREAM_PATH {
            continue;
        }

        let bytes = decode_delayed_wmf_record(&delayed, record_source)
            .with_context(|| format!("materialize delayed WMF asset slot {}", asset.slot))?;
        let digest = Sha256::digest(&bytes);
        let mut sha = [0_u8; 32];
        sha.copy_from_slice(&digest);

        let mut uses = asset.uses;
        uses.sort();
        uses.dedup();
        output.push(PubMaterializedWmfAsset {
            slot: asset.slot,
            uses,
            sha256: Sha256Digest::from_bytes(sha),
            bytes,
        });
    }
    output.sort_by_key(|asset| asset.slot);
    Ok(output)
}

fn decode_delayed_wmf_record(delayed: &[u8], source: &RawSpan) -> Result<Vec<u8>> {
    let start = usize::try_from(source.offset).context("WMF record offset does not fit usize")?;
    let len = usize::try_from(source.len).context("WMF record length does not fit usize")?;
    let end = start.checked_add(len).context("WMF record range overflow")?;
    let record = delayed
        .get(start..end)
        .context("WMF record source is outside delayed stream")?;
    if record.len() < 8 {
        bail!("WMF OfficeArt record is shorter than 8-byte header");
    }

    let initial = u16::from_le_bytes([record[0], record[1]]);
    let rec_type = u16::from_le_bytes([record[2], record[3]]);
    let declared = u32::from_le_bytes([record[4], record[5], record[6], record[7]]);
    let declared = usize::try_from(declared).context("WMF BLIP length does not fit usize")?;
    if rec_type != 0xF01B || declared.checked_add(8) != Some(record.len()) {
        bail!("WMF OfficeArt record identity/length mismatch");
    }

    let rec_instance = initial >> 4;
    if rec_instance != 0x0216 {
        bail!("unsupported WMF BLIP instance 0x{rec_instance:03X}");
    }

    let payload = &record[8..];
    const UID_BYTES: usize = 16;
    const META_HEADER_BYTES: usize = 34;
    if payload.len() < UID_BYTES + META_HEADER_BYTES {
        bail!("WMF BLIP payload is shorter than UID + metafile header");
    }
    let header = &payload[UID_BYTES..UID_BYTES + META_HEADER_BYTES];
    let uncompressed_len =
        u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let stored_len =
        u32::from_le_bytes([header[28], header[29], header[30], header[31]]) as usize;
    let compression = header[32];
    let filter = header[33];
    if filter != 0xFE {
        bail!("unsupported WMF BLIP filter 0x{filter:02X}");
    }

    let stored = payload
        .get(UID_BYTES + META_HEADER_BYTES..)
        .context("missing WMF BLIP body")?;
    if stored.len() != stored_len {
        bail!(
            "WMF BLIP stored length mismatch: declared {stored_len}, actual {}",
            stored.len()
        );
    }

    let decoded = match compression {
        0x00 => {
            let mut decoder = ZlibDecoder::new(stored);
            let mut bytes = Vec::with_capacity(uncompressed_len);
            decoder
                .read_to_end(&mut bytes)
                .context("inflate WMF BLIP body")?;
            bytes
        }
        0xFE => stored.to_vec(),
        other => bail!("unsupported WMF BLIP compression 0x{other:02X}"),
    };
    if decoded.len() != uncompressed_len {
        bail!(
            "WMF BLIP uncompressed length mismatch: declared {uncompressed_len}, actual {}",
            decoded.len()
        );
    }
    crate::validate_wmf_metafile(&decoded).context("validate decompressed WMF BLIP")?;
    Ok(decoded)
}

/// Builds the exact embedded-image export bundle directly from one complete PUB file.
///
/// This keeps CFB/Escher stream handling inside the format-aware reader layer.
/// Missing EscherDelayStm is treated as an empty delayed stream; unsupported or
/// non-exact image payloads remain explicit manifest diagnostics rather than
/// being guessed from other source state.
pub fn build_mature_0x2c_asset_export_bundle_from_bytes(
    pub_bytes: &[u8],
    graph: &PubSourceGraph,
) -> Result<PubAssetExportBundle> {
    let escher = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_STREAM_PATH)
        .context("read Escher stream for exact asset export")?;

    let inventory = pub_cfb::inspect_reader(Cursor::new(pub_bytes))
        .context("inspect CFB for delayed image stream")?;
    let has_delayed_stream = inventory
        .entries
        .iter()
        .any(|entry| entry.path == ESCHER_DELAY_STREAM_PATH);
    let delayed = if has_delayed_stream {
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes), ESCHER_DELAY_STREAM_PATH)
            .context("read delayed Escher stream for exact asset export")?
    } else {
        Vec::new()
    };

    let manifest = build_pub_asset_manifest(graph, &escher, &delayed)
        .context("build exact PUB asset manifest")?;
    let catalog = build_pub_image_resource_catalog(graph, &manifest)
        .context("promote exact PUB assets to image resources")?;
    let raw = RawPublication::from_streams([
        (pub_core::StreamPath(ESCHER_STREAM_PATH.into()), escher),
        (
            pub_core::StreamPath(ESCHER_DELAY_STREAM_PATH.into()),
            delayed,
        ),
    ]);

    build_pub_asset_export_bundle(&manifest, &catalog, &raw)
        .context("materialize exact embedded PUB image bytes")
}

pub fn build_pub_asset_export_bundle(
    asset_manifest: &PubAssetManifest,
    catalog: &PubImageResourceCatalog,
    raw: &RawPublication,
) -> Result<PubAssetExportBundle> {
    let mut files_by_resource = BTreeMap::<ResourceId, PubAssetExportFile>::new();
    let mut entries_by_resource = BTreeMap::<ResourceId, PubAssetExportManifestEntry>::new();
    let mut diagnostics = Vec::new();

    for asset in &asset_manifest.assets {
        let (Some(payload_sha256), Some(payload_source)) =
            (asset.payload_sha256, asset.image_payload_source.as_ref())
        else {
            diagnostics.push(PubAssetExportDiagnostic::AssetNotPromoted { slot: asset.slot });
            continue;
        };

        let mut resource_ids = BTreeSet::new();
        for usage in &asset.uses {
            if let Some(resource_id) = catalog.node_resources.get(&usage.node_id) {
                resource_ids.insert(*resource_id);
            }
        }

        if resource_ids.is_empty() {
            diagnostics.push(PubAssetExportDiagnostic::AssetNotPromoted { slot: asset.slot });
            continue;
        }
        if resource_ids.len() != 1 {
            bail!(
                "asset slot {} maps to multiple semantic ImageResource ids: {:?}",
                asset.slot,
                resource_ids
            );
        }

        let resource_id = *resource_ids.iter().next().expect("one resource id");
        let resource = catalog
            .resources
            .get(&resource_id)
            .with_context(|| format!("missing ImageResource for asset slot {}", asset.slot))?;

        if resource.source_hash != payload_sha256 {
            bail!(
                "ImageResource {} hash differs from asset manifest slot {}",
                resource_id.as_canonical(),
                asset.slot
            );
        }
        if &resource.original_blob.source != payload_source {
            bail!(
                "ImageResource {} source span differs from asset manifest slot {}",
                resource_id.as_canonical(),
                asset.slot
            );
        }

        let bytes = raw
            .bytes(&resource.original_blob.source)
            .with_context(|| {
                format!(
                    "source bytes unavailable for ImageResource {} at {:?}",
                    resource_id.as_canonical(),
                    resource.original_blob.source
                )
            })?
            .to_vec();

        let digest = Sha256::digest(&bytes);
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&digest);
        let actual_sha256 = Sha256Digest::from_bytes(digest_bytes);
        if actual_sha256 != resource.source_hash {
            bail!(
                "source bytes hash mismatch for ImageResource {}",
                resource_id.as_canonical()
            );
        }

        let filename = asset_filename(resource_id, &resource.mime);
        let mut uses = asset.uses.clone();
        uses.sort();
        uses.dedup();

        let entry = PubAssetExportManifestEntry {
            resource_id,
            filename: filename.clone(),
            mime: resource.mime.clone(),
            sha256: resource.source_hash,
            byte_len: u64::try_from(bytes.len()).context("asset length does not fit u64")?,
            source: resource.original_blob.source.clone(),
            uses,
        };

        if let Some(existing) = entries_by_resource.get(&resource_id) {
            if existing != &entry {
                bail!(
                    "shared ImageResource {} has inconsistent export metadata",
                    resource_id.as_canonical()
                );
            }
            continue;
        }

        files_by_resource.insert(
            resource_id,
            PubAssetExportFile {
                resource_id,
                filename,
                bytes,
            },
        );
        entries_by_resource.insert(resource_id, entry);
    }

    let files = files_by_resource.into_values().collect();
    let assets = entries_by_resource.into_values().collect();

    Ok(PubAssetExportBundle {
        files,
        manifest: PubAssetExportManifest {
            schema_version: PUB_ASSET_EXPORT_SCHEMA_V0_1.to_owned(),
            assets,
            diagnostics,
        },
    })
}

pub fn pub_asset_manifest_json(manifest: &PubAssetExportManifest) -> Result<String> {
    serde_json::to_string_pretty(manifest).context("serialize PUB asset export manifest")
}

pub fn write_pub_asset_export_bundle(
    bundle: &PubAssetExportBundle,
    output_dir: &Path,
) -> Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("create asset output directory {}", output_dir.display()))?;

    for file in &bundle.files {
        let path = output_dir.join(&file.filename);
        fs::write(&path, &file.bytes)
            .with_context(|| format!("write extracted asset {}", path.display()))?;
    }

    let manifest = pub_asset_manifest_json(&bundle.manifest)?;
    let manifest_path = output_dir.join(PUB_ASSET_MANIFEST_FILENAME);
    fs::write(&manifest_path, manifest.as_bytes())
        .with_context(|| format!("write asset manifest {}", manifest_path.display()))?;

    Ok(())
}

fn asset_filename(resource_id: ResourceId, mime: &str) -> String {
    let extension = extension_for_mime(mime);
    format!("asset-{}.{}", resource_id.as_canonical(), extension)
}

fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/x-ms-bmp-dib" => "dib",
        "image/tiff" => "tif",
        "image/x-emf" => "emf",
        "image/x-wmf" => "wmf",
        "image/x-pict" => "pict",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PubAssetManifestEntry, PubImageAlpha, PubImageBlobRef, PubImageResource,
        PubImageResourceDiagnostic,
    };
    use pub_core::StreamPath;
    use pub_escher::BlipKind;
    use pub_model::{CanonicalId, NodeId, PageId};
    use std::collections::BTreeMap;

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn digest(bytes: &[u8]) -> Sha256Digest {
        let hash = Sha256::digest(bytes);
        let mut output = [0_u8; 32];
        output.copy_from_slice(&hash);
        Sha256Digest::from_bytes(output)
    }

    #[test]
    fn bundle_preserves_original_bytes_and_deduplicates_shared_resource() {
        let stream = StreamPath("/Escher/EscherDelayStm".into());
        let payload = vec![0xff, 0xd8, 0xff, 0xd9];
        let raw = RawPublication::from_streams([(stream.clone(), payload.clone())]);
        let source = RawSpan {
            stream,
            offset: 0,
            len: payload.len() as u64,
        };

        let page_id = PageId::from_canonical(canonical(1));
        let node_a = NodeId::from_canonical(canonical(2));
        let node_b = NodeId::from_canonical(canonical(3));
        let resource_id = ResourceId::from_canonical(canonical(4));
        let sha = digest(&payload);

        let manifest = PubAssetManifest {
            assets: vec![PubAssetManifestEntry {
                slot: 1,
                c_ref: 2,
                uid: [0x11; 16],
                bstore_record_source: RawSpan {
                    stream: StreamPath("/Escher/EscherStm".into()),
                    offset: 1,
                    len: 2,
                },
                blip_kind: Some(BlipKind::Jpeg),
                blip_record_source: Some(source.clone()),
                image_payload_source: Some(source.clone()),
                payload_sha256: Some(sha),
                payload_len: Some(payload.len() as u64),
                uses: vec![
                    PubAssetUse {
                        page_id,
                        node_id: node_b,
                    },
                    PubAssetUse {
                        page_id,
                        node_id: node_a,
                    },
                ],
            }],
            diagnostics: Vec::new(),
        };

        let mut resources = BTreeMap::new();
        resources.insert(
            resource_id,
            PubImageResource {
                id: resource_id,
                mime: "image/jpeg".into(),
                source_hash: sha,
                intrinsic_size_px: None,
                color_profile: None,
                alpha: PubImageAlpha::Unknown,
                original_blob: PubImageBlobRef {
                    source: source.clone(),
                    kind: BlipKind::Jpeg,
                },
            },
        );
        let catalog = PubImageResourceCatalog {
            resources,
            node_resources: BTreeMap::from([(node_a, resource_id), (node_b, resource_id)]),
            diagnostics: Vec::<PubImageResourceDiagnostic>::new(),
        };

        let bundle =
            build_pub_asset_export_bundle(&manifest, &catalog, &raw).expect("bundle should build");

        assert_eq!(bundle.files.len(), 1);
        assert_eq!(bundle.files[0].bytes, payload);
        assert!(bundle.files[0].filename.ends_with(".jpg"));
        assert_eq!(bundle.manifest.assets.len(), 1);
        assert_eq!(
            bundle.manifest.assets[0]
                .uses
                .iter()
                .map(|usage| usage.node_id)
                .collect::<Vec<_>>(),
            vec![node_a, node_b]
        );

        let json_a = pub_asset_manifest_json(&bundle.manifest).expect("manifest json");
        let json_b = pub_asset_manifest_json(&bundle.manifest).expect("manifest json");
        assert_eq!(json_a, json_b);
        assert!(json_a.contains(PUB_ASSET_EXPORT_SCHEMA_V0_1));
        assert!(json_a.contains(&bundle.files[0].filename));
    }

    #[test]
    fn decodes_bounded_one_uid_compressed_wmf_blip() {
        let mut wmf = Vec::new();
        wmf.extend_from_slice(&1_u16.to_le_bytes());
        wmf.extend_from_slice(&9_u16.to_le_bytes());
        wmf.extend_from_slice(&0x0300_u16.to_le_bytes());
        wmf.extend_from_slice(&12_u32.to_le_bytes());
        wmf.extend_from_slice(&0_u16.to_le_bytes());
        wmf.extend_from_slice(&3_u32.to_le_bytes());
        wmf.extend_from_slice(&0_u16.to_le_bytes());
        wmf.extend_from_slice(&3_u32.to_le_bytes());
        wmf.extend_from_slice(&0_u16.to_le_bytes());
        crate::validate_wmf_metafile(&wmf).expect("fixture WMF");

        use flate2::{Compression, write::ZlibEncoder};
        use std::io::Write;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&wmf).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut payload = vec![0xAB; 16];
        payload.extend_from_slice(&(wmf.len() as u32).to_le_bytes());
        payload.extend_from_slice(&0_i32.to_le_bytes());
        payload.extend_from_slice(&0_i32.to_le_bytes());
        payload.extend_from_slice(&8_i32.to_le_bytes());
        payload.extend_from_slice(&8_i32.to_le_bytes());
        payload.extend_from_slice(&8_i32.to_le_bytes());
        payload.extend_from_slice(&8_i32.to_le_bytes());
        payload.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        payload.push(0x00);
        payload.push(0xFE);
        payload.extend_from_slice(&compressed);

        let initial = (0x0216_u16 << 4) | 0x0;
        let mut record = Vec::new();
        record.extend_from_slice(&initial.to_le_bytes());
        record.extend_from_slice(&0xF01B_u16.to_le_bytes());
        record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        record.extend_from_slice(&payload);

        let source = RawSpan {
            stream: pub_core::StreamPath(ESCHER_DELAY_STREAM_PATH.into()),
            offset: 0,
            len: record.len() as u64,
        };
        let decoded = decode_delayed_wmf_record(&record, &source).expect("decode WMF BLIP");
        assert_eq!(decoded, wmf);
    }

    #[test]
    fn unknown_mime_uses_bin_without_guessing() {
        let id = ResourceId::from_canonical(canonical(9));
        assert!(asset_filename(id, "application/octet-stream").ends_with(".bin"));
    }
}
