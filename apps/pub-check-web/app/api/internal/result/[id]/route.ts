import { get } from '@vercel/blob';
import { createHash } from 'node:crypto';

import { internalAuthorized } from '../../../../../lib/auth';
import { projectCanonicalReport } from '../../../../../lib/canonical-report';
import {
  type CheckResult,
  type Compatibility,
  type LegacyCheckResult,
  type CheckRecord,
  readCheck,
  writeCheck,
} from '../../../../../lib/checks';
import { sendResultEmail } from '../../../../../lib/email';

const MAX_RECEIPT_BYTES = 64 * 1024;
const MAX_SOURCE_BYTES = 64 * 1024 * 1024;

const COMPATIBILITY: Compatibility[] = [
  'compatible', 'partial', 'unsupported', 'invalid', 'failed',
];

async function boundedJson(request: Request): Promise<unknown> {
  if (!request.body) throw new Error('missing body');
  const declared = request.headers.get('content-length');
  if (declared && Number(declared) > MAX_RECEIPT_BYTES) throw new Error('receipt too large');
  let size = 0;
  const chunks: Uint8Array[] = [];
  const reader = request.body.getReader();
  try {
    for (;;) {
      const next = await reader.read();
      if (next.done) break;
      size += next.value.byteLength;
      if (size > MAX_RECEIPT_BYTES) throw new Error('receipt too large');
      chunks.push(next.value);
    }
  } finally {
    reader.releaseLock();
  }
  const all = Buffer.concat(chunks);
  return JSON.parse(all.toString('utf8')) as unknown;
}

// The private Blob, not the received JSON and not the uploaded filename,
// is the authority for the exact file identity. No source bytes are persisted
// in the public check receipt.
async function hashAdmittedPrivateSource(record: CheckRecord): Promise<string | null> {
  if (!Number.isSafeInteger(record.source.byteLength)
      || record.source.byteLength <= 0
      || record.source.byteLength > MAX_SOURCE_BYTES) return null;
  const source = await get(record.source.url, { access: 'private', useCache: false });
  if (!source || source.statusCode !== 200 || !source.stream) return null;
  const hash = createHash('sha256');
  const reader = source.stream.getReader();
  let count = 0;
  try {
    for (;;) {
      const part = await reader.read();
      if (part.done) break;
      count += part.value.byteLength;
      if (count > record.source.byteLength) return null;
      hash.update(part.value);
    }
  } finally {
    reader.releaseLock();
  }
  return count === record.source.byteLength ? hash.digest('hex') : null;
}

function validLegacyResult(value: unknown): value is LegacyCheckResult {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const r = value as LegacyCheckResult;
  return (
    COMPATIBILITY.includes(r.compatibility) &&
    typeof r.summary === 'string' && r.summary.trim().length > 0 &&
    r.summary.length <= 4000 &&
    (r.pages === undefined ||
      (Number.isSafeInteger(r.pages) && r.pages >= 0 && r.pages <= 100000)) &&
    (r.publisherFamily === undefined || (typeof r.publisherFamily === 'string' && r.publisherFamily.length <= 180)) &&
    (r.diagnosticsCode === undefined || (typeof r.diagnosticsCode === 'string'
      && /^pub_check\.[a-z0-9_]{1,60}$/.test(r.diagnosticsCode))) &&
    (r.checkerVersion === undefined || (typeof r.checkerVersion === 'string' && r.checkerVersion.length <= 128)) &&
    (r.limitations === undefined ||
      (Array.isArray(r.limitations) && r.limitations.length <= 16 &&
        r.limitations.every(item => typeof item === 'string' && item.length <= 900)))
  );
}

export async function POST(
  request: Request,
  context: { params: Promise<{ id: string }> },
) {
  if (!internalAuthorized(request)) {
    return new Response('Unauthorized', { status: 401 });
  }

  const { id } = await context.params;
  const record = await readCheck(id);
  if (!record) return Response.json({ error: 'Check not found.' }, { status: 404 });

  if ((record.status === 'complete' || record.status === 'failed') && record.result) {
    return Response.json({
      ok: true, alreadyFinalized: true,
      status: record.status, emailStatus: record.emailStatus,
    });
  }

  let raw: unknown;
  try {
    raw = await boundedJson(request);
  } catch {
    return Response.json({ error: 'Invalid or oversized result.' }, { status: 400 });
  }

  let result: CheckResult;
  if (raw && typeof raw === 'object' && 'protocol_version' in raw) {
    // Only this branch may produce the canonical Cloud Reader public verdict.
    let exactSourceSha: string | null;
    try {
      exactSourceSha = await hashAdmittedPrivateSource(record);
    } catch {
      exactSourceSha = null;
    }
    if (!exactSourceSha) {
      return Response.json({ error: 'Private source identity unavailable.' }, { status: 409 });
    }
    const canonical = projectCanonicalReport(raw, exactSourceSha);
    if (!canonical) {
      return Response.json({ error: 'Canonical report authority mismatch.' }, { status: 400 });
    }
    result = { canonical };
  } else if (validLegacyResult(raw)) {
    // Keep in-flight older worker receipts compatible during deployment.
    // Never treat them as the canonical Cloud Reader protocol.
    result = {
      compatibility: raw.compatibility,
      summary: raw.summary.trim(),
      ...(raw.publisherFamily !== undefined ? { publisherFamily: raw.publisherFamily } : {}),
      ...(raw.pages !== undefined ? { pages: raw.pages } : {}),
      ...(raw.diagnosticsCode !== undefined ? { diagnosticsCode: raw.diagnosticsCode } : {}),
      ...(raw.limitations !== undefined ? { limitations: raw.limitations.map(s => s.trim()).filter(Boolean) } : {}),
      ...(raw.checkerVersion !== undefined ? { checkerVersion: raw.checkerVersion } : {}),
    };
  } else {
    return Response.json({ error: 'Result does not match a supported bounded report schema.' }, { status: 400 });
  }

  record.result = result;
  record.status = 'canonical' in result || result.compatibility !== 'failed' ? 'complete' : 'failed';
  record.updatedAt = new Date().toISOString();
  await writeCheck(record);

  record.emailStatus = await sendResultEmail(record);
  record.updatedAt = new Date().toISOString();
  await writeCheck(record);

  return Response.json({
    ok: true, status: record.status, emailStatus: record.emailStatus,
  });
}
