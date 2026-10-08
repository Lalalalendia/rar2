# Chaptera PUB Check landing

A small public surface for validating real Microsoft Publisher files before the
desktop Reader is ready for broad distribution.

## User flow

1. User drops a `.pub` file and provides an email address.
2. Browser uploads directly to a **private Vercel Blob** object.
3. The app creates a private check record and dispatches a worker job.
4. The checker downloads the exact private file through an authenticated
   internal endpoint.
5. The checker returns a bounded JSON compatibility receipt.
6. Only after that receipt is accepted, the app sends the report by email.
7. File + record expire after seven days. Uploads that never become a check are
   removed after one day.

The landing must never claim compatibility merely because upload succeeded.

## Environment

```
BLOB_READ_WRITE_TOKEN=...
CHECKER_ADMIN_TOKEN=<long random shared secret used by checker for source/result endpoints>
GITHUB_CHECKER_TOKEN=<fine-grained GitHub token with Actions write access to the checker repo>
GITHUB_CHECKER_REPOSITORY=Lalalalendia/rar2
GITHUB_CHECKER_REF=main
PUB_CHECK_CLOUD_READER_ORIGIN=https://reader.chaptera.online
CHECKER_WEBHOOK_URL=<optional fallback checker webhook>
CHECKER_WEBHOOK_TOKEN=<optional auth from landing to fallback checker>
RESEND_API_KEY=...
REPORT_FROM_EMAIL=Chaptera <reports@your-domain.example>
CRON_SECRET=<long random Vercel cron secret>
```

Attach a **private** Vercel Blob store to the deployed project. Client uploads
are capped at 64 MB.

## Checker dispatch contract

The preferred zero-new-server path is GitHub Actions. When `GITHUB_CHECKER_TOKEN`
is configured, the landing dispatches `.github/workflows/pub-check-worker.yml`
with only the opaque `check_id`. The worker builds the source/result URLs from
the **GitHub Secret** `PUB_CHECK_ORIGIN`, so a manual workflow dispatch cannot
redirect `CHECKER_ADMIN_TOKEN` to an arbitrary host.

Configure these GitHub repository secrets after merge:

```
PUB_CHECK_ORIGIN=https://<canonical-landing-host>
CHECKER_ADMIN_TOKEN=<same high-entropy value configured on the landing>
```

The fine-grained token stored on Vercel should have the minimum repository
permission required to dispatch Actions workflows.

If `GITHUB_CHECKER_TOKEN` is absent, an optional generic checker can be used.
When `CHECKER_WEBHOOK_URL` is configured, the landing sends:

```json
{
  "schema": "chaptera.pub-check-dispatch.v1",
  "checkId": "<uuid>",
  "sourceUrl": "https://landing.example/api/internal/source/<uuid>",
  "resultUrl": "https://landing.example/api/internal/result/<uuid>"
}
```

The generic checker is configured separately with `CHECKER_ADMIN_TOKEN` and uses:

```
Authorization: Bearer <CHECKER_ADMIN_TOKEN>
```

to fetch `sourceUrl` and POST `resultUrl`.

## Result schema (Cloud Reader canonical authority)

The current `.github/workflows/pub-check-worker.yml` (default repository
`Lalalalendia/rar2`) now calls the existing Chaptera **Cloud Reader guest-session
API**, not the older Viewer-only `tools/pub-check-cli`. It sends the exact
private uploaded PUB to the fixed `https://reader.chaptera.online` origin,
receives `chaptera.reader-compatibility-report.v1`, and returns **only**
the service-owned compatibility report (no Scene, Story text or source bytes).
The worker uses no Rust toolchain.

Server-side `/api/internal/result/:id` **re-reads the private Vercel Blob**,
computes SHA-256 of the exact stored bytes and requires the report
`source_sha256` to match. It only accepts the known canonical
state/route/limitation message matrix and strips unknown private fields.
Public polling remains protected by the existing check token. The email uses
the same source-safe canonical result.

Canonical states are `opens_normally`, `needs_review`,
`opens_with_salvage`, and `unsupported`. `content_summary` contains bounded
numeric counts only; `limitations` are server-owned customer-safe messages;
`output_routes` and `recommended_next_step` are not guessed by the web app.
The current deployed server returns both editable routes as `not_verified`.
A newer Cloud API declaring other editable states must be separately admitted
and tested before this public bridge can expose that claim.

Failure to contact Cloud Reader or a rejected/unsafe source produces a bounded
transport `failed` result, never a fabricated document compatibility decision.

For a rolling deployment, previously queued Viewer-style result records remain
readable as legacy reports. New Cloud responses use the canonical schema only.
The separate `CHECKER_WEBHOOK_URL` seam can be used for a trusted, bounded
service adapter which follows the same canonical result contract.

### Source-free tests

```bash
cd apps/pub-check-web
npm install
npm run typecheck
node --experimental-strip-types --test tests/canonical-report.test.mjs

cd ../..
python3 -m unittest discover -s tools/pub-check-cloud-bridge -p "test_*.py" -v
```

## Retention

`GET /api/internal/cleanup` is configured as a daily Vercel Cron endpoint and
requires `Authorization: Bearer $CRON_SECRET`. It removes expired checks and
old orphan uploads.

## Development

```bash
cd apps/pub-check-web
npm install
npm run typecheck
npm run build
npm run dev
```

The frontend can be previewed without a checker, but checks remain queued until
GitHub Actions or the fallback webhook checker is connected. That is deliberate; the UI does not fake a
successful compatibility result.
