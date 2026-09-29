use chaptera_process_launch::{BoundProgram, current_environment_allowlist};
use chaptera_update_engine::{UpdateEngine, UpdateError, UpdatePhase};
use chaptera_update_orchestrator::{OrchestrationError, UpdateOrchestrator};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

pub const CONTROL_REQUEST_SCHEMA_VERSION: &str = "chaptera.update-control-request.v1";
pub const CONTROL_RECEIPT_SCHEMA_VERSION: &str = "chaptera.update-control-receipt.v1";
pub const CONTROL_MODE_ARG: &str = "--chaptera-update-control";
const CONTROL_ENV_ALLOWLIST: &[&str] = &["SystemRoot", "WINDIR", "TEMP", "TMP"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlHandoffRequest {
    pub schema_version: String,
    pub install_root: PathBuf,
    pub transaction_id: String,
    pub candidate_version: String,
    pub updater_relative_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlReceipt {
    pub schema_version: String,
    pub transaction_id: String,
    pub pid: u32,
    pub executable: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedControlHandoff {
    pub control_updater: PathBuf,
    pub control_updater_sha256: String,
    pub request_path: PathBuf,
    pub working_directory: PathBuf,
}

impl PreparedControlHandoff {
    pub fn spawn(&self) -> Result<Child> {
        if !self.request_path.is_absolute() || !self.request_path.is_file() {
            return Err(HandoffError::ControlRequestMissing(self.request_path.clone()));
        }

        let program = BoundProgram::from_expected(
            self.control_updater.clone(),
            self.control_updater_sha256.clone(),
            self.working_directory.clone(),
        )
        .map_err(|_| HandoffError::ControlUpdaterIdentityChanged(self.control_updater.clone()))?;
        let mut command = program
            .command(current_environment_allowlist(CONTROL_ENV_ALLOWLIST))
            .map_err(|_| HandoffError::ControlUpdaterIdentityChanged(self.control_updater.clone()))?;
        command.arg(CONTROL_MODE_ARG).arg(&self.request_path);
        command.spawn().map_err(HandoffError::Io)
    }
}

#[derive(Debug)]
pub enum HandoffError {
    Io(io::Error),
    Json(serde_json::Error),
    Engine(UpdateError),
    Orchestration(OrchestrationError),
    JournalMissing,
    JournalNotPrepared(UpdatePhase),
    Schema(String),
    Mismatch(String),
    ControlUpdaterMissing(PathBuf),
    ControlUpdaterIdentityChanged(PathBuf),
    ControlRequestMissing(PathBuf),
    ControlWorkingDirectoryMissing(PathBuf),
    RequestAlreadyExists(PathBuf),
}

impl fmt::Display for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "handoff JSON error: {err}"),
            Self::Engine(err) => write!(f, "update engine error: {err}"),
            Self::Orchestration(err) => write!(f, "update orchestration error: {err}"),
            Self::JournalMissing => write!(f, "active update journal is missing"),
            Self::JournalNotPrepared(phase) => {
                write!(f, "control handoff requires Prepared journal, got {phase:?}")
            }
            Self::Schema(schema) => write!(f, "unsupported control request schema: {schema}"),
            Self::Mismatch(message) => write!(f, "control request mismatch: {message}"),
            Self::ControlUpdaterMissing(path) => {
                write!(f, "copied control updater is missing: {}", path.display())
            }
            Self::ControlUpdaterIdentityChanged(path) => {
                write!(f, "copied control updater identity changed before launch: {}", path.display())
            }
            Self::ControlRequestMissing(path) => {
                write!(f, "control request is missing or not absolute: {}", path.display())
            }
            Self::ControlWorkingDirectoryMissing(path) => {
                write!(f, "control working directory is missing or changed: {}", path.display())
            }
            Self::RequestAlreadyExists(path) => {
                write!(f, "control request already exists: {}", path.display())
            }
        }
    }
}

impl std::error::Error for HandoffError {}

impl From<io::Error> for HandoffError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for HandoffError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<UpdateError> for HandoffError {
    fn from(value: UpdateError) -> Self {
        Self::Engine(value)
    }
}

impl From<OrchestrationError> for HandoffError {
    fn from(value: OrchestrationError) -> Self {
        Self::Orchestration(value)
    }
}

pub type Result<T> = std::result::Result<T, HandoffError>;

pub fn spawn_preverified_candidate_handoff(
    orchestrator: &UpdateOrchestrator,
    transaction_id: &str,
    candidate_version: &str,
    candidate_source: &Path,
    updater_relative_path: &Path,
) -> Result<(PreparedControlHandoff, Child)> {
    let prepared = orchestrator.prepare_verified_candidate_for_handoff(
        transaction_id,
        candidate_version,
        candidate_source,
        updater_relative_path,
    )?;

    let handoff = match prepare_control_handoff(orchestrator.engine()) {
        Ok(handoff) => handoff,
        Err(error) => {
            // The Prepared transaction remains recoverable while the guard
            // still owns the install lock.
            drop(prepared);
            return Err(error);
        }
    };

    let child = match handoff.spawn() {
        Ok(child) => child,
        Err(error) => {
            drop(prepared);
            return Err(error);
        }
    };

    // Copied U1 is now running but blocked on the same install lock.
    // Releasing the front-door guard transfers mutation authority to it.
    drop(prepared);
    Ok((handoff, child))
}

pub fn prepare_control_handoff(engine: &UpdateEngine) -> Result<PreparedControlHandoff> {
    let journal = engine.read_journal()?.ok_or(HandoffError::JournalMissing)?;
    if journal.phase != UpdatePhase::Prepared {
        return Err(HandoffError::JournalNotPrepared(journal.phase));
    }

    let paths = engine.paths_for(&journal.transaction_id, &journal.updater_relative_path)?;
    if !paths.control_updater.is_file() {
        return Err(HandoffError::ControlUpdaterMissing(paths.control_updater));
    }

    let request_path = paths
        .staging_transaction
        .join("control")
        .join("handoff-request.json");
    if request_path.exists() {
        return Err(HandoffError::RequestAlreadyExists(request_path));
    }

    let request = ControlHandoffRequest {
        schema_version: CONTROL_REQUEST_SCHEMA_VERSION.to_owned(),
        install_root: engine.root().to_path_buf(),
        transaction_id: journal.transaction_id,
        candidate_version: journal.candidate_version,
        updater_relative_path: journal.updater_relative_path,
    };
    write_json_durable(&request_path, &request)?;

    let control_updater = fs::canonicalize(&paths.control_updater)
        .map_err(|_| HandoffError::ControlUpdaterMissing(paths.control_updater.clone()))?;
    if !control_updater.is_absolute() || !control_updater.is_file() {
        return Err(HandoffError::ControlUpdaterMissing(control_updater));
    }
    let control_updater_sha256 = sha256_file(&control_updater)?;

    let request_path = fs::canonicalize(&request_path)
        .map_err(|_| HandoffError::ControlRequestMissing(request_path.clone()))?;
    let working_directory = request_path
        .parent()
        .ok_or_else(|| HandoffError::ControlWorkingDirectoryMissing(request_path.clone()))
        .and_then(|parent| {
            fs::canonicalize(parent)
                .map_err(|_| HandoffError::ControlWorkingDirectoryMissing(parent.to_path_buf()))
        })?;

    Ok(PreparedControlHandoff {
        control_updater,
        control_updater_sha256,
        request_path,
        working_directory,
    })
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn apply_control_environment(command: &mut Command) {
    command.env_clear();
    for key in CONTROL_ENV_ALLOWLIST {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

pub fn read_control_request(path: &Path) -> Result<ControlHandoffRequest> {
    let request: ControlHandoffRequest = serde_json::from_slice(&fs::read(path)?)?;
    if request.schema_version != CONTROL_REQUEST_SCHEMA_VERSION {
        return Err(HandoffError::Schema(request.schema_version));
    }
    Ok(request)
}

pub fn validate_request_against_engine(
    request: &ControlHandoffRequest,
    engine: &UpdateEngine,
) -> Result<()> {
    if request.install_root != engine.root() {
        return Err(HandoffError::Mismatch("install_root".into()));
    }

    let journal = engine.read_journal()?.ok_or(HandoffError::JournalMissing)?;
    if journal.phase != UpdatePhase::Prepared {
        return Err(HandoffError::JournalNotPrepared(journal.phase));
    }
    if request.transaction_id != journal.transaction_id {
        return Err(HandoffError::Mismatch("transaction_id".into()));
    }
    if request.candidate_version != journal.candidate_version {
        return Err(HandoffError::Mismatch("candidate_version".into()));
    }
    if request.updater_relative_path != journal.updater_relative_path {
        return Err(HandoffError::Mismatch("updater_relative_path".into()));
    }

    let paths = engine.paths_for(&journal.transaction_id, &journal.updater_relative_path)?;
    if !paths.control_updater.is_file() {
        return Err(HandoffError::ControlUpdaterMissing(paths.control_updater));
    }
    Ok(())
}

pub fn started_path(request_path: &Path) -> PathBuf {
    request_path.with_extension("started")
}

pub fn receipt_path(request_path: &Path) -> PathBuf {
    request_path.with_extension("receipt.json")
}

pub fn write_control_receipt(path: &Path, receipt: &ControlReceipt) -> Result<()> {
    write_json_durable(path, receipt)
}

fn write_json_durable<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let next = path.with_extension("next");
    match fs::remove_file(&next) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&next)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);

    fs::rename(&next, path)?;
    sync_parent(path)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod launch_policy_tests {
    use super::*;

    #[test]
    fn control_environment_clears_hostile_resolution_and_tooling_variables() {
        let mut command = Command::new("chaptera-control-probe");
        command
            .env("PATH", "C:\\attacker")
            .env("PATHEXT", ".EXE;.BAT")
            .env("PYTHONPATH", "C:\\attacker\\python")
            .env("RUSTFLAGS", "-C linker=C:\\attacker\\link.exe")
            .env("CARGO_HOME", "C:\\attacker\\cargo")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("CHAPTERA_HOSTILE_PARENT", "present");

        apply_control_environment(&mut command);

        let environment = command
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))
            .collect::<std::collections::BTreeMap<_, _>>();

        for forbidden in [
            "PATH",
            "PATHEXT",
            "PYTHONPATH",
            "RUSTFLAGS",
            "CARGO_HOME",
            "HTTPS_PROXY",
            "CHAPTERA_HOSTILE_PARENT",
        ] {
            assert!(
                !environment.contains_key(std::ffi::OsStr::new(forbidden)),
                "{forbidden} must not survive env_clear"
            );
        }
        assert!(environment.keys().all(|key| CONTROL_ENV_ALLOWLIST
            .iter()
            .any(|allowed| key == std::ffi::OsStr::new(allowed))));
    }
}

