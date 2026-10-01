# Embedded Cloud Reader release

The Cloud Reader UI is part of the canonical Rust `chaptera` application
artifact. The server embeds the five admitted files from `apps/cloud-reader`
and serves them on the same origin as `/v1/reader/*`.

There is no production requirement to build, copy, extract or switch a separate
Reader ZIP/site directory. The browser still contains no PUB parser, customer
document or research-retention authority.

## Build and verify

Build the canonical application artifact:

```sh
cargo build --release -p chaptera-server --bin chaptera
```

The exact source commit remains part of the binary build identity. `GET
/version` also reports the embedded Reader asset count, byte lengths and
SHA-256 hashes.

`python3 apps/cloud-reader/build_release.py` is retained only as a CI/reference
oracle for the five committed frontend files. Its deterministic ZIP and manifest
can prove byte identity, but that ZIP is no longer an operator deployment
artifact.

## Same-origin edge

Use `deploy/caddy/CloudReader.Caddyfile.example` with only the public Reader
origin and private Chaptera upstream:

```sh
export CHAPTERA_READER_SITE=reader.example.invalid
export CHAPTERA_READER_API=127.0.0.1:8080
caddy validate --config deploy/caddy/CloudReader.Caddyfile.example --adapter caddyfile
```

Caddy terminates HTTPS and exposes only the embedded Reader files plus
`/v1/reader/*`. It does not read Reader files from disk. Health, auth,
authoring and unrelated `/v1/*` surfaces remain outside the public Reader
origin.

The current Rust edge classifies guest `/content` as an ordinary API body.
Configure `cloud_reader_guest.max_file_bytes` no higher than
`edge.max_api_body_bytes`; the current Reader edge bound is 8,388,608 bytes
(8 MiB). The Caddy recipe enforces the same request-body ceiling.

The production server example leaves guest reading disabled. Enable it in the
operator's typed server config with the existing section:

```toml
[cloud_reader_guest]
session_ttl_seconds = 600
max_file_bytes = 8388608
max_concurrent_uploads = 1
max_reserved_bytes = 8388608

[cloud_reader_guest.rate_subject_secret]
source = "systemd"
name = "reader_rate_subject_secret"
```

Provision the rate-subject secret through the existing systemd credential
mechanism. The server still requires compatible schema, private BlobStore, real
scanner, isolated worker and cleanup configuration before guest reading is
usable.

## Install and rollback

The Chaptera-specific application payload is one binary:

```text
chaptera
```

Install that binary as an immutable release and point the existing web/worker
systemd units at the same artifact. Both roles already use the same executable;
the isolated guest Reader scene worker also self-execs that same file.

A normal Reader upgrade no longer has a second static deployment transaction.
Validate the new binary/config, switch the application release, restart the
roles, wait for `/ready`, then run the HTTPS upload → open → Scene → expiry
smoke.

Rollback restores the previous `chaptera` artifact subject to the existing
schema-compatibility rule. There is no independent Reader-static rollback.

## Production gates

Embedding changes deployment packaging only. It does not weaken or replace the
existing security/runtime gates:

- hosted exact-head checks and reviewed source;
- configured HTTPS host;
- real scanner and isolated child-process execution;
- private BlobStore/schema state and TTL cleanup;
- measured real-PUB browser/reference comparison with explicit Partial gaps;
- distinct unsupported/damaged/NOT_PUB/security outcomes;
- separately consented contribution flow where required by product scope.

Viewing never authorizes durable research retention.
