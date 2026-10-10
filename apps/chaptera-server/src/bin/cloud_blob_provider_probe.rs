use std::{
    env,
    io::Cursor,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aws_config::BehaviorVersion;
use chaptera_server::{
    blob_store::{
        BlobProvider, GrantOperation, ProviderErrorKind, ProviderGrantRequest,
    },
    s3_blob_provider::S3BlobProvider,
};
use rand::{rngs::OsRng, RngCore};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

const RECEIPT_V1: &str = "chaptera.cloud-blob-provider-receipt.v1";

#[derive(Serialize)]
struct ProviderReceiptV1 {
    protocol_version: &'static str,
    provider_family: &'static str,
    region: Option<String>,
    quarantine_bucket_label_sha256: String,
    private_bucket_label_sha256: String,
    expected_bucket_owner_configured: bool,
    payload_sha256: String,
    byte_len: u64,
    create_once_collision_rejected: bool,
    head_length_exact: bool,
    read_sha256_exact: bool,
    stale_read_rejected: bool,
    stale_delete_rejected: bool,
    delete_head_absent: bool,
    download_grant_available: bool,
    direct_upload_grant_available: bool,
    hard_create_only_reported: bool,
    hard_exact_or_max_upload_size_reported: bool,
    signed_content_type_reported: bool,
    strong_head_after_put_reported: bool,
    serve_list_denied: Option<bool>,
    worker_list_denied: Option<bool>,
    anonymous_access_denied: Option<bool>,
    cors_allowed_origin_only: Option<bool>,
    physical_delete_v0: Option<bool>,
    core_adapter_probe_passed: bool,
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok((receipt, passed)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&receipt)
                    .expect("serializing provider receipt cannot fail")
            );
            if !passed {
                std::process::exit(2);
            }
        }
        Err(message) => {
            eprintln!("chaptera cloud blob provider probe failed: {message}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<(ProviderReceiptV1, bool), String> {
    let quarantine_bucket = required_env("CHAPTERA_S3_QUARANTINE_BUCKET")?;
    let private_bucket = required_env("CHAPTERA_S3_PRIVATE_BUCKET")?;
    let expected_owner = optional_env("CHAPTERA_S3_EXPECTED_BUCKET_OWNER");
    let region = optional_env("AWS_REGION").or_else(|| optional_env("AWS_DEFAULT_REGION"));

    let shared_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = aws_sdk_s3::Client::new(&shared_config);
    let provider = S3BlobProvider::new(
        client,
        quarantine_bucket.clone(),
        private_bucket.clone(),
        expected_owner.clone(),
    )
    .map_err(|error| format!("provider config rejected: {}", error.code))?;

    let capabilities = provider.capabilities();
    let mut random = [0_u8; 16];
    OsRng.fill_bytes(&mut random);
    let suffix = hex(&random);
    let locator = format!("canonical/provider-receipt/{suffix}");

    let payload = format!("chaptera-provider-probe-v1:{suffix}").into_bytes();
    let payload_sha256 = sha256_hex(&payload);
    let byte_len = u64::try_from(payload.len()).map_err(|_| "payload length overflow")?;

    let created = provider
        .create_immutable(
            &locator,
            byte_len,
            Box::new(Cursor::new(payload.clone())),
        )
        .await
        .map_err(|error| format!("initial immutable create failed: {}", error.code))?;

    let collision_payload = format!("chaptera-provider-collision:{suffix}").into_bytes();
    let collision = provider
        .create_immutable(
            &locator,
            u64::try_from(collision_payload.len()).map_err(|_| "collision payload overflow")?,
            Box::new(Cursor::new(collision_payload)),
        )
        .await;
    let create_once_collision_rejected =
        matches!(collision, Err(ref error) if error.kind == ProviderErrorKind::AlreadyExists);

    let head_length_exact = matches!(
        provider.head_exact(&locator).await,
        Ok(Some(ref metadata))
            if metadata.byte_len == byte_len && metadata.generation == created.generation
    );

    let read_sha256_exact = match provider.open_read(&locator, &created.generation).await {
        Ok(mut reader) => {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await.is_ok()
                && bytes.len() == payload.len()
                && sha256_hex(&bytes) == payload_sha256
        }
        Err(_) => false,
    };

    let stale_generation = mutate_generation(&created.generation);
    let stale_read_rejected = matches!(
        provider.open_read(&locator, &stale_generation).await,
        Err(ref error) if matches!(error.kind, ProviderErrorKind::NotFound | ProviderErrorKind::AccessDenied)
    );
    let stale_delete_rejected = matches!(
        provider.delete_exact(&locator, &stale_generation).await,
        Err(ref error) if matches!(error.kind, ProviderErrorKind::NotFound | ProviderErrorKind::AccessDenied)
    );

    let download_grant_available = provider
        .issue_grant(&ProviderGrantRequest {
            tenant_id: "tenant:provider-probe".to_owned(),
            object_locator: locator.clone(),
            operation: GrantOperation::Download,
            expected_byte_len: Some(byte_len),
            required_content_type: None,
            expires_at_ms: now_ms()?.saturating_add(Duration::from_secs(60).as_millis() as u64),
        })
        .await
        .map(|grant| !grant.opaque_url.is_empty())
        .unwrap_or(false);

    let direct_upload_grant_available = provider
        .issue_grant(&ProviderGrantRequest {
            tenant_id: "tenant:provider-probe".to_owned(),
            object_locator: format!("quarantine/provider-receipt/{suffix}"),
            operation: GrantOperation::UploadCreateOnly,
            expected_byte_len: Some(byte_len),
            required_content_type: Some("application/octet-stream".to_owned()),
            expires_at_ms: now_ms()?.saturating_add(Duration::from_secs(60).as_millis() as u64),
        })
        .await
        .is_ok();

    let exact_delete_succeeded = provider
        .delete_exact(&locator, &created.generation)
        .await
        .is_ok();
    let delete_head_absent =
        exact_delete_succeeded && matches!(provider.head_exact(&locator).await, Ok(None));

    let core_adapter_probe_passed = create_once_collision_rejected
        && head_length_exact
        && read_sha256_exact
        && stale_read_rejected
        && stale_delete_rejected
        && delete_head_absent
        && download_grant_available
        && !direct_upload_grant_available;

    let receipt = ProviderReceiptV1 {
        protocol_version: RECEIPT_V1,
        provider_family: "s3-compatible",
        region,
        quarantine_bucket_label_sha256: sha256_hex(quarantine_bucket.as_bytes()),
        private_bucket_label_sha256: sha256_hex(private_bucket.as_bytes()),
        expected_bucket_owner_configured: expected_owner.is_some(),
        payload_sha256,
        byte_len,
        create_once_collision_rejected,
        head_length_exact,
        read_sha256_exact,
        stale_read_rejected,
        stale_delete_rejected,
        delete_head_absent,
        download_grant_available,
        direct_upload_grant_available,
        hard_create_only_reported: capabilities.hard_create_only,
        hard_exact_or_max_upload_size_reported: capabilities.hard_exact_or_max_upload_size,
        signed_content_type_reported: capabilities.signed_content_type,
        strong_head_after_put_reported: capabilities.strong_head_after_put,
        serve_list_denied: None,
        worker_list_denied: None,
        anonymous_access_denied: None,
        cors_allowed_origin_only: None,
        physical_delete_v0: None,
        core_adapter_probe_passed,
    };
    Ok((receipt, core_adapter_probe_passed))
}

fn required_env(name: &str) -> Result<String, String> {
    optional_env(name).ok_or_else(|| format!("{name} is required"))
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn mutate_generation(value: &str) -> String {
    let mut bytes = value.as_bytes().to_vec();
    if let Some(last) = bytes.last_mut() {
        *last = if *last == b'0' { b'1' } else { b'0' };
    }
    String::from_utf8(bytes).expect("generation tokens are ASCII")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex(&digest)
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").expect("writing hex into String cannot fail");
    }
    value
}

fn now_ms() -> Result<u64, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is before Unix epoch")?
        .as_millis();
    u64::try_from(millis).map_err(|_| "system time does not fit u64 milliseconds".to_owned())
}
