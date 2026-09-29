#![forbid(unsafe_code)]

use anyhow::{Context, Result, anyhow};
use chaptera_desktop_open_protocol::{
    CONTROL_JSON_MAX_BYTES_V1, DESKTOP_OPEN_RESPONSE_SCHEMA_V1, DesktopOpenOutcomeV1,
    DesktopOpenRequestV1, DesktopOpenResponseV1, DesktopOpenSuccessV1, FrameKindV1,
    FrameLimitsV1, FrameV1, PayloadDescriptorV1, SourceIdentityV1, WorkerFailureCodeV1,
    decode_control_json, decode_frames, encode_control_json, encode_frames,
    validate_request_frames, validate_success_frames,
};
use chaptera_untrusted_pub_scan::{DEFAULT_MAX_FILE_BYTES, SECURITY_PROFILE_V1};

pub const MAX_IMAGE_BLOBS_V1: u32 = 1024;

pub fn request_frame_limits_v1() -> FrameLimitsV1 {
    FrameLimitsV1 {
        max_key_bytes: 128,
        max_frame_bytes: DEFAULT_MAX_FILE_BYTES,
        max_total_bytes: DEFAULT_MAX_FILE_BYTES
            + u64::try_from(CONTROL_JSON_MAX_BYTES_V1).expect("control limit fits u64")
            + 64 * 1024,
        max_frames: 3,
    }
}

pub fn response_frame_limits_v1() -> FrameLimitsV1 {
    FrameLimitsV1 {
        max_key_bytes: 128,
        max_frame_bytes: DEFAULT_MAX_FILE_BYTES,
        max_total_bytes: DEFAULT_MAX_FILE_BYTES,
        max_frames: MAX_IMAGE_BLOBS_V1 + 3,
    }
}

pub fn request_wire_max_bytes_v1() -> u64 {
    request_frame_limits_v1().max_total_bytes
}

fn encode_failure(
    source: SourceIdentityV1,
    code: WorkerFailureCodeV1,
) -> Result<Vec<u8>> {
    let response = DesktopOpenResponseV1 {
        schema_version: DESKTOP_OPEN_RESPONSE_SCHEMA_V1.to_owned(),
        source,
        outcome: DesktopOpenOutcomeV1::Failure { code },
    };
    let control = FrameV1 {
        kind: FrameKindV1::ControlJson,
        key: "response".to_owned(),
        payload: encode_control_json(&response).context("encode failure control")?,
    };
    encode_frames(&[control], response_frame_limits_v1())
        .context("encode failure response frames")
}

pub fn process_wire_v1(input: &[u8]) -> Result<Vec<u8>> {
    let frames = decode_frames(input, request_frame_limits_v1())
        .context("decode desktop-open request frames")?;

    let mut control = None;
    let mut source_frames = Vec::new();
    for frame in frames {
        match frame.kind {
            FrameKindV1::ControlJson if frame.key == "request" => {
                if control.replace(frame).is_some() {
                    return Err(anyhow!("duplicate request control frame"));
                }
            }
            FrameKindV1::SourcePub => source_frames.push(frame),
            _ => return Err(anyhow!("unexpected request frame")),
        }
    }

    let control = control.ok_or_else(|| anyhow!("missing request control frame"))?;
    let request: DesktopOpenRequestV1 =
        decode_control_json(&control.payload).context("decode desktop-open request control")?;
    validate_request_frames(&request, &source_frames)
        .context("validate desktop-open source identity")?;

    if request.security_profile != SECURITY_PROFILE_V1 {
        return encode_failure(request.source, WorkerFailureCodeV1::RejectedByInputPolicy);
    }

    let source_bytes = &source_frames[0].payload;
    let bundle = match pub_viewer::open_pub_bundle(
        source_bytes,
        pub_viewer::viewer_geometry_environment_v0_1(),
    ) {
        Ok(bundle) => bundle,
        Err(_) => {
            return encode_failure(request.source, WorkerFailureCodeV1::ViewerOpenFailed);
        }
    };

    let mut payload_frames = Vec::new();
    let viewer = FrameV1 {
        kind: FrameKindV1::ViewerJson,
        key: "viewer".to_owned(),
        payload: serde_json::to_vec(&bundle.geometry).context("serialize Viewer geometry")?,
    };
    let graph = FrameV1 {
        kind: FrameKindV1::EditorGraphJson,
        key: "editor_graph".to_owned(),
        payload: serde_json::to_vec(&bundle.resolved_graph)
            .context("serialize resolved editor graph")?,
    };
    payload_frames.push(viewer);
    payload_frames.push(graph);

    for image in &bundle.geometry.images {
        if image.bytes.is_empty() {
            continue;
        }
        payload_frames.push(FrameV1 {
            kind: FrameKindV1::ImageBlob,
            key: format!("resource:{}", image.resource_id.as_canonical()),
            payload: image.bytes.clone(),
        });
    }

    if u32::try_from(payload_frames.len().saturating_sub(2)).unwrap_or(u32::MAX)
        > MAX_IMAGE_BLOBS_V1
    {
        return encode_failure(request.source, WorkerFailureCodeV1::ResourceLimit);
    }

    let success = DesktopOpenSuccessV1 {
        viewer_json: PayloadDescriptorV1::from_frame(&payload_frames[0]),
        editor_graph_json: PayloadDescriptorV1::from_frame(&payload_frames[1]),
        image_blobs: payload_frames[2..]
            .iter()
            .map(PayloadDescriptorV1::from_frame)
            .collect(),
    };
    validate_success_frames(&success, &payload_frames, MAX_IMAGE_BLOBS_V1)
        .context("validate desktop-open success payloads")?;

    let response = DesktopOpenResponseV1 {
        schema_version: DESKTOP_OPEN_RESPONSE_SCHEMA_V1.to_owned(),
        source: request.source.clone(),
        outcome: DesktopOpenOutcomeV1::Success { payloads: success },
    };
    let control = FrameV1 {
        kind: FrameKindV1::ControlJson,
        key: "response".to_owned(),
        payload: encode_control_json(&response).context("encode success control")?,
    };

    let mut response_frames = Vec::with_capacity(payload_frames.len() + 1);
    response_frames.push(control);
    response_frames.extend(payload_frames);
    match encode_frames(&response_frames, response_frame_limits_v1()) {
        Ok(wire) => Ok(wire),
        Err(_) => encode_failure(request.source, WorkerFailureCodeV1::ResourceLimit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chaptera_desktop_open_protocol::{
        DESKTOP_OPEN_REQUEST_SCHEMA_V1, DesktopOpenOutcomeV1, FrameKindV1, FrameV1,
        SourceIdentityV1,
    };

    fn request_wire(source: Vec<u8>, security_profile: &str) -> Vec<u8> {
        let request = DesktopOpenRequestV1 {
            schema_version: DESKTOP_OPEN_REQUEST_SCHEMA_V1.to_owned(),
            security_profile: security_profile.to_owned(),
            source: SourceIdentityV1::from_bytes(&source),
            source_frame_key: "source".to_owned(),
        };
        let frames = vec![
            FrameV1 {
                kind: FrameKindV1::ControlJson,
                key: "request".to_owned(),
                payload: encode_control_json(&request).unwrap(),
            },
            FrameV1 {
                kind: FrameKindV1::SourcePub,
                key: "source".to_owned(),
                payload: source,
            },
        ];
        encode_frames(&frames, request_frame_limits_v1()).unwrap()
    }

    fn decode_response(wire: &[u8]) -> DesktopOpenResponseV1 {
        let frames = decode_frames(wire, response_frame_limits_v1()).unwrap();
        let control = frames
            .iter()
            .find(|frame| frame.kind == FrameKindV1::ControlJson && frame.key == "response")
            .unwrap();
        decode_control_json(&control.payload).unwrap()
    }

    #[test]
    fn unsupported_source_returns_typed_source_free_failure() {
        let wire = process_wire_v1(&request_wire(b"not a PUB".to_vec(), SECURITY_PROFILE_V1))
            .expect("worker response");
        let response = decode_response(&wire);
        assert!(matches!(
            response.outcome,
            DesktopOpenOutcomeV1::Failure {
                code: WorkerFailureCodeV1::ViewerOpenFailed
            }
        ));
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("not a PUB"));
        assert!(!json.contains("source_path"));
    }

    #[test]
    fn wrong_security_profile_is_rejected_before_parse() {
        let wire = process_wire_v1(&request_wire(b"not a PUB".to_vec(), "other-profile"))
            .expect("worker response");
        let response = decode_response(&wire);
        assert!(matches!(
            response.outcome,
            DesktopOpenOutcomeV1::Failure {
                code: WorkerFailureCodeV1::RejectedByInputPolicy
            }
        ));
    }
}
