use anyhow::{Context, Result, bail};
use pub_cfb::{CfbEntry, CfbInventory, EntryKind, inspect_reader, read_stream_reader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

pub const EXPERIMENT_SCHEMA_V1: &str = "chaptera.pub-re-experiment.v1";
pub const RECEIPT_SCHEMA_V1: &str = "chaptera.pub-re-receipt.v1";

#[derive(Debug, Clone, Deserialize)]
pub struct ExperimentManifestV1 {
    pub schema: String,
    pub experiment_id: String,
    pub question: String,
    pub before: ExperimentInputV1,
    pub after: ExperimentInputV1,
    #[serde(default)]
    pub policy: ExperimentPolicyV1,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExperimentInputV1 {
    pub path: PathBuf,
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExperimentPolicyV1 {
    #[serde(default = "default_max_stream_bytes")]
    pub max_stream_bytes: u64,
    #[serde(default = "default_max_changed_ranges_per_stream")]
    pub max_changed_ranges_per_stream: usize,
}

impl Default for ExperimentPolicyV1 {
    fn default() -> Self {
        Self {
            max_stream_bytes: default_max_stream_bytes(),
            max_changed_ranges_per_stream: default_max_changed_ranges_per_stream(),
        }
    }
}

fn default_max_stream_bytes() -> u64 {
    64 * 1024 * 1024
}

fn default_max_changed_ranges_per_stream() -> usize {
    128
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PubReReceiptV1 {
    pub schema: &'static str,
    pub analyzer_version: &'static str,
    pub experiment_id: String,
    pub question: String,
    pub status: &'static str,
    pub inputs: BTreeMap<&'static str, InputReceiptV1>,
    pub cfb: CfbDiffReceiptV1,
    pub invariants: ReceiptInvariantsV1,
    pub limitations: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InputReceiptV1 {
    pub sha256: String,
    pub byte_len: u64,
    pub entry_count: usize,
    pub stream_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CfbDiffReceiptV1 {
    pub added_entries: Vec<EntryFingerprintV1>,
    pub removed_entries: Vec<EntryFingerprintV1>,
    pub changed_entry_shapes: Vec<EntryShapeChangeV1>,
    pub changed_streams: Vec<ChangedStreamV1>,
    pub logical_change_detected: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EntryFingerprintV1 {
    pub path: String,
    pub kind: &'static str,
    pub len: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EntryShapeChangeV1 {
    pub path: String,
    pub before_kind: &'static str,
    pub after_kind: &'static str,
    pub before_len: u64,
    pub after_len: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChangedStreamV1 {
    pub path: String,
    pub before_len: u64,
    pub after_len: u64,
    pub before_sha256: String,
    pub after_sha256: String,
    pub equal_length_changed_byte_count: Option<u64>,
    pub common_prefix_len: u64,
    pub common_suffix_len: u64,
    pub changed_ranges: Vec<ByteRangeDeltaV1>,
    pub changed_ranges_truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ByteRangeDeltaV1 {
    pub start: u64,
    pub before_len: u64,
    pub after_len: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReceiptInvariantsV1 {
    pub raw_document_bytes_emitted: bool,
    pub raw_stream_bytes_emitted: bool,
    pub absolute_input_paths_emitted: bool,
    pub expected_input_hashes_verified: bool,
    pub deterministic_ordering: bool,
}

struct LoadedInput {
    bytes: Vec<u8>,
    sha256: String,
    inventory: CfbInventory,
}

pub fn analyze_manifest_file(path: &Path) -> Result<PubReReceiptV1> {
    let source = fs::read(path).with_context(|| format!("read manifest {}", path.display()))?;
    let manifest: ExperimentManifestV1 = serde_json::from_slice(&source)
        .with_context(|| format!("parse manifest {}", path.display()))?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    analyze_manifest(&manifest, base_dir)
}

pub fn analyze_manifest(
    manifest: &ExperimentManifestV1,
    manifest_base_dir: &Path,
) -> Result<PubReReceiptV1> {
    validate_manifest(manifest)?;

    let before = load_input(
        &manifest.before,
        manifest_base_dir,
        "before",
        &manifest.policy,
    )?;
    let after = load_input(
        &manifest.after,
        manifest_base_dir,
        "after",
        &manifest.policy,
    )?;

    let cfb = compare_cfb(&before, &after, &manifest.policy)?;
    let status = if before.sha256 == after.sha256 {
        "raw_identical"
    } else if cfb.logical_change_detected {
        "logical_cfb_change"
    } else {
        "no_stream_payload_change"
    };

    let mut inputs = BTreeMap::new();
    inputs.insert("before", input_receipt(&before));
    inputs.insert("after", input_receipt(&after));

    Ok(PubReReceiptV1 {
        schema: RECEIPT_SCHEMA_V1,
        analyzer_version: env!("CARGO_PKG_VERSION"),
        experiment_id: manifest.experiment_id.clone(),
        question: manifest.question.clone(),
        status,
        inputs,
        cfb,
        invariants: ReceiptInvariantsV1 {
            raw_document_bytes_emitted: false,
            raw_stream_bytes_emitted: false,
            absolute_input_paths_emitted: false,
            expected_input_hashes_verified: true,
            deterministic_ordering: true,
        },
        limitations: vec![
            "v0 compares CFB inventory and logical stream payloads; storage metadata such as timestamps/CLSID/state bits is not yet included in the logical verdict",
            "v0 does not infer Publisher semantics; it localizes byte/stream deltas for later oracle joins",
        ],
    })
}

fn validate_manifest(manifest: &ExperimentManifestV1) -> Result<()> {
    if manifest.schema != EXPERIMENT_SCHEMA_V1 {
        bail!(
            "unsupported experiment schema {}; expected {}",
            manifest.schema,
            EXPERIMENT_SCHEMA_V1
        );
    }
    if manifest.experiment_id.trim().is_empty() {
        bail!("experiment_id must not be empty");
    }
    if manifest.question.trim().is_empty() {
        bail!("question must not be empty");
    }
    if manifest.policy.max_stream_bytes == 0 {
        bail!("policy.max_stream_bytes must be greater than zero");
    }
    if manifest.policy.max_changed_ranges_per_stream == 0 {
        bail!("policy.max_changed_ranges_per_stream must be greater than zero");
    }
    Ok(())
}

fn resolve_input_path(base_dir: &Path, configured: &Path) -> PathBuf {
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        base_dir.join(configured)
    }
}

fn load_input(
    input: &ExperimentInputV1,
    base_dir: &Path,
    label: &str,
    policy: &ExperimentPolicyV1,
) -> Result<LoadedInput> {
    let path = resolve_input_path(base_dir, &input.path);
    let bytes = fs::read(&path).with_context(|| format!("read {label} input"))?;
    let sha256 = sha256_hex(&bytes);

    if let Some(expected) = input.expected_sha256.as_deref() {
        validate_expected_sha(expected, label)?;
        if !sha256.eq_ignore_ascii_case(expected) {
            bail!(
                "{label} SHA-256 mismatch: expected {}, got {}",
                expected.to_ascii_lowercase(),
                sha256
            );
        }
    }

    let inventory = inspect_reader(Cursor::new(bytes.as_slice()))
        .with_context(|| format!("inspect {label} input as CFB"))?;

    for entry in &inventory.entries {
        if entry.kind == EntryKind::Stream && entry.len > policy.max_stream_bytes {
            bail!(
                "{label} stream {} is {} bytes, exceeding policy.max_stream_bytes={}",
                entry.path,
                entry.len,
                policy.max_stream_bytes
            );
        }
    }

    Ok(LoadedInput {
        bytes,
        sha256,
        inventory,
    })
}

fn validate_expected_sha(value: &str, label: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{label}.expected_sha256 must be exactly 64 hexadecimal characters");
    }
    Ok(())
}

fn input_receipt(input: &LoadedInput) -> InputReceiptV1 {
    InputReceiptV1 {
        sha256: input.sha256.clone(),
        byte_len: input.bytes.len() as u64,
        entry_count: input.inventory.entries.len(),
        stream_count: input
            .inventory
            .entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Stream)
            .count(),
    }
}

fn compare_cfb(
    before: &LoadedInput,
    after: &LoadedInput,
    policy: &ExperimentPolicyV1,
) -> Result<CfbDiffReceiptV1> {
    let before_map = entry_map(&before.inventory);
    let after_map = entry_map(&after.inventory);
    let all_paths = before_map
        .keys()
        .chain(after_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut added_entries = Vec::new();
    let mut removed_entries = Vec::new();
    let mut changed_entry_shapes = Vec::new();
    let mut changed_streams = Vec::new();

    for path in all_paths {
        match (before_map.get(&path), after_map.get(&path)) {
            (None, Some(entry)) => {
                added_entries.push(entry_fingerprint(after, entry)?);
            }
            (Some(entry), None) => {
                removed_entries.push(entry_fingerprint(before, entry)?);
            }
            (Some(before_entry), Some(after_entry)) => {
                if before_entry.kind != after_entry.kind
                    || (before_entry.kind != EntryKind::Stream
                        && before_entry.len != after_entry.len)
                {
                    changed_entry_shapes.push(EntryShapeChangeV1 {
                        path: path.clone(),
                        before_kind: kind_name(before_entry.kind),
                        after_kind: kind_name(after_entry.kind),
                        before_len: before_entry.len,
                        after_len: after_entry.len,
                    });
                }

                if before_entry.kind == EntryKind::Stream && after_entry.kind == EntryKind::Stream {
                    let before_bytes = stream_bytes(before, &path)?;
                    let after_bytes = stream_bytes(after, &path)?;
                    if before_bytes != after_bytes {
                        changed_streams.push(changed_stream(
                            path,
                            &before_bytes,
                            &after_bytes,
                            policy.max_changed_ranges_per_stream,
                        ));
                    }
                }
            }
            (None, None) => unreachable!("path originated from union"),
        }
    }

    let logical_change_detected = !added_entries.is_empty()
        || !removed_entries.is_empty()
        || !changed_entry_shapes.is_empty()
        || !changed_streams.is_empty();

    Ok(CfbDiffReceiptV1 {
        added_entries,
        removed_entries,
        changed_entry_shapes,
        changed_streams,
        logical_change_detected,
    })
}

fn entry_map(inventory: &CfbInventory) -> BTreeMap<String, &CfbEntry> {
    inventory
        .entries
        .iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect()
}

fn entry_fingerprint(input: &LoadedInput, entry: &CfbEntry) -> Result<EntryFingerprintV1> {
    let sha256 = if entry.kind == EntryKind::Stream {
        Some(sha256_hex(&stream_bytes(input, &entry.path)?))
    } else {
        None
    };
    Ok(EntryFingerprintV1 {
        path: entry.path.clone(),
        kind: kind_name(entry.kind),
        len: entry.len,
        sha256,
    })
}

fn stream_bytes(input: &LoadedInput, path: &str) -> Result<Vec<u8>> {
    read_stream_reader(Cursor::new(input.bytes.as_slice()), path)
        .with_context(|| format!("read logical stream {path}"))
}

fn changed_stream(path: String, before: &[u8], after: &[u8], max_ranges: usize) -> ChangedStreamV1 {
    let prefix = common_prefix_len(before, after);
    let suffix = common_suffix_len(before, after, prefix);

    let (equal_length_changed_byte_count, changed_ranges, changed_ranges_truncated) =
        if before.len() == after.len() {
            equal_length_ranges(before, after, max_ranges)
        } else {
            let before_mid = before.len().saturating_sub(prefix + suffix);
            let after_mid = after.len().saturating_sub(prefix + suffix);
            (
                None,
                vec![ByteRangeDeltaV1 {
                    start: prefix as u64,
                    before_len: before_mid as u64,
                    after_len: after_mid as u64,
                }],
                false,
            )
        };

    ChangedStreamV1 {
        path,
        before_len: before.len() as u64,
        after_len: after.len() as u64,
        before_sha256: sha256_hex(before),
        after_sha256: sha256_hex(after),
        equal_length_changed_byte_count,
        common_prefix_len: prefix as u64,
        common_suffix_len: suffix as u64,
        changed_ranges,
        changed_ranges_truncated,
    }
}

fn equal_length_ranges(
    before: &[u8],
    after: &[u8],
    max_ranges: usize,
) -> (Option<u64>, Vec<ByteRangeDeltaV1>, bool) {
    let mut index = 0usize;
    let mut changed = 0u64;
    let mut ranges = Vec::new();
    let mut truncated = false;

    while index < before.len() {
        if before[index] == after[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < before.len() && before[index] != after[index] {
            index += 1;
        }
        let len = index - start;
        changed += len as u64;
        if ranges.len() < max_ranges {
            ranges.push(ByteRangeDeltaV1 {
                start: start as u64,
                before_len: len as u64,
                after_len: len as u64,
            });
        } else {
            truncated = true;
        }
    }

    (Some(changed), ranges, truncated)
}

fn common_prefix_len(before: &[u8], after: &[u8]) -> usize {
    before
        .iter()
        .zip(after.iter())
        .take_while(|(left, right)| left == right)
        .count()
}

fn common_suffix_len(before: &[u8], after: &[u8], prefix: usize) -> usize {
    let max_suffix = before.len().min(after.len()).saturating_sub(prefix);
    before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(max_suffix)
        .take_while(|(left, right)| left == right)
        .count()
}

fn kind_name(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Root => "root",
        EntryKind::Storage => "storage",
        EntryKind::Stream => "stream",
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfb::CompoundFile;
    use std::io::{Cursor, Write};
    use tempfile::TempDir;

    fn synthetic_cfb(contents: &[u8], state_bits: u32) -> Vec<u8> {
        let mut compound =
            CompoundFile::create(Cursor::new(Vec::new())).expect("create synthetic CFB");
        compound.create_storage("/Meta").expect("create Meta");
        compound
            .set_state_bits("/Meta", state_bits)
            .expect("set Meta state bits");
        compound
            .create_stream("/Contents")
            .expect("create Contents")
            .write_all(contents)
            .expect("write Contents");
        compound
            .create_stream("/Stable")
            .expect("create Stable")
            .write_all(b"stable")
            .expect("write Stable");
        compound.flush().expect("flush synthetic CFB");
        compound.into_inner().into_inner()
    }

    fn write_fixture(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, bytes).expect("write fixture");
        path
    }

    fn manifest(before: &Path, after: &Path) -> ExperimentManifestV1 {
        ExperimentManifestV1 {
            schema: EXPERIMENT_SCHEMA_V1.to_owned(),
            experiment_id: "test-diff".to_owned(),
            question: "which logical stream changed?".to_owned(),
            before: ExperimentInputV1 {
                path: before.to_path_buf(),
                expected_sha256: None,
            },
            after: ExperimentInputV1 {
                path: after.to_path_buf(),
                expected_sha256: None,
            },
            policy: ExperimentPolicyV1::default(),
        }
    }

    #[test]
    fn localizes_equal_length_stream_delta_without_emitting_bytes() {
        let dir = TempDir::new().expect("temp dir");
        let before = write_fixture(&dir, "before.pub", &synthetic_cfb(b"abcdefghij", 1));
        let after = write_fixture(&dir, "after.pub", &synthetic_cfb(b"abcXXfghYj", 1));

        let receipt = analyze_manifest(&manifest(&before, &after), dir.path()).expect("analyze");
        assert_eq!(receipt.status, "logical_cfb_change");
        assert_eq!(receipt.cfb.changed_streams.len(), 1);

        let stream = &receipt.cfb.changed_streams[0];
        assert_eq!(stream.path, "/Contents");
        assert_eq!(stream.equal_length_changed_byte_count, Some(3));
        assert_eq!(
            stream.changed_ranges,
            vec![
                ByteRangeDeltaV1 {
                    start: 3,
                    before_len: 2,
                    after_len: 2,
                },
                ByteRangeDeltaV1 {
                    start: 8,
                    before_len: 1,
                    after_len: 1,
                },
            ]
        );

        let json = serde_json::to_string(&receipt).expect("serialize receipt");
        assert!(!json.contains("abcdefghij"));
        assert!(!json.contains("abcXXfghYj"));
        assert!(!json.contains(dir.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn distinguishes_container_metadata_change_from_stream_payload_change() {
        let dir = TempDir::new().expect("temp dir");
        let before = write_fixture(&dir, "before.pub", &synthetic_cfb(b"same", 1));
        let after = write_fixture(&dir, "after.pub", &synthetic_cfb(b"same", 2));

        let receipt = analyze_manifest(&manifest(&before, &after), dir.path()).expect("analyze");
        assert_eq!(receipt.status, "no_stream_payload_change");
        assert!(!receipt.cfb.logical_change_detected);
        assert!(receipt.cfb.changed_streams.is_empty());
        assert_ne!(
            receipt.inputs["before"].sha256,
            receipt.inputs["after"].sha256
        );
    }

    #[test]
    fn expected_hash_is_fail_closed() {
        let dir = TempDir::new().expect("temp dir");
        let before = write_fixture(&dir, "before.pub", &synthetic_cfb(b"same", 1));
        let after = write_fixture(&dir, "after.pub", &synthetic_cfb(b"same", 1));
        let mut input = manifest(&before, &after);
        input.before.expected_sha256 = Some("00".repeat(32));

        let error = analyze_manifest(&input, dir.path()).expect_err("hash mismatch must fail");
        assert!(error.to_string().contains("before SHA-256 mismatch"));
    }

    #[test]
    fn unequal_length_delta_reports_prefix_and_suffix_without_guessing_byte_count() {
        let dir = TempDir::new().expect("temp dir");
        let before = write_fixture(&dir, "before.pub", &synthetic_cfb(b"abc123xyz", 1));
        let after = write_fixture(&dir, "after.pub", &synthetic_cfb(b"abcLONGERxyz", 1));

        let receipt = analyze_manifest(&manifest(&before, &after), dir.path()).expect("analyze");
        let stream = &receipt.cfb.changed_streams[0];
        assert_eq!(stream.equal_length_changed_byte_count, None);
        assert_eq!(stream.common_prefix_len, 3);
        assert_eq!(stream.common_suffix_len, 3);
        assert_eq!(
            stream.changed_ranges,
            vec![ByteRangeDeltaV1 {
                start: 3,
                before_len: 3,
                after_len: 6,
            }]
        );
    }
}
