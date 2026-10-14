# Chaptera desktop suite cross-app handoff v1

The desktop suite uses separate product processes. Cross-app transfer therefore carries **intent + immutable source identity**, not an in-memory document graph.

## V1 roles

- `chaptera-reader.exe` is the sender.
- `chaptera-editor.exe` accepts `edit_supported_pub` only after Reader already proved the source opens through the current Viewer path; Editor re-hashes the source and repeats its own open/capability gate.
- `chaptera-rescue.exe` accepts `diagnose_or_recover` only after Reader failure classification says the source is recovery-eligible damaged Publisher material. Rescue re-hashes the source **and independently reruns the canonical `pub-reader` failure-intake classifier**; it accepts only `FailureIntakeClass::PubDamaged`, without pretending that recovery already ran.

V1 does not route Migration yet.

## Packet vs durable receipt

`chaptera.suite-handoff.v1` is **local IPC** and therefore may contain a local source path. It also carries:

- exact source SHA-256;
- sender/target product IDs;
- requested job;
- Reader capability/loss context;
- explicit user-initiated local handoff;
- provenance proving the sender verified the source identity;
- `mutable_document_state_included=false`.

The receiving process never trusts the path alone. It re-hashes the file and rejects the handoff if bytes changed after sender admission.

On Windows, source admission opens with `FILE_SHARE_READ` only for the bounded
read and SHA-256 calculation. An incompatible existing writer/deleter returns
`source_busy`; later write/truncate/rename attempts are excluded during that
window. After hashing, `ReOpenFile` retains the same opened filesystem object
with ordinary read/write/delete sharing, then releases the short restrictive
handle. It never reopens the source path to reconstruct identity. Failure to
retain that object returns `source_unstable`, without a path-based fallback.
The application does not lock a source for its entire session: later edits are
permitted and existing exact-handle rehash checks still detect changes.

The existing Windows path-identity gate pauses a real source read and runs a
separate process attempting write, truncate and rename. It also proves typed
busy/retry admission and post-window rename/write without source-path rebinding.
Only source-free, exact-candidate receipts leave that synthetic test boundary.

Before decoding or opening the source, `HandoffPacketAdmissionV1` rejects packet
metadata over 256 KiB, then reads at most that cap plus one sentinel byte. The
streamed length is authoritative when metadata is stale or the file grows.
Every V1 object rejects unknown/duplicate fields. Decoded source paths are
limited to 128 KiB of UTF-8; other strings are limited to 128 bytes, with a
64-byte SHA-256 ceiling. The 256 KiB cap covers a representative packet below
1 KiB and 32767 fully escaped Windows UTF-16 path units with generous field
headroom. Platform path support remains owned by the existing file authority.

Admission errors use stable source-free `handoff_packet_*` codes and never echo
input JSON or local paths. Existing version, receiver, job, capability, consent
and source-identity checks still run before source access. Acceptance hashes the
exact admitted bytes, including whitespace. Senders enforce the same limits
before changing a packet output. Future schema semantics require a new version.

`chaptera.suite-handoff-acceptance.v1` is the durable/source-free proof. It contains hashes and product/job identity but no local path and no document content.

## Fail-closed rules

- Reader cannot route a successfully opened source to Rescue under V1.
- Reader cannot route a source that failed its Reader open gate to Editor.\n- A Reader failure that is merely unsupported/unknown is **not** automatically Rescue; V1 Rescue handoff requires `FailureIntakeClass::PubDamaged`.
- Rescue cannot accept an Editor-targeted packet and vice versa.\n- Rescue does not trust the sender's damage label: receiver-side classification must independently resolve to `PubDamaged`.
- A source changed after packet creation is rejected.
- No mutable Editor/Viewer graph crosses the process boundary.
- Handoff does not imply recovery success, native PUB validity or editability beyond the receiver's own capability gate.

The Windows acceptance workflow builds the current real Reader, Editor and Rescue binaries and proves Reader->Editor plus Reader->Rescue end-to-end using a bounded synthetic damaged-CFB classification witness. The witness proves routing/transport only, not recovery semantics.
