# Chaptera Failure Intake Protocol

This crate defines the source-neutral protocol boundary for explicit compatibility-research contribution.

It deliberately does **not** own HTTP, storage, guest-session lifetime, scanning, clustering, or UI.

The client may assert only the protocol and consent versions. Classification, failure evidence, exact-byte identity, storage identity, deduplication and retention outcomes are server authorities.

Exact-file contribution is fail-closed to the canonical `PUB_HIGH_VALUE` and `PUB_DAMAGED` classes.
