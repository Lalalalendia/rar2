# Manual Publisher experiment — AUTOFIT-2019-01

## Purpose

Determine the persisted behavior of Publisher 2019 text-fit modes using one fixed text box. Your role is only to apply UI settings and save named files.

## Environment to record

Before starting, write down:
- Publisher version/build from **File → Account → About Publisher**.
- Windows version.

## Create the baseline

1. Start Publisher 2019.
2. Create a new blank publication.
3. Insert one text box.
4. Set the text box size to approximately **2.0 in × 0.6 in**. Use the same box for every arm; exact size is less important than keeping it unchanged between arms.
5. Enter exactly:

   `Chaptera Publisher AutoFit discriminator with enough text to overflow a deliberately small frame.`

6. Select all text.
7. Set font to **Arial**, **18 pt**.
8. Set Text Fit to **Do Not Autofit / None**.
9. Save as `autofit-base.pub`.
10. Close the document.

## Arms

Always reopen `autofit-base.pub` before each arm.

### NONE
1. Leave Text Fit at **Do Not Autofit / None**.
2. Save As `autofit-none.pub`.
3. Close.

### SHRINK
1. Reopen `autofit-base.pub`.
2. Set Text Fit to **Shrink Text on Overflow**.
3. Save As `autofit-shrink.pub`.
4. Close.

### BEST
1. Reopen `autofit-base.pub`.
2. Set Text Fit to **Best Fit**.
3. Save As `autofit-best.pub`.
4. Close.

### GROW — separate UI discriminator
1. Reopen `autofit-base.pub`.
2. Set Text Fit to **Grow Text Box to Fit**.
3. Save As `autofit-grow.pub`.
4. Close.

If your Publisher build uses slightly different wording, record the exact wording you see. Do not substitute another setting when one is absent.

## Required return

Return privately:
- `autofit-base.pub`
- `autofit-none.pub`
- `autofit-shrink.pub`
- `autofit-best.pub`
- `autofit-grow.pub`
- a short note with Publisher build, Windows version, and exact UI wording for the four modes

Attaching the files directly in the Chaptera/ChatGPT project is acceptable.

## Do not do

- Do not manually resize the box after applying a Text Fit mode.
- Do not change text/font/size between arms.
- Do not create one arm from another arm.
- Do not upload the PUB files to the public `rar2` repository.
