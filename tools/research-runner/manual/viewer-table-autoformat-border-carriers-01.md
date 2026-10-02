# VIEWER-TABLE-AUTOFORMAT-BORDER-CARRIERS-01 native runbook

This experiment maps Publisher TABLE AutoFormat border/decor carriers to individual COM cell-border sides without using geometry or PDF pixels as semantic authority.

## Existing authority

- #735 already consumes only the exact T840 cell-fill carrier law.
- #737 proves the remaining TABLE-linked cohort has no cell ordinal and uses ClientAnchor field families around `0x6802 + 0x2001/0x2004..0x2007`.
- T840 v9 proves `Borders=False` removes this category independently from fill/text.
- T595 proves independent `BorderTop/Right/Bottom/Left` setters persist and mirror to the adjacent cell's opposite side.

## Causal experiment

The operation creates a fresh 4x4 table, applies CheckbookRegister with all categories enabled, saves/reopens, then creates four matched arms from the same persisted fixture.

Each arm changes **only one** `R2C2` border side's `Color.RGB`:

- top
- right
- bottom
- left

After SaveAs/close/fresh reopen it requires:

1. the target side changes through COM;
2. the adjacent cell's opposite side changes through COM;
3. raw OfficeArt diff localizes at least one TABLE-linked border/decor carrier.

The raw oracle uses complete ClientAnchor values only as private matching keys. Upload-safe evidence emits only anchor **field-ID signatures**, changed FOPT property IDs and carrier counts. It never emits anchor values, object IDs, RGB values, coordinates or source bytes.

## Verdict

- `border-side-carrier-delta-localized`: all four independent side mutations persist and each causes a raw carrier delta. Use the per-side anchor signature/property-ID receipt to define a bounded Reader border consumer.
- `inconclusive`: any COM persistence/mirroring or raw localization guard fails. Do not infer sides.

## Fences

- no geometry/proximity side inference;
- no default grid;
- no broad `0x6802` admission;
- no reuse of border/decor carriers as fill;
- no merged-cell widening;
- no PDF-derived color, width or side semantics.
