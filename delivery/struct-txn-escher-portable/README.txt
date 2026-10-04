PUB2019 STRUCT-TXN-ESCHER-01 PORTABLE
======================================

Purpose
-------
Run one bounded native Publisher 2019 experiment for PUB-T-351 / STRUCT-TXN-ESCHER-01.

This bundle does NOT need a rar2 checkout, Git, Cargo, Python, or Internet access.
It needs only Windows + the exact Publisher 2019 build already used by the project.

What it does
------------
1. Verifies the bundled Apache POI Sample.pub seed by SHA-256.
2. Opens a disposable copy in Publisher 2019 and performs one in-place Save.
3. Requires that this materializes the exact previously admitted T370 base:
   905bf75b00c0ff8680f61a20d5df4d843d753d3544a233fd237dc5a38a0a0599
4. Captures the base Contents/Escher structural manifest with the bundled helper.
5. Creates exactly one page-local non-text rectangle with non-default geometry/fill/line.
6. Save -> close -> fresh reopen.
7. Captures the post-mutation structural manifest and emits a bounded structural diff.
8. Creates a RETURN-TO-CHAT-STRUCT-TXN-ESCHER-*.zip containing only upload-safe receipts.

Run
---
Double-click RUN_NATIVE.cmd

Do not manually open Publisher while the run is active.

Return
------
Upload only the generated:
RETURN-TO-CHAT-STRUCT-TXN-ESCHER-*.zip

The changed PUB and full before/after manifests stay under out/private and are intentionally not placed in the return ZIP.
