import {
  isCanonicalCheckResult,
  publicTokenMatches,
  readCheck,
} from '../../../../lib/checks';

export const dynamic = 'force-dynamic';

export async function GET(
  request: Request,
  context: { params: Promise<{ id: string }> },
) {
  const { id } = await context.params;
  const token = new URL(request.url).searchParams.get('token') ?? '';
  const record = await readCheck(id);

  if (!record || !token || !publicTokenMatches(record.publicToken, token)) {
    return Response.json({ error: 'Check not found.' }, { status: 404 });
  }

  let result;
  if (record.result && isCanonicalCheckResult(record.result)) {
    result = {
      kind: 'canonical' as const,
      state: record.result.state,
      pages: record.result.pages,
      limitations: record.result.limitations,
      outputRoutes: record.result.outputRoutes,
      recommendedNextStep: record.result.recommendedNextStep,
    };
  } else if (record.result) {
    result = {
      kind: 'legacy' as const,
      compatibility: record.result.compatibility,
      summary: record.result.summary,
      publisherFamily: record.result.publisherFamily,
      pages: record.result.pages,
      diagnosticsCode: record.result.diagnosticsCode,
    };
  }

  return Response.json(
    {
      status: record.status,
      emailStatus: record.emailStatus,
      result,
    },
    {
      headers: {
        'cache-control': 'no-store',
      },
    },
  );
}
