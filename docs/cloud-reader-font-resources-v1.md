# Cloud Reader configured source-font resources v1

Cloud Reader executes source-proven font families only when an operator explicitly supplies exact physical font bytes.

## Deployment contract

Configure resources under `cloud_reader_guest.font_resources`.

Each entry contains:

- `source_family`: exact source family identity admitted by Reader typography;
- `path`: operator-managed local font file; absolute in production;
- `expected_sha256`: lowercase SHA-256 of the exact file bytes;
- `face_index`: collection face index, default `0`;
- `mime`: `font/ttf` or `font/otf`.

Chaptera does not bundle proprietary fonts, search ambient Linux fonts, or download fonts from the network.

Configured resources are copied into the isolated guest-scene worker through the bounded private font manifest. The worker re-hashes the bytes before use.

## Fidelity states

Reader Scene exposes source-safe diagnostics without including document text, font-family names, or font bytes:

- `source_font_family_unresolved`: visible text lacks bounded source-family authority;
- `source_font_resource_unavailable`: a source family is resolved but no configured physical resource exists;
- `source_font_resource_admitted`: one or more exact configured resources were admitted into the Scene.

The compatibility report maps the two warning states to stable user-facing limitations. Cloud UI renders those warnings as fidelity limitations and does not present informational diagnostics as problems.

When a configured file is present but its bytes do not match `expected_sha256`, scene production fails closed with `guest_scene_font_hash_mismatch`. An unreadable configured file fails with `guest_scene_font_read_failed`. These are configuration failures rather than visual fallback states.

A missing configured source font makes fidelity partial; the renderer may use the deterministic Reader fallback/preview path, but must not silently claim source-font parity.

## Acceptance

The source-free server regression writes the redistribution-safe Chaptera fallback font to a temporary operator-style path and proves the real loader:

1. reads the configured local file;
2. accepts the exact pinned SHA-256 and face index;
3. constructs the exact configured resource identity;
4. rejects SHA drift with `guest_scene_font_hash_mismatch`;
5. rejects a missing configured file with `guest_scene_font_read_failed`.

No proprietary font bytes are required for this acceptance.

## Proprietary-font boundary

Repositories and release artifacts must not contain licensed Microsoft or third-party font bytes unless redistribution rights explicitly permit it.

For documents that require families such as Elephant or Times New Roman, production parity requires the deployment operator to supply licensed exact resources through this contract. A private/local grand-opening A/B may use those operator-supplied licensed bytes, but they must never be committed as fixtures.
