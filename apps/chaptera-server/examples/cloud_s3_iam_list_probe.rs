//! AWS-first physical IAM negative probe (separate from the S3 provider adapter).
//! Operator only: provide two *different* short-lived AWS role profiles, both
//! buckets, and two existing synthetic 32-hex fixture objects with exact sizes.
//! No customer objects, paths, credentials, profile names, ARNs or URLs go to
//! stdout, stderr, workflow artifacts or the sanitized receipt.
//! Sampled ListBucket denial is NOT an exhaustive IAM/bucket-policy audit.

use std::{env, time::Duration};

use aws_config::{BehaviorVersion, Region, profile::ProfileFileCredentialsProvider};
use aws_sdk_s3::Client as S3Client;
use aws_sdk_sts::Client as StsClient;
use serde::Serialize;
use tokio::time::timeout;

const MAX_AWS_CALL: Duration = Duration::from_secs(20);
const QUARANTINE_PREFIX: &str = "quarantine/provider-receipt/";
const PRIVATE_PREFIX: &str = "canonical/provider-receipt/";
const RECEIPT_VERSION: &str = "chaptera.cloud-s3-iam-list-receipt.v1";

#[derive(Serialize)]
struct IamListReceiptV1 {
    protocol_version: &'static str,
    provider_family: &'static str,
    expected_bucket_owner_verified: bool,
    underlying_aws_roles_distinct: bool,
    serve_quarantine_head_passed: bool,
    serve_private_head_passed: bool,
    worker_quarantine_head_passed: bool,
    worker_private_head_passed: bool,
    serve_list_denied_tested_prefixes: bool,
    worker_list_denied_tested_prefixes: bool,
    // These require independent operator policy review and must never
    // silently inherit the sampled ListObjectsV2 result.
    complete_iam_policy_reviewed: bool,
    anonymous_access_denied: Option<bool>,
    cors_allowed_origin_only: Option<bool>,
    iam_negative_probe_passed: bool,
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(receipt) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&receipt)
                    .expect("serializing sanitized receipt cannot fail")
            );
            if !receipt.iam_negative_probe_passed {
                std::process::exit(2);
            }
        }
        Err(code) => {
            // Never include SDK error text: it can contain endpoints,
            // object names, principal identifiers and authorization context.
            eprintln!("chaptera S3 IAM probe refused: {code}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<IamListReceiptV1, String> {
    reject_ambient_credentials()?;
    let region = required_env("AWS_REGION")?;
    let owner = required_env("CHAPTERA_S3_EXPECTED_BUCKET_OWNER")?;
    if owner.len() != 12 || !owner.bytes().all(|ch| ch.is_ascii_digit()) {
        return Err("invalid_expected_bucket_owner".to_owned());
    }

    let quarantine_bucket = required_env("CHAPTERA_S3_QUARANTINE_BUCKET")?;
    let private_bucket = required_env("CHAPTERA_S3_PRIVATE_BUCKET")?;
    if quarantine_bucket == private_bucket {
        return Err("identical_quarantine_and_private_buckets".to_owned());
    }

    let quarantine_fixture = required_env("CHAPTERA_S3_IAM_QUARANTINE_FIXTURE_KEY")?;
    let private_fixture = required_env("CHAPTERA_S3_IAM_PRIVATE_FIXTURE_KEY")?;
    if !valid_synthetic_fixture(&quarantine_fixture, QUARANTINE_PREFIX)
        || !valid_synthetic_fixture(&private_fixture, PRIVATE_PREFIX)
    {
        return Err("only_synthetic_provider_receipt_fixtures_allowed".to_owned());
    }
    let quarantine_length = required_length("CHAPTERA_S3_IAM_QUARANTINE_FIXTURE_BYTES")?;
    let private_length = required_length("CHAPTERA_S3_IAM_PRIVATE_FIXTURE_BYTES")?;

    let serve_profile = required_env("CHAPTERA_S3_IAM_SERVE_PROFILE")?;
    let worker_profile = required_env("CHAPTERA_S3_IAM_WORKER_PROFILE")?;
    if !valid_role_profile(&serve_profile)
        || !valid_role_profile(&worker_profile)
        || serve_profile == worker_profile
    {
        return Err("two_distinct_named_role_profiles_required".to_owned());
    }

    let serve_config = load_role_config(&serve_profile, &region).await;
    let worker_config = load_role_config(&worker_profile, &region).await;
    let serve_identity = role_identity(&StsClient::new(&serve_config), &owner).await?;
    let worker_identity = role_identity(&StsClient::new(&worker_config), &owner).await?;
    if serve_identity == worker_identity {
        return Err("same_iam_role_for_serve_and_worker".to_owned());
    }

    let serve = S3Client::new(&serve_config);
    let worker = S3Client::new(&worker_config);
    // A 403 can be caused by bad credentials, missing objects or wrong bucket
    // authority. Four successful owner-pinned HEAD controls are mandatory.
    let controls = [
        head_control(
            &serve,
            &quarantine_bucket,
            &quarantine_fixture,
            quarantine_length,
            &owner,
        )
        .await,
        head_control(
            &serve,
            &private_bucket,
            &private_fixture,
            private_length,
            &owner,
        )
        .await,
        head_control(
            &worker,
            &quarantine_bucket,
            &quarantine_fixture,
            quarantine_length,
            &owner,
        )
        .await,
        head_control(
            &worker,
            &private_bucket,
            &private_fixture,
            private_length,
            &owner,
        )
        .await,
    ];
    // Do not even attempt to classify denial if a positive control failed.
    let denials = if controls.iter().all(|passed| *passed) {
        [
            sampled_list_denial(&serve, &quarantine_bucket, &private_bucket, &owner).await,
            sampled_list_denial(&worker, &quarantine_bucket, &private_bucket, &owner).await,
        ]
    } else {
        [false, false]
    };
    let passed = sampled_iam_gate(controls, denials);

    Ok(IamListReceiptV1 {
        protocol_version: RECEIPT_VERSION,
        provider_family: "aws-s3",
        expected_bucket_owner_verified: true,
        underlying_aws_roles_distinct: true,
        serve_quarantine_head_passed: controls[0],
        serve_private_head_passed: controls[1],
        worker_quarantine_head_passed: controls[2],
        worker_private_head_passed: controls[3],
        serve_list_denied_tested_prefixes: denials[0],
        worker_list_denied_tested_prefixes: denials[1],
        complete_iam_policy_reviewed: false,
        anonymous_access_denied: None,
        cors_allowed_origin_only: None,
        iam_negative_probe_passed: passed,
    })
}

async fn load_role_config(profile: &str, region: &str) -> aws_config::SdkConfig {
    // Explicit profile-file provider overrides the SDK's default credentials
    // chain, which might otherwise select operator env/instance credentials.
    let provider = ProfileFileCredentialsProvider::builder()
        .profile_name(profile)
        .build();
    aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(region.to_owned()))
        .profile_name(profile)
        .credentials_provider(provider)
        .load()
        .await
}

async fn role_identity(client: &StsClient, expected_owner: &str) -> Result<String, String> {
    let identity = timeout(MAX_AWS_CALL, client.get_caller_identity().send())
        .await
        .map_err(|_| "sts_identity_timeout".to_owned())?
        .map_err(|_| "sts_identity_failed".to_owned())?;
    if identity.account() != Some(expected_owner) {
        return Err("iam_role_account_does_not_match_expected_bucket_owner".to_owned());
    }
    normalize_assumed_role_arn(
        identity
            .arn()
            .ok_or_else(|| "sts_identity_missing_role".to_owned())?,
    )
    .ok_or_else(|| "only_distinct_assumed_iam_roles_accepted".to_owned())
}

async fn head_control(
    client: &S3Client,
    bucket: &str,
    key: &str,
    expected_len: u64,
    expected_owner: &str,
) -> bool {
    matches!(
        timeout(
            MAX_AWS_CALL,
            client
                .head_object()
                .bucket(bucket)
                .key(key)
                .expected_bucket_owner(expected_owner)
                .send()
        )
        .await,
        Ok(Ok(metadata))
            if metadata.content_length().and_then(|v| u64::try_from(v).ok()) == Some(expected_len)
                && metadata.e_tag().is_some()
    )
}

async fn sampled_list_denial(
    client: &S3Client,
    quarantine: &str,
    private: &str,
    expected_owner: &str,
) -> bool {
    // Empty-root and known namespaces are tested. A real IAM policy review
    // must additionally exclude ANY permitted prefix outside these samples.
    let samples = [
        (quarantine, ""),
        (quarantine, "quarantine/"),
        (private, ""),
        (private, "canonical/"),
        (private, "exports/"),
        (private, "derived/"),
        (private, "checkpoints/"),
    ];
    for (bucket, prefix) in samples {
        let outcome = timeout(
            MAX_AWS_CALL,
            client
                .list_objects_v2()
                .bucket(bucket)
                .prefix(prefix)
                .max_keys(1)
                .expected_bucket_owner(expected_owner)
                .send(),
        )
        .await;
        let denied = match outcome {
            Ok(Err(error)) => list_is_denied(
                error.as_service_error().is_some(),
                error.raw_response().map(|response| response.status().as_u16()),
            ),
            _ => false,
        };
        if !denied {
            return false;
        }
    }
    true
}

fn list_is_denied(service_error: bool, status: Option<u16>) -> bool {
    service_error && status == Some(403)
}

fn sampled_iam_gate(head_controls: [bool; 4], list_denials: [bool; 2]) -> bool {
    head_controls.into_iter().all(|passed| passed)
        && list_denials.into_iter().all(|passed| passed)
}

fn normalize_assumed_role_arn(value: &str) -> Option<String> {
    let (prefix, tail) = value.split_once(":assumed-role/")?;
    if !prefix.starts_with("arn:aws:sts::") {
        return None;
    }
    let (role, session) = tail.rsplit_once('/')?;
    if role.is_empty() || session.is_empty() {
        return None;
    }
    Some(format!("{prefix}:assumed-role/{role}"))
}

fn valid_role_profile(value: &str) -> bool {
    !value.is_empty()
        && value != "default"
        && value.len() <= 64
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-')
}

fn valid_synthetic_fixture(value: &str, prefix: &str) -> bool {
    let Some(token) = value.strip_prefix(prefix) else {
        return false;
    };
    token.len() == 32
        && token.bytes().all(|ch| ch.is_ascii_digit() || (b'a'..=b'f').contains(&ch))
}

fn reject_ambient_credentials() -> Result<(), String> {
    const AMBIENT: &[&str] = &[
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_WEB_IDENTITY_TOKEN_FILE",
        "AWS_ROLE_ARN",
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
    ];
    if AMBIENT.iter().any(|name| env::var_os(name).is_some()) {
        return Err("ambient_aws_credentials_not_allowed".to_owned());
    }
    // AWS-only acceptance must not unknowingly test an operator-supplied
    // S3/STS emulator or endpoint override instead of genuine AWS policy.
    const ENDPOINT_OVERRIDES: &[&str] = &[
        "AWS_ENDPOINT_URL",
        "AWS_ENDPOINT_URL_S3",
        "AWS_ENDPOINT_URL_STS",
    ];
    if ENDPOINT_OVERRIDES
        .iter()
        .any(|name| env::var_os(name).is_some())
    {
        return Err("custom_aws_endpoints_not_supported".to_owned());
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing_{name}"))
}

fn required_length(name: &str) -> Result<u64, String> {
    required_env(name)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0 && *value <= 65_536)
        .ok_or_else(|| format!("invalid_{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_weak_profile_or_arbitrary_customer_fixture_accepted() {
        assert!(valid_role_profile("chaptera-serve-probe"));
        assert!(!valid_role_profile("default"));
        assert!(!valid_role_profile("../operator"));
        let key = format!("{QUARANTINE_PREFIX}{}", "a".repeat(32));
        assert!(valid_synthetic_fixture(&key, QUARANTINE_PREFIX));
        assert!(!valid_synthetic_fixture(&key, PRIVATE_PREFIX));
        assert!(!valid_synthetic_fixture(
            "quarantine/provider-receipt/customer",
            QUARANTINE_PREFIX
        ));
    }

    #[test]
    fn distinct_sessions_of_same_iam_role_are_not_distinct_authority() {
        let a = "arn:aws:sts::123456789012:assumed-role/Serve/Session-A";
        let b = "arn:aws:sts::123456789012:assumed-role/Serve/Session-B";
        let c = "arn:aws:sts::123456789012:assumed-role/Worker/Session-A";
        assert_eq!(normalize_assumed_role_arn(a), normalize_assumed_role_arn(b));
        assert_ne!(normalize_assumed_role_arn(a), normalize_assumed_role_arn(c));
        assert!(normalize_assumed_role_arn("arn:aws:iam::123456789012:user/root").is_none());
        assert!(normalize_assumed_role_arn("arn:aws:sts::123456789012:assumed-role/no-session").is_none());
    }

    #[test]
    fn only_typed_forbidden_service_response_is_denial_evidence() {
        assert!(list_is_denied(true, Some(403)));
        assert!(!list_is_denied(false, Some(403)));
        assert!(!list_is_denied(true, Some(404)));
        assert!(!list_is_denied(true, Some(301)));
        assert!(!list_is_denied(true, None));
    }

    #[test]
    fn sampled_list_deny_cannot_pass_without_all_positive_controls() {
        assert!(sampled_iam_gate([true; 4], [true; 2]));
        for missing in 0..4 {
            let mut controls = [true; 4];
            controls[missing] = false;
            assert!(!sampled_iam_gate(controls, [true; 2]));
        }
        assert!(!sampled_iam_gate([true; 4], [true, false]));
        assert!(!sampled_iam_gate([true; 4], [false, true]));
    }
}
