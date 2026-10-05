QUILL SCHEME TEXT COLOR AUTHORITY - PORTABLE V2
================================================

Purpose
-------
Run the bounded native Publisher2019 experiment for GitHub #1405 without a local
rar2 checkout, Git, Python, Rust/Cargo, network access, or GitHub runner registration.

External requirement
--------------------
Microsoft Publisher 2019 exact environment already used by Chaptera authority work:
- Application.Version 16.0
- Application.Build 12527
- MSPUB.EXE file version 16.0.12527.22145
- MSPUB.EXE SHA-256 e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b

Run
---
1. Save and close your Publisher documents.
2. Extract the bundle.
3. Double-click RUN_NATIVE.cmd as the normal interactive user. Do not Run as Administrator.
4. When it finishes, send RETURN-TO-CHAT-<timestamp>.zip back to ChatGPT.

What it does
------------
A. Creates a disposable native Publisher publication containing:
   - 8 one-character ranges assigned SchemeColor roles 1..8;
   - 1 explicit RGB control.
   Save -> Close -> fresh Reopen is mandatory.

B. Copies that saved synthetic PUB, finds a second installed Publisher ColorScheme
   whose effective eight-color vector differs in at least two slots, applies it,
   Save -> Close -> fresh Reopen, and verifies:
   - all eight characters remain bound to the same SchemeColor roles;
   - at least two effective RGB values change;
   - the explicit RGB control remains unchanged.

C. Opens the bundled exact public Carlton March PUB read-only, records its current
   8-role scheme tuple privately, and emits only an ordered tuple SHA-256 fingerprint
   in the public-safe analysis receipt.

D. Runs a bundled static Quill probe over both generated synthetic PUBs and Carlton:
   - binds COM SchemeColor roles 1..8 to persisted Quill eight-slot scheme references;
   - verifies the persisted role->slot mapping is unchanged after document ColorScheme switch;
   - retains only source-safe aggregate Carlton scheme-slot counts in the public receipt.

Returned evidence
-----------------
The RETURN-TO-CHAT ZIP contains:
- source-safe analysis/native-scheme-color-auth.json;
- source-safe analysis/carlton-scheme-fingerprint.json;
- private native COM snapshots;
- private generated synthetic PUBs needed for later Quill carrier parsing;
- run log and manifest.

The generated synthetic PUBs are private handoff evidence. They are not uploaded to
GitHub by this bundle.

Fences
------
- no assumption that persisted Quill slot 0..7 maps to COM role 1..8;
- no default-black inference;
- no mutation of the bundled Carlton source;
- no PDF/raster-derived color semantics;
- no process killing;
- no automatic closing of user documents;
- no network access during native execution.
