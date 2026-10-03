use anyhow::{Context, Result, bail};
use pub_contents::{ContentsFamily, detect_family};
use pub_reader::{
    PubBorderArtAssetEntryV1, rasterize_wmf_preview,
    read_mature_0x2c_borderart_assets_from_pub_bytes_v1,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const CONTENTS_STREAM_PATH: &str = "/Contents";
const PREVIEW_SIDE_PX: u32 = 256;

#[derive(Debug, Serialize)]
struct ResourceReceipt {
    catalog_ordinal: u32,
    pool_index: u8,
    source_wmf_sha256: String,
    rasterizable: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ShapeUseReceipt {
    contents_seq_num: u32,
    fbid: u16,
    catalog_entry_available: bool,
    selected_resource_count: usize,
    all_selected_resources_rasterizable: bool,
}

#[derive(Debug, Serialize)]
struct FileReceipt {
    source_sha256: String,
    status: String,
    shape_uses: Vec<ShapeUseReceipt>,
    selected_resources: Vec<ResourceReceipt>,
    diagnostic_codes: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CorpusReceipt {
    schema: &'static str,
    corpus_file_count: usize,
    mature_0x2c_file_count: usize,
    files_with_shape_uses: usize,
    shape_use_count: usize,
    resolved_shape_use_count: usize,
    unresolved_shape_use_count: usize,
    complete_rasterizable_shape_use_count: usize,
    selected_resource_count: usize,
    rasterizable_resource_count: usize,
    nonrasterizable_resource_count: usize,
    files_with_nonrasterizable_selected_resource: usize,
    rows: Vec<FileReceipt>,
    evidence_boundary: &'static str,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn scan_mature_file(pub_bytes: &[u8], source_sha256: String) -> FileReceipt {
    let read = match read_mature_0x2c_borderart_assets_from_pub_bytes_v1(pub_bytes) {
        Ok(read) => read,
        Err(error) => {
            return FileReceipt {
                source_sha256,
                status: "mature_read_error".into(),
                shape_uses: Vec::new(),
                selected_resources: Vec::new(),
                diagnostic_codes: Vec::new(),
                error: Some(format!("{error:#}")),
            };
        }
    };

    let diagnostic_codes = read
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect::<Vec<_>>();
    let entries = read
        .entries
        .iter()
        .map(|entry| (entry.ordinal, entry))
        .collect::<BTreeMap<u32, &PubBorderArtAssetEntryV1>>();

    let selected_keys = read
        .shape_uses
        .iter()
        .filter_map(|shape_use| {
            entries
                .get(&u32::from(shape_use.fbid))
                .map(|entry| {
                    entry
                        .resources
                        .iter()
                        .map(|resource| (entry.ordinal, resource.pool_index))
                        .collect::<Vec<_>>()
                })
        })
        .flatten()
        .collect::<BTreeSet<_>>();

    let mut selected_resources = Vec::new();
    let mut raster_ok = BTreeMap::<(u32, u8), bool>::new();
    for (ordinal, pool_index) in selected_keys {
        let entry = entries
            .get(&ordinal)
            .expect("selected BorderArt entry must remain available");
        let resource = entry
            .resources
            .iter()
            .find(|resource| resource.pool_index == pool_index)
            .expect("selected BorderArt pool index must remain available");

        let result = rasterize_wmf_preview(
            &resource.bytes,
            PREVIEW_SIDE_PX,
            PREVIEW_SIDE_PX,
        );
        let (rasterizable, error) = match result {
            Ok(preview) if preview.width > 0 && preview.height > 0 && !preview.rgba.is_empty() => {
                (true, None)
            }
            Ok(_) => (
                false,
                Some("bounded rasterizer returned an empty preview".into()),
            ),
            Err(error) => (false, Some(error.to_string())),
        };
        raster_ok.insert((ordinal, pool_index), rasterizable);
        selected_resources.push(ResourceReceipt {
            catalog_ordinal: ordinal,
            pool_index,
            source_wmf_sha256: resource.sha256.clone(),
            rasterizable,
            error,
        });
    }

    let shape_uses = read
        .shape_uses
        .iter()
        .map(|shape_use| {
            let entry = entries.get(&u32::from(shape_use.fbid)).copied();
            let selected_resource_count = entry.map_or(0, |entry| entry.resources.len());
            let all_selected_resources_rasterizable = entry.is_some_and(|entry| {
                !entry.resources.is_empty()
                    && entry.resources.iter().all(|resource| {
                        raster_ok
                            .get(&(entry.ordinal, resource.pool_index))
                            .copied()
                            .unwrap_or(false)
                    })
            });
            ShapeUseReceipt {
                contents_seq_num: shape_use.contents_seq_num,
                fbid: shape_use.fbid,
                catalog_entry_available: entry.is_some(),
                selected_resource_count,
                all_selected_resources_rasterizable,
            }
        })
        .collect::<Vec<_>>();

    FileReceipt {
        source_sha256,
        status: "mature_scanned".into(),
        shape_uses,
        selected_resources,
        diagnostic_codes,
        error: None,
    }
}

fn scan_file(path: &Path) -> FileReceipt {
    let pub_bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return FileReceipt {
                source_sha256: String::new(),
                status: "read_error".into(),
                shape_uses: Vec::new(),
                selected_resources: Vec::new(),
                diagnostic_codes: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };
    let source_sha256 = sha256_hex(&pub_bytes);

    let contents = match pub_cfb::read_stream_reader(
        Cursor::new(pub_bytes.as_slice()),
        CONTENTS_STREAM_PATH,
    ) {
        Ok(contents) => contents,
        Err(error) => {
            return FileReceipt {
                source_sha256,
                status: "contents_error".into(),
                shape_uses: Vec::new(),
                selected_resources: Vec::new(),
                diagnostic_codes: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };

    match detect_family(&contents) {
        Ok(ContentsFamily::Family0x2c) => scan_mature_file(&pub_bytes, source_sha256),
        Ok(_) => FileReceipt {
            source_sha256,
            status: "skipped_non_0x2c".into(),
            shape_uses: Vec::new(),
            selected_resources: Vec::new(),
            diagnostic_codes: Vec::new(),
            error: None,
        },
        Err(error) => FileReceipt {
            source_sha256,
            status: "family_error".into(),
            shape_uses: Vec::new(),
            selected_resources: Vec::new(),
            diagnostic_codes: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

fn main() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        bail!("usage: borderart-viewer-raster-census <corpus-dir> <out.json>");
    }
    let corpus_dir = PathBuf::from(&args[1]);
    let out_path = PathBuf::from(&args[2]);

    let mut paths = fs::read_dir(&corpus_dir)
        .with_context(|| format!("read corpus dir {}", corpus_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    paths.sort();

    let mut rows = paths.iter().map(|path| scan_file(path)).collect::<Vec<_>>();
    rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));

    let mature_0x2c_file_count = rows
        .iter()
        .filter(|row| row.status == "mature_scanned")
        .count();
    let files_with_shape_uses = rows
        .iter()
        .filter(|row| !row.shape_uses.is_empty())
        .count();
    let shape_use_count = rows.iter().map(|row| row.shape_uses.len()).sum();
    let resolved_shape_use_count = rows
        .iter()
        .flat_map(|row| row.shape_uses.iter())
        .filter(|shape_use| shape_use.catalog_entry_available)
        .count();
    let unresolved_shape_use_count = shape_use_count - resolved_shape_use_count;
    let complete_rasterizable_shape_use_count = rows
        .iter()
        .flat_map(|row| row.shape_uses.iter())
        .filter(|shape_use| shape_use.all_selected_resources_rasterizable)
        .count();
    let selected_resource_count = rows
        .iter()
        .map(|row| row.selected_resources.len())
        .sum();
    let rasterizable_resource_count = rows
        .iter()
        .flat_map(|row| row.selected_resources.iter())
        .filter(|resource| resource.rasterizable)
        .count();
    let nonrasterizable_resource_count = selected_resource_count - rasterizable_resource_count;
    let files_with_nonrasterizable_selected_resource = rows
        .iter()
        .filter(|row| {
            row.selected_resources
                .iter()
                .any(|resource| !resource.rasterizable)
        })
        .count();

    let receipt = CorpusReceipt {
        schema: "chaptera.borderart-viewer-raster-census.v1",
        corpus_file_count: rows.len(),
        mature_0x2c_file_count,
        files_with_shape_uses,
        shape_use_count,
        resolved_shape_use_count,
        unresolved_shape_use_count,
        complete_rasterizable_shape_use_count,
        selected_resource_count,
        rasterizable_resource_count,
        nonrasterizable_resource_count,
        files_with_nonrasterizable_selected_resource,
        rows,
        evidence_boundary: "Exact pinned-corpus read-only product-path census. Only mature-0x2C shapes with exact persisted Fbid are followed into the production BorderArt catalog projection. Only resources selected by those entries are tested through the existing bounded Viewer WMF rasterizer at a fixed square preview target. This does not infer StretchPictures, BorderArt weight/color, native mutation causality, or authoring behavior.",
    };

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
