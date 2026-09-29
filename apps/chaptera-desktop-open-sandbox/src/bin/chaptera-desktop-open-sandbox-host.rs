#![forbid(unsafe_code)]

use anyhow::{Context, Result, anyhow, bail};
use chaptera_desktop_open_protocol::{
    CONTROL_JSON_MAX_BYTES_V1, DesktopOpenOutcomeV1, DesktopOpenRequestV1, DesktopOpenResponseV1,
    FrameKindV1, FrameLimitsV1, SourceIdentityV1, decode_control_json, decode_frames,
    validate_request_frames, validate_response_source, validate_success_frames,
};
use chaptera_desktop_open_sandbox::{DEFAULT_WALL_TIMEOUT, launch_contained};
use chaptera_untrusted_pub_scan::DEFAULT_MAX_FILE_BYTES;
use std::io::{Read, Write};
use std::path::PathBuf;

const MAX_IMAGE_BLOBS_V1: u32 = 1024;

fn request_limits() -> FrameLimitsV1 {
    FrameLimitsV1 {
        max_key_bytes: 128,
        max_frame_bytes: DEFAULT_MAX_FILE_BYTES,
        max_total_bytes: DEFAULT_MAX_FILE_BYTES
            + u64::try_from(CONTROL_JSON_MAX_BYTES_V1).expect("control limit fits u64")
            + 64 * 1024,
        max_frames: 3,
    }
}

fn response_limits() -> FrameLimitsV1 {
    FrameLimitsV1 {
        max_key_bytes: 128,
        max_frame_bytes: DEFAULT_MAX_FILE_BYTES,
        max_total_bytes: DEFAULT_MAX_FILE_BYTES,
        max_frames: MAX_IMAGE_BLOBS_V1 + 3,
    }
}

fn sibling_worker_path() -> Result<PathBuf> {
    let current = std::env::current_exe().context("resolve sandbox host executable")?;
    let directory = current
        .parent()
        .ok_or_else(|| anyhow!("sandbox host executable has no parent directory"))?;
    let worker = directory.join("chaptera-desktop-open-worker.exe");
    if !worker.is_file() {
        bail!("contained PUB worker sibling is unavailable");
    }
    Ok(worker)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("chaptera desktop-open sandbox host failed closed: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    if std::env::args_os().nth(1).is_some() {
        bail!("sandbox host accepts no arguments");
    }

    let limits = request_limits();
    let mut request_wire = Vec::new();
    std::io::stdin()
        .lock()
        .take(limits.max_total_bytes.saturating_add(1))
        .read_to_end(&mut request_wire)
        .context("read desktop-open request")?;
    if u64::try_from(request_wire.len()).unwrap_or(u64::MAX) > limits.max_total_bytes {
        bail!("desktop-open request exceeds bounded wire limit");
    }

    let request_frames =
        decode_frames(&request_wire, limits).context("decode desktop-open request frames")?;
    let control = request_frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::ControlJson && frame.key == "request")
        .ok_or_else(|| anyhow!("desktop-open request omitted control frame"))?;
    let request: DesktopOpenRequestV1 =
        decode_control_json(&control.payload).context("decode desktop-open request control")?;
    let source_frames = request_frames
        .iter()
        .filter(|frame| frame.kind == FrameKindV1::SourcePub)
        .cloned()
        .collect::<Vec<_>>();
    validate_request_frames(&request, &source_frames)
        .context("validate desktop-open request source identity")?;
    let expected_source = SourceIdentityV1 {
        sha256_hex: request.source.sha256_hex.clone(),
        byte_len: request.source.byte_len,
    };

    let worker = sibling_worker_path()?;
    let output = launch_contained(&worker, &request_wire, DEFAULT_WALL_TIMEOUT)
        .context("launch contained desktop-open worker")?;
    if output.receipt.exit_code != 0 {
        bail!("contained desktop-open worker exited unsuccessfully");
    }

    let response_frames =
        decode_frames(&output.stdout, response_limits()).context("decode worker response frames")?;
    let response_control = response_frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::ControlJson && frame.key == "response")
        .ok_or_else(|| anyhow!("worker response omitted control frame"))?;
    let response: DesktopOpenResponseV1 = decode_control_json(&response_control.payload)
        .context("decode worker response control")?;
    validate_response_source(&response, &expected_source).context("worker response source identity")?;

    if let DesktopOpenOutcomeV1::Success { payloads } = &response.outcome {
        let payload_frames = response_frames
            .iter()
            .filter(|frame| frame.kind != FrameKindV1::ControlJson)
            .cloned()
            .collect::<Vec<_>>();
        validate_success_frames(payloads, &payload_frames, MAX_IMAGE_BLOBS_V1)
            .context("validate worker response payload descriptors")?;
    }

    std::io::stdout()
        .lock()
        .write_all(&output.stdout)
        .context("write desktop-open response")?;
    Ok(())
}
