#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NegativeRequest {
    forbidden_file: PathBuf,
}

#[derive(Debug, Serialize)]
struct NegativeResult {
    schema_version: &'static str,
    forbidden_file_read_denied: bool,
    network_connect_denied: bool,
    child_process_spawn_denied: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("negative containment probe failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    std::io::stdin().take(1024 * 1024).read_to_end(&mut input)?;
    let request: NegativeRequest = serde_json::from_slice(&input)?;

    let forbidden_file_read_denied = std::fs::read(&request.forbidden_file).is_err();

    let address: SocketAddr = "1.1.1.1:80".parse()?;
    // With zero AppContainer network capabilities, Windows may reject Winsock
    // initialization itself before TcpStream can return an io::Error. Rust's
    // Windows std::net bootstrap currently asserts on that WSAStartup failure,
    // so contain that exact negative operation and treat either an io::Error or
    // the runtime-unavailable panic as proof that no TCP connection was created.
    let network_attempt =
        std::panic::catch_unwind(|| TcpStream::connect_timeout(&address, Duration::from_secs(2)));
    let network_connect_denied = !matches!(network_attempt, Ok(Ok(_)));

    let child_process_spawn_denied = Command::new("C:\\Windows\\System32\\cmd.exe")
        .args(["/C", "exit", "0"])
        .status()
        .is_err();

    let result = NegativeResult {
        schema_version: "chaptera.desktop-pub-containment-negative.v1",
        forbidden_file_read_denied,
        network_connect_denied,
        child_process_spawn_denied,
    };
    serde_json::to_writer(std::io::stdout(), &result)?;
    std::io::stdout().flush()?;

    if !result.forbidden_file_read_denied
        || !result.network_connect_denied
        || !result.child_process_spawn_denied
    {
        std::process::exit(3);
    }
    Ok(())
}
