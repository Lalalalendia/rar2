use anyhow::{bail, Context, Result};
use pub_core::StreamPath;
use pub_escher::{
    inspect_validated_delayed_blips_prefix, BlipKind, DelayedBlipPrefixGap,
    RejectedDelayedBlipDisposition,
};
use pub_reader::{
    build_reader_partial_escherdelay_evidence, build_reader_partial_source_graph,
    probe_reader_salvage_candidate,
    recovered_resource::{
        ReaderRecoveredResourcePlacementStatus, ReaderRecoveredResourcePreviewStatus,
    },
    ReaderPartialEscherDelayDiscoveryMode, ReaderPartialSourceFact, ReaderSalvageEligibility,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const INPUT_SCHEMA: &str = "chaptera.partial-escherdelay-cohort.v1";
const OUTPUT_SCHEMA: &str = "chaptera.partial-escherdelay-breadth-census.v2";
const ESCHER_DELAY_STREAM: &str = "/Escher/EscherDelayStm";
const EXPECTED_DENOMINATOR: usize = 45;
const EXPECTED_AUDIT_SHA256: &str =
    "f2538bbb53486eab2ff434bd75ef58fb112a3d27a8670bba927c37957364c43c";
const EXPECTED_RETAINED_ARCHIVE_SHA256: &str =
    "475d191c7ed2f02239c5448357bcb3787e934c449c6c360d8cb7f4fb0aabf57d";
const EXPECTED_SOURCE_AUDIT_SHA256: &str =
    "2c4d3f2825eac2dd0b346d5e243bf4726f556c7996d599ee8900992454c3865b";
const EXPECTED_EXPORTED_BUNDLE_MANIFEST_SHA256: &str =
    "f58a4fb5e9dd44f109d0ef18f458acb37be092b4540b1865d49c87d67f6b23f7";
const EXPECTED_GDI_DECODE_AUDIT_SHA256: &str =
    "45d84b5e627240b189a0cd505435eaca3083d127d764329b1115dc00be571603";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct HistoricalKindCounts {
    jpeg: usize,
    png: usize,
    emf: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CohortAuthority {
    audit_sha256: String,
    retained_archive_sha256: String,
    source_audit_sha256: String,
    exported_bundle_manifest_sha256: String,
    gdi_decode_audit_sha256: String,
    historical_positive_files: usize,
    historical_raster_positive_files: usize,
    historical_emf_only_positive_files: usize,
    historical_typed_blip_records: usize,
    historical_verified_records: usize,
    historical_unique_payload_sha256: usize,
    historical_kind_counts: HistoricalKindCounts,
}

#[derive(Debug, Deserialize)]
struct CohortManifest {
    schema: String,
    authority: CohortAuthority,
    sources: Vec<CohortSource>,
}

#[derive(Debug, Deserialize)]
struct CohortSource {
    source_sha256: String,
    #[serde(default)]
    expected_stream_sid: Option<u32>,
    #[serde(default)]
    expected_declared_len: Option<u64>,
    #[serde(default)]
    expected_available_prefix_len: Option<u64>,
    #[serde(default)]
    expected_prefix_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum CensusOutcome {
    ImageSalvagePositive,
    MetafileResourceOnlyPositive,
    StrictRasterZero,
    ProductEvidenceUnavailable,
    ParserTerminalBeforeImage,
    UnsupportedOnly,
    IneligibleProductProbe,
    SourceMissing,
}

#[derive(Debug, Serialize)]
struct CensusRow {
    source_sha256: String,
    source_copy_count: usize,
    reader_eligibility: Option<ReaderSalvageEligibility>,
    discovery_mode: Option<ReaderPartialEscherDelayDiscoveryMode>,
    logical_path_proven: Option<bool>,
    physical_context_gap_count: Option<usize>,
    raw_directory_rejected_active_entry_count: Option<usize>,
    stream_sid: Option<u32>,
    declared_len: Option<u64>,
    available_prefix_len: Option<u64>,
    prefix_sha256: Option<String>,
    physical_status: Option<pub_cfb::RootRegularStreamPrefixStatus>,
    truncation_reason: Option<pub_cfb::RootRegularStreamTruncationReason>,
    terminal_gap: Option<DelayedBlipPrefixGap>,
    scanned_record_count: u32,
    typed_blip_record_count: usize,
    strict_validated_resource_count: usize,
    strict_validated_raster_count: usize,
    strict_validated_metafile_count: usize,
    product_admitted_metafile_count: usize,
    previewable_metafile_count: usize,
    unowned_detached_metafile_count: usize,
    validated_but_product_unadmitted_count: usize,
    validated_kind_counts: BTreeMap<String, usize>,
    rejected_complete_blip_count: usize,
    rejected_disposition_counts: BTreeMap<String, usize>,
    rejected_kind_counts: BTreeMap<String, usize>,
    reader_admissible_image_count: usize,
    source_modified: Option<bool>,
    error_signature_sha256: Option<String>,
    outcome: CensusOutcome,
}

#[derive(Debug, Serialize)]
struct CensusSummary {
    schema: &'static str,
    input_manifest_sha256: String,
    authority: CohortAuthority,
    delta_image_positive_files_vs_historical: i64,
    delta_typed_blip_records_vs_historical: i64,
    delta_strict_validated_resources_vs_historical: i64,
    expected_denominator: usize,
    manifest_source_count: usize,
    located_source_count: usize,
    missing_source_count: usize,
    product_evidence_available_files: usize,
    logical_path_proven_files: usize,
    raw_carrier_files: usize,
    total_physical_context_gaps: usize,
    total_raw_directory_rejected_active_entries: usize,
    image_salvage_positive_files: usize,
    product_admitted_metafile_files: usize,
    metafile_only_product_positive_files: usize,
    total_scanned_records: u64,
    total_typed_blip_records: usize,
    total_reader_admissible_images: usize,
    total_strict_validated_resources: usize,
    total_strict_validated_rasters: usize,
    total_strict_validated_metafiles: usize,
    total_product_admitted_metafiles: usize,
    total_previewable_metafiles: usize,
    total_unowned_detached_metafiles: usize,
    total_validated_but_product_unadmitted: usize,
    validated_kind_counts: BTreeMap<String, usize>,
    rejected_disposition_counts: BTreeMap<String, usize>,
    rejected_kind_counts: BTreeMap<String, usize>,
    terminal_gap_counts: BTreeMap<String, usize>,
    outcome_counts: BTreeMap<String, usize>,
    rows: Vec<CensusRow>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_sha256(value: &str) -> Result<String> {
    let normalized = value.to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("manifest source_sha256 must be 64 hexadecimal characters");
    }
    Ok(normalized)
}

fn collect_pub_paths(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_pub_paths(&path, out)?;
            continue;
        }
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn outcome_name(value: &CensusOutcome) -> &'static str {
    match value {
        CensusOutcome::ImageSalvagePositive => "image_salvage_positive",
        CensusOutcome::MetafileResourceOnlyPositive => "metafile_resource_only_positive",
        CensusOutcome::StrictRasterZero => "strict_raster_zero",
        CensusOutcome::ProductEvidenceUnavailable => "product_evidence_unavailable",
        CensusOutcome::ParserTerminalBeforeImage => "parser_terminal_before_image",
        CensusOutcome::UnsupportedOnly => "unsupported_only",
        CensusOutcome::IneligibleProductProbe => "ineligible_product_probe",
        CensusOutcome::SourceMissing => "source_missing",
    }
}

fn is_reader_raster_kind(value: BlipKind) -> bool {
    matches!(
        value,
        BlipKind::Jpeg | BlipKind::Png | BlipKind::Gif | BlipKind::Dib | BlipKind::Tiff
    )
}

fn is_metafile_kind(value: BlipKind) -> bool {
    matches!(value, BlipKind::Emf | BlipKind::Wmf)
}

fn kind_name(value: BlipKind) -> &'static str {
    match value {
        BlipKind::Emf => "emf",
        BlipKind::Wmf => "wmf",
        BlipKind::Pict => "pict",
        BlipKind::Jpeg => "jpeg",
        BlipKind::Png => "png",
        BlipKind::Gif => "gif",
        BlipKind::Dib => "dib",
        BlipKind::Tiff => "tiff",
        BlipKind::Unknown => "unknown",
    }
}

fn disposition_name(value: RejectedDelayedBlipDisposition) -> &'static str {
    match value {
        RejectedDelayedBlipDisposition::UnsupportedMetafile => "unsupported_metafile",
        RejectedDelayedBlipDisposition::UnsupportedPicture => "unsupported_picture",
        RejectedDelayedBlipDisposition::UnsupportedBlipType => "unsupported_blip_type",
        RejectedDelayedBlipDisposition::StrictValidationFailed => "strict_validation_failed",
    }
}

fn terminal_gap_name(value: &DelayedBlipPrefixGap) -> &'static str {
    match value {
        DelayedBlipPrefixGap::TruncatedHeader { .. } => "truncated_header",
        DelayedBlipPrefixGap::DeclaredRecordOutOfBounds { .. } => "declared_record_out_of_bounds",
        DelayedBlipPrefixGap::RecordParseFailure { .. } => "record_parse_failure",
        DelayedBlipPrefixGap::NonAdvancingRecord { .. } => "non_advancing_record",
    }
}

fn verify_source_unchanged(path: &Path, source_sha256: &str) -> Result<()> {
    let post =
        fs::read(path).with_context(|| format!("re-read admitted source {source_sha256}"))?;
    if sha256_hex(&post) != source_sha256 {
        bail!("source modified during census: {source_sha256}");
    }
    Ok(())
}

fn blank_row(source_sha256: String, source_copy_count: usize) -> CensusRow {
    CensusRow {
        source_sha256,
        source_copy_count,
        reader_eligibility: None,
        discovery_mode: None,
        logical_path_proven: None,
        physical_context_gap_count: None,
        raw_directory_rejected_active_entry_count: None,
        stream_sid: None,
        declared_len: None,
        available_prefix_len: None,
        prefix_sha256: None,
        physical_status: None,
        truncation_reason: None,
        terminal_gap: None,
        scanned_record_count: 0,
        typed_blip_record_count: 0,
        strict_validated_resource_count: 0,
        strict_validated_raster_count: 0,
        strict_validated_metafile_count: 0,
        product_admitted_metafile_count: 0,
        previewable_metafile_count: 0,
        unowned_detached_metafile_count: 0,
        validated_but_product_unadmitted_count: 0,
        validated_kind_counts: BTreeMap::new(),
        rejected_complete_blip_count: 0,
        rejected_disposition_counts: BTreeMap::new(),
        rejected_kind_counts: BTreeMap::new(),
        reader_admissible_image_count: 0,
        source_modified: None,
        error_signature_sha256: None,
        outcome: CensusOutcome::SourceMissing,
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(args.next().context(
        "usage: partial-escherdelay-breadth-census CORPUS_DIR MANIFEST.json OUTPUT.json",
    )?);
    let manifest_path = PathBuf::from(args.next().context(
        "usage: partial-escherdelay-breadth-census CORPUS_DIR MANIFEST.json OUTPUT.json",
    )?);
    let output = PathBuf::from(args.next().context(
        "usage: partial-escherdelay-breadth-census CORPUS_DIR MANIFEST.json OUTPUT.json",
    )?);
    if args.next().is_some() {
        bail!("partial-escherdelay-breadth-census accepts CORPUS_DIR MANIFEST.json OUTPUT.json");
    }

    let manifest_bytes =
        fs::read(&manifest_path).with_context(|| format!("read {}", manifest_path.display()))?;
    let input_manifest_sha256 = sha256_hex(&manifest_bytes);
    let manifest: CohortManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    if manifest.schema != INPUT_SCHEMA {
        bail!(
            "unexpected manifest schema {:?}; expected {:?}",
            manifest.schema,
            INPUT_SCHEMA
        );
    }
    if manifest.sources.len() != EXPECTED_DENOMINATOR {
        bail!(
            "exact retained denominator is required: expected {}, manifest has {}",
            EXPECTED_DENOMINATOR,
            manifest.sources.len()
        );
    }

    let mut authority = manifest.authority;
    authority.audit_sha256 = validate_sha256(&authority.audit_sha256)?;
    authority.retained_archive_sha256 = validate_sha256(&authority.retained_archive_sha256)?;
    authority.source_audit_sha256 = validate_sha256(&authority.source_audit_sha256)?;
    authority.exported_bundle_manifest_sha256 =
        validate_sha256(&authority.exported_bundle_manifest_sha256)?;
    authority.gdi_decode_audit_sha256 = validate_sha256(&authority.gdi_decode_audit_sha256)?;

    if authority.audit_sha256 != EXPECTED_AUDIT_SHA256
        || authority.retained_archive_sha256 != EXPECTED_RETAINED_ARCHIVE_SHA256
        || authority.source_audit_sha256 != EXPECTED_SOURCE_AUDIT_SHA256
        || authority.exported_bundle_manifest_sha256 != EXPECTED_EXPORTED_BUNDLE_MANIFEST_SHA256
        || authority.gdi_decode_audit_sha256 != EXPECTED_GDI_DECODE_AUDIT_SHA256
    {
        bail!("cohort provenance does not match the retained exact-audit authority");
    }
    if authority.historical_positive_files != 41
        || authority.historical_raster_positive_files != 41
        || authority.historical_emf_only_positive_files != 0
        || authority.historical_typed_blip_records != 209
        || authority.historical_verified_records != 188
        || authority.historical_unique_payload_sha256 != 112
        || authority.historical_kind_counts.jpeg != 154
        || authority.historical_kind_counts.png != 24
        || authority.historical_kind_counts.emf != 10
        || authority.historical_kind_counts.jpeg
            + authority.historical_kind_counts.png
            + authority.historical_kind_counts.emf
            != authority.historical_verified_records
    {
        bail!("cohort historical comparator metadata does not match the retained audit");
    }

    let mut expected = BTreeMap::<String, CohortSource>::new();
    for mut source in manifest.sources {
        let sha = validate_sha256(&source.source_sha256)?;
        source.source_sha256 = sha.clone();
        if let Some(prefix) = source.expected_prefix_sha256.as_deref() {
            source.expected_prefix_sha256 = Some(validate_sha256(prefix)?);
        }
        if expected.insert(sha.clone(), source).is_some() {
            bail!("duplicate source_sha256 in manifest: {sha}");
        }
    }

    let mut paths = Vec::new();
    collect_pub_paths(&root, &mut paths)?;
    paths.sort();

    let mut located = BTreeMap::<String, Vec<PathBuf>>::new();
    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let sha = sha256_hex(&bytes);
        if expected.contains_key(&sha) {
            located.entry(sha).or_default().push(path);
        }
    }

    let mut rows = Vec::with_capacity(EXPECTED_DENOMINATOR);
    for (source_sha256, expected_source) in expected {
        let Some(copies) = located.get(&source_sha256) else {
            rows.push(blank_row(source_sha256, 0));
            continue;
        };

        let path = &copies[0];
        let bytes =
            fs::read(path).with_context(|| format!("read admitted source {}", source_sha256))?;
        if sha256_hex(&bytes) != source_sha256 {
            bail!("source changed between scan and census: {source_sha256}");
        }

        let mut row = blank_row(source_sha256.clone(), copies.len());
        let probe = probe_reader_salvage_candidate(&bytes);
        row.reader_eligibility = Some(probe.eligibility);
        if !probe.eligibility.is_eligible() {
            row.outcome = CensusOutcome::IneligibleProductProbe;
            verify_source_unchanged(path, &source_sha256)?;
            row.source_modified = Some(false);
            rows.push(row);
            continue;
        }

        let evidence = match build_reader_partial_escherdelay_evidence(&bytes, &probe) {
            Some(value) => value,
            None => {
                row.error_signature_sha256 = Some(sha256_hex(
                    b"reader_partial_escherdelay_evidence_unavailable",
                ));
                row.outcome = CensusOutcome::ProductEvidenceUnavailable;
                verify_source_unchanged(path, &source_sha256)?;
                row.source_modified = Some(false);
                rows.push(row);
                continue;
            }
        };
        if evidence.source_sha256 != source_sha256 || evidence.logical_path != ESCHER_DELAY_STREAM {
            bail!("Reader product evidence identity mismatch for {source_sha256}");
        }
        if expected_source
            .expected_stream_sid
            .is_some_and(|expected_sid| expected_sid != evidence.stream_sid)
        {
            bail!("historical stream SID mismatch for {source_sha256}");
        }
        if expected_source
            .expected_declared_len
            .is_some_and(|expected_len| expected_len != evidence.declared_len)
        {
            bail!("historical declared length mismatch for {source_sha256}");
        }

        row.discovery_mode = Some(evidence.discovery_mode);
        row.logical_path_proven = Some(evidence.logical_path_proven);
        row.physical_context_gap_count = Some(evidence.physical_context_gap_count);
        row.raw_directory_rejected_active_entry_count =
            Some(evidence.raw_directory_rejected_active_entry_count);
        row.stream_sid = Some(evidence.stream_sid);
        row.declared_len = Some(evidence.declared_len);
        row.available_prefix_len = Some(evidence.available_prefix_len);
        row.prefix_sha256 = Some(evidence.prefix_sha256.clone());
        row.physical_status = Some(evidence.physical_stream_status);
        row.truncation_reason = evidence.truncation_reason;

        let recovered = match evidence.discovery_mode {
            ReaderPartialEscherDelayDiscoveryMode::LogicalPath => {
                pub_cfb::recover_regular_stream_prefix_by_sid_reader_with_expected_sha(
                    Cursor::new(&bytes),
                    evidence.stream_sid,
                    &source_sha256,
                )
                .with_context(|| {
                    format!(
                        "re-recover strict product-selected EscherDelay SID for {source_sha256}"
                    )
                })?
            }
            ReaderPartialEscherDelayDiscoveryMode::UniqueRawCarrierNames => {
                pub_cfb::recover_truncated_regular_stream_prefix_by_sid_reader_with_expected_sha(
                    Cursor::new(&bytes),
                    evidence.stream_sid,
                    &source_sha256,
                )
                .with_context(|| {
                    format!("re-recover raw product-selected EscherDelay SID for {source_sha256}")
                })?
            }
        };
        if recovered.source_modified
            || recovered.stream_sid != evidence.stream_sid
            || recovered.declared_len != evidence.declared_len
            || recovered.available_prefix_len != evidence.available_prefix_len
            || recovered.prefix_sha256 != evidence.prefix_sha256
            || recovered.status != evidence.physical_stream_status
            || recovered.truncation_reason != evidence.truncation_reason
        {
            bail!("Reader product evidence / census re-recovery mismatch for {source_sha256}");
        }
        if expected_source
            .expected_available_prefix_len
            .is_some_and(|expected_len| expected_len != evidence.available_prefix_len)
        {
            bail!("historical available prefix length mismatch for {source_sha256}");
        }
        if expected_source
            .expected_prefix_sha256
            .as_deref()
            .is_some_and(|expected_sha| expected_sha != evidence.prefix_sha256)
        {
            bail!("historical prefix SHA mismatch for {source_sha256}");
        }

        let inventory = inspect_validated_delayed_blips_prefix(
            StreamPath(ESCHER_DELAY_STREAM.into()),
            &recovered.bytes,
        );
        if inventory.available_prefix_len != recovered.available_prefix_len {
            bail!("prefix inventory length mismatch for {source_sha256}");
        }
        row.scanned_record_count = inventory.scanned_record_count;
        row.strict_validated_resource_count = inventory.records.len();
        row.strict_validated_raster_count = inventory
            .records
            .iter()
            .filter(|validated| is_reader_raster_kind(validated.kind))
            .count();
        row.strict_validated_metafile_count = inventory
            .records
            .iter()
            .filter(|validated| is_metafile_kind(validated.kind))
            .count();
        row.terminal_gap = inventory.terminal_gap.clone();

        for validated in &inventory.records {
            *row.validated_kind_counts
                .entry(kind_name(validated.kind).to_owned())
                .or_default() += 1;
        }
        row.rejected_complete_blip_count = inventory.rejected_complete_blips.len();
        row.typed_blip_record_count =
            row.strict_validated_resource_count + row.rejected_complete_blip_count;
        for rejected in &inventory.rejected_complete_blips {
            *row.rejected_disposition_counts
                .entry(disposition_name(rejected.disposition).to_owned())
                .or_default() += 1;
            *row.rejected_kind_counts
                .entry(kind_name(rejected.kind).to_owned())
                .or_default() += 1;
        }

        let graph = build_reader_partial_source_graph(&bytes, &probe).ok();
        row.reader_admissible_image_count = graph.as_ref().map_or(0, |value| {
            value
                .facts
                .iter()
                .filter(|fact| matches!(fact, ReaderPartialSourceFact::VerifiedImage { .. }))
                .count()
        });
        if graph.as_ref().is_some_and(|value| {
            value
                .recovered_resources
                .iter()
                .any(|resource| !is_metafile_kind(resource.kind))
        }) {
            bail!(
                "unexpected non-metafile recovered resource in EscherDelay V1 census for {}",
                source_sha256
            );
        }
        row.product_admitted_metafile_count = graph
            .as_ref()
            .map_or(0, |value| value.recovered_resources.len());
        row.previewable_metafile_count = graph.as_ref().map_or(0, |value| {
            value
                .recovered_resources
                .iter()
                .filter(|resource| {
                    !matches!(
                        resource.preview_status,
                        ReaderRecoveredResourcePreviewStatus::NotProven
                    )
                })
                .count()
        });
        row.unowned_detached_metafile_count = graph.as_ref().map_or(0, |value| {
            value
                .recovered_resources
                .iter()
                .filter(|resource| {
                    matches!(
                        resource.placement_status,
                        ReaderRecoveredResourcePlacementStatus::DetachedOwnershipNotProven
                    )
                })
                .count()
        });

        if row.reader_admissible_image_count != row.strict_validated_raster_count {
            bail!(
                "strict raster parser/product projection disagreement for {}: {} vs {}",
                source_sha256,
                row.strict_validated_raster_count,
                row.reader_admissible_image_count
            );
        }
        if row.product_admitted_metafile_count != row.strict_validated_metafile_count {
            bail!(
                "strict metafile parser/product projection disagreement for {}: {} vs {}",
                source_sha256,
                row.strict_validated_metafile_count,
                row.product_admitted_metafile_count
            );
        }
        row.validated_but_product_unadmitted_count = row
            .strict_validated_resource_count
            .checked_sub(
                row.reader_admissible_image_count
                    .checked_add(row.product_admitted_metafile_count)
                    .context("product-admitted resource count overflow")?,
            )
            .context("product admitted more resources than strict validation produced")?;

        let rejected_only_unsupported = row.rejected_disposition_counts.keys().all(|key| {
            matches!(
                key.as_str(),
                "unsupported_metafile" | "unsupported_picture" | "unsupported_blip_type"
            )
        });
        let has_unsupported_only_evidence =
            row.strict_validated_metafile_count > 0 || !row.rejected_disposition_counts.is_empty();

        row.outcome = if row.reader_admissible_image_count > 0 {
            CensusOutcome::ImageSalvagePositive
        } else if row.product_admitted_metafile_count > 0 {
            CensusOutcome::MetafileResourceOnlyPositive
        } else if has_unsupported_only_evidence && rejected_only_unsupported {
            CensusOutcome::UnsupportedOnly
        } else if row.terminal_gap.is_some() && row.scanned_record_count == 0 {
            CensusOutcome::ParserTerminalBeforeImage
        } else {
            CensusOutcome::StrictRasterZero
        };

        verify_source_unchanged(path, &source_sha256)?;
        row.source_modified = Some(false);

        rows.push(row);
    }

    rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));

    let mut validated_kind_counts = BTreeMap::<String, usize>::new();
    let mut rejected_disposition_counts = BTreeMap::<String, usize>::new();
    let mut rejected_kind_counts = BTreeMap::<String, usize>::new();
    let mut terminal_gap_counts = BTreeMap::<String, usize>::new();
    let mut outcome_counts = BTreeMap::<String, usize>::new();
    let mut total_scanned_records = 0u64;
    let mut total_typed_blip_records = 0usize;
    let mut total_reader_admissible_images = 0usize;
    let mut total_strict_validated_resources = 0usize;
    let mut total_strict_validated_rasters = 0usize;
    let mut total_strict_validated_metafiles = 0usize;
    let mut total_product_admitted_metafiles = 0usize;
    let mut total_previewable_metafiles = 0usize;
    let mut total_unowned_detached_metafiles = 0usize;
    let mut total_validated_but_product_unadmitted = 0usize;

    for row in &rows {
        for (kind, count) in &row.validated_kind_counts {
            *validated_kind_counts.entry(kind.clone()).or_default() += count;
        }
        for (disposition, count) in &row.rejected_disposition_counts {
            *rejected_disposition_counts
                .entry(disposition.clone())
                .or_default() += count;
        }
        for (kind, count) in &row.rejected_kind_counts {
            *rejected_kind_counts.entry(kind.clone()).or_default() += count;
        }
        if let Some(gap) = &row.terminal_gap {
            *terminal_gap_counts
                .entry(terminal_gap_name(gap).to_owned())
                .or_default() += 1;
        }
        *outcome_counts
            .entry(outcome_name(&row.outcome).to_owned())
            .or_default() += 1;
        total_scanned_records += u64::from(row.scanned_record_count);
        total_typed_blip_records += row.typed_blip_record_count;
        total_reader_admissible_images += row.reader_admissible_image_count;
        total_strict_validated_resources += row.strict_validated_resource_count;
        total_strict_validated_rasters += row.strict_validated_raster_count;
        total_strict_validated_metafiles += row.strict_validated_metafile_count;
        total_product_admitted_metafiles += row.product_admitted_metafile_count;
        total_previewable_metafiles += row.previewable_metafile_count;
        total_unowned_detached_metafiles += row.unowned_detached_metafile_count;
        total_validated_but_product_unadmitted += row.validated_but_product_unadmitted_count;
    }

    let located_source_count = rows
        .iter()
        .filter(|row| !matches!(row.outcome, CensusOutcome::SourceMissing))
        .count();
    let missing_source_count = EXPECTED_DENOMINATOR - located_source_count;
    let product_evidence_available_files = rows
        .iter()
        .filter(|row| row.discovery_mode.is_some())
        .count();
    let logical_path_proven_files = rows
        .iter()
        .filter(|row| row.logical_path_proven == Some(true))
        .count();
    let raw_carrier_files = rows
        .iter()
        .filter(|row| {
            matches!(
                row.discovery_mode,
                Some(ReaderPartialEscherDelayDiscoveryMode::UniqueRawCarrierNames)
            )
        })
        .count();
    let total_physical_context_gaps = rows
        .iter()
        .map(|row| row.physical_context_gap_count.unwrap_or(0))
        .sum();
    let total_raw_directory_rejected_active_entries = rows
        .iter()
        .map(|row| row.raw_directory_rejected_active_entry_count.unwrap_or(0))
        .sum();
    let image_salvage_positive_files = rows
        .iter()
        .filter(|row| matches!(row.outcome, CensusOutcome::ImageSalvagePositive))
        .count();
    let product_admitted_metafile_files = rows
        .iter()
        .filter(|row| row.product_admitted_metafile_count > 0)
        .count();
    let metafile_only_product_positive_files = rows
        .iter()
        .filter(|row| matches!(row.outcome, CensusOutcome::MetafileResourceOnlyPositive))
        .count();

    let summary = CensusSummary {
        schema: OUTPUT_SCHEMA,
        input_manifest_sha256,
        delta_image_positive_files_vs_historical: image_salvage_positive_files as i64
            - authority.historical_raster_positive_files as i64,
        delta_typed_blip_records_vs_historical: total_typed_blip_records as i64
            - authority.historical_typed_blip_records as i64,
        delta_strict_validated_resources_vs_historical: total_strict_validated_resources as i64
            - authority.historical_verified_records as i64,
        authority,
        expected_denominator: EXPECTED_DENOMINATOR,
        manifest_source_count: EXPECTED_DENOMINATOR,
        located_source_count,
        missing_source_count,
        product_evidence_available_files,
        logical_path_proven_files,
        raw_carrier_files,
        total_physical_context_gaps,
        total_raw_directory_rejected_active_entries,
        image_salvage_positive_files,
        product_admitted_metafile_files,
        metafile_only_product_positive_files,
        total_scanned_records,
        total_typed_blip_records,
        total_reader_admissible_images,
        total_strict_validated_resources,
        total_strict_validated_rasters,
        total_strict_validated_metafiles,
        total_product_admitted_metafiles,
        total_previewable_metafiles,
        total_unowned_detached_metafiles,
        total_validated_but_product_unadmitted,
        validated_kind_counts,
        rejected_disposition_counts,
        rejected_kind_counts,
        terminal_gap_counts,
        outcome_counts,
        rows,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&summary).context("serialize EscherDelay breadth census")?,
    )
    .with_context(|| format!("write {}", output.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn bundled_cohort_manifest_is_exact_45_source_authority() {
        let manifest: CohortManifest = serde_json::from_str(include_str!(
            "../../data/partial-escherdelay-cohort-v1.json"
        ))
        .expect("bundled partial EscherDelay cohort manifest");

        assert_eq!(manifest.schema, INPUT_SCHEMA);
        assert_eq!(manifest.sources.len(), EXPECTED_DENOMINATOR);
        assert_eq!(manifest.authority.audit_sha256, EXPECTED_AUDIT_SHA256);
        assert_eq!(
            manifest.authority.retained_archive_sha256,
            EXPECTED_RETAINED_ARCHIVE_SHA256
        );
        assert_eq!(
            manifest.authority.source_audit_sha256,
            EXPECTED_SOURCE_AUDIT_SHA256
        );
        assert_eq!(
            manifest.authority.exported_bundle_manifest_sha256,
            EXPECTED_EXPORTED_BUNDLE_MANIFEST_SHA256
        );
        assert_eq!(
            manifest.authority.gdi_decode_audit_sha256,
            EXPECTED_GDI_DECODE_AUDIT_SHA256
        );
        assert_eq!(manifest.authority.historical_raster_positive_files, 41);
        assert_eq!(manifest.authority.historical_typed_blip_records, 209);
        assert_eq!(manifest.authority.historical_verified_records, 188);

        let mut unique_sources = BTreeSet::new();
        for source in manifest.sources {
            let source_sha = validate_sha256(&source.source_sha256).expect("source SHA-256");
            assert!(unique_sources.insert(source_sha));
            assert!(source.expected_declared_len.is_some_and(|value| value > 0));
            assert!(source
                .expected_available_prefix_len
                .is_some_and(|value| value > 0));
            assert!(source
                .expected_available_prefix_len
                .zip(source.expected_declared_len)
                .is_some_and(|(available, declared)| available < declared));
            validate_sha256(
                source
                    .expected_prefix_sha256
                    .as_deref()
                    .expect("prefix SHA-256"),
            )
            .expect("valid prefix SHA-256");
        }

        assert_eq!(unique_sources.len(), EXPECTED_DENOMINATOR);
    }
}
