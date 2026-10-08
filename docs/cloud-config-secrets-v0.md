# CLOUD-SECRETS-CONFIG-01 — typed production config and role-scoped secrets

Chaptera V0 uses one typed TOML configuration root. Production services do not
assemble security-critical configuration from dozens of ad-hoc environment
variables.

## Entry point

Production:

```text
chaptera --config /etc/chaptera/chaptera.toml serve
chaptera --config /etc/chaptera/chaptera.toml worker
chaptera --config /etc/chaptera/chaptera.toml doctor
chaptera --config /etc/chaptera/chaptera.toml migrate status
chaptera --config /etc/chaptera/chaptera.toml migrate up
```

Development may omit `--config`; only that path retains bounded development
defaults and the historical `CHAPTERA_LISTEN` / `CHAPTERA_SQLITE_PATH`
overrides.

## Typed modes

`environment` is one of:

- `dev`
- `test`
- `prod`

The parser rejects unknown TOML fields. Production additionally requires:

- private/loopback application listener;
- HTTPS `public_origin`;
- absolute SQLite path;
- WAL + `synchronous=FULL`;
- bounded SQLite pool/timeouts;
- bounded worker concurrency;
- distinct quarantine/private storage namespaces;
- explicit AuthN login-flow, idle-session and absolute-session TTLs;
- OIDC configuration with HTTPS issuer;
- a secret **reference**, never an inline OIDC client secret.

AuthN TTLs are deployment policy, not hidden Chaptera defaults. Production
configuration must state all three values explicitly. They must be positive,
the absolute session TTL must not be shorter than the idle TTL, and all values
must fit the runtime's millisecond timestamp range. The values in
`chaptera.prod.example.toml` are examples only, not canonical product policy.

## Secret references

Supported V0 sources:

```toml
client_secret = { source = "env", name = "CHAPTERA_OIDC_CLIENT_SECRET" }
client_secret = { source = "file", path = "/run/secret/oidc" }
client_secret = { source = "systemd", name = "oidc_client_secret" }
```

`systemd` resolves only inside `$CREDENTIALS_DIRECTORY`. The canonical web
unit uses:

```ini
LoadCredential=oidc_client_secret:/etc/chaptera/credentials/oidc_client_secret
```

The worker unit intentionally does **not** receive that credential.

Production file-backed secrets must not be group/world accessible on Unix.
Secret files are bounded to 64 KiB and a single trailing CRLF/LF is stripped.

Process environment resolution is lazy: Chaptera reads only the explicitly
requested secret name. It does not copy the complete process environment into a
Rust map.

Resolved secret memory is wrapped in `zeroize::Zeroizing`; Debug output is
redacted and never renders secret bytes.

## Role boundary

- `serve`: resolves the OIDC client secret and active application key ring.
- `doctor`: resolves required secrets so an operator can detect missing
  credentials before admission.
- `worker`: parses/validates the same structural config but is not handed the
  OIDC client secret.
- `migrate`: consumes the typed SQLite path and busy timeout without requiring
  AuthN secrets.

This is deliberate least privilege, not four separate config formats.

## Short-lived key ring

When an owning protocol actually uses an application-managed symmetric key,
configuration may include:

```toml
[key_ring]
active = "grant-2026-09"
previous = ["grant-2026-08"]

[[key_ring.keys]]
id = "grant-2026-09"
secret = { source = "systemd", name = "grant_key_2026_09" }

[[key_ring.keys]]
id = "grant-2026-08"
secret = { source = "systemd", name = "grant_key_2026_08" }
```

V0 validates a unique active key, at most three previous overlap keys, and that
all referenced IDs exist.

This ring is **not** the long-lived migration-evidence signing authority.
Asymmetric long-lived issuer verification history remains a separate contract.

## Cloud Reader configured source fonts

The full deployment and acceptance contract is documented in `docs/cloud-reader-font-resources-v1.md`.

Cloud Reader source-font bytes are explicit deployment resources, not ambient
machine state. Chaptera never searches installed Linux fonts and never downloads
a missing font from the network.

When a licensed source font should be executable by Reader, configure it under
`cloud_reader_guest.font_resources` with an exact source-family name, absolute
production path, expected lowercase SHA-256, face index and `font/ttf` or
`font/otf` MIME type. The canonical production example keeps this block
commented because deployments must supply only font files they are licensed to
use.

The boundary is fail-closed:

- unreadable configured bytes fail worker startup/admission with
  `guest_scene_font_read_failed`;
- bytes whose digest differs from the configured SHA-256 fail with
  `guest_scene_font_hash_mismatch`;
- a source typography run with no stable family identity is exposed as
  `source_font_family_unresolved`;
- a known source family with no configured exact resource is exposed as
  `source_font_resource_unavailable`;
- when an exact configured resource is actually used for layout, Scene records
  `source_font_resource_admitted` as an informational diagnostic.

The two source-availability warnings make the Reader compatibility state
explicit instead of silently implying source-font fidelity while fallback is in
use. They do not authorize host-font guessing or automatic acquisition.
Configured font bytes are copied only through the existing private bounded
manifest path and are never written into public receipts or repository
fixtures.

For documents that depend on proprietary families such as Elephant or Times New
Roman, the operator must provide licensed local files and exact digests. The
repository intentionally contains no such font bytes.

## Failure policy

Missing, empty, oversized, over-permissive, malformed or unresolved required
secrets fail the affected role before normal traffic admission. Secret values
must never appear in config dumps, Debug output, logs or receipts.

## Dependency choices

- `toml 1.1.6+spec-1.1.0` for Serde-compatible typed TOML parsing.
- `zeroize 1.9.0` for compiler-resistant in-memory secret clearing.
- `url 2.5.8` for structural origin/issuer validation.

No custom secret encryption format or Vault replacement is introduced.
