# chaptera-receiver-qualification

Source-neutral V1 protocol types for receiver/RIP qualification.

This crate owns:

- runtime configuration closure and deterministic manifest identity;
- oracle scope (L0..L4);
- normalized separation evidence;
- PASS / FAIL / REVIEW / UNKNOWN / WAIVED assertion results;
- capability claims and immutable qualification receipts;
- fail-closed validation and canonical JSON hashing.

It deliberately does **not** own vendor adapters, PDF parsing/rendering, live RIP execution,
credential/storage authority, or receiver business policy. Hosted tests prove the protocol
contract only; native receiver claims require evidence from the actual runtime.

Canonical hashes recursively sort JSON object keys, preserve array order, use UTF-8 without
Unicode normalization, and apply a versioned domain separator before SHA-256.
