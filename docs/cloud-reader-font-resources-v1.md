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

## Private Windows export helper

For a private deployment where the operator already has lawful source-font bytes installed on a Windows workstation, Chaptera can build a local-only configured-font packet without committing the font files:

```powershell
python tools\export_cloud_reader_windows_fonts.py --family Calibri --family Cambria --output-dir C:\chaptera-cloud-fonts
```

The helper scans the selected Windows font directory (default: `%WINDIR%\Fonts`), parses TTF/OTF/TTC/OTC name tables, requires exactly one Regular face for each requested family, copies the exact source container bytes into the private packet, records SHA-256 + face index, and emits:

- `fonts/` — private exact font containers;
- `manifest.json` — source-safe packet metadata without the local source path;
- `cloud-reader-font-resources.toml` — ready-to-append `cloud_reader_guest.font_resources` entries;
- `README.txt` — private deployment instructions and licensing fence.

This helper does not grant redistribution or server-use rights. Its output must not be committed or uploaded to public CI.

The current Cloud browser transport does not independently select a nonzero face from a font collection. Therefore the helper fails closed when a requested family resolves to `face_index > 0` in TTC/OTC bytes. A collection face at index 0 is allowed and is recorded as `first_face_only`; keep this fence until browser-side collection-face selection has its own acceptance proof.
## Create a private font requirements plan from the actual PUB Reader output

The source-backed Viewer JSON can now produce an **operator-only** requirements plan. It preserves exact source family spellings, candidate Quill font indices, script slots, and effective regular/bold/italic runs; it checks every reported scalar interval against the original Story text hash. It does not carry the Story text, font bytes, or grant source-font/PDF authority.

```powershell
python tools/pub_source_font_requirements_v1.py --viewer C:\\private\\publisher-viewer.json --output C:\\private\\source-font-requirements.json
python tools/export_cloud_reader_windows_fonts.py --requirements C:\\private\\source-font-requirements.json --output-dir C:\\private\\source-font-packet
```

The existing private Windows helper then verifies installed font face names and exact SHA-256 bytes. This operation must run **only with font files licensed for the intended private use**. The plan's Quill font index is not a physical file hash or server-authoring grant.

The exporter deliberately supports **Regular faces only**. If the original PUB needs bold/italic styles, contains unknown style bits, or has source runs without family names, the second command **fails before copying any files**. To generate a clearly marked partial packet for investigation only, add `--allow-incomplete-styles`. Its manifest explicitly records `partial_style_or_source_coverage`, `private_licensed_source_face_mapping_unverified`, and `publisher_visual_parity_verified=false`; no such packet authorizes native Publisher layout, PDF output, redistribution or a durable Editor authoring grant. Different Quill source font indices sharing one family name are never silently treated as a byte-identical original.

The planning and parser regression runs on the public `SampleNewsletter.pub` Viewer JSON without requiring or committing licensed third-party fonts. Private `fonts/` packets and provenance files must stay off public CI artifacts.

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
