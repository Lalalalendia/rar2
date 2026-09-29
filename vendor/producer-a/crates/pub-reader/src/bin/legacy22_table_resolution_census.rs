use anyhow::{Context, Result};
use pub_contents::{
    detect_family, parse_legacy_0x22_directory, parse_legacy_0x22_resolved_tables,
    parse_legacy_0x22_table_catalog, parse_preamble, ContentsFamily,
    Legacy0x22TableCatalogReadError, Legacy0x22TableTextReadError, LEGACY_0X22_TABLE_CHUNK_TYPE,
};
use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_reader::{build_legacy_0x22_noquill_source_graph, PubBridgeDiagnostic};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const CONTENTS_STREAM_PATH: &str = "/Contents";

#[derive(Default)]
struct ErrorAggregate {
    file_count: usize,
    raw_0001_count: usize,
    reader_target_file_count: usize,
    reader_target_residual_count: usize,
    detail_counts: BTreeMap<String, usize>,
    raw_0001_per_file_counts: BTreeMap<usize, usize>,
    reader_target_per_file_counts: BTreeMap<usize, usize>,
    revision_counts: BTreeMap<u16, usize>,
    raw_header_profile_counts: BTreeMap<String, usize>,
    materialized_object_profile_counts: BTreeMap<String, usize>,
    example_source_sha256: Vec<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

fn reader_raw_0001_target_count(bytes: &[u8]) -> usize {
    let Ok(build) =
        build_legacy_0x22_noquill_source_graph(Cursor::new(bytes), source_hash(bytes))
    else {
        return 0;
    };
    build
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic,
                PubBridgeDiagnostic::LegacyObjectNotMaterialized {
                    raw_type: Some(LEGACY_0X22_TABLE_CHUNK_TYPE),
                    reason,
                    ..
                } if reason == "legacy_table_not_resolved_v1"
            )
        })
        .count()
}

fn text_error_kind(error: &Legacy0x22TableTextReadError) -> &'static str {
    match error {
        Legacy0x22TableTextReadError::Formatting(_) => "text_formatting",
        Legacy0x22TableTextReadError::TextInfo(_) => "text_info",
        Legacy0x22TableTextReadError::TextInfoOwnerEndOutOfBounds { .. } => {
            "text_info_owner_end_out_of_bounds"
        }
        Legacy0x22TableTextReadError::DuplicateStyleBoundary { .. } => "duplicate_style_boundary",
        Legacy0x22TableTextReadError::CellSeparatorWithoutStyleBoundary { .. } => {
            "cell_separator_without_style_boundary"
        }
    }
}

fn table_error_kind(error: &Legacy0x22TableCatalogReadError) -> &'static str {
    match error {
        Legacy0x22TableCatalogReadError::Contents(_) => "contents",
        Legacy0x22TableCatalogReadError::UnexpectedFamily(_) => "unexpected_family",
        Legacy0x22TableCatalogReadError::TrailerPointerOutOfBounds { .. } => {
            "trailer_pointer_out_of_bounds"
        }
        Legacy0x22TableCatalogReadError::DirectoryOutOfBounds { .. } => "directory_out_of_bounds",
        Legacy0x22TableCatalogReadError::ChunkOffsetOutOfBounds { .. } => {
            "chunk_offset_out_of_bounds"
        }
        Legacy0x22TableCatalogReadError::TableHeaderTooShort { .. } => "table_header_too_short",
        Legacy0x22TableCatalogReadError::TableDataOffsetOutOfBounds { .. } => {
            "table_data_offset_out_of_bounds"
        }
        Legacy0x22TableCatalogReadError::UnsupportedExtendedListHeader { .. } => {
            "unsupported_extended_list_header"
        }
        Legacy0x22TableCatalogReadError::UnexpectedTableRecordSize { .. } => {
            "unexpected_table_record_size"
        }
        Legacy0x22TableCatalogReadError::TooFewTableRecords { .. } => "too_few_table_records",
        Legacy0x22TableCatalogReadError::TableRecordOutOfBounds { .. } => {
            "table_record_out_of_bounds"
        }
        Legacy0x22TableCatalogReadError::AxisNotMonotonic { .. } => "axis_not_monotonic",
        Legacy0x22TableCatalogReadError::Text(error) => text_error_kind(error),
        Legacy0x22TableCatalogReadError::TableCountMismatch { .. } => "table_count_mismatch",
        Legacy0x22TableCatalogReadError::DuplicateTextIdentity { .. } => "duplicate_text_identity",
        Legacy0x22TableCatalogReadError::DuplicateObjectTextIdentity { .. } => {
            "duplicate_object_text_identity"
        }
        Legacy0x22TableCatalogReadError::MissingTableTextIdentity { .. } => {
            "missing_table_text_identity"
        }
        Legacy0x22TableCatalogReadError::UnmatchedTableTextIdentities { .. } => {
            "unmatched_table_text_identities"
        }
        Legacy0x22TableCatalogReadError::TableCellCountMismatch { .. } => {
            "table_cell_count_mismatch"
        }
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn raw_header_profiles(
    contents: &[u8],
    directory: &pub_contents::Legacy0x22Directory,
) -> BTreeMap<String, usize> {
    let mut profiles = BTreeMap::new();
    for entry in directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == LEGACY_0X22_TABLE_CHUNK_TYPE)
    {
        let Ok(start) = usize::try_from(entry.chunk_source.offset) else {
            continue;
        };
        let Ok(len) = usize::try_from(entry.chunk_source.len) else {
            continue;
        };
        let Some(chunk) = contents.get(start..start.saturating_add(len)) else {
            continue;
        };
        let values = [44usize, 46, 48, 50, 52, 54, 56, 58, 60]
            .into_iter()
            .map(|offset| {
                read_u16(chunk, offset)
                    .map(|value| format!("{offset}:{value}"))
                    .unwrap_or_else(|| format!("{offset}:missing"))
            })
            .collect::<Vec<_>>()
            .join(",");
        let data_delta = chunk.get(3).copied().unwrap_or(0);
        let data_offset = usize::from(data_delta);
        let list_count = read_u16(chunk, data_offset)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "missing".into());
        let list_record_size = read_u16(chunk, data_offset + 4)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "missing".into());
        let key = format!(
            "len={len}:data={data_delta}:count={list_count}:record={list_record_size}:{values}"
        );
        *profiles.entry(key).or_default() += 1;
    }
    profiles
}

fn materialized_object_profiles(
    stream: StreamPath,
    contents: &[u8],
) -> BTreeMap<String, usize> {
    let mut profiles = BTreeMap::new();
    if let Ok(catalog) = parse_legacy_0x22_table_catalog(stream, contents) {
        for table in catalog.table_chunks {
            let key = format!(
                "materialized={}:cols={}:rows={}:local_text={}:u16_48={}:axis_count={}:record={}",
                table.is_materialized_grid(),
                table.column_count,
                table.row_count,
                table.local_text_index,
                table.observed_header_u16_at_48,
                table.list_header.count,
                table.list_header.record_size,
            );
            *profiles.entry(key).or_default() += 1;
        }
    }
    profiles
}

fn table_error_detail(error: &Legacy0x22TableCatalogReadError) -> Option<String> {
    match error {
        Legacy0x22TableCatalogReadError::TableCountMismatch {
            object_tables,
            text_tables,
        } => Some(format!("object_tables={object_tables}:text_tables={text_tables}")),
        Legacy0x22TableCatalogReadError::TooFewTableRecords {
            count, required, ..
        } => Some(format!("count={count}:required={required}")),
        _ => None,
    }
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(root)
        .with_context(|| format!("read corpus directory {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let corpus_root = PathBuf::from(
        args.next()
            .context("usage: legacy22_table_resolution_census CORPUS_DIR OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: legacy22_table_resolution_census CORPUS_DIR OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("legacy22_table_resolution_census accepts exactly CORPUS_DIR OUTPUT.json");
    }

    let paths = pub_paths(&corpus_root)?;
    let corpus_file_count = paths.len();
    let mut legacy_0x22_file_count = 0usize;
    let mut files_with_raw_0001 = 0usize;
    let mut raw_0001_physical_count = 0usize;
    let mut resolver_ok_files = 0usize;
    let mut resolver_ok_reader_target_files = 0usize;
    let mut resolver_ok_reader_target_residual_count = 0usize;
    let mut reader_target_files = 0usize;
    let mut reader_target_residual_count = 0usize;
    let mut resolved_table_count = 0usize;
    let mut placeholder_chunk_count = 0usize;
    let mut cfb_contents_unavailable = 0usize;
    let mut directory_unavailable = 0usize;
    let mut errors = BTreeMap::<String, ErrorAggregate>::new();

    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let source_sha256 = sha256_hex(&bytes);
        let Ok(contents) =
            pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), CONTENTS_STREAM_PATH)
        else {
            cfb_contents_unavailable += 1;
            continue;
        };
        if detect_family(&contents).ok() != Some(ContentsFamily::Family0x22) {
            continue;
        }
        legacy_0x22_file_count += 1;

        let Ok(directory) =
            parse_legacy_0x22_directory(StreamPath(CONTENTS_STREAM_PATH.into()), &contents)
        else {
            directory_unavailable += 1;
            continue;
        };
        let revision = parse_preamble(StreamPath(CONTENTS_STREAM_PATH.into()), &contents)
            .ok()
            .map(|preamble| preamble.serialization_revision);
        let raw_0001_count = directory
            .entries
            .iter()
            .filter(|entry| entry.chunk_type == LEGACY_0X22_TABLE_CHUNK_TYPE)
            .count();
        if raw_0001_count == 0 {
            continue;
        }

        files_with_raw_0001 += 1;
        raw_0001_physical_count += raw_0001_count;
        let reader_target_count = reader_raw_0001_target_count(&bytes);
        reader_target_residual_count += reader_target_count;
        if reader_target_count > 0 {
            reader_target_files += 1;
        }

        match parse_legacy_0x22_resolved_tables(
            StreamPath(CONTENTS_STREAM_PATH.into()),
            &contents,
        ) {
            Ok(catalog) => {
                resolver_ok_files += 1;
                if reader_target_count > 0 {
                    resolver_ok_reader_target_files += 1;
                    resolver_ok_reader_target_residual_count += reader_target_count;
                }
                resolved_table_count += catalog.tables.len();
                placeholder_chunk_count += catalog.placeholder_chunk_indices.len();
            }
            Err(error) => {
                let kind = table_error_kind(&error).to_owned();
                let aggregate = errors.entry(kind).or_default();
                aggregate.file_count += 1;
                aggregate.raw_0001_count += raw_0001_count;
                if reader_target_count > 0 {
                    aggregate.reader_target_file_count += 1;
                    aggregate.reader_target_residual_count += reader_target_count;
                }
                if let Some(revision) = revision {
                    *aggregate.revision_counts.entry(revision).or_default() += 1;
                }
                for (profile, count) in raw_header_profiles(&contents, &directory) {
                    *aggregate.raw_header_profile_counts.entry(profile).or_default() += count;
                }
                for (profile, count) in materialized_object_profiles(
                    StreamPath(CONTENTS_STREAM_PATH.into()),
                    &contents,
                ) {
                    *aggregate
                        .materialized_object_profile_counts
                        .entry(profile)
                        .or_default() += count;
                }
                *aggregate
                    .raw_0001_per_file_counts
                    .entry(raw_0001_count)
                    .or_default() += 1;
                *aggregate
                    .reader_target_per_file_counts
                    .entry(reader_target_count)
                    .or_default() += 1;
                if let Some(detail) = table_error_detail(&error) {
                    *aggregate.detail_counts.entry(detail).or_default() += 1;
                }
                if aggregate.example_source_sha256.len() < 5 {
                    aggregate.example_source_sha256.push(source_sha256);
                }
            }
        }
    }

    let error_file_count = errors.values().map(|entry| entry.file_count).sum::<usize>();
    let error_raw_0001_count = errors
        .values()
        .map(|entry| entry.raw_0001_count)
        .sum::<usize>();
    let error_classes = errors
        .into_iter()
        .map(|(kind, aggregate)| {
            json!({
                "kind": kind,
                "file_count": aggregate.file_count,
                "raw_0001_physical_count": aggregate.raw_0001_count,
                "reader_target_file_count": aggregate.reader_target_file_count,
                "reader_target_residual_count": aggregate.reader_target_residual_count,
                "detail_counts": aggregate.detail_counts,
                "raw_0001_per_file_counts": aggregate.raw_0001_per_file_counts,
                "reader_target_per_file_counts": aggregate.reader_target_per_file_counts,
                "revision_counts": aggregate.revision_counts,
                "raw_header_profile_counts": aggregate.raw_header_profile_counts,
                "materialized_object_profile_counts": aggregate.materialized_object_profile_counts,
                "example_source_sha256": aggregate.example_source_sha256,
            })
        })
        .collect::<Vec<_>>();

    let receipt = json!({
        "schema": "chaptera.legacy22-table-resolution-census.v1",
        "corpus_file_count": corpus_file_count,
        "legacy_0x22_file_count": legacy_0x22_file_count,
        "files_with_raw_0001": files_with_raw_0001,
        "raw_0001_physical_count": raw_0001_physical_count,
        "resolver_ok_files": resolver_ok_files,
        "resolver_ok_reader_target_files": resolver_ok_reader_target_files,
        "resolver_ok_reader_target_residual_count": resolver_ok_reader_target_residual_count,
        "reader_target_files": reader_target_files,
        "reader_target_residual_count": reader_target_residual_count,
        "resolved_table_count": resolved_table_count,
        "placeholder_chunk_count": placeholder_chunk_count,
        "resolver_error_files": error_file_count,
        "resolver_error_raw_0001_physical_count": error_raw_0001_count,
        "cfb_contents_unavailable": cfb_contents_unavailable,
        "directory_unavailable": directory_unavailable,
        "error_classes": error_classes,
        "source_safe": true,
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output.display()))?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
