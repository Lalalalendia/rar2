# PUB Canonical V5: exact Notion identity observation

The connected Notion interface supplies `formulaResult://` references, not computed `Semantic Identity v1` strings. Never synthesize the string from Claim fields.

The read-only utility `canonical_v5_identity_receipt.py` performs four direct Notion API GET requests: page, formula property, page again, formula again. It checks that the Claim still belongs to the correct canonical data source, ID, revision, status, semantic body and `last_edited_time` are unchanged, and the Notion-calculated formula is an actual string. It refuses redirects, does not create snapshots or merge, and writes a private, non-overwriting JSON observation outside the Git checkout.

Use **only** on an authorized local computer with an already-provisioned `NOTION_TOKEN`. Never paste the token into CLI arguments, GitHub, Notion, or logs.

For CHG40 / C154, in PowerShell from the repository root:

```powershell
New-Item -ItemType Directory -Force -Path "$env:LOCALAPPDATA\PUB-V5-Private" | Out-Null
python tools/notion/canonical_v5_identity_receipt.py `
  --claim-page-id 3f132a84-beec-81b2-8e76-ceed28680f77 `
  --expect-claim-id PUB-C-154 --expect-revision 1 --expect-status OPEN `
  --output "$env:LOCALAPPDATA\PUB-V5-Private\c154-observed-identity.json"
```

For C156 / CHG42, page ID: `3f132a84-beec-8188-be20-dd1e141f6c2d` and expected ID `PUB-C-156`. Use another private output filename.

Offline tests without any Notion connection:

```sh
python -m unittest discover -s tools/notion -p 'test_canonical_v5_identity_receipt.py' -v
```

A read-only observation is **not** a legitimate V5 `proposal-base` snapshot, Verification Run, raw-PUB verifier or admission. The serialized canonical merger must independently confirm current authority, evidence, membership and exact formula before creating a snapshot or event. On HTTP 401/403/404 or drift, keep CHG in `draft`, Claim `OPEN`, and record the blocker.
