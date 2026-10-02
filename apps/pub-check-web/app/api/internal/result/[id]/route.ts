import { internalAuthorized } from '../../../../../lib/auth';
import {
  type CanonicalCompatibilityResult,
  type CanonicalCompatibilityState,
  type CanonicalOutputRouteState,
  type CheckResult,
  type Compatibility,
  readCheck,
  writeCheck,
} from '../../../../../lib/checks';
import { sendResultEmail } from '../../../../../lib/email';

const COMPATIBILITY: Compatibility[] = [
  'compatible',
  'partial',
  'unsupported',
  'invalid',
  'failed',
];

const CANONICAL_STATES: CanonicalCompatibilityState[] = [
  'opens_normally',
  'needs_review',
  'opens_with_salvage',
  'unsupported',
];

const ROUTE_STATES: CanonicalOutputRouteState[] = [
  'available',
  'available_with_limitations',
  'available_with_declared_losses',
  'unavailable',
  'not_verified',
  'not_applicable',
];

const NEXT_STEPS: CanonicalCompatibilityResult['recommendedNextStep'][] = [
  'migration_pilot_preview',
  'review_preview_before_migration',
  'rescue_review',
  'unsupported_or_manual_review',
];

type JsonObject = Record<string, unknown>;

function object(value: unknown): JsonObject | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as JsonObject)
    : null;
}

function boundedString(value: unknown, max: number) {
  return typeof value === 'string' && value.length > 0 && value.length <= max;
}

function canonicalResult(value: unknown): CanonicalCompatibilityResult | null {
  const report = object(value);
  if (!report || report.protocol_version !== 'chaptera.reader-compatibility-report.v1') {
    return null;
  }
  if (
    typeof report.source_sha256 !== 'string' ||
    !/^[0-9a-f]{64}$/.test(report.source_sha256)
  ) {
    return null;
  }
  if (
    typeof report.state !== 'string' ||
    !CANONICAL_STATES.includes(report.state as CanonicalCompatibilityState)
  ) {
    return null;
  }
  if (!boundedString(report.engine_classification, 64)) return null;

  const summary = report.content_summary === undefined || report.content_summary === null
    ? null
    : object(report.content_summary);
  if (report.content_summary !== undefined && report.content_summary !== null && !summary) {
    return null;
  }
  const pageCount = summary?.page_count;
  if (
    pageCount !== undefined &&
    (!Number.isSafeInteger(pageCount) || (pageCount as number) < 0 || (pageCount as number) > 100000)
  ) {
    return null;
  }

  if (!Array.isArray(report.limitations) || report.limitations.length > 16) return null;
  const limitations: Array<{ code: string; message: string }> = [];
  for (const raw of report.limitations) {
    const item = object(raw);
    if (
      !item ||
      !boundedString(item.code, 128) ||
      !/^[a-z0-9_.-]+$/.test(item.code as string) ||
      !boundedString(item.message, 1000)
    ) {
      return null;
    }
    limitations.push({
      code: item.code as string,
      message: item.message as string,
    });
  }

  const routes = object(report.output_routes);
  if (!routes) return null;
  const route = (name: string): CanonicalOutputRouteState | null => {
    const value = routes[name];
    return typeof value === 'string' &&
      ROUTE_STATES.includes(value as CanonicalOutputRouteState)
      ? (value as CanonicalOutputRouteState)
      : null;
  };
  const readOnlyPreview = route('read_only_preview');
  const salvageRecovery = route('salvage_recovery');
  const editableIdml = route('editable_idml');
  const editableOdg = route('editable_odg');
  if (!readOnlyPreview || !salvageRecovery || !editableIdml || !editableOdg) return null;

  if (
    typeof report.recommended_next_step !== 'string' ||
    !NEXT_STEPS.includes(
      report.recommended_next_step as CanonicalCompatibilityResult['recommendedNextStep'],
    )
  ) {
    return null;
  }

  return {
    kind: 'canonical',
    protocolVersion: 'chaptera.reader-compatibility-report.v1',
    sourceSha256: report.source_sha256,
    state: report.state as CanonicalCompatibilityState,
    engineClassification: report.engine_classification as string,
    pages: typeof pageCount === 'number' ? pageCount : undefined,
    limitations,
    outputRoutes: {
      readOnlyPreview,
      salvageRecovery,
      editableIdml,
      editableOdg,
    },
    recommendedNextStep:
      report.recommended_next_step as CanonicalCompatibilityResult['recommendedNextStep'],
  };
}

function legacyResult(value: unknown): CheckResult | null {
  const result = object(value);
  if (!result) return null;
  if (
    typeof result.compatibility !== 'string' ||
    !COMPATIBILITY.includes(result.compatibility as Compatibility) ||
    typeof result.summary !== 'string' ||
    result.summary.trim().length === 0 ||
    result.summary.length > 4000
  ) {
    return null;
  }
  if (
    result.pages !== undefined &&
    (!Number.isSafeInteger(result.pages) ||
      (result.pages as number) < 0 ||
      (result.pages as number) > 100000)
  ) {
    return null;
  }
  if (
    result.limitations !== undefined &&
    (!Array.isArray(result.limitations) ||
      result.limitations.length > 100 ||
      !result.limitations.every(
        (item) => typeof item === 'string' && item.length <= 1000,
      ))
  ) {
    return null;
  }

  return {
    ...result,
    kind: 'legacy',
    compatibility: result.compatibility as Compatibility,
    summary: (result.summary as string).trim(),
    limitations: Array.isArray(result.limitations)
      ? result.limitations.map((item) => (item as string).trim()).filter(Boolean)
      : undefined,
  } as CheckResult;
}

function normalizedResult(value: unknown): CheckResult | null {
  return canonicalResult(value) ?? legacyResult(value);
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
      ok: true,
      alreadyFinalized: true,
      status: record.status,
      emailStatus: record.emailStatus,
    });
  }

  let raw: unknown;
  try {
    raw = await request.json();
  } catch {
    return Response.json({ error: 'Invalid JSON result.' }, { status: 400 });
  }
  const result = normalizedResult(raw);
  if (!result) {
    return Response.json(
      { error: 'Result does not match the bounded report schema.' },
      { status: 400 },
    );
  }

  record.result = result;
  record.status =
    result.kind === 'legacy' && result.compatibility === 'failed' ? 'failed' : 'complete';
  record.updatedAt = new Date().toISOString();
  await writeCheck(record);

  record.emailStatus = await sendResultEmail(record);
  record.updatedAt = new Date().toISOString();
  await writeCheck(record);

  return Response.json({
    ok: true,
    status: record.status,
    emailStatus: record.emailStatus,
  });
}
