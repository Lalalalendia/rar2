use std::ffi::{OsStr, OsString};
use std::iter::Peekable;
use std::path::{Path, PathBuf};

#[cfg(feature = "reader-only")]
struct ReaderControlHooks;

#[cfg(feature = "reader-only")]
impl chaptera_update_orchestrator::UpdateHooks for ReaderControlHooks {
    fn quiesce(&mut self, _control_updater: &Path) -> std::result::Result<(), String> {
        // Ownership of the install lock proves the front-door U1 released its
        // mutation authority before copied U1 reaches this point. Product-level
        // process shutdown is deliberately a later slice.
        Ok(())
    }

    fn health_check(&mut self, current_tree: &Path) -> std::result::Result<(), String> {
        use sha2::{Digest, Sha256};
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        const HEALTH_TIMEOUT: Duration = Duration::from_secs(15);

        let candidate = current_tree.join(
            std::env::current_exe()
                .map_err(|error| format!("resolve control executable: {error}"))?
                .file_name()
                .ok_or_else(|| "control executable has no file name".to_owned())?,
        );
        let bytes = std::fs::read(&candidate)
            .map_err(|error| format!("read activated Reader {}: {error}", candidate.display()))?;
        if bytes.is_empty() {
            return Err(format!(
                "activated Reader executable is empty: {}",
                candidate.display()
            ));
        }
        let sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        let mut child = Command::new(&candidate)
            .arg("--product-smoke-v1")
            .env("CHAPTERA_PRODUCT_SMOKE_BINARY_SHA256", sha256)
            .env(
                "CHAPTERA_PRODUCT_SMOKE_BINARY_BYTE_LEN",
                bytes.len().to_string(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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
    let request_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| {
            "usage: chaptera-reader --chaptera-update-control HANDOFF-REQUEST.json".to_owned()
        })?;
    if args.next().is_some() {
        return Err(
            "Reader update control mode accepts exactly one handoff request".to_owned(),
        );
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
}
