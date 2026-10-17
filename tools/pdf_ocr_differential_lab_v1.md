# PDF-OCR-LAB-01 — native text versus raster OCR

**Status:** research-only, synthetic differential probe. No product OCR, no ABBYY code, no PDF modification.

## Rationale

A PDF can paint the correct word yet expose incorrect Unicode through `/ToUnicode`. An extracted text layer is therefore evidence, not proof of correct transcription. Before adding PDF search to Akula, establish which sources of text disagree.

## Reproduce

1. Install optional `PyMuPDF==1.26.7` and local Tesseract 5 with English language data.
2. Run `python -m unittest discover -s tools -p test_pdf_ocr_differential_lab_v1.py -v`.
3. Run `python tools/pdf_ocr_differential_lab_v1.py --synthetic`.

All four PDFs are generated in memory from controlled synthetic text. The runner accepts no external PDF path. JSON contains only synthetic hashes, counts, environment version and routing decisions. It is safe as a synthetic CI receipt but not a generalized user-document processing service.

## Four deterministic cases

| Case | Visible | Native extraction | Route |
|---|---|---|---|
| `native_vector` | HELLO 2026 | HELLO 2026 | `NATIVE_TEXT_CORROBORATED` |
| `wrong_tounicode` | HELLO 2026 | WELLO 2026 | `UNICODE_CONFLICT_REVIEW` |
| `raster_only` | image with HELLO 2026 | empty | `OCR_REQUIRED` |
| `invisible_wrong_layer` | image with HELLO 2026 | invisible WELLO 2026 | `HIDDEN_TEXT_LAYER_REVIEW` |

The first two PDFs produce byte-identical RGB pixels at 240 DPI, while the text extraction differs. Two parsers can agree with the corrupted `/ToUnicode`; pixel/OCR disagreement is an independent falsifier. The route is always **review**, not automatic replacement: OCR also makes mistakes.

## Product and security boundaries

- Keep input bytes unchanged. For Akula, first offer temporary searchable text; persistent output is an explicit later user action.
- Never treat `NATIVE_TEXT_CORROBORATED` as a universal guarantee; it is a two-engine observation under a pinned environment.
- Native extraction, glyph visibility, image count and OCR output are separate evidence fields.
- There is no confidence calibration yet, no word boxes, no multilingual fixtures, no layout reconstruction and no arbitrary PDF support.
- This probe uses PyMuPDF as a local optional research dependency (AGPL-3.0/commercial dual licensing). Do not embed or deploy it without a licensing review.
- Tesseract runs as a local subprocess with a 20-second timeout per generated page. No network, LLM, ABBYY license checks or proprietary binaries.
- If a future version accepts real documents, isolate the parser/rasterizer/OCR workers and never upload customer text/images/paths in CI.

## Follow-up: black-box ABBYY and Akula

1. On an authorized installed FineReader/SDK, pin edition, build, configuration, language and recognition settings; load only the four synthetic fixtures.
2. Measure native extraction, background recognition, copy/search outcome, cached layer lifetime, process/module and time profile. Record variation with background OCR on/off.
3. If SDK is available, inspect word/character recognition variants via its documented Voting API. Do not infer hidden internal decisions from the output alone.
4. Compare FineReader, Tesseract legacy/current and CuneiForm on a broader labeled corpus (Cyrillic, scans, columns, tables, damaged ToUnicode), measuring CER/WER, geometry and cost.
5. Only then design an Akula read-only OCR adapter and decide whether any custom classifier is justified.

## Sources

- ABBYY background recognition: https://help.abbyy.com/en-us/finereader/16/user_guide/backgroundrecognition/
- ABBYY Engine 12 Voting API: https://help.abbyy.com/en-us/finereaderengine/12/user_guide/guidedtour_voting/
- ABBYY Engine 12 distribution module list: https://help.abbyy.com/en-us/finereaderengine/12/user_guide/distribution_distributionkit/
- Existing bounded Chaptera PDF extraction: `tools/pdf_pub_extract_v1.py`.

**Not claimed:** reverse-engineered ABBYY binaries; ABBYY recognition quality; complete PDF parser; Akula production integration.
