# AWS S3 IAM negative physical probe — operator-only

This is an **executable but unqualified** physical acceptance tool. It is **not** a substitute for the protected GitHub Environment (#2613), review of effective bucket/IAM policies, anonymous access, CORS or tenant isolation.

## Operator-owned prerequisites

Use a disposable account containing **two distinct private, never-versioned general-purpose S3 buckets** owned by the same 12-digit AWS account. Keep S3 Block Public Access enabled. The physical provider probe (#2611/#2615) separately checks versioning and exact deletion.

Configure two **different short-lived AWS assumed-role profiles** in a private AWS CLI/shared credentials configuration: one Chaptera serve role and one worker role. They must resolve to *different underlying roles*, not merely differently named sessions of one role, and belong to the expected bucket-owner account. The program uses the explicit profile-file credentials provider and rejects ambient static/container/web-identity credentials rather than silently falling back to the operator role.

With the **separate operator identity**, create two disposable **synthetic, non-customer** fixture objects before running this test. Each physical key must match exactly one of:

- `quarantine/provider-receipt/<32 lowercase hex digits>` in the quarantine bucket;
- `canonical/provider-receipt/<32 lowercase hex digits>` in the private bucket.

Know both exact byte lengths (1–65536 bytes), using harmless arbitrary test content. Ensure both roles have only the necessary `GetObject` permission to HEAD these fixture objects. Do not use customer keys or a known-missing key. Delete both fixtures with the operator identity after the probe and inspect their absence/versions independently. **The two fixture keys and account/bucket labels must not be copied into GitHub, Notion or any public receipt.**

## Run locally under an authorized operator session

Set these environment variables locally using a secret-safe mechanism (never in a CI log or committed file):

```plain
AWS_REGION
CHAPTERA_S3_EXPECTED_BUCKET_OWNER
CHAPTERA_S3_QUARANTINE_BUCKET
CHAPTERA_S3_PRIVATE_BUCKET
CHAPTERA_S3_IAM_QUARANTINE_FIXTURE_KEY
CHAPTERA_S3_IAM_QUARANTINE_FIXTURE_BYTES
CHAPTERA_S3_IAM_PRIVATE_FIXTURE_KEY
CHAPTERA_S3_IAM_PRIVATE_FIXTURE_BYTES
CHAPTERA_S3_IAM_SERVE_PROFILE
CHAPTERA_S3_IAM_WORKER_PROFILE
```

Unset `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, `AWS_ROLE_ARN`, `AWS_WEB_IDENTITY_TOKEN_FILE`, and container credentials variables. With an isolated AWS profile file available, execute:

```sh
cargo run -p chaptera-server --example cloud_s3_iam_list_probe --quiet > iam-list-receipt.json
```

This read-only probe performs `sts:GetCallerIdentity` for both roles, then **owner-pinned positive HEAD** for both synthetic fixtures under both roles. If *any* positive control is missing, it refuses all `ListBucket` denial claims. After positive controls, it requires a typed 403 ServiceError for `ListObjectsV2` against empty-root and relevant namespace prefixes from both identities on both buckets. Unexpected 200, 301, 404, network errors and timeouts fail closed.

Receipt protocol: `chaptera.cloud-s3-iam-list-receipt.v1`. The output contains **booleans only** plus fixed version/provider labels: it never includes profiles, role/account IDs, bucket names, object keys, URLs, secret values, status error bodies or fixtures. Exit code 0 requires all sampled checks; 1 means invalid preconditions/identity; 2 means one or more sampled operations failed.

## Evidence boundary

A successful sample establishes *observed denial for the tested prefixes and principals*, **not a global proof that the effective IAM policy denies every possible S3 prefix**. An authorized operator must independently inspect role policies, permissions boundaries, bucket policies, SCPs and any applicable VPC endpoint policy. `complete_iam_policy_reviewed=false` is deliberately constant in this receipt.

Separate physical receipts remain required for anonymous GET/HEAD, allowed-vs-denied CORS, S3 object hard delete/version history, and Chaptera tenant binding. Do not mark Controlled External Pilot GO until those proofs and the protected Actions environment (#2613) are complete.
