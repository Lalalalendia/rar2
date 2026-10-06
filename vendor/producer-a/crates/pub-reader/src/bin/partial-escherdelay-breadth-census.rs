use anyhow::{bail, Context, Result};
use pub_core::StreamPath;
use pub_escher::{
    inspect_validated_delayed_blips_prefix, BlipKind, DelayedBlipPrefixGap,
    RejectedDelayedBlipDisposition,
};
use pub_reader::{
    build_reader_partial_escherdelay_evidence, probe_reader_salvage_candidate,
    ReaderSalvageEligibility,
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
const OUTPUT_SCHEMA: &str = "chaptera.partial-escherdelay-breadth-census.v1";
const ESCHER_DELAY_STREAM: &str = "/Escher/EscherDelayStm";
const EXPECTED_DENOMINATOR: usize = 45;

#[derive(Debug, Deserialize)]
struct CohortManifest {
    schema: String,
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
    StrictRasterZero,
    PhysicalDiscoveryFail,
    PhysicalPrefixFail,
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
    stream_sid: Option<u32>,
    declared_len: Option<u64>,
    available_prefix_len: Option<u64>,
    prefix_sha256: Option<String>,
    physical_status: Option<String>,
    truncation_reason: Option<String>,
    terminal_gap: Option<DelayedBlipPrefixGap>,
    scanned_record_count: u32,
    strict_validated_image_count: usize,
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
    expected_denominator: usize,
    manifest_source_count: usize,
    located_source_count: usize,
    missing_source_count: usize,
    image_salvage_positive_files: usize,
    total_reader_admissible_images: usize,
    validated_kind_counts: BTreeMap<String, usize>,
    rejected_disposition_counts: BTreeMap<String, usize>,
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

fn error_signature(error: &impl std::fmt::Display) -> String {
    sha256_hex(error.to_string().as_bytes())
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

fn eligibility_name(value: ReaderSalvageEligibility) -> &'static str {
    use ReaderSalvageEligibility::*;
    match value {
        EligibleDamagedPublisher => "eligible_damaged_publisher",
        EligibleKnownPublisherCorruption => "eligible_known_publisher_corruption",
        AwaitingTypedCorruptionEvidence => "awaiting_typed_corruption_evidence",
        IneligibleUnproven => "ineligible_unproven",
        IneligibleArchive => "ineligible_archive",
        IneligibleForeign => "ineligible_foreign",
        IneligibleSuspicious => "ineligible_suspicious",
        IneligibleResourceLimit => "ineligible_resource_limit",
    }
}

fn outcome_name(value: &CensusOutcome) -> &'static str {
    match value {
        CensusOutcome::ImageSalvagePositive => "image_salvage_positive",
        CensusOutcome::StrictRasterZero => "strict_raster_zero",
        CensusOutcome::PhysicalDiscoveryFail => "physical_discovery_fail",
        CensusOutcome::PhysicalPrefixFail => "physical_prefix_fail",
        CensusOutcome::ParserTerminalBeforeImage => "parser_terminal_before_image",
        CensusOutcome::UnsupportedOnly => "unsupported_only",
        CensusOutcome::IneligibleProductProbe => "ineligible_product_probe",
        CensusOutcome::SourceMissing => "source_missing",
    }
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
        DelayedBlipPrefixGap::DeclaredRecordOutOfBounds { .. } => {
            "declared_record_out_of_bounds"
        }
        DelayedBlipPrefixGap::RecordParseFailure { .. } => "record_parse_failure",
        DelayedBlipPrefixGap::NonAdvancingRecord { .. } => "non_advancing_record",
    }
}

fn blank_row(source_sha256: String, source_copy_count: usize) -> CensusRow {
    CensusRow {
        source_sha256,
        source_copy_count,
        reader_eligibility: None,
        stream_sid: None,
        declared_len: None,
        available_prefix_len: None,
        prefix_sha256: None,
        physical_status: None,
        truncation_reason: None,
        terminal_gap: None,
        scanned_record_count: 0,
        strict_validated_image_count: 0,
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
    let root = PathBuf::from(
        args.next()
            .context("usage: partial-escherdelay-breadth-census CORPUS_DIR MANIFEST.json OUTPUT.json")?,
    );
    let manifest_path = PathBuf::from(
        args.next()
            .context("usage: partial-escherdelay-breadth-census CORPUS_DIR MANIFEST.json OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: partial-escherdelay-breadth-census CORPUS_DIR MANIFEST.json OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!(
            "partial-escherdelay-breadth-census accepts CORPUS_DIR MANIFEST.json OUTPUT.json"
        );
    }

    let manifest: CohortManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?,
    )
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
        let bytes = fs::read(path).with_context(|| format!("read admitted source {}", source_sha256))?;
        if sha256_hex(&bytes) != source_sha256 {
            bail!("source changed between scan and census: {source_sha256}");
        }

        let mut row = blank_row(source_sha256.clone(), copies.len());
        let probe = probe_reader_salvage_candidate(&bytes);
        row.reader_eligibility = Some(probe.eligibility);
        if !probe.eligibility.is_eligible() {
            row.outcome = CensusOutcome::IneligibleProductProbe;
            let post = fs::read(path)
                .with_context(|| format!("re-read admitted source {}", source_sha256))?;
            row.source_modified = Some(sha256_hex(&post) != source_sha256);
            rows.push(row);
            continue;
        }

        let discovered = match pub_cfb::discover_regular_stream_sid_reader(
            Cursor::new(&bytes),
            ESCHER_DELAY_STREAM,
        ) {
            Ok(value) => value,
            Err(error) => {
                row.error_signature_sha256 = Some(error_signature(&error));
                row.outcome = CensusOutcome::PhysicalDiscoveryFail;
                let post = fs::read(path)
                    .with_context(|| format!("re-read admitted source {}", source_sha256))?;
                row.source_modified = Some(sha256_hex(&post) != source_sha256);
                rows.push(row);
                continue;
            }
        };
        if discovered.source_sha256 != source_sha256
            || discovered.logical_path != ESCHER_DELAY_STREAM
        {
            bail!("physical discovery identity mismatch for {source_sha256}");
        }
        if expected_source
            .expected_stream_sid
            .is_some_and(|expected_sid| expected_sid != discovered.stream_sid)
        {
            bail!("historical stream SID mismatch for {source_sha256}");
        }
        if expected_source
            .expected_declared_len
            .is_some_and(|expected_len| expected_len != discovered.declared_len)
        {
            bail!("historical declared length mismatch for {source_sha256}");
        }
        row.stream_sid = Some(discovered.stream_sid);
        row.declared_len = Some(discovered.declared_len);

        let recovered = match pub_cfb::recover_regular_stream_prefix_by_sid_reader_with_expected_sha(
            Cursor::new(&bytes),
            discovered.stream_sid,
            &source_sha256,
        ) {
            Ok(value) => value,
            Err(error) => {
                row.error_signature_sha256 = Some(error_signature(&error));
                row.outcome = CensusOutcome::PhysicalPrefixFail;
                let post = fs::read(path)
                    .with_context(|| format!("re-read admitted source {}", source_sha256))?;
                row.source_modified = Some(sha256_hex(&post) != source_sha256);
                rows.push(row);
                continue;
            }
        };
        if expected_source
            .expected_available_prefix_len
            .is_some_and(|expected_len| expected_len != recovered.available_prefix_len)
        {
            bail!("historical available prefix length mismatch for {source_sha256}");
        }
        if expected_source
            .expected_prefix_sha256
            .as_deref()
            .is_some_and(|expected_sha| expected_sha != recovered.prefix_sha256)
        {
            bail!("historical prefix SHA mismatch for {source_sha256}");
        }
        row.available_prefix_len = Some(recovered.available_prefix_len);
        row.prefix_sha256 = Some(recovered.prefix_sha256.clone());
        row.physical_status = Some(format!("{:?}", recovered.status).to_ascii_lowercase());
        row.truncation_reason = recovered
            .truncation_reason
            .map(|value| format!("{value:?}").to_ascii_lowercase());

        let inventory = inspect_validated_delayed_blips_prefix(
            StreamPath(ESCHER_DELAY_STREAM.into()),
            &recovered.bytes,
        );
        if inventory.available_prefix_len != recovered.available_prefix_len {
            bail!("prefix inventory length mismatch for {source_sha256}");
        }
        row.scanned_record_count = inventory.scanned_record_count;
        row.strict_validated_image_count = inventory.records.len();
        row.terminal_gap = inventory.terminal_gap.clone();

        for validated in &inventory.records {
            *row.validated_kind_counts
                .entry(kind_name(validated.kind).to_owned())
                .or_default() += 1;
        }
        row.rejected_complete_blip_count = inventory.rejected_complete_blips.len();
        for rejected in &inventory.rejected_complete_blips {
            *row.rejected_disposition_counts
                .entry(disposition_name(rejected.disposition).to_owned())
                .or_default() += 1;
            *row.rejected_kind_counts
                .entry(kind_name(rejected.kind).to_owned())
                .or_default() += 1;
        }

        let evidence = build_reader_partial_escherdelay_evidence(&bytes, &probe);
        row.reader_admissible_image_count = evidence
            .as_ref()
            .map_or(0, |value| value.validated_images.len());

        if row.reader_admissible_image_count != row.strict_validated_image_count {
            bail!(
                "strict parser/product projection disagreement for {}: {} vs {}",
                source_sha256,
                row.strict_validated_image_count,
                row.reader_admissible_image_count
            );
        }

        row.outcome = if row.reader_admissible_image_count > 0 {
            CensusOutcome::ImageSalvagePositive
        } else if !row.rejected_disposition_counts.is_empty()
            && row.rejected_disposition_counts.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "unsupported_metafile" | "unsupported_picture" | "unsupported_blip_type"
                )
            })
        {
            CensusOutcome::UnsupportedOnly
        } else if row.terminal_gap.is_some() && row.scanned_record_count == 0 {
            CensusOutcome::ParserTerminalBeforeImage
        } else {
            CensusOutcome::StrictRasterZero
        };

        let post =
            fs::read(path).with_context(|| format!("re-read admitted source {}", source_sha256))?;
        row.source_modified = Some(sha256_hex(&post) != source_sha256);
        if row.source_modified == Some(true) {
            bail!("source modified during census: {source_sha256}");
        }

        rows.push(row);
    }

    rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));

    let mut validated_kind_counts = BTreeMap::<String, usize>::new();
    let mut rejected_disposition_counts = BTreeMap::<String, usize>::new();
    let mut terminal_gap_counts = BTreeMap::<String, usize>::new();
    let mut outcome_counts = BTreeMap::<String, usize>::new();
    let mut total_reader_admissible_images = 0usize;

    for row in &rows {
        for (kind, count) in &row.validated_kind_counts {
            *validated_kind_counts.entry(kind.clone()).or_default() += count;
        }
        for (disposition, count) in &row.rejected_disposition_counts {
            *rejected_disposition_counts
                .entry(disposition.clone())
                .or_default() += count;
        }
        if let Some(gap) = &row.terminal_gap {
            *terminal_gap_counts
                .entry(terminal_gap_name(gap).to_owned())
                .or_default() += 1;
        }
        *outcome_counts
            .entry(outcome_name(&row.outcome).to_owned())
            .or_default() += 1;
        total_reader_admissible_images += row.reader_admissible_image_count;
    }

    let located_source_count = rows
        .iter()
        .filter(|row| !matches!(row.outcome, CensusOutcome::SourceMissing))
        .count();
    let missing_source_count = EXPECTED_DENOMINATOR - located_source_count;
    let image_salvage_positive_files = rows
        .iter()
        .filter(|row| matches!(row.outcome, CensusOutcome::ImageSalvagePositive))
        .count();

    let summary = CensusSummary {
        schema: OUTPUT_SCHEMA,
        expected_denominator: EXPECTED_DENOMINATOR,
        manifest_source_count: EXPECTED_DENOMINATOR,
        located_source_count,
        missing_source_count,
        image_salvage_positive_files,
        total_reader_admissible_images,
        validated_kind_counts,
        rejected_disposition_counts,
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
