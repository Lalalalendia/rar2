use anyhow::{bail, Context, Result};
use pub_contents::{
    detect_family, parse_confirmed_block, ContentsCursor, ContentsFamily, RawContentsBlock,
    RawContentsBlockBody, BLOCK_TYPE_BINARY, BLOCK_TYPE_CONTAINER_88, BLOCK_TYPE_CONTAINER_90,
    BLOCK_TYPE_CONTAINER_A0, BLOCK_TYPE_U16, BLOCK_TYPE_U32,
};
use pub_core::RawSpan;
use pub_reader::{
    read_mature_0x2c_borderart_catalog_v1, validate_wmf_metafile, PubBorderArtCatalogEntryV1,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const CONTENTS_STREAM_PATH: &str = "/Contents";

#[derive(Debug, Serialize)]
struct ResourceRow {
    slot: usize,
    field_id: u16,
    declared_length: u32,
    payload_len: usize,
    source_offset: u64,
    sha256: String,
    wmf_valid: bool,
    wmf_version: Option<u16>,
    wmf_declared_bytes: Option<usize>,
    wmf_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct EntryRow {
    ordinal: u32,
    name: String,
    dzl_corner: Option<u32>,
    dxl_horiz: Option<u32>,
    dyl_vert: Option<u32>,
    cmeta: Option<u32>,
    metadata_offsets: Vec<u16>,
    cfbmd: Option<u16>,
    resources: Vec<ResourceRow>,
    all_required_scalars_present: bool,
    exact_eight_offsets: bool,
    exact_eight_resources: bool,
    resource_count_matches_cfbmd: bool,
    resource_starts: Vec<u32>,
    slot_resource_indices: Vec<usize>,
    exact_eight_slot_refs: bool,
    first_payload_offset_108: bool,
    all_slot_offsets_resolve_to_resource_pool: bool,
    all_resources_strict_wmf: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct FileRow {
    source_sha256: String,
    file_name: String,
    status: String,
    ifbmax: Option<u32>,
    entry_count: usize,
    entries: Vec<EntryRow>,
    catalog_diagnostic_codes: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CorpusReceipt {
    schema: &'static str,
    corpus_file_count: usize,
    mature_0x2c_file_count: usize,
    files_with_usable_catalog: usize,
    catalog_entry_count: usize,
    entries_with_exact_eight_resources: usize,
    entries_with_resource_count_matching_cfbmd: usize,
    entries_with_eight_slot_resolution: usize,
    binary_resource_count: usize,
    strict_wmf_resource_count: usize,
    invalid_wmf_resource_count: usize,
    cfbmd_value_counts: BTreeMap<u16, usize>,
    geometry_triplet_count: usize,
    name_counts: BTreeMap<String, usize>,
    rows: Vec<FileRow>,
    evidence_boundary: &'static str,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn span_slice<'a>(bytes: &'a [u8], span: &RawSpan) -> Result<&'a [u8]> {
    let start = usize::try_from(span.offset).context("span start")?;
    let len = usize::try_from(span.len).context("span length")?;
    let end = start.checked_add(len).context("span end overflow")?;
    bytes.get(start..end).with_context(|| {
        format!(
            "span {start}..{end} exceeds Contents length {}",
            bytes.len()
        )
    })
}

fn exact_u32(block: &RawContentsBlock, field_id: u16) -> Option<u32> {
    if block.id != field_id || block.block_type != BLOCK_TYPE_U32 {
        return None;
    }
    match block.body {
        RawContentsBlockBody::U32 { value, .. } => Some(value),
        _ => None,
    }
}

fn exact_u16(block: &RawContentsBlock, field_id: u16) -> Option<u16> {
    if block.id != field_id || block.block_type != BLOCK_TYPE_U16 {
        return None;
    }
    match block.body {
        RawContentsBlockBody::U16 { value, .. } => Some(value),
        _ => None,
    }
}

fn container_source(block: &RawContentsBlock, field_id: u16, wire: u8) -> Option<RawSpan> {
    if block.id != field_id || block.block_type != wire {
        return None;
    }
    match &block.body {
        RawContentsBlockBody::Container { content_source, .. } => Some(content_source.clone()),
        _ => None,
    }
}

fn parse_metadata_offsets(contents: &[u8], source: &RawSpan) -> Result<Vec<u16>> {
    let start = usize::try_from(source.offset).context("metadata start")?;
    let len = usize::try_from(source.len).context("metadata length")?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), contents, start, len)?;
    let mut rows = Vec::new();
    while cursor.remaining() > 0 {
        let block = parse_confirmed_block(&mut cursor).context("parse OplRgfbMeta.Data")?;
        if block.block_type != BLOCK_TYPE_U16 {
            bail!(
                "OplRgfbMeta child field0x{:03X} uses wire0x{:02X}, expected U16",
                block.id,
                block.block_type
            );
        }
        let RawContentsBlockBody::U16 { value, .. } = block.body else {
            bail!("OplRgfbMeta U16 has wrong body");
        };
        if block.id != 0 {
            bail!(
                "OplRgfbMeta repeated Data coordinate mismatch: expected field0, got {}",
                block.id
            );
        }
        rows.push(value);
    }
    Ok(rows)
}

fn parse_resources(contents: &[u8], source: &RawSpan) -> Result<Vec<ResourceRow>> {
    let start = usize::try_from(source.offset).context("RgFbmd start")?;
    let len = usize::try_from(source.len).context("RgFbmd length")?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), contents, start, len)?;
    let mut rows = Vec::new();

    while cursor.remaining() > 0 {
        let child = parse_confirmed_block(&mut cursor).context("parse OplFbmd child")?;
        if child.block_type != BLOCK_TYPE_CONTAINER_88 {
            bail!(
                "OplFbmd child uses wire0x{:02X}, expected 0x{BLOCK_TYPE_CONTAINER_88:02X}",
                child.block_type
            );
        }
        let RawContentsBlockBody::Container { content_source, .. } = child.body else {
            bail!("OplFbmd child is not a container");
        };

        let child_start =
            usize::try_from(content_source.offset).context("OplFbmd content start")?;
        let child_len = usize::try_from(content_source.len).context("OplFbmd content length")?;
        let mut child_cursor = ContentsCursor::bounded(
            content_source.stream.clone(),
            contents,
            child_start,
            child_len,
        )?;
        let blob = parse_confirmed_block(&mut child_cursor).context("parse OplFbmd.RgbMeta")?;
        if blob.id != 0x01 || blob.block_type != BLOCK_TYPE_BINARY {
            bail!(
                "OplFbmd payload uses field0x{:03X}/wire0x{:02X}, expected field0x001/wire0x80",
                blob.id,
                blob.block_type
            );
        }
        if child_cursor.remaining() != 0 {
            bail!("OplFbmd child contains bytes after the single RgbMeta blob");
        }

        let RawContentsBlockBody::Binary {
            declared_length,
            value_source,
            ..
        } = blob.body
        else {
            bail!("OplFbmd RgbMeta has non-binary body");
        };
        let payload = span_slice(contents, &value_source)?;
        let wmf = validate_wmf_metafile(payload);
        let (wmf_valid, wmf_version, wmf_declared_bytes, wmf_error) = match wmf {
            Ok(info) => (true, Some(info.version), Some(info.declared_bytes), None),
            Err(error) => (false, None, None, Some(error.to_string())),
        };

        rows.push(ResourceRow {
            slot: rows.len(),
            field_id: blob.id,
            declared_length,
            payload_len: payload.len(),
            source_offset: value_source.offset,
            sha256: sha256_hex(payload),
            wmf_valid,
            wmf_version,
            wmf_declared_bytes,
            wmf_error,
        });
    }
    Ok(rows)
}

fn parse_entry(contents: &[u8], entry: &PubBorderArtCatalogEntryV1) -> Result<EntryRow> {
    let child_start = usize::try_from(entry.source.offset).context("OplFb source start")?;
    let child_len = usize::try_from(entry.source.len).context("OplFb source length")?;
    let mut child_cursor = ContentsCursor::bounded(
        entry.source.stream.clone(),
        contents,
        child_start,
        child_len,
    )?;
    let child = parse_confirmed_block(&mut child_cursor).context("reparse OplFb child")?;
    if child.block_type != BLOCK_TYPE_CONTAINER_88 || child_cursor.remaining() != 0 {
        bail!("catalog entry source is not one exact OplFb wire0x88 child");
    }
    let RawContentsBlockBody::Container { content_source, .. } = child.body else {
        bail!("OplFb child is not a container");
    };

    let content_end = content_source
        .offset
        .checked_add(content_source.len)
        .context("OplFb content end overflow")?;
    let after_name = entry
        .name_source
        .offset
        .checked_add(entry.name_source.len)
        .context("OplFb name end overflow")?;
    if after_name > content_end {
        bail!("SzFBrdName source crosses OplFb content");
    }

    let start = usize::try_from(after_name).context("post-name start")?;
    let len = usize::try_from(content_end - after_name).context("post-name length")?;
    let mut cursor = ContentsCursor::bounded(content_source.stream.clone(), contents, start, len)?;

    let mut dzl_corner = None;
    let mut dxl_horiz = None;
    let mut dyl_vert = None;
    let mut cmeta = None;
    let mut metadata_source = None;
    let mut cfbmd = None;
    let mut resources_source = None;

    while cursor.remaining() > 0 {
        let block = parse_confirmed_block(&mut cursor).context("parse OplFb post-name field")?;
        if let Some(value) = exact_u32(&block, 0x04) {
            dzl_corner = Some(value);
        } else if let Some(value) = exact_u32(&block, 0x05) {
            dxl_horiz = Some(value);
        } else if let Some(value) = exact_u32(&block, 0x06) {
            dyl_vert = Some(value);
        } else if let Some(value) = exact_u32(&block, 0x07) {
            cmeta = Some(value);
        } else if let Some(source) = container_source(&block, 0x08, BLOCK_TYPE_CONTAINER_90) {
            metadata_source = Some(source);
        } else if let Some(value) = exact_u16(&block, 0x09) {
            cfbmd = Some(value);
        } else if let Some(source) = container_source(&block, 0x0A, BLOCK_TYPE_CONTAINER_A0) {
            resources_source = Some(source);
        }
    }

    let metadata_offsets = match metadata_source {
        Some(source) => parse_metadata_offsets(contents, &source)?,
        None => Vec::new(),
    };
    let resources = match resources_source {
        Some(source) => parse_resources(contents, &source)?,
        None => Vec::new(),
    };

    let all_required_scalars_present = dzl_corner.is_some()
        && dxl_horiz.is_some()
        && dyl_vert.is_some()
        && cmeta == Some(8)
        && cfbmd.is_some();
    let exact_eight_offsets = metadata_offsets.len() == 8;
    let exact_eight_resources = resources.len() == 8;
    let resource_count_matches_cfbmd = cfbmd.map(usize::from) == Some(resources.len());

    let mut resource_starts = Vec::with_capacity(resources.len());
    let mut next_start = 108u32;
    for resource in &resources {
        resource_starts.push(next_start);
        next_start = next_start
            .checked_add(
                u32::try_from(resource.payload_len)
                    .context("BorderArt resource size exceeds u32")?,
            )
            .context("BorderArt resource pool offset overflow")?;
    }
    let resource_start_to_index = resource_starts
        .iter()
        .enumerate()
        .map(|(index, offset)| (*offset, index))
        .collect::<BTreeMap<_, _>>();
    let slot_resource_indices = metadata_offsets
        .iter()
        .filter_map(|offset| resource_start_to_index.get(&u32::from(*offset)).copied())
        .collect::<Vec<_>>();
    let exact_eight_slot_refs = exact_eight_offsets && slot_resource_indices.len() == 8;
    let first_payload_offset_108 = metadata_offsets.first() == Some(&108);
    let all_slot_offsets_resolve_to_resource_pool = exact_eight_slot_refs;
    let all_resources_strict_wmf =
        !resources.is_empty() && resources.iter().all(|resource| resource.wmf_valid);

    Ok(EntryRow {
        ordinal: entry.ordinal,
        name: entry.name.clone(),
        dzl_corner,
        dxl_horiz,
        dyl_vert,
        cmeta,
        metadata_offsets,
        cfbmd,
        resources,
        all_required_scalars_present,
        exact_eight_offsets,
        exact_eight_resources,
        resource_count_matches_cfbmd,
        resource_starts,
        slot_resource_indices,
        exact_eight_slot_refs,
        first_payload_offset_108,
        all_slot_offsets_resolve_to_resource_pool,
        all_resources_strict_wmf,
        error: None,
    })
}

fn scan_file(path: &Path) -> FileRow {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_owned();
    let pub_bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return FileRow {
                source_sha256: String::new(),
                file_name,
                status: "error".into(),
                ifbmax: None,
                entry_count: 0,
                entries: Vec::new(),
                catalog_diagnostic_codes: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };
    let source_sha256 = sha256_hex(&pub_bytes);

    let contents = match pub_cfb::read_stream_reader(
        Cursor::new(pub_bytes.as_slice()),
        CONTENTS_STREAM_PATH,
    ) {
        Ok(bytes) => bytes,
        Err(error) => {
            return FileRow {
                source_sha256,
                file_name,
                status: "error".into(),
                ifbmax: None,
                entry_count: 0,
                entries: Vec::new(),
                catalog_diagnostic_codes: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };
    match detect_family(&contents) {
        Ok(ContentsFamily::Family0x2c) => {}
        Ok(_) => {
            return FileRow {
                source_sha256,
                file_name,
                status: "skipped_non_0x2c".into(),
                ifbmax: None,
                entry_count: 0,
                entries: Vec::new(),
                catalog_diagnostic_codes: Vec::new(),
                error: None,
            };
        }
        Err(error) => {
            return FileRow {
                source_sha256,
                file_name,
                status: "error".into(),
                ifbmax: None,
                entry_count: 0,
                entries: Vec::new(),
                catalog_diagnostic_codes: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    }

    let read = match read_mature_0x2c_borderart_catalog_v1(&contents) {
        Ok(read) => read,
        Err(error) => {
            return FileRow {
                source_sha256,
                file_name,
                status: "error".into(),
                ifbmax: None,
                entry_count: 0,
                entries: Vec::new(),
                catalog_diagnostic_codes: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };
    let diagnostic_codes = read
        .diagnostics
        .iter()
        .map(|row| row.code.clone())
        .collect::<Vec<_>>();
    let Some(catalog) = read.catalog else {
        return FileRow {
            source_sha256,
            file_name,
            status: "mature_no_usable_catalog".into(),
            ifbmax: None,
            entry_count: 0,
            entries: Vec::new(),
            catalog_diagnostic_codes: diagnostic_codes,
            error: None,
        };
    };

    let mut entries = Vec::new();
    for entry in &catalog.entries {
        match parse_entry(&contents, entry) {
            Ok(row) => entries.push(row),
            Err(error) => entries.push(EntryRow {
                ordinal: entry.ordinal,
                name: entry.name.clone(),
                dzl_corner: None,
                dxl_horiz: None,
                dyl_vert: None,
                cmeta: None,
                metadata_offsets: Vec::new(),
                cfbmd: None,
                resources: Vec::new(),
                all_required_scalars_present: false,
                exact_eight_offsets: false,
                exact_eight_resources: false,
                resource_count_matches_cfbmd: false,
                resource_starts: Vec::new(),
                slot_resource_indices: Vec::new(),
                exact_eight_slot_refs: false,
                first_payload_offset_108: false,
                all_slot_offsets_resolve_to_resource_pool: false,
                all_resources_strict_wmf: false,
                error: Some(format!("{error:#}")),
            }),
        }
    }

    FileRow {
        source_sha256,
        file_name,
        status: "mature_catalog_scanned".into(),
        ifbmax: Some(catalog.ifbmax),
        entry_count: entries.len(),
        entries,
        catalog_diagnostic_codes: diagnostic_codes,
        error: None,
    }
}

fn main() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        bail!("usage: borderart-wire80-census <corpus-dir> <out.json>");
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
        .filter(|row| row.status.starts_with("mature_"))
        .count();
    let catalog_rows = rows
        .iter()
        .filter(|row| row.status == "mature_catalog_scanned")
        .collect::<Vec<_>>();
    let catalog_entry_count = catalog_rows.iter().map(|row| row.entry_count).sum();
    let entries = catalog_rows
        .iter()
        .flat_map(|row| row.entries.iter())
        .collect::<Vec<_>>();
    let resources = entries
        .iter()
        .flat_map(|entry| entry.resources.iter())
        .collect::<Vec<_>>();

    let mut name_counts = BTreeMap::new();
    for entry in &entries {
        *name_counts.entry(entry.name.clone()).or_insert(0usize) += 1;
    }

    let mut cfbmd_value_counts = BTreeMap::new();
    for entry in &entries {
        if let Some(value) = entry.cfbmd {
            *cfbmd_value_counts.entry(value).or_insert(0usize) += 1;
        }
    }

    let receipt = CorpusReceipt {
        schema: "chaptera.borderart-wire80-census.v2",
        corpus_file_count: rows.len(),
        mature_0x2c_file_count,
        files_with_usable_catalog: catalog_rows.len(),
        catalog_entry_count,
        entries_with_exact_eight_resources: entries
            .iter()
            .filter(|entry| entry.exact_eight_resources)
            .count(),
        entries_with_resource_count_matching_cfbmd: entries
            .iter()
            .filter(|entry| entry.resource_count_matches_cfbmd)
            .count(),
        entries_with_eight_slot_resolution: entries
            .iter()
            .filter(|entry| entry.all_slot_offsets_resolve_to_resource_pool)
            .count(),
        binary_resource_count: resources.len(),
        strict_wmf_resource_count: resources.iter().filter(|row| row.wmf_valid).count(),
        invalid_wmf_resource_count: resources.iter().filter(|row| !row.wmf_valid).count(),
        cfbmd_value_counts,
        geometry_triplet_count: entries
            .iter()
            .filter(|entry| {
                entry.dzl_corner.is_some()
                    && entry.dxl_horiz.is_some()
                    && entry.dyl_vert.is_some()
            })
            .count(),
        name_counts,
        rows,
        evidence_boundary: "Exact mature-0x2C physical BorderArt census. Each OplFb has eight semantic metadata offsets (CMeta=8) but may deduplicate those slots onto a 1..8-member OplFbmd WMF resource pool (CFbmd). Wire0x80 is consumed only at the bounded OplFbmd.RgbMeta coordinate after exact raw0x46/Rgfb/OplFb/RgFbmd ancestry. Every admitted payload is checked by the existing strict WMF validator, and every semantic slot offset must resolve to an exact resource-pool start. This receipt does not infer authoring semantics, color/weight/stretch behavior, or custom-border ownership.",
    };

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
