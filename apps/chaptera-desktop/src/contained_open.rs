use pub_reader::PubResolvedGraph;
use pub_viewer::ViewerGeometryDocument;

#[derive(Debug)]
pub struct DesktopContainedOpen {
    pub visual: ViewerGeometryDocument,
    pub resolved_graph: PubResolvedGraph,
}

#[cfg(not(target_os = "windows"))]
pub fn open_admitted_bytes(bytes: &[u8]) -> Result<DesktopContainedOpen, String> {
    let bundle = pub_viewer::open_pub_bundle(bytes, pub_viewer::viewer_geometry_environment_v0_1())
        .map_err(|error| format!("{error:#}"))?;
    Ok(DesktopContainedOpen {
        visual: bundle.geometry,
        resolved_graph: bundle.resolved_graph,
    })
}

#[cfg(target_os = "windows")]
mod windows {
    use super::DesktopContainedOpen;
    use chaptera_desktop_open_protocol::{
        DESKTOP_OPEN_REQUEST_SCHEMA_V1, DesktopOpenOutcomeV1, DesktopOpenRequestV1,
        DesktopOpenResponseV1, FrameKindV1, FrameLimitsV1, FrameV1, SourceIdentityV1,
        decode_control_json, decode_frames, encode_control_json, encode_frames,
        validate_response_source, validate_success_frames,
    };
    use chaptera_desktop_open_sandbox::{DEFAULT_WALL_TIMEOUT, launch_contained};
    use chaptera_untrusted_pub_scan::{DEFAULT_MAX_FILE_BYTES, SECURITY_PROFILE_V1};
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};

    const WORKER_FILE_NAME: &str = "chaptera-desktop-open-worker.exe";
    const MAX_IMAGE_BLOBS: u32 = 1024;

    #[derive(Debug, Clone)]
    struct AdmittedWorker {
        path: PathBuf,
        sha256_hex: String,
    }

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

    fn sha256_file(path: &Path) -> Result<String, String> {
        let mut file =
            fs::File::open(path).map_err(|error| format!("open worker {}: {error}", path.display()))?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| format!("read worker {}: {error}", path.display()))?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
        Ok(format!("{:x}", digest.finalize()))
    }

    fn admit_sibling_worker() -> Result<AdmittedWorker, String> {
        let current = std::env::current_exe()
            .map_err(|error| format!("resolve current desktop executable: {error}"))?;
        let current = fs::canonicalize(&current)
            .map_err(|error| format!("canonicalize desktop executable {}: {error}", current.display()))?;
        let parent = current
            .parent()
            .ok_or_else(|| "desktop executable has no parent directory".to_owned())?;
        let candidate = parent.join(WORKER_FILE_NAME);
        if !candidate.is_absolute() {
            return Err("contained worker path must be absolute".to_owned());
        }
        let canonical = fs::canonicalize(&candidate)
            .map_err(|error| format!("canonicalize contained worker {}: {error}", candidate.display()))?;
        if canonical.parent() != Some(parent) {
            return Err("contained worker escaped the desktop executable directory".to_owned());
        }
        let metadata = fs::metadata(&canonical)
            .map_err(|error| format!("stat contained worker {}: {error}", canonical.display()))?;
        if !metadata.is_file() {
            return Err("contained worker must be an existing file".to_owned());
        }
        let sha256_hex = sha256_file(&canonical)?;
        Ok(AdmittedWorker {
            path: canonical,
            sha256_hex,
        })
    }

    fn revalidate_worker(worker: &AdmittedWorker) -> Result<(), String> {
        let canonical = fs::canonicalize(&worker.path).map_err(|error| {
            format!(
                "canonicalize contained worker before launch {}: {error}",
                worker.path.display()
            )
        })?;
        if canonical != worker.path {
            return Err("contained worker canonical identity changed before launch".to_owned());
        }
        let sha256_hex = sha256_file(&canonical)?;
        if sha256_hex != worker.sha256_hex {
            return Err("contained worker bytes changed after admission".to_owned());
        }
        Ok(())
    }

    fn decode_success(
        response_bytes: &[u8],
        source: &SourceIdentityV1,
    ) -> Result<DesktopContainedOpen, String> {
        let frames = decode_frames(response_bytes, response_limits())
            .map_err(|error| format!("decode contained-worker response: {error}"))?;
        let control = frames
            .iter()
            .find(|frame| frame.kind == FrameKindV1::ControlJson && frame.key == "response")
            .ok_or_else(|| "contained worker response is missing control frame".to_owned())?;
        let response: DesktopOpenResponseV1 = decode_control_json(&control.payload)
            .map_err(|error| format!("decode contained-worker control: {error}"))?;
        validate_response_source(&response, source)
            .map_err(|error| format!("contained-worker source identity mismatch: {error}"))?;

        let payloads = match &response.outcome {
            DesktopOpenOutcomeV1::Success { payloads } => payloads,
            DesktopOpenOutcomeV1::Failure { code } => {
                return Err(format!("contained worker rejected PUB: {code:?}"));
            }
        };
        let payload_frames = frames
            .iter()
            .filter(|frame| frame.kind != FrameKindV1::ControlJson)
            .cloned()
            .collect::<Vec<_>>();
        validate_success_frames(payloads, &payload_frames, MAX_IMAGE_BLOBS)
            .map_err(|error| format!("contained-worker payload validation failed: {error}"))?;

        let mut viewer_json = None;
        let mut editor_graph_json = None;
        let mut image_blobs = BTreeMap::<String, Vec<u8>>::new();
        for frame in payload_frames {
            match frame.kind {
                FrameKindV1::ViewerJson => viewer_json = Some(frame.payload),
                FrameKindV1::EditorGraphJson => editor_graph_json = Some(frame.payload),
                FrameKindV1::ImageBlob => {
                    if image_blobs.insert(frame.key, frame.payload).is_some() {
                        return Err("contained worker returned duplicate image blob key".to_owned());
                    }
                }
                _ => return Err("contained worker returned an unexpected payload frame".to_owned()),
            }
        }

        let mut visual: ViewerGeometryDocument = serde_json::from_slice(
            &viewer_json.ok_or_else(|| "contained worker omitted Viewer JSON".to_owned())?,
        )
        .map_err(|error| format!("decode contained Viewer JSON: {error}"))?;
        let resolved_graph: PubResolvedGraph = serde_json::from_slice(
            &editor_graph_json
                .ok_or_else(|| "contained worker omitted resolved editor graph".to_owned())?,
        )
        .map_err(|error| format!("decode contained editor graph: {error}"))?;

        let mut expected_image_keys = BTreeSet::new();
        for image in &visual.images {
            expected_image_keys.insert(format!("resource:{}", image.resource_id.as_canonical()));
        }
        for key in image_blobs.keys() {
            if !expected_image_keys.contains(key) {
                return Err(format!(
                    "contained worker returned image blob for unknown resource key {key}"
                ));
            }
        }
        for image in &mut visual.images {
            let key = format!("resource:{}", image.resource_id.as_canonical());
            if let Some(bytes) = image_blobs.remove(&key) {
                image.bytes = bytes;
            }
        }
        if !image_blobs.is_empty() {
            return Err("contained worker left unmatched image blobs".to_owned());
        }

        if visual.document.source.byte_len != source.byte_len
            || visual.document.source.source_hash.to_string() != source.sha256_hex
        {
            return Err("contained Viewer source identity does not match admitted source".to_owned());
        }
        if resolved_graph.source.source_hash != visual.document.source.source_hash
            || resolved_graph.document.source_hash != visual.document.source.source_hash
        {
            return Err("contained editor graph source identity does not match Viewer".to_owned());
        }

        Ok(DesktopContainedOpen {
            visual,
            resolved_graph,
        })
    }

    pub(super) fn open_admitted_bytes(bytes: &[u8]) -> Result<DesktopContainedOpen, String> {
        let worker = admit_sibling_worker()?;
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
        .map_err(|error| format!("encode contained-open frames: {error}"))?;

        revalidate_worker(&worker)?;
        let output = launch_contained(&worker.path, &wire, DEFAULT_WALL_TIMEOUT)
            .map_err(|error| format!("establish Windows PUB sandbox: {error:#}"))?;
        if output.receipt.exit_code != 0 {
            return Err(format!(
                "contained PUB worker exited with code {}",
                output.receipt.exit_code
            ));
        }
        decode_success(&output.stdout, &source)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn production_worker_name_is_exact_and_not_path_searched() {
            assert_eq!(WORKER_FILE_NAME, "chaptera-desktop-open-worker.exe");
            assert!(!WORKER_FILE_NAME.contains('\\'));
            assert!(!WORKER_FILE_NAME.contains('/'));
        }
    }
}

#[cfg(target_os = "windows")]
pub fn open_admitted_bytes(bytes: &[u8]) -> Result<DesktopContainedOpen, String> {
    windows::open_admitted_bytes(bytes)
}
