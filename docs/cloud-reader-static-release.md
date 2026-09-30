# Standalone Cloud Reader release

The Reader is a static artifact from `apps/cloud-reader`, deployed separately
from `chaptera-server`. Its same-origin `/v1/reader/*` API remains server-owned.
The site does not contain a PUB parser, server binary, customer document or
research-retention capability.

## Build and verify

From a committed checkout:

```sh
python3 apps/cloud-reader/build_release.py --output target/cloud-reader-release
```

The builder reads the five assets from the exact Git commit, creates
`site/manifest.json` with their SHA-256 hashes and byte lengths, and emits
`cloud-reader.zip` plus `build-receipt.json`. ZIP entry times and permissions are
fixed, so rebuilding the same commit yields identical bytes. Uncommitted asset
edits are not silently included in a release. An explicit `--commit <full-sha>`
can select another available committed version.

The Cloud Reader workflow tests the released files through pinned Caddy 2.10.2
and Chromium with a synthetic API. Its artifact contains the release ZIP,
desktop/mobile screenshots and exact-head receipts. Check the source commit
and archive digest against the reviewed receipt before installation.

## Same-origin edge

Use `deploy/caddy/CloudReader.Caddyfile.example` with:

```sh
export CHAPTERA_READER_SITE=cloud.example.invalid
export CHAPTERA_READER_ROOT=/opt/chaptera/cloud-reader/current
export CHAPTERA_READER_API=127.0.0.1:8080
caddy validate --config deploy/caddy/CloudReader.Caddyfile.example --adapter caddyfile
```

Replace the example origin with the approved canonical HTTPS origin. Configure
the existing Rust edge with that exact canonical host/origin and only its
immediate Caddy proxy peer as trusted. The application listener remains private.
The recipe collapses forwarded authority headers, serves the static release
with a restrictive CSP, and proxies only Reader API routes. Authenticated
saved-document viewing requires an already established account session at that
same origin; this recipe does not introduce a login flow or expose other
product APIs.

The current Rust edge classifies guest `/content` as an ordinary API body.
Configure `cloud_reader_guest.max_file_bytes` no higher than
`edge.max_api_body_bytes`; the example edge limit is 8,388,608 bytes (8 MiB).
The Caddy recipe also uses a finite 8 MiB Reader body limit. Raising the guest
limit requires a tested change to both edge layers. The upstream response
header deadline is finite at 120 seconds; the Rust request deadline and
scanner/isolated-worker budgets remain independently enforced.

The production server example leaves guest reading disabled. Enable it in the
operator's typed server config with the existing `cloud_reader_guest` section,
for example:

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
mechanism, and add the matching `LoadCredential` entry to the web service's
operator drop-in. Do not put its value in the static artifact or repository.
The server still requires compatible schema, private BlobStore, real scanner,
isolated worker and cleanup configuration before the guest API is usable.

## Install and rollback

1. Verify all extracted files against `manifest.json` and the archive digest.
2. Install into a new immutable directory under
   `/opt/chaptera/cloud-reader/releases/<source-commit>/`; expose only the five
   assets and manifest, with no source/test/private files beside them.
3. Validate the configured edge and verify the existing server is ready.
4. Atomically point `current` at the new directory. Avoid overwriting a serving
   release. Assets use revalidation rather than a stale long-lived cache.
5. Run an HTTPS smoke using a public-safe known PUB, verify scene protocol and
   display, and check that temporary bytes/session metadata expire.
6. If smoke fails, atomically restore the previous static directory. Server
   migration/rollback follows the existing server contract independently.

## Production gates

Local Caddy acceptance uses HTTP loopback and a synthetic service. It does not
prove TLS, real scanning/isolation, BlobStore policy, TTL cleanup, admission
capacity or source fidelity. Release approval still requires:

- hosted exact-head checks and reviewed source;
- measured real-PUB browser/reference comparison with explicit Partial gaps;
- the configured HTTPS host, real isolated guest service and expiry cleanup;
- distinct unsupported/damaged/NOT_PUB/security outcomes;
- the separately consented contribution flow where required by product scope.

Viewing never authorizes durable research retention. The current static
release does not expose an unimplemented contribution action.
