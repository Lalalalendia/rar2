# Rescue Desktop — Windows GUI acceptance

This document complements `tools/probe_rescue_gui_window_windows.py`. It is **not** a claim that a clean Windows 10/11 machine or the private PUB corpus has already passed.

## Automated hosted-Windows gate

The portable CI must **build the complete ZIP, extract it to a new directory**, verify every file against `portable-build.json`, hide installed Python from the GUI's `PATH`, start the actual `chaptera-rescue.exe`, and observe a visible top-level Win32 window owned by that PID. The probe sends `WM_CLOSE`, waits for process exit, then starts the GUI again and terminates it abruptly, asserting its window disappears. No mocks, fake window handle, or silent skip on a headless runner.

Receipt: `chaptera.rescue-gui-window-smoke.v1`, source-free stdout. `pointer_events_proven=false` and `clean_machine=false` MUST remain false. Failure to open the window is a failing acceptance gate, not permission to skip the probe.

The existing Python batch/CFB physical tests continue to prove source immutability, dedup, replay, tamper rejection and limited worker cancellation independently. Those tests do not replace mouse/keyboard coverage.

## Independent Windows 10 and 11 user acceptance — required before release

On each clean Windows 10 x64 and Windows 11 x64 VM without Python or project checkout:

1. Extract the complete unsigned portable ZIP, preserve its `_internal` directory and verify the SHA-256 inventory; work offline and do not weaken malware/OS protections.
2. Launch `chaptera-rescue.exe` from Explorer. Record OS build, source Git SHA, portable artifact SHA, GPU/driver, exit code, window appearance, shutdown, and whether the UI renders legibly.
3. Use a synthetic CFB/PUB sample first. Drag and drop a source file, then a folder with Unicode paths, nested paths and a duplicate. Choose a dedicated output directory **outside** the inputs. Check the visible SHA/dedup/queue counts.
4. Click Scan, Run, Pause, Resume, Cancel and Retry using actual mouse/keyboard events. Assert each control visibly changes the *correct* batch; delayed Cancel must not stop a newer run.
5. While **at least two** salvage workers are active, close the GUI normally, then run a separate hard-kill case. Verify the engine and descendant PIDs exit within a bounded period, the user source hashes do not change and the private queue remains recoverable on restart.
6. Re-open the GUI, reuse the same output directory, resume interrupted files, revalidate completed bundles and reject a deliberately tampered synthetic bundle. Exercise repeated launch/close and a full/readonly disk case.
7. Capture a local screen recording or screenshots with **synthetic paths only**. Review startup, typography, text clipping, error messages, dialogs, focus, progress and final summary. Do not publish customer paths, names, PUB bytes, queue-private.json or raw bundle contents.
8. Repeat relevant checks with a real **authorized** set of damaged/healthy PUB files on an isolated machine. Record exact number of materialized SHA versus the 1,556 registered SHA manifest. Do not claim 1,556 physical passes unless 1,556 actual bytes were read and verified.

## Source-free acceptance record

Record one receipt per tested machine:

- `schema_version: chaptera.rescue-gui-clean-machine.v1`
- `source_commit`, `portable_zip_sha256`, Windows edition/build, architecture, isolated/offline flag and Python-installed flag
- Counts of button, keyboard, drag/drop, pause/cancel/retry, graceful/forced shutdown and restart scenarios actually exercised
- Exact synthetic source hash before/after and verified result classification; no raw source, client name or private paths
- Orphan engine/worker PID **count** after shutdown, never a blanket `processes_clean` assertion without a running worker
- Test outcome `pass`, `fail` or `not_run` per scenario, first failing step and bounded reproduction method
- Whether private real-corpus bytes were materialized, and `materialized_sha_count/1556` independent of synthetic samples
- Acceptance reviewer + artifact identifier, but no secrets, credentials or original PUB files

Release #2840 and GUI acceptance #2848 stay open until the real UI interaction matrix has been accepted. FirstVDS #528 stays frozen (`v0_go=false`).
