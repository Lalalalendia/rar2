use chaptera_untrusted_pub_scan::{
    PubScanPolicyV1, filesystem_default_deny_supported, inspect_pub_bytes_v1,
    install_post_read_filesystem_default_deny,
};
use serde::Serialize;
use std::{
    env,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
};

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is required"))
}

fn output_file() -> Result<BufWriter<File>, String> {
    let root = PathBuf::from(required_env("CHAPTERA_WORKER_OUTPUT_DIR")?);
    let path = root.join("result.json");
    File::create(&path)
        .map(BufWriter::new)
        .map_err(|error| format!("create {}: {error}", path.display()))
}

fn input_bytes() -> Result<Vec<u8>, String> {
    let path = PathBuf::from(required_env("CHAPTERA_WORKER_INPUT")?);
    fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))
}

fn write_json<T: Serialize>(mut output: BufWriter<File>, value: &T) -> Result<(), String> {
    serde_json::to_writer_pretty(&mut output, value)
        .map_err(|error| format!("serialize result: {error}"))?;
    output
        .write_all(b"\n")
        .map_err(|error| format!("write result newline: {error}"))?;
    output
        .flush()
        .map_err(|error| format!("flush result: {error}"))
}

pub fn run_inspect(
    max_file_bytes: u64,
    max_cfb_entries: u64,
    max_declared_stream_bytes: u64,
) -> Result<(), String> {
    let policy = PubScanPolicyV1 {
        max_file_bytes,
        max_cfb_entries,
        max_declared_stream_bytes,
    }
    .validate()?;

    let output = output_file()?;
    let bytes = input_bytes()?;

    if !filesystem_default_deny_supported() {
        return Err("post-read filesystem confinement is required for this worker".to_owned());
    }
    install_post_read_filesystem_default_deny()
        .map_err(|error| format!("install post-read filesystem sandbox: {error}"))?;

    let result = inspect_pub_bytes_v1(&bytes, policy, true);
    write_json(output, &result)
}
