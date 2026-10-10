# Verified local RAW packs — personal translation pilot

This tool creates **small, private ZIP packs of five (at most ten) chapters** for the two first-choice finished novels. It is not a server, a public chapter mirror, a translator, or a Rulate publisher. Nothing stores original chapters in GitHub or Notion.

The underlying metadata-only [RAW-CENSUS run](https://github.com/Lalalalendia/rar2/actions/runs/38050639248) was successful at SHA `1d4dfb85199864f95c8eed0b05203abb32326f08`. All 166 + 201 chapter bodies were validated; **the census did not save the original prose**.

## Local prerequisites

- Python 3.12, `requests==2.32.3`, `beautifulsoup4==4.12.3`
- Download **metadata-only** artifacts `raw-census-fencing` (artifact 11668859804) and `raw-census-return` (artifact 11669875027) from that run before their expiration on October 24, 2026
- Unzip each artifact in a **local private directory** and point to `fencing.summary.json` / `fencing.manifest.json`, or corresponding `return.*.json`.
- Output must be outside this repository. Respect the source site's current rules and any applicable terms; no bypass of authentication, paywall, CAPTCHA, or robots restrictions.

## Run

```sh
python -m pip install requests==2.32.3 beautifulsoup4==4.12.3
python research/translation_pilot/export_verified.py \
  --book fencing \
  --summary /path/to/private/fencing.summary.json \
  --manifest /path/to/private/fencing.manifest.json \
  --start 1 --count 5 \
  --output /path/to/private/output
```

For the second book use `--book return` and its matching metadata files. A later range uses `--start 6 --count 5`. The exported ZIP contains `001.txt` through `005.txt` plus `index.json` recording chapter URLs, canonical source SHA-256 and rendered text SHA-256. No output is created until **all five** chapter texts match the pinned census digest and character count. A stale/edited source, missing body, non-contiguous census, duplicate chapter hash, source mismatch, foreign host, denied robots policy or blocked HTTP response fails without creating a partial pack.

**Important:** This tool was tested only with synthetic local fixtures. The current conversation environment does not allow direct web downloads in the working container, and no actual chapter ZIP has been generated or translated in this PR. Run locally with the original metadata receipts and inspect paragraph formatting.

## Tests (offline; no fiction text in CI)

```sh
python -m unittest discover -s research/translation_pilot -p 'test_*.py' -v
```

Only synthetic paragraphs are used. The CI workflow does not download a single external novel page.

## Workflow integration

After exporting a five-chapter private pack, perform a manual pilot translation from the first source chapter. Use the existing Notion policies (RAW → actual full RU page → quality check → queue). No `Draft RU` claim before the **complete Russian chapter body** is written to and fetched again from its canonical page. Public posting needs a separate rights decision.