#![forbid(unsafe_code)]

use anyhow::{Context, Result, anyhow, bail};
use chaptera_desktop_open_protocol::{
    DESKTOP_OPEN_REQUEST_SCHEMA_V1, DesktopOpenOutcomeV1, DesktopOpenRequestV1,
    DesktopOpenResponseV1, FrameKindV1, FrameLimitsV1, FrameV1, SourceIdentityV1,
    decode_control_json, decode_frames, encode_control_json, encode_frames,
    validate_response_source, validate_success_frames,
};
use chaptera_desktop_open_sandbox::{DEFAULT_WALL_TIMEOUT, launch_contained};
use chaptera_untrusted_pub_scan::{DEFAULT_MAX_FILE_BYTES, SECURITY_PROFILE_V1};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

const MAX_IMAGE_BLOBS: u32 = 1024;

fn request_limits() -> FrameLimitsV1 {
    FrameLimitsV1 {
        max_key_bytes: 128,
        max_frame_bytes: DEFAULT_MAX_FILE_BYTES,
        max_total_bytes: DEFAULT_MAX_FILE_BYTES + 2 * 1024 * 1024,
        max_frames: 3,
    }
}

fn response_limits() -> FrameLimitsV1 {
    FrameLimitsV1 {
        max_key_bytes: 128,
        max_frame_bytes: DEFAULT_MAX_FILE_BYTES,
        max_total_bytes: DEFAULT_MAX_FILE_BYTES,
        max_frames: MAX_IMAGE_BLOBS + 3,
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("contained worker acceptance failed: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args_os();
    let _program = args.next();
    let worker = PathBuf::from(args.next().ok_or_else(|| anyhow!("missing worker path"))?);
    let fixture = PathBuf::from(args.next().ok_or_else(|| anyhow!("missing fixture path"))?);
    let receipt_path = PathBuf::from(args.next().ok_or_else(|| anyhow!("missing receipt path"))?);
    if args.next().is_some() {
        bail!("unexpected arguments");
    }

    let before = fs::read(&fixture).with_context(|| format!("read {}", fixture.display()))?;
    let source = SourceIdentityV1::from_bytes(&before);
    let request = DesktopOpenRequestV1 {
        schema_version: DESKTOP_OPEN_REQUEST_SCHEMA_V1.to_owned(),
        security_profile: SECURITY_PROFILE_V1.to_owned(),
        source: source.clone(),
        source_frame_key: "source".to_owned(),
    };
    let wire = encode_frames(
        &[
            FrameV1 {
                kind: FrameKindV1::ControlJson,
                key: "request".to_owned(),
                payload: encode_control_json(&request).context("encode request control")?,
            },
            FrameV1 {
                kind: FrameKindV1::SourcePub,
                key: "source".to_owned(),
                payload: before.clone(),
            },
        ],
        request_limits(),
    )
    .context("encode contained-worker request")?;

    let output = launch_contained(&worker, &wire, DEFAULT_WALL_TIMEOUT)
        .context("launch contained worker")?;
    if output.receipt.exit_code != 0 {
        bail!(
            "contained worker exited with {}: {}",
            output.receipt.exit_code,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let frames = decode_frames(&output.stdout, response_limits())
        .context("decode contained-worker response")?;
    let control = frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::ControlJson && frame.key == "response")
        .ok_or_else(|| anyhow!("missing response control frame"))?;
    let response: DesktopOpenResponseV1 =
        decode_control_json(&control.payload).context("decode response control")?;
    validate_response_source(&response, &source).context("response source identity")?;

    let DesktopOpenOutcomeV1::Success { payloads } = &response.outcome else {
        bail!("contained worker returned typed failure for pinned real PUB");
    };
    let payload_frames = frames
        .iter()
        .filter(|frame| frame.kind != FrameKindV1::ControlJson)
        .cloned()
        .collect::<Vec<_>>();
    validate_success_frames(payloads, &payload_frames, MAX_IMAGE_BLOBS)
        .context("validate contained-worker payload descriptors")?;

    let viewer = payload_frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::ViewerJson)
        .ok_or_else(|| anyhow!("missing Viewer JSON frame"))?;
    let viewer_json: serde_json::Value =
        serde_json::from_slice(&viewer.payload).context("parse Viewer JSON")?;
    let page_count = viewer_json["document"]["pages"]
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| anyhow!("Viewer JSON missing pages"))?;
    if page_count != 4 {
        bail!("pinned SampleNewsletter page count drifted: {page_count}");
    }

    let after = fs::read(&fixture).context("re-read pinned source")?;
    if SourceIdentityV1::from_bytes(&after) != source {
        bail!("contained worker mutated source");
    }

    let receipt = json!({
        "schema_version": "chaptera.desktop-pub-containment-acceptance.v1",
        "source_sha256": source.sha256_hex,
        "source_byte_len": source.byte_len,
        "source_unchanged": true,
        "viewer_page_count": page_count,
        "sandbox": output.receipt,
    });
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).context("create receipt directory")?;
    }
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", receipt_path.display()))?;
    Ok(())
}
