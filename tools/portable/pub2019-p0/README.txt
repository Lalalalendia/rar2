PUB2019 PORTABLE P0 V1
=======================

TRUE SELF-CONTAINED HANDOFF.

External requirement: installed Microsoft Publisher 2019 exact build 16.0.12527.22145 only.

NOT REQUIRED:
- local rar2/rar/PUBTool checkout
- Git
- GitHub or network
- Python installation
- Rust/Cargo
- PowerShell 7
- local fixture files
- GitHub Actions runner registration

Run:
1. Save any Publisher work. The launcher will safely close an already-running Publisher session only when it has zero open documents; it never kills MSPUB.EXE or closes user documents automatically.
2. Extract the ZIP.
3. Double-click RUN_NATIVE.cmd as the normal interactive user. DO NOT Run as Administrator.
4. When complete, send RETURN-TO-CHAT-<timestamp>.zip back to ChatGPT.

Validation:
- bundle manifest verifies every carried file before COM
- paragraph-metrics-probe.exe is built with static MSVC CRT; PE dependencies are audited to exclude VCRUNTIME/MSVCP/UCRT imports and rechecked before COM
- 033 receipt is checked against the same verdict/arm/visibility/source fences as the direct launcher
- 029 native receipt + classifier are checked fail-closed before success
- paragraph native/blast/structural receipts + evidence manifest are cross-checked before success

Payload:
- exact 033 and 029 PUB bytes recovered from retained corpus artifact 11038217075
- pinned current research operations and PubRuntime
- embedded official CPython 3.13.16 x64 runtime
- prebuilt paragraph-metrics-probe.exe
- current source-safe analyzers/classifier

Paragraph note:
The historical 5bf605... profile-instantiated blank was private-local and is no longer remotely retained.
This portable bundle therefore creates its blank via Publisher itself (NewDocument/Documents.Add),
verifies one page / zero shapes, then runs the same common-seed eleven-arm paragraph matrix.
This provenance difference is explicit in the returned environment receipt; it is not silently
represented as the historical 5bf605 fixture.
