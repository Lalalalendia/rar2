# Chaptera Rescue Desktop — UI acceptance

**Owner:** RESCUE-DESKTOP-UI-01 (#2853 / Draft PR #2854). This checklist covers the egui shell, not the native mouse Scan/Run release gate (#2848 / paused Draft #2852). FirstVDS #528 is frozen.

## Visual reference

Reuse the *principles* from user-owned Canva Chaptera Editor UI v4 and Cloud Cabinet:

- Accent `#6953E8`, main text `#222536`, canvas `#E9EBF0`, border `#E3E5ED`.
- 8 px rhythm, clear content hierarchy, white cards, visible empty states and limitations.
- Labels must supplement colors. No faux-green "recovered" state before the engine reports it.

The native Rust/egui interface is the product. A Canva concept or HTML mock is not visual acceptance evidence for the built Windows application.

## Required window states

| State | What must remain visible |
| --- | --- |
| Fresh launch | Chaptera branding, offline technical-preview label, Batch recovery default tab, source/output fields, Scan and Run actions, honest empty results |
| Valid source paths | Long Unicode paths don't obscure the primary controls, which remain in a scrollable content area |
| Running Scan/Run | Busy status with textual message; paths protected from edits; Pause/Cancel enabled only for an active Run |
| Switching to Validate evidence during Run | Worker continues under existing process guard; reaping and progress polling remain active after switching tabs |
| Completed batch | Count of unique SHA sources, partial/unsupported/failed/cancelled, duplicates, rejections and source-free short hashes agree with canonical `summary.json` |
| Bad or absent summary | Old queue metrics disappear, explicit error is shown, no success color derived from an invalid summary |
| Evidence selected | Original SHA inspection, source-matched receipt, independent JSON export are separate steps |
| Source or receipt path changed | Previous validated proof and prior success text disappear until revalidation |
| Source file changed externally | Export still re-hashes source before and after; no unverified native repaired PUB claim |
| Advanced settings | Python, checkout, optional CLI and worker count are behind disclosure; portable mode does not require installed Python |

## Viewports and accessibility

Perform a real **Windows** visual inspection at client areas approximately 860×640 (declared minimum), 1180×820 (default), and larger than 1440×900, with 100%, 125%, and 150% scale as practical. Record the exact executable SHA, host OS, viewport and scaling.

- No overlapping cards, clipped action labels, uncontrolled horizontal scrolling, unreadable tooltips, or invisible focus targets.
- Scroll position and section switching must not hide active operation status or make cancellation impossible after returning to Batch.
- Keyboard Tab traversal and text fields remain usable; errors are communicated in text, not only by hue.
- User-facing percentages must be derived from source-free per-status counts, never fabricated.
- Use **synthetic** PUB and source-free screenshots only. Do not publish real file names, paths, book contents, source bytes or private runtime state.

## What CI proves today

The existing `Rescue Desktop Portable Windows V1` workflow builds a portable ZIP, hashes its component inventory, runs synthetic frozen CFB checks, opens a real DX12 GUI window, and tests WM_CLOSE/relaunch/forced exit. **It does not capture layout screenshots or prove OS pointer events.**

Native physical pointer Scan/Run remains explicitly user-paused (#2852), and this UI PR must neither re-enable that workflow nor claim the paused release gate is satisfied.

Mark the visual requirement **accepted** only after an independently inspected real-window screenshot/record for the exact PR head shows the prescribed states and viewports. Otherwise keep the PR Draft even with green compilation.
