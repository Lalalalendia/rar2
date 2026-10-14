import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';

import { dispatchCheck, dispatchFailureResult } from '../lib/dispatch.ts';

const keys = ['GITHUB_CHECKER_TOKEN', 'GITHUB_CHECKER_REPOSITORY', 'GITHUB_CHECKER_REF', 'CHECKER_WEBHOOK_URL'];
async function withDispatchEnv(values, fetchMock, run) {
  const saved = Object.fromEntries(keys.map(key => [key, process.env[key]]));
  const originalFetch = globalThis.fetch;
  for (const key of keys) delete process.env[key];
  for (const [key, value] of Object.entries(values)) process.env[key] = value;
  globalThis.fetch = fetchMock;
  try { await run(); }
  finally {
    globalThis.fetch = originalFetch;
    for (const key of keys) {
      if (saved[key] === undefined) delete process.env[key];
      else process.env[key] = saved[key];
    }
  }
}

test('current deployment example must not point to historical repository', async () => {
  const example = await readFile(new URL('../.env.example', import.meta.url), 'utf8');
  assert.match(example, /^GITHUB_CHECKER_REPOSITORY=Lalalalendia\/rar2$/m);
  assert.match(example, /^GITHUB_CHECKER_REF=main$/m);
  assert.doesNotMatch(example, /HeisLuka\/rar/);
});

test('historical repository override fails closed without network dispatch', async () => {
  let requests = 0;
  await withDispatchEnv(
    { GITHUB_CHECKER_TOKEN: 'dummy', GITHUB_CHECKER_REPOSITORY: 'HeisLuka/rar' },
    async () => { requests += 1; throw new Error('should not dispatch'); },
    async () => assert.equal(await dispatchCheck('a-check-id', 'https://example.test'), 'failed'),
  );
  assert.equal(requests, 0);
});

test('unreviewed worker ref fails closed', async () => {
  let requests = 0;
  await withDispatchEnv(
    { GITHUB_CHECKER_TOKEN: 'dummy', GITHUB_CHECKER_REF: 'legacy-checker' },
    async () => { requests += 1; return { status: 204 }; },
    async () => assert.equal(await dispatchCheck('a-check-id', 'https://example.test'), 'failed'),
  );
  assert.equal(requests, 0);
});

test('main dispatch sends only opaque check id to canonical repository', async () => {
  await withDispatchEnv(
    { GITHUB_CHECKER_TOKEN: 'dummy', GITHUB_CHECKER_REPOSITORY: 'Lalalalendia/rar2', GITHUB_CHECKER_REF: 'main' },
    async (url, options) => {
      assert.equal(url, 'https://api.github.com/repos/Lalalalendia/rar2/actions/workflows/pub-check-worker.yml/dispatches');
      assert.equal(options.method, 'POST');
      assert.ok(options.signal);
      assert.deepEqual(JSON.parse(options.body), { ref: 'main', inputs: { check_id: 'opaque-123' } });
      return { status: 204 };
    },
    async () => assert.equal(await dispatchCheck('opaque-123', 'https://example.test'), 'sent'),
  );
  assert.equal(dispatchFailureResult('sent'), null);
});

test('GitHub 403, network error and missing credentials return terminal failure', async () => {
  await withDispatchEnv({ GITHUB_CHECKER_TOKEN: 'dummy' }, async () => ({ status: 403 }), async () => {
    assert.equal(await dispatchCheck('id', 'https://example.test'), 'failed');
  });
  await withDispatchEnv({ GITHUB_CHECKER_TOKEN: 'dummy' }, async () => { throw new Error('simulated network error'); }, async () => {
    assert.equal(await dispatchCheck('id', 'https://example.test'), 'failed');
  });
  await withDispatchEnv({}, async () => { throw new Error('never'); }, async () => {
    assert.equal(await dispatchCheck('id', 'https://example.test'), 'not_configured');
  });
  for (const state of ['failed', 'not_configured']) {
    assert.deepEqual(dispatchFailureResult(state), {
      compatibility: 'failed',
      summary: 'The compatibility check could not be started reliably.',
      diagnosticsCode: 'pub_check.dispatch_failure',
    });
  }
});

test('webhook transport remains available when GitHub token is absent', async () => {
  await withDispatchEnv(
    { CHECKER_WEBHOOK_URL: 'https://trusted-worker.example/check' },
    async (url, options) => {
      assert.equal(url, 'https://trusted-worker.example/check');
      assert.equal(JSON.parse(options.body).checkId, 'check-456');
      return { ok: true };
    },
    async () => assert.equal(await dispatchCheck('check-456', 'https://chaptera.example'), 'sent'),
  );
});
