# Manual Publisher experiment — OPENTYPE-STYLISTICSETS-2019-01

## Purpose

Determine whether Publisher 2019 persists and restores Stylistic Sets state for one text run. This is a controlled semantic experiment. Do not inspect binary bytes manually.

## Environment to record

Before starting, write down:
- Publisher version/build from **File → Account → About Publisher**.
- Windows version.
- Whether the font **Gabriola** is available.

If Gabriola is unavailable, stop and report that fact. Do not substitute another font without changing the experiment ID.

## Create the baseline

1. Start Publisher 2019.
2. Create a new blank publication.
3. Insert one text box.
4. Enter exactly:

   `office affine stylistic alternate sample`

5. Select the whole text.
6. Set font to **Gabriola**, **24 pt**.
7. Save as `opentype-ss-base.pub`.
8. Close the document.

## Arms

For every arm below, reopen `opentype-ss-base.pub` so that arms do not inherit from one another.

### SS0
1. Select all text in the single text box.
2. In Publisher typography controls, choose the default/no stylistic-set state.
3. Save As `opentype-ss-0.pub`.
4. Close Publisher document.

### SS1
1. Reopen `opentype-ss-base.pub`.
2. Select all text.
3. Choose **Stylistic Set 1**.
4. Save As `opentype-ss-1.pub`.
5. Close.

### SS2
Repeat from the baseline with **Stylistic Set 2** and save `opentype-ss-2.pub`.

### SS3
Repeat from the baseline with **Stylistic Set 3** and save `opentype-ss-3.pub`.

If the UI does not expose a requested set, record `UNAVAILABLE` for that arm rather than improvising.

## Required return

Return these files through a controlled/private channel, not a public repository:

- `opentype-ss-base.pub`
- `opentype-ss-0.pub`
- `opentype-ss-1.pub`
- `opentype-ss-2.pub`
- `opentype-ss-3.pub`
- a short text note containing Publisher build, Windows version, and any unavailable arm

Attaching the files directly in the Chaptera/ChatGPT project is acceptable.

## Do not do

- Do not edit any other typography property.
- Do not change font, size, box geometry, or text between arms.
- Do not resave one arm into the next arm.
- Do not upload these files to the public `rar2` repository.
