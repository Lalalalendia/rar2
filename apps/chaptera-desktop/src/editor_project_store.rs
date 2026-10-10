use chaptera_suite_handoff::{AdmittedDestination, DestinationWriteError, identify_existing_path};
use pub_editor::{EditorProject, EditorProjectAsset, Sha256Digest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

pub const STORAGE_SCHEMA_V1: &str = "chaptera.editor-project-storage.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreDisposition {
    Current,
    Recovery,
    Legacy,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct OpenedEditorProject {
    pub project: EditorProject,
    pub asset_bytes: BTreeMap<Sha256Digest, Vec<u8>>,
    pub generation: Option<u64>,
    pub disposition: StoreDisposition,
    pub manifest_path: PathBuf,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct EditorProjectStoreReceipt {
    pub visible_path: PathBuf,
    pub generation_manifest_path: PathBuf,
    pub generation_asset_dir: PathBuf,
    pub generation: u64,
    pub project_state_id: String,
}

#[derive(Debug)]
pub enum EditorProjectStoreError {
    Path(&'static str),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    ConcurrentSave {
        path: PathBuf,
    },
    Destination(DestinationWriteError),
    Serialize(String),
    Corrupt {
        path: PathBuf,
        reason: String,
    },
    GenerationOverflow,
    RequiredAssetMissing {
        sha256: Sha256Digest,
    },
    AssetLengthMismatch {
        sha256: Sha256Digest,
        expected: u64,
        found: u64,
    },
    AssetHashMismatch {
        expected: Sha256Digest,
        found: String,
    },
    AssetSignatureMismatch {
        sha256: Sha256Digest,
        mime: String,
    },
}

impl fmt::Display for EditorProjectStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(reason) => write!(formatter, "invalid EditorProject store path: {reason}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(formatter, "{operation} {}: {source}", path.display()),
            Self::ConcurrentSave { path } => write!(
                formatter,
                "another EditorProject save holds the live transaction lock {}",
                path.display()
            ),
            Self::Destination(error) => write!(formatter, "durable destination rejected: {error}"),
            Self::Serialize(message) => {
                write!(
                    formatter,
                    "serialize EditorProject storage envelope: {message}"
                )
            }
            Self::Corrupt { path, reason } => write!(
                formatter,
                "EditorProject storage is corrupt/incomplete at {}: {reason}",
                path.display()
            ),
            Self::GenerationOverflow => {
                formatter.write_str("EditorProject storage generation overflow")
            }
            Self::RequiredAssetMissing { sha256 } => write!(
                formatter,
                "project-required replacement asset {sha256} has no runtime bytes"
            ),
            Self::AssetLengthMismatch {
                sha256,
                expected,
                found,
            } => write!(
                formatter,
                "project-required replacement asset {sha256} has {found} bytes; expected {expected}"
            ),
            Self::AssetHashMismatch { expected, found } => write!(
                formatter,
                "project-required replacement asset hash {found} does not match {expected}"
            ),
            Self::AssetSignatureMismatch { sha256, mime } => write!(
                formatter,
                "project-required replacement asset {sha256} does not match MIME {mime}"
            ),
        }
    }
}

impl std::error::Error for EditorProjectStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Destination(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DestinationWriteError> for EditorProjectStoreError {
    fn from(value: DestinationWriteError) -> Self {
        Self::Destination(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EditorProjectStorageEnvelopeV1 {
    storage_schema: String,
    generation: u64,
    source_sha256: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    project_state_id: String,
    project_payload_byte_len: u64,
    project_payload_sha256: String,
    asset_directory: String,
    project: EditorProject,
}

struct StoreLock {
    _file: File,
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> EditorProjectStoreError {
    EditorProjectStoreError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

pub fn sidecar_path(source_path: &Path) -> Option<PathBuf> {
    let file_name = source_path.file_name()?;
    let mut sidecar_name = file_name.to_os_string();
    sidecar_name.push(".pub-editor.json");
    Some(source_path.with_file_name(sidecar_name))
}

fn legacy_asset_dir_path(source_path: &Path) -> Option<PathBuf> {
    let file_name = source_path.file_name()?;
    let mut asset_dir_name = file_name.to_os_string();
    asset_dir_name.push(".pub-editor.assets");
    Some(source_path.with_file_name(asset_dir_name))
}

fn source_parent(source_path: &Path) -> Result<PathBuf, EditorProjectStoreError> {
    source_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or(EditorProjectStoreError::Path(
            "source path must have a parent directory",
        ))
}

fn namespace(source_path: &Path) -> Result<String, EditorProjectStoreError> {
    let sidecar = sidecar_path(source_path).ok_or(EditorProjectStoreError::Path(
        "source path must have a file name",
    ))?;
    let file_name = sidecar.file_name().ok_or(EditorProjectStoreError::Path(
        "sidecar path must have a file name",
    ))?;
    let digest = Sha256::digest(file_name.to_string_lossy().as_bytes());
    Ok(format!("{:x}", digest)[..24].to_owned())
}

fn generation_prefix(source_path: &Path) -> Result<String, EditorProjectStoreError> {
    Ok(format!(
        ".chaptera-editor-project-{}-g",
        namespace(source_path)?
    ))
}

fn generation_manifest_name(
    source_path: &Path,
    generation: u64,
) -> Result<String, EditorProjectStoreError> {
    Ok(format!(
        "{}{generation:020}.json",
        generation_prefix(source_path)?
    ))
}

fn generation_asset_dir_name(
    source_path: &Path,
    generation: u64,
) -> Result<String, EditorProjectStoreError> {
    Ok(format!(
        "{}{generation:020}.assets",
        generation_prefix(source_path)?
    ))
}

fn generation_manifest_path(
    source_path: &Path,
    generation: u64,
) -> Result<PathBuf, EditorProjectStoreError> {
    Ok(source_parent(source_path)?.join(generation_manifest_name(source_path, generation)?))
}

fn generation_asset_dir_path(
    source_path: &Path,
    generation: u64,
) -> Result<PathBuf, EditorProjectStoreError> {
    Ok(source_parent(source_path)?.join(generation_asset_dir_name(source_path, generation)?))
}

fn recovery_path(source_path: &Path) -> Result<PathBuf, EditorProjectStoreError> {
    Ok(source_parent(source_path)?.join(format!(
        ".chaptera-editor-project-{}.recovery.json",
        namespace(source_path)?
    )))
}

fn lock_path(source_path: &Path) -> Result<PathBuf, EditorProjectStoreError> {
    Ok(source_parent(source_path)?.join(format!(
        ".chaptera-editor-project-{}.lock",
        namespace(source_path)?
    )))
}

#[cfg(target_os = "windows")]
fn acquire_lock(source_path: &Path) -> Result<StoreLock, EditorProjectStoreError> {
    use std::os::windows::fs::OpenOptionsExt;

    let path = lock_path(source_path)?;
    reject_reparse_path_if_present(&path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(&path)
        .map_err(|error| {
            if matches!(error.raw_os_error(), Some(32 | 33)) {
                EditorProjectStoreError::ConcurrentSave { path: path.clone() }
            } else {
                io_error("acquire EditorProject save lock", &path, error)
            }
        })?;
    Ok(StoreLock { _file: file })
}

#[cfg(not(target_os = "windows"))]
fn acquire_lock(source_path: &Path) -> Result<StoreLock, EditorProjectStoreError> {
    // Chaptera Desktop's durable-save product contract is Windows-owned.
    // Non-Windows builds keep the store testable/portable but make no
    // cross-process locking claim.
    let path = lock_path(source_path)?;
    reject_reparse_path_if_present(&path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| io_error("open EditorProject test lock", &path, error))?;
    Ok(StoreLock { _file: file })
}

pub fn inspect(
    source_path: &Path,
    expected_source_hash: Sha256Digest,
) -> Result<Option<OpenedEditorProject>, EditorProjectStoreError> {
    let visible = sidecar_path(source_path).ok_or(EditorProjectStoreError::Path(
        "source path must have a file name",
    ))?;

    let mut primary_error = None;
    if path_exists(&visible)? {
        match inspect_manifest(
            source_path,
            &visible,
            expected_source_hash,
            StoreDisposition::Current,
        ) {
            Ok(opened) => return Ok(Some(opened)),
            Err(error) => primary_error = Some(error),
        }
    }

    let recovery = recovery_path(source_path)?;
    if path_exists(&recovery)?
        && let Ok(opened) = inspect_manifest(
            source_path,
            &recovery,
            expected_source_hash,
            StoreDisposition::Recovery,
        )
    {
        return Ok(Some(opened));
    }

    if let Some(error) = primary_error {
        return Err(error);
    }
    Ok(None)
}

fn inspect_manifest(
    source_path: &Path,
    manifest_path: &Path,
    expected_source_hash: Sha256Digest,
    disposition: StoreDisposition,
) -> Result<OpenedEditorProject, EditorProjectStoreError> {
    reject_reparse_existing(manifest_path)?;
    let bytes = fs::read(manifest_path)
        .map_err(|error| io_error("read EditorProject manifest", manifest_path, error))?;

    if let Ok(envelope) = serde_json::from_slice::<EditorProjectStorageEnvelopeV1>(&bytes) {
        return inspect_envelope(
            source_path,
            manifest_path,
            expected_source_hash,
            disposition,
            envelope,
        );
    }

    let project: EditorProject =
        serde_json::from_slice(&bytes).map_err(|error| EditorProjectStoreError::Corrupt {
            path: manifest_path.to_path_buf(),
            reason: format!("neither storage envelope nor legacy EditorProject JSON: {error}"),
        })?;
    if project.source_hash != expected_source_hash {
        return Err(EditorProjectStoreError::Corrupt {
            path: manifest_path.to_path_buf(),
            reason: "legacy EditorProject source SHA does not match opened PUB".to_owned(),
        });
    }
    let asset_bytes = load_assets(
        &project.assets,
        legacy_asset_dir_path(source_path).ok_or(EditorProjectStoreError::Path(
            "source path must have a file name",
        ))?,
    )?;
    Ok(OpenedEditorProject {
        project,
        asset_bytes,
        generation: None,
        disposition: StoreDisposition::Legacy,
        manifest_path: manifest_path.to_path_buf(),
    })
}

fn inspect_envelope(
    source_path: &Path,
    manifest_path: &Path,
    expected_source_hash: Sha256Digest,
    disposition: StoreDisposition,
    envelope: EditorProjectStorageEnvelopeV1,
) -> Result<OpenedEditorProject, EditorProjectStoreError> {
    if envelope.storage_schema != STORAGE_SCHEMA_V1 {
        return Err(corrupt(
            manifest_path,
            format!("unsupported storage schema {:?}", envelope.storage_schema),
        ));
    }
    if envelope.generation == 0 {
        return Err(corrupt(manifest_path, "generation must be non-zero"));
    }
    if envelope.source_sha256 != expected_source_hash
        || envelope.project.source_hash != expected_source_hash
    {
        return Err(corrupt(
            manifest_path,
            "storage/source project SHA does not match opened PUB",
        ));
    }

    let project_id = envelope
        .project
        .identity
        .as_ref()
        .map(|identity| identity.project_id.clone());
    if envelope.project_id != project_id {
        return Err(corrupt(
            manifest_path,
            "storage project identity does not match project payload",
        ));
    }
    if envelope.project_state_id != envelope.project.state_id_v1() {
        return Err(corrupt(
            manifest_path,
            "project state identity does not match project payload",
        ));
    }

    let payload = serde_json::to_vec(&envelope.project)
        .map_err(|error| EditorProjectStoreError::Serialize(error.to_string()))?;
    let payload_len =
        u64::try_from(payload.len()).map_err(|_| EditorProjectStoreError::GenerationOverflow)?;
    if payload_len != envelope.project_payload_byte_len {
        return Err(corrupt(
            manifest_path,
            "project payload byte length does not match envelope",
        ));
    }
    if sha256_hex(&payload) != envelope.project_payload_sha256 {
        return Err(corrupt(
            manifest_path,
            "project payload SHA-256 does not match envelope",
        ));
    }

    let expected_asset_directory = generation_asset_dir_name(source_path, envelope.generation)?;
    if envelope.asset_directory != expected_asset_directory {
        return Err(corrupt(
            manifest_path,
            "asset directory token is not the generated store path",
        ));
    }
    let asset_dir = source_parent(source_path)?.join(&envelope.asset_directory);
    validate_generation_asset_directory(&asset_dir, &envelope.project.assets)?;
    let asset_bytes = load_assets(&envelope.project.assets, asset_dir)?;

    Ok(OpenedEditorProject {
        project: envelope.project,
        asset_bytes,
        generation: Some(envelope.generation),
        disposition,
        manifest_path: manifest_path.to_path_buf(),
    })
}

pub fn commit(
    source_path: &Path,
    project: &EditorProject,
    runtime_assets: &BTreeMap<Sha256Digest, Vec<u8>>,
) -> Result<EditorProjectStoreReceipt, EditorProjectStoreError> {
    let _lock = acquire_lock(source_path)?;
    let source_identity = identify_existing_path(source_path)?;
    let protected = [source_identity];

    let previous = inspect(source_path, project.source_hash)?;
    let previous_generation = previous.as_ref().and_then(|opened| opened.generation);

    let mut recovery_replace_backup = None;
    if let Some(previous) = previous.as_ref() {
        let bytes = fs::read(&previous.manifest_path).map_err(|error| {
            io_error(
                "read last-known-good EditorProject manifest",
                &previous.manifest_path,
                error,
            )
        })?;
        let receipt = AdmittedDestination::admit(&recovery_path(source_path)?, &protected)?
            .commit_bytes(&bytes)?;
        recovery_replace_backup = receipt.backup_path;
    }

    let generation = next_available_generation(source_path)?;
    let asset_dir = generation_asset_dir_path(source_path, generation)?;
    fs::create_dir(&asset_dir).map_err(|error| {
        io_error(
            "create EditorProject generation asset directory",
            &asset_dir,
            error,
        )
    })?;
    reject_reparse_existing(&asset_dir)?;

    let expected_asset_dir_identity = identify_existing_path(&asset_dir)?;
    for metadata in &project.assets {
        let bytes = runtime_assets.get(&metadata.sha256).ok_or(
            EditorProjectStoreError::RequiredAssetMissing {
                sha256: metadata.sha256,
            },
        )?;
        validate_asset_bytes(metadata, bytes)?;

        let file_name = metadata
            .file_name()
            .map_err(|error| EditorProjectStoreError::Corrupt {
                path: asset_dir.clone(),
                reason: format!("invalid project asset name: {error}"),
            })?;
        let target = asset_dir.join(file_name);
        let destination = AdmittedDestination::admit(&target, &protected)?;

        if identify_existing_path(&asset_dir)? != expected_asset_dir_identity {
            return Err(EditorProjectStoreError::Corrupt {
                path: asset_dir.clone(),
                reason: "generation asset directory identity changed during save".to_owned(),
            });
        }
        destination.commit_bytes(bytes)?;
    }

    validate_generation_asset_directory(&asset_dir, &project.assets)?;

    let envelope = build_envelope(source_path, generation, project)?;
    let mut envelope_bytes = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| EditorProjectStoreError::Serialize(error.to_string()))?;
    envelope_bytes.push(b'\n');

    let generation_manifest = generation_manifest_path(source_path, generation)?;
    if path_exists(&generation_manifest)? {
        return Err(EditorProjectStoreError::Corrupt {
            path: generation_manifest,
            reason: "new generation manifest path unexpectedly already exists".to_owned(),
        });
    }
    AdmittedDestination::admit(&generation_manifest, &protected)?.commit_bytes(&envelope_bytes)?;

    let visible = sidecar_path(source_path).ok_or(EditorProjectStoreError::Path(
        "source path must have a file name",
    ))?;
    let visible_receipt =
        AdmittedDestination::admit(&visible, &protected)?.commit_bytes(&envelope_bytes)?;

    let admitted = inspect_manifest(
        source_path,
        &visible,
        project.source_hash,
        StoreDisposition::Current,
    )?;
    if admitted.generation != Some(generation)
        || admitted.project.state_id_v1() != project.state_id_v1()
    {
        return Err(EditorProjectStoreError::Corrupt {
            path: visible.clone(),
            reason: "reopened visible generation does not match committed project".to_owned(),
        });
    }

    // First durable save has no previous committed generation. Seed the
    // deterministic recovery slot only after the visible generation has been
    // reopened and admitted. On later saves, the recovery slot intentionally
    // remains the previous committed generation.
    if previous.is_none() {
        let receipt = AdmittedDestination::admit(&recovery_path(source_path)?, &protected)?
            .commit_bytes(&envelope_bytes)?;
        if let Some(backup) = receipt.backup_path {
            let _ = remove_regular_non_reparse_file(&backup);
        }
    }

    if let Some(backup) = visible_receipt.backup_path {
        let _ = remove_regular_non_reparse_file(&backup);
    }
    if let Some(backup) = recovery_replace_backup {
        let _ = remove_regular_non_reparse_file(&backup);
    }
    cleanup_old_generations(source_path, generation, previous_generation);

    Ok(EditorProjectStoreReceipt {
        visible_path: visible,
        generation_manifest_path: generation_manifest,
        generation_asset_dir: asset_dir,
        generation,
        project_state_id: project.state_id_v1(),
    })
}

fn build_envelope(
    source_path: &Path,
    generation: u64,
    project: &EditorProject,
) -> Result<EditorProjectStorageEnvelopeV1, EditorProjectStoreError> {
    let payload = serde_json::to_vec(project)
        .map_err(|error| EditorProjectStoreError::Serialize(error.to_string()))?;
    let payload_len =
        u64::try_from(payload.len()).map_err(|_| EditorProjectStoreError::GenerationOverflow)?;
    Ok(EditorProjectStorageEnvelopeV1 {
        storage_schema: STORAGE_SCHEMA_V1.to_owned(),
        generation,
        source_sha256: project.source_hash,
        project_id: project
            .identity
            .as_ref()
            .map(|identity| identity.project_id.clone()),
        project_state_id: project.state_id_v1(),
        project_payload_byte_len: payload_len,
        project_payload_sha256: sha256_hex(&payload),
        asset_directory: generation_asset_dir_name(source_path, generation)?,
        project: project.clone(),
    })
}

fn load_assets(
    metadata: &[EditorProjectAsset],
    asset_dir: PathBuf,
) -> Result<BTreeMap<Sha256Digest, Vec<u8>>, EditorProjectStoreError> {
    if metadata.is_empty() {
        if path_exists(&asset_dir)? {
            reject_reparse_existing(&asset_dir)?;
        }
        return Ok(BTreeMap::new());
    }

    reject_reparse_existing(&asset_dir)?;
    let directory_metadata = fs::metadata(&asset_dir)
        .map_err(|error| io_error("stat EditorProject asset directory", &asset_dir, error))?;
    if !directory_metadata.is_dir() {
        return Err(corrupt(&asset_dir, "asset path is not a directory"));
    }

    let mut assets = BTreeMap::new();
    for item in metadata {
        let file_name = item
            .file_name()
            .map_err(|error| corrupt(&asset_dir, format!("invalid asset file name: {error}")))?;
        let path = asset_dir.join(file_name);
        reject_reparse_existing(&path)?;
        let bytes =
            fs::read(&path).map_err(|error| io_error("read EditorProject asset", &path, error))?;
        validate_asset_bytes(item, &bytes)?;
        assets.insert(item.sha256, bytes);
    }

    Ok(assets)
}

fn validate_asset_bytes(
    metadata: &EditorProjectAsset,
    bytes: &[u8],
) -> Result<(), EditorProjectStoreError> {
    let found_len =
        u64::try_from(bytes.len()).map_err(|_| EditorProjectStoreError::GenerationOverflow)?;
    if found_len != metadata.byte_len {
        return Err(EditorProjectStoreError::AssetLengthMismatch {
            sha256: metadata.sha256,
            expected: metadata.byte_len,
            found: found_len,
        });
    }

    let found_sha = sha256_hex(bytes);
    if found_sha != metadata.sha256.to_string() {
        return Err(EditorProjectStoreError::AssetHashMismatch {
            expected: metadata.sha256,
            found: found_sha,
        });
    }

    let signature_matches = match metadata.mime.as_str() {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        _ => false,
    };
    if !signature_matches {
        return Err(EditorProjectStoreError::AssetSignatureMismatch {
            sha256: metadata.sha256,
            mime: metadata.mime.clone(),
        });
    }
    Ok(())
}

fn validate_generation_asset_directory(
    asset_dir: &Path,
    metadata: &[EditorProjectAsset],
) -> Result<(), EditorProjectStoreError> {
    reject_reparse_existing(asset_dir)?;
    let expected = metadata
        .iter()
        .map(|asset| {
            asset
                .file_name()
                .map_err(|error| corrupt(asset_dir, format!("invalid asset file name: {error}")))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;

    let mut found = BTreeSet::new();
    for entry in fs::read_dir(asset_dir)
        .map_err(|error| io_error("list EditorProject generation assets", asset_dir, error))?
    {
        let entry = entry
            .map_err(|error| io_error("read EditorProject generation entry", asset_dir, error))?;
        let path = entry.path();
        reject_reparse_existing(&path)?;
        let metadata = entry
            .metadata()
            .map_err(|error| io_error("stat EditorProject generation entry", &path, error))?;
        if !metadata.is_file() {
            return Err(corrupt(
                &path,
                "generation asset directory contains a non-file entry",
            ));
        }
        found.insert(entry.file_name().to_string_lossy().into_owned());
    }
    if found != expected {
        return Err(corrupt(
            asset_dir,
            "generation asset directory does not equal exact project.assets closure",
        ));
    }
    Ok(())
}

fn next_available_generation(source_path: &Path) -> Result<u64, EditorProjectStoreError> {
    let max = observed_generations(source_path)?
        .into_iter()
        .max()
        .unwrap_or(0);
    let mut generation = max
        .checked_add(1)
        .ok_or(EditorProjectStoreError::GenerationOverflow)?;

    for _ in 0..64 {
        let manifest = generation_manifest_path(source_path, generation)?;
        let assets = generation_asset_dir_path(source_path, generation)?;
        if !path_exists(&manifest)? && !path_exists(&assets)? {
            return Ok(generation);
        }
        generation = generation
            .checked_add(1)
            .ok_or(EditorProjectStoreError::GenerationOverflow)?;
    }
    Err(EditorProjectStoreError::Corrupt {
        path: source_parent(source_path)?,
        reason: "could not allocate a fresh bounded storage generation".to_owned(),
    })
}

fn observed_generations(source_path: &Path) -> Result<Vec<u64>, EditorProjectStoreError> {
    let parent = source_parent(source_path)?;
    let prefix = generation_prefix(source_path)?;
    let mut generations = Vec::new();

    for entry in fs::read_dir(&parent)
        .map_err(|error| io_error("list EditorProject store directory", &parent, error))?
    {
        let entry =
            entry.map_err(|error| io_error("read EditorProject store entry", &parent, error))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        let digits = rest
            .strip_suffix(".json")
            .or_else(|| rest.strip_suffix(".assets"));
        if let Some(digits) = digits
            && digits.len() == 20
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && let Ok(value) = digits.parse::<u64>()
        {
            generations.push(value);
        }
    }
    Ok(generations)
}

fn cleanup_old_generations(
    source_path: &Path,
    current_generation: u64,
    previous_generation: Option<u64>,
) {
    let Ok(generations) = observed_generations(source_path) else {
        return;
    };

    for generation in generations.into_iter().collect::<BTreeSet<_>>() {
        if generation == current_generation || Some(generation) == previous_generation {
            continue;
        }
        let Ok(asset_dir) = generation_asset_dir_path(source_path, generation) else {
            continue;
        };
        let Ok(manifest) = generation_manifest_path(source_path, generation) else {
            continue;
        };
        if remove_generated_asset_dir(&asset_dir).is_err() {
            continue;
        }
        let _ = remove_regular_non_reparse_file(&manifest);
    }
}

fn remove_generated_asset_dir(path: &Path) -> Result<(), EditorProjectStoreError> {
    if !path_exists(path)? {
        return Ok(());
    }
    reject_reparse_existing(path)?;
    let metadata = fs::metadata(path)
        .map_err(|error| io_error("stat old generation directory", path, error))?;
    if !metadata.is_dir() {
        return Err(corrupt(
            path,
            "old generation asset path is not a directory",
        ));
    }

    for entry in fs::read_dir(path)
        .map_err(|error| io_error("list old generation directory", path, error))?
    {
        let entry = entry.map_err(|error| io_error("read old generation entry", path, error))?;
        let child = entry.path();
        reject_reparse_existing(&child)?;
        let metadata = entry
            .metadata()
            .map_err(|error| io_error("stat old generation entry", &child, error))?;
        if !metadata.is_file() || !entry.file_name().to_string_lossy().starts_with("asset-") {
            return Err(corrupt(
                &child,
                "refusing cleanup of unexpected generation entry",
            ));
        }
        fs::remove_file(&child)
            .map_err(|error| io_error("remove old generation asset", &child, error))?;
    }
    fs::remove_dir(path)
        .map_err(|error| io_error("remove old generation asset directory", path, error))
}

fn remove_regular_non_reparse_file(path: &Path) -> Result<(), EditorProjectStoreError> {
    if !path_exists(path)? {
        return Ok(());
    }
    reject_reparse_existing(path)?;
    let metadata =
        fs::metadata(path).map_err(|error| io_error("stat cleanup file", path, error))?;
    if !metadata.is_file() {
        return Err(corrupt(path, "cleanup target is not a regular file"));
    }
    fs::remove_file(path).map_err(|error| io_error("remove cleanup file", path, error))
}

fn path_exists(path: &Path) -> Result<bool, EditorProjectStoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error("stat EditorProject store path", path, error)),
    }
}

fn reject_reparse_path_if_present(path: &Path) -> Result<(), EditorProjectStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_reparse_or_symlink(&metadata) => {
            Err(corrupt(path, "store path is a symlink/reparse point"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("stat EditorProject store path", path, error)),
    }
}

fn reject_reparse_existing(path: &Path) -> Result<(), EditorProjectStoreError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| io_error("stat store path", path, error))?;
    if is_reparse_or_symlink(&metadata) {
        return Err(corrupt(path, "store path is a symlink/reparse point"));
    }
    Ok(())
}

#[cfg(unix)]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(target_os = "windows")]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, target_os = "windows")))]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn corrupt(path: &Path, reason: impl Into<String>) -> EditorProjectStoreError {
    EditorProjectStoreError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_editor::{EDITOR_PROJECT_VERSION_V0_11, EditOperation, EditorProjectIdentity};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn temp_root(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "chaptera-editor-project-store-{label}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create store test root");
        root
    }

    fn source_and_project(root: &Path) -> (PathBuf, EditorProject) {
        let source = root.join("fixture.pub");
        fs::write(&source, b"immutable source").expect("write source");
        let source_hash = {
            let digest = Sha256::digest(b"immutable source");
            let mut bytes = [0_u8; 32];
            bytes.copy_from_slice(&digest);
            Sha256Digest::from_bytes(bytes)
        };
        let project = EditorProject {
            schema_version: EDITOR_PROJECT_VERSION_V0_11.to_owned(),
            source_hash,
            identity: Some(EditorProjectIdentity {
                project_id: "018f0000-0000-7000-8000-000000000001".to_owned(),
                document_id: "018f0000-0000-7000-8000-000000000002".to_owned(),
                history_id: "018f0000-0000-7000-8000-000000000003".to_owned(),
                genesis_revision_id: "018f0000-0000-7000-8000-000000000004".to_owned(),
                forked_from: None,
            }),
            assets: Vec::new(),
            table_grids: Vec::new(),
            operations: Vec::new(),
        };
        (source, project)
    }

    #[test]
    fn envelope_roundtrip_uses_one_inspector() {
        let root = temp_root("roundtrip");
        let (source, project) = source_and_project(&root);

        let source_before = fs::read(&source).expect("read source before save");
        let source_identity_before =
            identify_existing_path(&source).expect("identify source before save");
        let receipt = commit(&source, &project, &BTreeMap::new()).expect("commit");
        assert_eq!(
            fs::read(&source).expect("read source after save"),
            source_before,
            "EditorProject durability must not mutate source PUB bytes"
        );
        assert_eq!(
            identify_existing_path(&source).expect("identify source after save"),
            source_identity_before,
            "EditorProject durability must preserve exact source PUB file identity"
        );
        assert_eq!(receipt.generation, 1);
        assert!(receipt.visible_path.is_file());
        assert!(receipt.generation_manifest_path.is_file());
        assert!(receipt.generation_asset_dir.is_dir());

        let opened = inspect(&source, project.source_hash)
            .expect("inspect")
            .expect("saved project");
        assert_eq!(opened.disposition, StoreDisposition::Current);
        assert_eq!(opened.generation, Some(1));
        assert_eq!(opened.project, project);
        assert!(opened.asset_bytes.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_visible_recovers_last_complete_generation() {
        let root = temp_root("recover");
        let (source, project) = source_and_project(&root);
        let receipt = commit(&source, &project, &BTreeMap::new()).expect("commit");
        fs::write(&receipt.visible_path, b"{ definitely corrupt").expect("corrupt visible sidecar");

        let opened = inspect(&source, project.source_hash)
            .expect("recovery inspect")
            .expect("recover generation");
        assert_eq!(opened.disposition, StoreDisposition::Recovery);
        assert_eq!(opened.generation, Some(1));
        assert_eq!(opened.project, project);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_existing_state_does_not_become_empty_project() {
        let root = temp_root("corrupt-no-recovery");
        let (source, project) = source_and_project(&root);
        let visible = sidecar_path(&source).expect("visible path");
        fs::write(&visible, b"not a project").expect("write corrupt sidecar");

        assert!(inspect(&source, project.source_hash).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn second_generation_retains_previous_recovery_generation() {
        let root = temp_root("generation");
        let (source, project) = source_and_project(&root);
        let first = commit(&source, &project, &BTreeMap::new()).expect("first commit");
        let second = commit(&source, &project, &BTreeMap::new()).expect("second commit");

        assert_eq!(first.generation, 1);
        assert_eq!(second.generation, 2);
        assert!(first.generation_manifest_path.exists());
        assert!(second.generation_manifest_path.exists());

        fs::write(&second.visible_path, b"corrupt visible").expect("corrupt visible");
        fs::write(&second.generation_manifest_path, b"corrupt generation")
            .expect("corrupt newest generation");
        let opened = inspect(&source, project.source_hash)
            .expect("recover previous")
            .expect("previous generation");
        assert_eq!(opened.disposition, StoreDisposition::Recovery);
        assert_eq!(opened.generation, Some(1));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generation_contains_only_project_required_assets() {
        let root = temp_root("assets");
        let (source, mut project) = source_and_project(&root);
        let required_bytes = [b"\x89PNG\r\n\x1a\n".as_slice(), b"required"].concat();
        let unused_bytes = [b"\x89PNG\r\n\x1a\n".as_slice(), b"unused"].concat();

        let required_sha = digest_value(&required_bytes);
        let unused_sha = digest_value(&unused_bytes);
        project.assets.push(EditorProjectAsset {
            sha256: required_sha,
            mime: "image/png".to_owned(),
            byte_len: u64::try_from(required_bytes.len()).expect("bounded test asset"),
        });
        project.operations.push(EditOperation::ReplaceImage {
            node_id: serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
                .expect("canonical NodeId"),
            before_asset: None,
            after_asset: required_sha,
        });

        let runtime = BTreeMap::from([(required_sha, required_bytes), (unused_sha, unused_bytes)]);
        let receipt = commit(&source, &project, &runtime).expect("asset generation commit");
        let entries = fs::read_dir(&receipt.generation_asset_dir)
            .expect("list generation")
            .collect::<Result<Vec<_>, _>>()
            .expect("read entries");
        assert_eq!(
            entries.len(),
            1,
            "unused runtime cache asset must not persist"
        );

        let opened = inspect(&source, project.source_hash)
            .expect("inspect")
            .expect("saved project");
        assert_eq!(opened.asset_bytes.len(), 1);
        assert!(opened.asset_bytes.contains_key(&required_sha));
        assert!(!opened.asset_bytes.contains_key(&unused_sha));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_raw_sidecar_remains_explicit_read_compatibility() {
        let root = temp_root("legacy");
        let (source, project) = source_and_project(&root);
        let visible = sidecar_path(&source).expect("visible path");
        fs::write(
            &visible,
            serde_json::to_vec_pretty(&project).expect("serialize legacy project"),
        )
        .expect("write legacy sidecar");

        let opened = inspect(&source, project.source_hash)
            .expect("inspect legacy")
            .expect("legacy project");
        assert_eq!(opened.disposition, StoreDisposition::Legacy);
        assert_eq!(opened.generation, None);
        assert_eq!(opened.project, project);

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn live_lock_handle_blocks_concurrent_save_but_stale_name_does_not() {
        let root = temp_root("lock");
        let (source, _) = source_and_project(&root);

        let first = acquire_lock(&source).expect("first lock");
        assert!(matches!(
            acquire_lock(&source),
            Err(EditorProjectStoreError::ConcurrentSave { .. })
        ));
        drop(first);
        acquire_lock(&source).expect("stale lock filename must not block a new live handle");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn staged_unpublished_generation_is_never_recovered_as_committed() {
        let root = temp_root("staged-unpublished");
        let (source, project) = source_and_project(&root);
        let first = commit(&source, &project, &BTreeMap::new()).expect("first commit");

        let staged_generation = 2;
        let staged_assets =
            generation_asset_dir_path(&source, staged_generation).expect("staged asset dir");
        fs::create_dir(&staged_assets).expect("create staged asset dir");
        let staged_envelope =
            build_envelope(&source, staged_generation, &project).expect("staged envelope");
        let mut staged_bytes =
            serde_json::to_vec_pretty(&staged_envelope).expect("serialize staged envelope");
        staged_bytes.push(b'\n');
        let staged_manifest =
            generation_manifest_path(&source, staged_generation).expect("staged manifest");
        let source_identity = identify_existing_path(&source).expect("source identity");
        AdmittedDestination::admit(&staged_manifest, &[source_identity])
            .expect("admit staged manifest")
            .commit_bytes(&staged_bytes)
            .expect("publish staged manifest");

        let opened = inspect(&source, project.source_hash)
            .expect("inspect with valid visible")
            .expect("current generation");
        assert_eq!(opened.disposition, StoreDisposition::Current);
        assert_eq!(opened.generation, Some(first.generation));

        fs::write(&first.visible_path, b"corrupt visible").expect("corrupt visible");
        let recovered = inspect(&source, project.source_hash)
            .expect("recover committed generation")
            .expect("recovery generation");
        assert_eq!(recovered.disposition, StoreDisposition::Recovery);
        assert_eq!(
            recovered.generation,
            Some(first.generation),
            "complete but never-visible generation must not be resurrected"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn orphan_pre_manifest_generation_is_cleaned_only_after_next_success() {
        let root = temp_root("orphan-cleanup");
        let (source, project) = source_and_project(&root);
        let first = commit(&source, &project, &BTreeMap::new()).expect("first commit");

        let orphan_generation = 2;
        let orphan_dir = generation_asset_dir_path(&source, orphan_generation).expect("orphan dir");
        fs::create_dir(&orphan_dir).expect("simulate crash after asset-dir creation");
        assert!(orphan_dir.exists());

        let next = commit(&source, &project, &BTreeMap::new()).expect("next complete commit");
        assert_eq!(
            next.generation, 3,
            "orphan generation identity must never be reused"
        );
        assert!(
            !orphan_dir.exists(),
            "stale generated orphan may be cleaned only after a newer complete save"
        );
        assert!(first.generation_manifest_path.exists());
        assert!(next.generation_manifest_path.exists());

        let third = commit(&source, &project, &BTreeMap::new()).expect("third complete commit");
        assert_eq!(third.generation, 4);
        assert!(
            !first.generation_manifest_path.exists(),
            "normal cleanup retains only current plus previous complete generation"
        );
        assert!(next.generation_manifest_path.exists());
        assert!(third.generation_manifest_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_open_upgrades_on_next_successful_save_without_losing_recovery() {
        let root = temp_root("legacy-upgrade");
        let (source, project) = source_and_project(&root);
        let visible = sidecar_path(&source).expect("visible path");
        let legacy_bytes = serde_json::to_vec_pretty(&project).expect("serialize legacy");
        fs::write(&visible, &legacy_bytes).expect("write legacy sidecar");

        let legacy = inspect(&source, project.source_hash)
            .expect("inspect legacy")
            .expect("legacy state");
        assert_eq!(legacy.disposition, StoreDisposition::Legacy);

        let receipt = commit(&source, &project, &BTreeMap::new()).expect("upgrade save");
        let current = inspect(&source, project.source_hash)
            .expect("inspect upgraded")
            .expect("upgraded state");
        assert_eq!(current.disposition, StoreDisposition::Current);
        assert_eq!(current.generation, Some(receipt.generation));

        fs::write(&receipt.visible_path, b"corrupt upgraded visible")
            .expect("corrupt upgraded visible");
        let recovered = inspect(&source, project.source_hash)
            .expect("recover legacy last-known-good")
            .expect("legacy recovery");
        assert_eq!(recovered.disposition, StoreDisposition::Legacy);
        assert_eq!(recovered.project, project);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_asset_closure_is_typed_and_never_empty() {
        let root = temp_root("asset-corruption");
        let (source, mut project) = source_and_project(&root);
        let bytes = [b"\x89PNG\r\n\x1a\n".as_slice(), b"required"].concat();
        let sha = digest_value(&bytes);
        project.assets.push(EditorProjectAsset {
            sha256: sha,
            mime: "image/png".to_owned(),
            byte_len: u64::try_from(bytes.len()).expect("bounded test asset"),
        });
        project.operations.push(EditOperation::ReplaceImage {
            node_id: serde_json::from_str("\"22000000-0000-4000-8000-000000000001\"")
                .expect("canonical NodeId"),
            before_asset: None,
            after_asset: sha,
        });
        let receipt = commit(&source, &project, &BTreeMap::from([(sha, bytes)]))
            .expect("asset project commit");

        let asset_name = project.assets[0].file_name().expect("asset file name");
        fs::write(receipt.generation_asset_dir.join(asset_name), b"corrupt")
            .expect("corrupt asset");

        assert!(
            inspect(&source, project.source_hash).is_err(),
            "corrupt current and same-generation recovery closure must fail typed, never become empty"
        );

        let _ = fs::remove_dir_all(root);
    }

    fn digest_value(bytes: &[u8]) -> Sha256Digest {
        let digest = Sha256::digest(bytes);
        let mut value = [0_u8; 32];
        value.copy_from_slice(&digest);
        Sha256Digest::from_bytes(value)
    }
}
