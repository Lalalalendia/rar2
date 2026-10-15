# CI-READER-RELEASE-EVENT-DEPLOY-01 — protected event after immutable release

Owner: [Notion task](https://app.notion.com/p/3fa32a84beec81209c73ee130de32eb5).
Paired site receiver: [chaptera-site PR #32](https://github.com/Lalalalendia/chaptera-site/pull/32).

## Source and promotion boundary

The protected-main `Cloud Reader Linux release packet` first publishes a SHA-bound
immutable Linux asset. Its separate read-only `workflow_run` successor requires
a successful **push on the exact Lalalalendia/rar2 main repository**, not a fork,
PR or manual invocation. It verifies `workflow_run.head_sha`, public release tag
and `target_commitish`, SHA256-digested nonempty asset, latest eligible published
release and ancestor relationship to current protected main. Untrusted workflow
artifacts are never loaded in this privileged successor.

After validation, a **short-lived GitHub App token limited to Actions:write** on
private `Lalalalendia/chaptera-site` calls that repository's existing
`deploy-reader-backend.yml` with `release_sha=<exact-40hex>` and
`release_mode=published`. No PAT, SSH credentials or `contents:write` are granted
to the source workflow. The site independently verifies the release, latest
published timestamp, digest, main ancestry and that the current host does
not already contain a descendant of the candidate. The same single-host
production environment performs rollback-provenance, real PUB, public asset
byte identity and public Chromium PDF/IDML edit/export/delete acceptance.
Existing site cron every ten minutes remains a fallback. An operator's explicit
`release_mode=manual` remains the only rollback / reinstall path.

## External provisioning

1. Review/merge the site receiver PR before source sender promotion.
2. Register/install a dedicated GitHub App with **Actions:write**, metadata read,
   scoped **only** to `Lalalalendia/chaptera-site`.
3. In `rar2` configure variable `CHAPTERA_DEPLOY_APP_CLIENT_ID` and protected
   secret `CHAPTERA_DEPLOY_APP_PRIVATE_KEY`. Do not commit the key.
4. Missing credentials produce a visible warning and no event dispatch;
   no claim of event-driven production is made. Cron continues.
5. On the next real immutable release, verify `rar2` dispatch run, pinned SHA
   site workflow run, installed `current`/`previous` SHA, and physical public
   source-neutral Chromium acceptance. Measure `published_at` to host and public
   PASS time rather than assuming a target improvement.

## Negative controls

`python3 tools/ci/test_cloud_reader_release_dispatch_guard.py` covers failure,
manual/fork/non-main/untrusted workflow, malformed SHA, missing digest or asset,
stale publication and non-ancestor main. The site policy suite additionally
tests no automatic rollback over a newer, same or diverged installed commit.
These tests do not provision the App or replace real HTTPS public acceptance.
