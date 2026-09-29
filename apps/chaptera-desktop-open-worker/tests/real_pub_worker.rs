use chaptera_desktop_open_protocol::{
    DESKTOP_OPEN_REQUEST_SCHEMA_V1, DesktopOpenOutcomeV1, DesktopOpenRequestV1,
    DesktopOpenResponseV1, FrameKindV1, FrameV1, SourceIdentityV1, decode_control_json,
    decode_frames, encode_control_json, encode_frames, validate_response_source,
    validate_success_frames,
};
use chaptera_desktop_open_worker::{
    MAX_IMAGE_BLOBS_V1, request_frame_limits_v1, response_frame_limits_v1,
};
use chaptera_untrusted_pub_scan::SECURITY_PROFILE_V1;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires pinned real PUB fixture from hosted workflow"]
fn real_pub_roundtrips_through_source_path_free_worker() {
    let fixture = std::env::var("CHAPTERA_SAMPLE_NEWSLETTER")
        .expect("CHAPTERA_SAMPLE_NEWSLETTER fixture path");
    let before = std::fs::read(&fixture).expect("read fixture");
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
                payload: encode_control_json(&request).unwrap(),
            },
            FrameV1 {
                kind: FrameKindV1::SourcePub,
                key: "source".to_owned(),
                payload: before.clone(),
            },
        ],
        request_frame_limits_v1(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_chaptera-desktop-open-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn worker");
    child.stdin.take().unwrap().write_all(&wire).unwrap();
    let output = child.wait_with_output().expect("worker output");
    assert!(
        output.status.success(),
        "worker failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let frames = decode_frames(&output.stdout, response_frame_limits_v1()).unwrap();
    let control = frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::ControlJson && frame.key == "response")
        .expect("response control");
    let response: DesktopOpenResponseV1 = decode_control_json(&control.payload).unwrap();
    validate_response_source(&response, &source).unwrap();

    let DesktopOpenOutcomeV1::Success { payloads } = &response.outcome else {
        panic!("real PUB worker response was not success");
    };
    let payload_frames = frames
        .iter()
        .filter(|frame| frame.kind != FrameKindV1::ControlJson)
        .cloned()
        .collect::<Vec<_>>();
    validate_success_frames(payloads, &payload_frames, MAX_IMAGE_BLOBS_V1).unwrap();

    let viewer = payload_frames
        .iter()
        .find(|frame| frame.kind == FrameKindV1::ViewerJson)
        .expect("viewer payload");
    let viewer_json: serde_json::Value = serde_json::from_slice(&viewer.payload).unwrap();
    assert_eq!(
        viewer_json["document"]["pages"].as_array().map(Vec::len),
        Some(4),
        "SampleNewsletter must retain the current 4-page Reader product profile"
    );

    let after = std::fs::read(&fixture).expect("re-read fixture");
    assert_eq!(SourceIdentityV1::from_bytes(&after), source);
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(&fixture),
        "worker response must not serialize the parent source path"
    );
}
