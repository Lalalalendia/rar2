use std::{
    env,
    io::Cursor,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aws_config::BehaviorVersion;
use chaptera_server::{
    blob_store::{
        BlobProvider, GrantOperation, ProviderError, ProviderErrorKind, ProviderGrant,
        ProviderGrantRequest,
    },
    s3_blob_provider::S3BlobProvider,
};
use rand::{RngCore, rngs::OsRng};
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
    owner_pinned_download_grant_denied: bool,
    download_grant_policy_passed: bool,
    direct_upload_grant_available: bool,
    hard_create_only_reported: bool,
    hard_exact_or_max_upload_size_reported: bool,
    signed_content_type_reported: bool,
    strong_head_after_put_reported: bool,
    serve_list_denied: Option<bool>,
    worker_list_denied: Option<bool>,
    anonymous_access_denied: Option<bool>,
    cors_allowed_origin_only: Option<bool>,
    private_bucket_never_versioned: bool,
    quarantine_bucket_never_versioned: bool,
    no_versions_after_delete: bool,
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
    let expected_owner_str = expected_owner
        .as_deref()
        .ok_or_else(|| "expected bucket owner required for physical S3 probe".to_owned())?;
    if quarantine_bucket == private_bucket {
        return Err("quarantine and private buckets must differ".to_owned());
    }
    let region = optional_env("AWS_REGION").or_else(|| optional_env("AWS_DEFAULT_REGION"));

    let shared_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = aws_sdk_s3::Client::new(&shared_config);

    // V0 hard delete is unsafe for an Enabled or Suspended bucket: a
    // successful HEAD absence would not rule out retained object versions.
    // These read-only checks must run before creating a disposable object.
    // Probe credentials are separate from the ordinary serve/worker roles.
    let private_bucket_never_versioned =
        verify_never_versioned_bucket(&client, &private_bucket, expected_owner_str).await?;
    let quarantine_bucket_never_versioned =
        verify_never_versioned_bucket(&client, &quarantine_bucket, expected_owner_str).await?;
    if !private_bucket_never_versioned || !quarantine_bucket_never_versioned {
        return Err("S3 probe requires buckets with versioning never enabled".to_owned());
    }

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
        .create_immutable(&locator, byte_len, Box::new(Cursor::new(payload.clone())))
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

    let download_grant = provider
        .issue_grant(&ProviderGrantRequest {
            tenant_id: "tenant:provider-probe".to_owned(),
            object_locator: locator.clone(),
            operation: GrantOperation::Download,
            expected_byte_len: Some(byte_len),
            required_content_type: None,
            expires_at_ms: now_ms()?.saturating_add(Duration::from_secs(60).as_millis() as u64),
        })
        .await;
    // Owner-pinned download MUST reject the URL-only grant envelope because
    // it cannot carry the signed expected-bucket-owner header. Treat that
    // exact fail-closed refusal as successful policy evidence, not a failure
    // of S3 create/HEAD/GET/delete conformance.
    let (
        download_grant_available,
        owner_pinned_download_grant_denied,
        download_grant_policy_passed,
    ) = evaluate_download_grant_policy(expected_owner.is_some(), download_grant);

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
    // HEAD absence alone is insufficient when a provider might retain old
    // versions or deletion markers. Query only this disposable probe prefix.
    // A truncated result, missing truncation flag or query failure is NO-GO.
    let no_versions_after_delete = if delete_head_absent {
        verify_no_version_residue(&client, &private_bucket, &locator, expected_owner_str).await?
    } else {
        false
    };
    let physical_delete_v0 = physical_delete_proven(
        private_bucket_never_versioned,
        quarantine_bucket_never_versioned,
        delete_head_absent,
        no_versions_after_delete,
    );

    let core_adapter_probe_passed = create_once_collision_rejected
        && head_length_exact
        && read_sha256_exact
        && stale_read_rejected
        && stale_delete_rejected
        && delete_head_absent
        && download_grant_policy_passed
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
        owner_pinned_download_grant_denied,
        download_grant_policy_passed,
        direct_upload_grant_available,
        hard_create_only_reported: capabilities.hard_create_only,
        hard_exact_or_max_upload_size_reported: capabilities.hard_exact_or_max_upload_size,
        signed_content_type_reported: capabilities.signed_content_type,
        strong_head_after_put_reported: capabilities.strong_head_after_put,
        serve_list_denied: None,
        worker_list_denied: None,
        anonymous_access_denied: None,
        cors_allowed_origin_only: None,
        private_bucket_never_versioned,
        quarantine_bucket_never_versioned,
        no_versions_after_delete,
        physical_delete_v0: Some(physical_delete_v0),
        core_adapter_probe_passed,
    };
    Ok((receipt, core_adapter_probe_passed))
}

// S3 GetBucketVersioning returns no Status only when versioning has never
// been enabled. A Suspended bucket is not safe for the current V0 hard-delete
// model, since noncurrent objects may survive a successful ordinary DELETE.
async fn verify_never_versioned_bucket(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    expected_owner: &str,
) -> Result<bool, String> {
    let output = client
        .get_bucket_versioning()
        .bucket(bucket)
        .expected_bucket_owner(expected_owner)
        .send()
        .await
        .map_err(|_| "s3_bucket_versioning_probe_failed".to_owned())?;
    Ok(output.status().is_none())
}

// This is an inspector-only permission on a synthetic key prefix, not a
// justification for granting ListBucketVersions to serve/worker identities.
async fn verify_no_version_residue(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    locator: &str,
    expected_owner: &str,
) -> Result<bool, String> {
    let output = client
        .list_object_versions()
        .bucket(bucket)
        .expected_bucket_owner(expected_owner)
        .prefix(locator)
        .send()
        .await
        .map_err(|_| "s3_object_version_probe_failed".to_owned())?;
    Ok(output.is_truncated() == Some(false)
        && output.versions().is_empty()
        && output.delete_markers().is_empty())
}

fn physical_delete_proven(
    private_bucket_never_versioned: bool,
    quarantine_bucket_never_versioned: bool,
    delete_head_absent: bool,
    no_versions_after_delete: bool,
) -> bool {
    private_bucket_never_versioned
        && quarantine_bucket_never_versioned
        && delete_head_absent
        && no_versions_after_delete
}

// Report the true effective grant boundary: URL-only grants are supported
// only without the expected bucket-owner pin. A pinned deployment must reject
// them with the exact provider security code, never succeed with a URL.
fn evaluate_download_grant_policy(
    owner_pinned: bool,
    grant: Result<ProviderGrant, ProviderError>,
) -> (bool, bool, bool) {
    let available = matches!(&grant, Ok(value) if !value.opaque_url.is_empty());
    let refused_by_owner_fence = matches!(
        &grant,
        Err(error)
            if error.kind == ProviderErrorKind::Other
                && error.code == "s3_download_grant_expected_owner_requires_headers"
    );
    let passed = if owner_pinned {
        refused_by_owner_fence && !available
    } else {
        available && !refused_by_owner_fence
    };
    (available, refused_by_owner_fence, passed)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn available_grant() -> ProviderGrant {
        ProviderGrant {
            opaque_url: "https://example.invalid/non-sensitive-synthetic-grant".to_owned(),
            expires_at_ms: 123,
        }
    }

    fn owner_denial() -> ProviderError {
        ProviderError::new(
            ProviderErrorKind::Other,
            "s3_download_grant_expected_owner_requires_headers",
        )
    }

    #[test]
    fn physical_delete_requires_never_versioned_buckets_and_no_residual_versions() {
        assert!(physical_delete_proven(true, true, true, true));
        assert!(!physical_delete_proven(false, true, true, true));
        assert!(!physical_delete_proven(true, false, true, true));
        assert!(!physical_delete_proven(true, true, false, true));
        assert!(!physical_delete_proven(true, true, true, false));
    }

    #[test]
    fn owner_pinned_download_requires_exact_fail_closed_refusal() {
        assert_eq!(
            evaluate_download_grant_policy(true, Err(owner_denial())),
            (false, true, true)
        );
        // A future regression that silently issues a URL despite owner
        // pinning must turn the physical provider receipt RED.
        assert_eq!(
            evaluate_download_grant_policy(true, Ok(available_grant())),
            (true, false, false)
        );
        assert_eq!(
            evaluate_download_grant_policy(
                true,
                Err(ProviderError::new(
                    ProviderErrorKind::Other,
                    "unrelated_provider_error"
                )),
            ),
            (false, false, false)
        );
    }

    #[test]
    fn unpinned_download_requires_actual_available_grant() {
        assert_eq!(
            evaluate_download_grant_policy(false, Ok(available_grant())),
            (true, false, true)
        );
        assert_eq!(
            evaluate_download_grant_policy(false, Err(owner_denial())),
            (false, true, false)
        );
        assert_eq!(
            evaluate_download_grant_policy(
                false,
                Err(ProviderError::new(
                    ProviderErrorKind::Other,
                    "presign_failed"
                )),
            ),
            (false, false, false)
        );
    }
}
