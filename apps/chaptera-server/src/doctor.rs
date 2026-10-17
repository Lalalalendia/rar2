use crate::{runtime_error::RuntimeError, state::AppState};

pub fn render(state: &AppState) -> Result<String, RuntimeError> {
    serde_json::to_string_pretty(&state.readiness_report()).map_err(|error| {
        RuntimeError::new(
            "doctor_serialization_failed",
            format!("could not serialize readiness report: {error}"),
        )
    })
}

pub fn run(state: &AppState) -> Result<(), RuntimeError> {
    let report = state.readiness_report();
    let rendered = serde_json::to_string_pretty(&report).map_err(|error| {
        RuntimeError::new(
            "doctor_serialization_failed",
            format!("could not serialize readiness report: {error}"),
        )
    })?;

    println!("{rendered}");

    if report.ready {
        Ok(())
    } else {
        Err(RuntimeError::new(
            "runtime_not_ready",
            "one or more required runtime producers are not connected",
        ))
    }
}

// Operator diagnostics MUST query the already-running production process,
// never create RuntimePorts::unconfigured() and mistake that snapshot for
// the readiness of the instance managed by systemd.
const MAX_DOCTOR_REPLY: usize = 32 * 1024;
const DOCTOR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn live_error(code: &'static str) -> RuntimeError {
    RuntimeError::new(
        code,
        "live Reader doctor could not prove the running instance",
    )
}

async fn local_json(
    listener: std::net::SocketAddr,
    path: &'static str,
) -> Result<(u16, serde_json::Value), RuntimeError> {
    if !listener.ip().is_loopback() {
        return Err(live_error("doctor_listener_not_loopback"));
    }
    let raw = tokio::time::timeout(DOCTOR_TIMEOUT, async {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let mut connection = tokio::net::TcpStream::connect(listener).await?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {listener}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        );
        connection.write_all(request.as_bytes()).await?;
        let mut raw = Vec::new();
        connection
            .take((MAX_DOCTOR_REPLY + 1) as u64)
            .read_to_end(&mut raw)
            .await?;
        Ok::<Vec<u8>, std::io::Error>(raw)
    })
    .await
    .map_err(|_| live_error("doctor_live_timeout"))?
    .map_err(|_| live_error("doctor_live_transport_failed"))?;
    if raw.len() > MAX_DOCTOR_REPLY {
        return Err(live_error("doctor_reply_oversized"));
    }
    let header_end = raw
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .ok_or_else(|| live_error("doctor_reply_invalid"))?;
    let headers =
        std::str::from_utf8(&raw[..header_end]).map_err(|_| live_error("doctor_reply_invalid"))?;
    let mut lines = headers.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| live_error("doctor_reply_invalid"))?;
    let mut fields = status_line.split_whitespace();
    if !matches!(fields.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(live_error("doctor_reply_invalid"));
    }
    let status = fields
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| live_error("doctor_reply_invalid"))?;
    if !matches!(status, 200 | 503) {
        return Err(live_error("doctor_reply_status_invalid"));
    }
    let mut declared_length = None;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| live_error("doctor_reply_invalid"))?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(live_error("doctor_reply_encoding_not_supported"));
        }
        if name.eq_ignore_ascii_case("content-length") {
            if declared_length.is_some() {
                return Err(live_error("doctor_reply_duplicate_length"));
            }
            declared_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| live_error("doctor_reply_invalid_length"))?,
            );
        }
    }
    let body = &raw[header_end + 4..];
    if declared_length != Some(body.len()) {
        return Err(live_error("doctor_reply_length_mismatch"));
    }
    let json = serde_json::from_slice::<serde_json::Value>(body)
        .map_err(|_| live_error("doctor_reply_json_invalid"))?;
    Ok((status, json))
}

#[derive(Debug, PartialEq, Eq)]
struct LiveDoctorResult {
    runtime_ready: bool,
    release_match: bool,
}

async fn probe_live(
    listener: std::net::SocketAddr,
    expected_git_sha: &str,
) -> Result<LiveDoctorResult, RuntimeError> {
    if !listener.ip().is_loopback() {
        return Err(live_error("doctor_listener_not_loopback"));
    }
    let (ready_status, readiness) = local_json(listener, "/ready").await?;
    let (version_status, version) = local_json(listener, "/version").await?;
    let components = readiness
        .get("components")
        .and_then(serde_json::Value::as_object);
    // FirstVDS Guest-Reader needs actual opened RevisionStream and BlobStore.
    // Preserve the server's own required-ports policy for any other profile.
    let essential_ready = ["revision_stream", "blob_store"].iter().all(|name| {
        components
            .and_then(|items| items.get(*name))
            .is_some_and(|component| {
                component
                    .get("required")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                    && component.get("ready").and_then(serde_json::Value::as_bool) == Some(true)
            })
    });
    let runtime_ready = ready_status == 200
        && readiness.get("ready").and_then(serde_json::Value::as_bool) == Some(true)
        && readiness.get("status").and_then(serde_json::Value::as_str) == Some("ready")
        && essential_ready;
    let release_match = version_status == 200
        && version.get("git_sha").and_then(serde_json::Value::as_str) == Some(expected_git_sha)
        && version.get("version").and_then(serde_json::Value::as_str)
            == Some(crate::build_info::BUILD_IDENTITY);
    Ok(LiveDoctorResult {
        runtime_ready,
        release_match,
    })
}

/// Read-only host doctor: the loopback service answers with its real open
/// producer readiness and immutable executable identity. No env secret
/// resolution, SQL writes, restart, network egress or source byte reads.
pub async fn run_live(
    listener: std::net::SocketAddr,
    expected_git_sha: &str,
) -> Result<(), RuntimeError> {
    let result = probe_live(listener, expected_git_sha).await?;
    let ready = result.runtime_ready && result.release_match;
    let receipt = serde_json::json!({
        "schema": "chaptera.doctor-live.v1",
        "status": if ready { "ready" } else { "not_ready" },
        "ready": result.runtime_ready,
        "release_match": result.release_match,
        "loopback": true
    });
    println!("{receipt}");
    if ready {
        Ok(())
    } else {
        Err(live_error("doctor_live_not_ready"))
    }
}

#[cfg(test)]
mod live_doctor_tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    async fn server(ready: bool, git_sha: &'static str) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut incoming = [0; 1024];
                let read = stream.read(&mut incoming).await.unwrap();
                let request = String::from_utf8_lossy(&incoming[..read]);
                let (status, body) = if request.starts_with("GET /ready ") {
                    let report = serde_json::json!({
                        "status": if ready { "ready" } else { "not_ready" },
                        "ready": ready,
                        "components": {
                            "revision_stream": {"required": true, "ready": ready},
                            "blob_store": {"required": true, "ready": ready}
                        }
                    });
                    (
                        if ready {
                            "200 OK"
                        } else {
                            "503 Service Unavailable"
                        },
                        report,
                    )
                } else if request.starts_with("GET /version ") {
                    (
                        "200 OK",
                        serde_json::json!({
                            "version": crate::build_info::BUILD_IDENTITY,
                            "git_sha": git_sha
                        }),
                    )
                } else {
                    panic!("unexpected local Doctor route");
                };
                let encoded = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{encoded}",
                    encoded.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        addr
    }

    #[tokio::test]
    async fn live_doctor_proves_ready_running_instance_and_exact_release() {
        let address = server(true, crate::build_info::BUILD_GIT_SHA).await;
        let result = probe_live(address, crate::build_info::BUILD_GIT_SHA)
            .await
            .unwrap();
        assert_eq!(
            result,
            LiveDoctorResult {
                runtime_ready: true,
                release_match: true
            }
        );
    }

    #[tokio::test]
    async fn live_doctor_rejects_unready_or_stale_binary() {
        let unready = server(false, crate::build_info::BUILD_GIT_SHA).await;
        let result = probe_live(unready, crate::build_info::BUILD_GIT_SHA)
            .await
            .unwrap();
        assert!(!result.runtime_ready);
        assert!(result.release_match);
        let stale = server(true, "0000000000000000000000000000000000000000").await;
        let result = probe_live(stale, crate::build_info::BUILD_GIT_SHA)
            .await
            .unwrap();
        assert!(result.runtime_ready);
        assert!(!result.release_match);
    }

    #[tokio::test]
    async fn live_doctor_rejects_any_nonloopback_listener_without_connecting() {
        let address: std::net::SocketAddr = "10.0.0.1:8080".parse().unwrap();
        let error = probe_live(address, crate::build_info::BUILD_GIT_SHA)
            .await
            .unwrap_err();
        assert_eq!(error.code, "doctor_listener_not_loopback");
    }
}
