use std::ffi::{OsStr, OsString};
use std::iter::Peekable;
use std::path::{Path, PathBuf};

#[cfg(feature = "reader-only")]
struct ReaderControlHooks;

#[cfg(feature = "reader-only")]
const READER_HEALTH_ENV_ALLOWLIST: &[&str] = &["SystemRoot", "WINDIR", "TEMP", "TMP"];

#[cfg(feature = "reader-only")]
#[derive(Debug, Clone)]
struct PreparedReaderHealthSmoke {
    program: PathBuf,
    working_directory: PathBuf,
    sha256: String,
    byte_len: usize,
}

#[cfg(feature = "reader-only")]
impl PreparedReaderHealthSmoke {
    fn prepare(current_tree: &Path) -> Result<Self, String> {
        let working_directory = std::fs::canonicalize(current_tree).map_err(|error| {
            format!(
                "canonicalize activated Reader tree {}: {error}",
                current_tree.display()
            )
        })?;
        if !working_directory.is_dir() {
            return Err(format!(
                "activated Reader tree is not a directory: {}",
                working_directory.display()
            ));
        }

        let executable_name = std::env::current_exe()
            .map_err(|error| format!("resolve control executable: {error}"))?
            .file_name()
            .ok_or_else(|| "control executable has no file name".to_owned())?
            .to_owned();
        let candidate = working_directory.join(executable_name);
        let program = std::fs::canonicalize(&candidate).map_err(|error| {
            format!(
                "canonicalize activated Reader {}: {error}",
                candidate.display()
            )
        })?;
        if !program.is_file() || program.parent() != Some(working_directory.as_path()) {
            return Err(format!(
                "activated Reader executable escaped the current tree: {}",
                program.display()
            ));
        }

        let bytes = std::fs::read(&program)
            .map_err(|error| format!("read activated Reader {}: {error}", program.display()))?;
        if bytes.is_empty() {
            return Err(format!(
                "activated Reader executable is empty: {}",
                program.display()
            ));
        }

        Ok(Self {
            program,
            working_directory,
            sha256: reader_health_sha256(&bytes),
            byte_len: bytes.len(),
        })
    }

    fn revalidate(&self) -> Result<(), String> {
        let current = std::fs::canonicalize(&self.program).map_err(|error| {
            format!(
                "canonicalize activated Reader before launch {}: {error}",
                self.program.display()
            )
        })?;
        if current != self.program
            || !current.is_file()
            || current.parent() != Some(self.working_directory.as_path())
        {
            return Err(format!(
                "activated Reader executable identity changed before launch: {}",
                self.program.display()
            ));
        }

        let bytes = std::fs::read(&current)
            .map_err(|error| format!("read activated Reader {}: {error}", current.display()))?;
        if bytes.len() != self.byte_len || reader_health_sha256(&bytes) != self.sha256 {
            return Err(format!(
                "activated Reader executable identity changed before launch: {}",
                self.program.display()
            ));
        }
        Ok(())
    }

    fn command(&self) -> Result<std::process::Command, String> {
        use std::process::{Command, Stdio};

        self.revalidate()?;

        let mut command = Command::new(&self.program);
        command
            .arg("--product-smoke-v1")
            .current_dir(&self.working_directory)
            .env_clear();
        for key in READER_HEALTH_ENV_ALLOWLIST {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("CHAPTERA_PRODUCT_SMOKE_BINARY_SHA256", &self.sha256)
            .env(
                "CHAPTERA_PRODUCT_SMOKE_BINARY_BYTE_LEN",
                self.byte_len.to_string(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Ok(command)
    }
}

#[cfg(feature = "reader-only")]
fn reader_health_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(feature = "reader-only")]
impl chaptera_update_orchestrator::UpdateHooks for ReaderControlHooks {
    fn quiesce(&mut self, _control_updater: &Path) -> std::result::Result<(), String> {
        // Ownership of the install lock proves the front-door U1 released its
        // mutation authority before copied U1 reaches this point. Product-level
        // process shutdown is deliberately a later slice.
        Ok(())
    }

    fn health_check(&mut self, current_tree: &Path) -> std::result::Result<(), String> {
        use std::thread;
        use std::time::{Duration, Instant};

        const HEALTH_TIMEOUT: Duration = Duration::from_secs(15);

        let prepared = PreparedReaderHealthSmoke::prepare(current_tree)?;
        let mut child = prepared
            .command()?
            .spawn()
            .map_err(|error| format!("launch activated Reader health smoke: {error}"))?;

        let deadline = Instant::now() + HEALTH_TIMEOUT;
        loop {
            match child
                .try_wait()
                .map_err(|error| format!("wait for activated Reader health smoke: {error}"))?
            {
                Some(status) if status.success() => return Ok(()),
                Some(status) => {
                    return Err(format!(
                        "activated Reader health smoke failed with status {status}"
                    ));
                }
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "activated Reader health smoke exceeded {} seconds",
                        HEALTH_TIMEOUT.as_secs()
                    ));
                }
            }
        }
    }
}

#[cfg(feature = "reader-only")]
fn run(request_path: &Path) -> Result<(), String> {
    let request = chaptera_update_handoff::read_control_request(request_path)
        .map_err(|error| error.to_string())?;
    let orchestrator = chaptera_update_orchestrator::UpdateOrchestrator::new(&request.install_root);
    chaptera_update_handoff::validate_request_against_engine(&request, orchestrator.engine())
        .map_err(|error| error.to_string())?;

    let _lock = chaptera_update_orchestrator::InstallLock::acquire(&request.install_root)
        .map_err(|error| error.to_string())?;
    // Revalidate after blocking lock acquisition: the request may have become
    // stale while copied U1 waited for its parent/front-door process to exit.
    chaptera_update_handoff::validate_request_against_engine(&request, orchestrator.engine())
        .map_err(|error| error.to_string())?;

    let receipt = chaptera_update_handoff::ControlReceipt {
        schema_version: chaptera_update_handoff::CONTROL_RECEIPT_SCHEMA_VERSION.to_owned(),
        transaction_id: request.transaction_id.clone(),
        pid: std::process::id(),
        executable: std::env::current_exe()
            .map_err(|error| format!("resolve control executable: {error}"))?,
    };
    chaptera_update_handoff::write_control_receipt(
        &chaptera_update_handoff::receipt_path(request_path),
        &receipt,
    )
    .map_err(|error| error.to_string())?;

    let mut hooks = ReaderControlHooks;
    orchestrator
        .continue_prepared_candidate(&mut hooks)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(not(feature = "reader-only"))]
fn run(_request_path: &Path) -> Result<(), String> {
    Err("update control mode is unavailable outside the Reader build".to_owned())
}

pub(crate) fn try_run_from_args<I>(
    args: &mut Peekable<I>,
    reader_only: bool,
) -> Result<bool, String>
where
    I: Iterator<Item = OsString>,
{
    if args.peek().map(OsString::as_os_str)
        != Some(OsStr::new(chaptera_update_handoff::CONTROL_MODE_ARG))
    {
        return Ok(false);
    }
    let _control_arg = args.next();

    if !reader_only {
        return Err("update control mode is reserved for the Chaptera Reader product".to_owned());
    }
    let request_path = args.next().map(PathBuf::from).ok_or_else(|| {
        "usage: chaptera-reader --chaptera-update-control HANDOFF-REQUEST.json".to_owned()
    })?;
    if args.next().is_some() {
        return Err("Reader update control mode accepts exactly one handoff request".to_owned());
    }

    run(&request_path).map_err(|error| format!("Reader update control failed: {error}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Peekable<impl Iterator<Item = OsString>> {
        values
            .iter()
            .map(|value| OsString::from(*value))
            .collect::<Vec<_>>()
            .into_iter()
            .peekable()
    }

    #[test]
    fn non_control_mode_is_not_consumed() {
        let mut input = args(&["--product-smoke-v1", "receipt.json"]);
        assert!(!try_run_from_args(&mut input, true).unwrap());
        assert_eq!(input.next(), Some(OsString::from("--product-smoke-v1")));
        assert_eq!(input.next(), Some(OsString::from("receipt.json")));
    }

    #[test]
    fn editor_build_rejects_reader_control_mode_before_request_io() {
        let mut input = args(&[
            chaptera_update_handoff::CONTROL_MODE_ARG,
            "handoff-request.json",
        ]);
        assert_eq!(
            try_run_from_args(&mut input, false).unwrap_err(),
            "update control mode is reserved for the Chaptera Reader product"
        );
    }

    #[test]
    fn reader_control_mode_requires_exactly_one_request_path() {
        let mut missing = args(&[chaptera_update_handoff::CONTROL_MODE_ARG]);
        assert_eq!(
            try_run_from_args(&mut missing, true).unwrap_err(),
            "usage: chaptera-reader --chaptera-update-control HANDOFF-REQUEST.json"
        );

        let mut extra = args(&[
            chaptera_update_handoff::CONTROL_MODE_ARG,
            "handoff-request.json",
            "extra",
        ]);
        assert_eq!(
            try_run_from_args(&mut extra, true).unwrap_err(),
            "Reader update control mode accepts exactly one handoff request"
        );
    }

    #[cfg(feature = "reader-only")]
    #[test]
    fn reader_health_smoke_command_uses_exact_program_cwd_and_bounded_environment() {
        let current_exe = std::env::current_exe().expect("current test executable");
        let current_tree = current_exe
            .parent()
            .expect("current test executable parent");
        let prepared =
            PreparedReaderHealthSmoke::prepare(current_tree).expect("prepare health smoke");
        let command = prepared.command().expect("build health smoke command");

        assert_eq!(command.get_program(), prepared.program.as_os_str());
        assert!(prepared.program.is_absolute());
        assert_eq!(
            command.get_current_dir(),
            Some(prepared.working_directory.as_path())
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("--product-smoke-v1")]
        );

        let environment = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            environment
                .get("CHAPTERA_PRODUCT_SMOKE_BINARY_SHA256")
                .and_then(Option::as_deref),
            Some(prepared.sha256.as_str())
        );
        assert_eq!(
            environment
                .get("CHAPTERA_PRODUCT_SMOKE_BINARY_BYTE_LEN")
                .and_then(Option::as_deref),
            Some(prepared.byte_len.to_string().as_str())
        );
        for forbidden in [
            "PATH",
            "PATHEXT",
            "PYTHONPATH",
            "RUSTUP_HOME",
            "CARGO_HOME",
            "USERPROFILE",
            "HOME",
        ] {
            assert!(
                !environment.contains_key(forbidden),
                "{forbidden} must not survive env_clear"
            );
        }
        assert!(environment.keys().all(|key| {
            READER_HEALTH_ENV_ALLOWLIST
                .iter()
                .any(|allowed| key.eq_ignore_ascii_case(allowed))
                || key == "CHAPTERA_PRODUCT_SMOKE_BINARY_SHA256"
                || key == "CHAPTERA_PRODUCT_SMOKE_BINARY_BYTE_LEN"
        }));
    }

    #[cfg(feature = "reader-only")]
    #[test]
    fn reader_health_smoke_rejects_executable_replacement_before_launch() {
        use std::io::Write;
        use std::time::{SystemTime, UNIX_EPOCH};

        let current_exe = std::env::current_exe().expect("current test executable");
        let executable_name = current_exe.file_name().expect("test executable file name");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "chaptera-reader-health-smoke-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create health-smoke temp directory");
        let copied = root.join(executable_name);
        std::fs::copy(&current_exe, &copied).expect("copy health-smoke executable");

        let prepared =
            PreparedReaderHealthSmoke::prepare(&root).expect("prepare copied executable");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&prepared.program)
            .expect("open copied executable for mutation");
        file.write_all(b"identity-drift")
            .expect("mutate copied executable");
        file.sync_all().expect("flush mutated executable");

        let error = prepared
            .revalidate()
            .expect_err("mutated executable must be rejected");
        assert!(error.contains("identity changed before launch"));

        drop(file);
        std::fs::remove_dir_all(&root).expect("remove health-smoke temp directory");
    }
}
