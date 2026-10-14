# Publisher VBA Migration Plan V1

`pub_vba_migration_plan.py` converts the **source-free** receipt from
`pub_vba_estate_scan.py` into a bounded Chaptera migration plan.

It is not a VBA translator and never executes code. It operates only on the
scanner's aggregate per-file call-family histogram.

## Contract

Input:

- `chaptera.pub-vba-estate-scan.v1`
- scanner must assert:
  - `vba_executed=false`
  - `ole_com_activated=false`
  - `source_text_emitted=false`

Output:

- `chaptera.pub-vba-migration-plan.v1`
- mapping version `v1`
- per family:
  - `automatic` — a bounded semantic family maps to a Chaptera recipe/API primitive;
  - `user_choice` — ambient Publisher state or external-resource policy must be made explicit;
  - `unsupported` — no bounded mapping exists yet.
- composite recipe candidates for recurrent call combinations.

**Important:** `automatic` means *family-level semantic mapping*, not that an
arbitrary VBA procedure can be translated without control-flow/data-flow
analysis or user review.

## V1 user-choice boundaries

Three Publisher surfaces deliberately do not auto-translate:

- `Selection` → replace ambient UI state with an explicit selector;
- `ScratchArea` → choose how off-page objects should be scoped;
- OLE/link update → choose an explicit inspect/relink/update policy; never activate COM implicitly.

Unknown future call families fail open into the report as `unsupported`; they
are never silently treated as translated.

## Composite recipes

V1 recognizes high-value combinations:

- MailMerge + output → Data Recipe + Batch Output;
- pages + stable page identity + hyperlinks → Computed Page Reference Refresh;
- shapes + pictures → Bulk Asset Replace;
- MailMerge + tables → Data-bound Table Recipe;
- documents + output → Batch File Output.

## Usage

```bash
python tools/pub_vba_migration_plan.py \
  --scan scanner-receipt.json \
  --output migration-plan.json
```

The output omits local source paths, module source and symbol-level source text.
