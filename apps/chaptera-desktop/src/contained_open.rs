#![forbid(unsafe_code)]

use chaptera_desktop_open_protocol::{
    CONTROL_JSON_MAX_BYTES_V1, DESKTOP_OPEN_REQUEST_SCHEMA_V1, DesktopOpenOutcomeV1,
    DesktopOpenRequestV1, DesktopOpenResponseV1, FrameKindV1, FrameLimitsV1, FrameV1,
    SourceIdentityV1, decode_control_json, decode_frames, encode_control_json, encode_frames,
    validate_response_source, validate_success_frames,
};
use chaptera_untrusted_pub_scan::{DEFAULT_MAX_FILE_BYTES, SECURITY_PROFILE_V1};
use pub_reader::PubResolvedGraph;
use pub_viewer::ViewerGeometryDocument;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;

const MAX_IMAGE_BLOBS_V1: u32 = 1024;
const MAX_HOST_STDERR_BYTES: u64 = 256 * 1024;

pub struct ContainedOpenBundle {
    pub visual: ViewerGeometryDocument,
    pub graph: PubResolvedGraph,
}

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

fn sibling_sandbox_host_path() -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .map_err(|error| format!("resolve current desktop executable: {error}"))?;
    let directory = current
        .parent()
        .ok_or_else(|| "desktop executable has no parent directory".to_owned())?;
    let host = directory.join("chaptera-desktop-open-sandbox-host.exe");
    if !host.is_file() {
        return Err("contained PUB sandbox host is unavailable".to_owned());
    }
    Ok(host)
}

pub fn open_admitted_source(bytes: &[u8]) -> Result<ContainedOpenBundle, String> {
    let host = sibling_sandbox_host_path()?;
    open_admitted_source_with_host(host, bytes)
}

fn open_admitted_source_with_host(
    host: PathBuf,
    bytes: &[u8],
) -> Result<ContainedOpenBundle, String> {
    let source = SourceIdentityV1::from_bytes(bytes);
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
                payload: encode_control_json(&request)
                    .map_err(|error| format!("encode contained-open request: {error}"))?,
            },
            FrameV1 {
                kind: FrameKindV1::SourcePub,
                key: "source".to_owned(),
                payload: bytes.to_vec(),
            },
        ],
        request_limits(),
    )
    .map_err(|error| format!("frame contained-open request: {error}"))?;

    let mut child = Command::new(&host)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start contained PUB sandbox host: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "sandbox host stdout pipe is unavailable".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "sandbox host stderr pipe is unavailable".to_owned())?;
    let response_limit = response_limits().max_total_bytes;
    let stdout_reader = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stdout
            .take(response_limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let stderr_reader = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stderr
            .take(MAX_HOST_STDERR_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    });

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "sandbox host stdin pipe is unavailable".to_owned())?;
        stdin
            .write_all(&wire)
            .map_err(|error| format!("write contained-open request: {error}"))?;
    }

    let status = child
        .wait()
        .map_err(|error| format!("wait for contained PUB sandbox host: {error}"))?;
    let stdout = stdout_reader
        .join()
        .map_err(|_| "sandbox host stdout reader panicked".to_owned())?
        .map_err(|error| format!("read sandbox host stdout: {error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "sandbox host stderr reader panicked".to_owned())?
        .map_err(|error| format!("read sandbox host stderr: {error}"))?;

    if u64::try_from(stdout.len()).unwrap_or(u64::MAX) > response_limit {
        return Err("contained PUB sandbox response exceeded bounded wire limit".to_owned());
    }
    if u64::try_from(stderr.len()).unwrap_or(u64::MAX) > MAX_HOST_STDERR_BYTES {
        return Err("contained PUB sandbox host stderr exceeded bounded capture limit".to_owned());
    }
    if !status.success() {
        let diagnostic = String::from_utf8_lossy(&stderr);
        return Err(format!(
            "contained PUB sandbox host failed with {}: {}",
            status,
            diagnostic.trim()
        ));
    }

    let frames = decode_frames(&stdout, response_limits())
        .map_err(|error| format!("decode contained-open response: {error}"))?;
    let control = frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::ControlJson && frame.key == "response")
        .ok_or_else(|| "contained-open response omitted control frame".to_owned())?;
    let response: DesktopOpenResponseV1 = decode_control_json(&control.payload)
        .map_err(|error| format!("decode contained-open control: {error}"))?;
    validate_response_source(&response, &source)
        .map_err(|error| format!("contained-open source identity mismatch: {error}"))?;

    let payloads = match &response.outcome {
        DesktopOpenOutcomeV1::Success { payloads } => payloads,
        DesktopOpenOutcomeV1::Failure { code } => {
            return Err(format!("contained PUB worker rejected source: {code:?}"));
        }
    };
    let payload_frames = frames
        .iter()
        .filter(|frame| frame.kind != FrameKindV1::ControlJson)
        .cloned()
        .collect::<Vec<_>>();
    validate_success_frames(payloads, &payload_frames, MAX_IMAGE_BLOBS_V1)
        .map_err(|error| format!("validate contained-open payloads: {error}"))?;

    let viewer_frame = payload_frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::ViewerJson && frame.key == "viewer")
        .ok_or_else(|| "contained-open response omitted Viewer JSON".to_owned())?;
    let graph_frame = payload_frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::EditorGraphJson && frame.key == "editor_graph")
        .ok_or_else(|| "contained-open response omitted editor graph".to_owned())?;

    let mut visual: ViewerGeometryDocument = serde_json::from_slice(&viewer_frame.payload)
        .map_err(|error| format!("parse contained Viewer JSON: {error}"))?;
    let graph: PubResolvedGraph = serde_json::from_slice(&graph_frame.payload)
        .map_err(|error| format!("parse contained editor graph: {error}"))?;

    let mut blobs = payload_frames
        .iter()
        .filter(|frame| frame.kind == FrameKindV1::ImageBlob)
        .map(|frame| (frame.key.clone(), frame.payload.clone()))
        .collect::<BTreeMap<_, _>>();
    for image in &mut visual.images {
        let key = format!("resource:{}", image.resource_id.as_canonical());
        image.bytes = blobs
            .remove(&key)
            .ok_or_else(|| format!("contained-open image blob missing for {key}"))?;
    }
    if let Some((key, _)) = blobs.into_iter().next() {
        return Err(format!("contained-open returned unbound image blob {key}"));
    }

    if visual.document.source.byte_len != source.byte_len {
        return Err("contained Viewer source byte length changed".to_owned());
    }

    Ok(ContainedOpenBundle { visual, graph })
}
