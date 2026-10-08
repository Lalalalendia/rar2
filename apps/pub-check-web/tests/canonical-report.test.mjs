import assert from 'node:assert/strict';
import { test } from 'node:test';
import { projectCanonicalReport } from '../lib/canonical-report.ts';

const sha = 'a'.repeat(64);
function valid(state = 'opens_normally') {
  const cases = {
    opens_normally: ['supported', 'available', 'not_applicable', 'migration_pilot_preview'],
    needs_review: ['partial', 'available_with_limitations', 'not_applicable', 'review_preview_before_migration'],
    opens_with_salvage: ['salvage', 'unavailable', 'available', 'rescue_review'],
    unsupported: ['unsupported', 'unavailable', 'unavailable', 'unsupported_or_manual_review'],
  };
  const [engine_classification, read_only_preview, salvage_recovery, recommended_next_step] = cases[state];
  return {
    protocol_version: 'chaptera.reader-compatibility-report.v1', source_sha256: sha,
    state, engine_classification,
    content_summary: state === 'unsupported' ? undefined : { page_count: 3, story_count: 1, private_text: 'DO NOT LEAK' },
    limitations: [], recommended_next_step,
    output_routes: { read_only_preview, salvage_recovery, editable_idml: 'not_verified', editable_odg: 'not_verified' },
    scene: { stories: ['PRIVATE STORY'] },
  };
}
test('project all four server-owned compatibility states without copied private fields', () => {
  for (const state of ['opens_normally', 'needs_review', 'opens_with_salvage', 'unsupported']) {
    const report = projectCanonicalReport(valid(state), sha);
    assert.equal(report?.state, state);
    const encoded = JSON.stringify(report);
    assert.ok(!encoded.includes('PRIVATE') && !encoded.includes('DO NOT LEAK'));
    assert.equal(report?.outputRoutes.editableIdml, 'not_verified');
  }
});
test('exact source identity is mandatory', () => {
  assert.equal(projectCanonicalReport(valid(), 'b'.repeat(64)), null);
  assert.equal(projectCanonicalReport({ ...valid(), source_sha256: 'not sha' }, sha), null);
});
test('routes and next action cannot be forged or inferred', () => {
  const broken = valid();
  broken.output_routes.editable_idml = 'available';
  assert.equal(projectCanonicalReport(broken, sha), null);
  const wrongStep = valid();
  wrongStep.recommended_next_step = 'rescue_review';
  assert.equal(projectCanonicalReport(wrongStep, sha), null);
  const wrongClass = valid();
  wrongClass.engine_classification = 'salvage';
  assert.equal(projectCanonicalReport(wrongClass, sha), null);
});
test('untrusted limitation text and impossible counts are rejected', () => {
  const badLimit = valid();
  badLimit.limitations = [{ code: 'other', message: '<private>anything</private>' }];
  assert.equal(projectCanonicalReport(badLimit, sha), null);
  const huge = valid();
  huge.content_summary.page_count = -1;
  assert.equal(projectCanonicalReport(huge, sha), null);
  const absent = valid();
  delete absent.content_summary;
  assert.equal(projectCanonicalReport(absent, sha), null);
});
test('canonical static limitation is retained and unrelated keys discarded', () => {
  const r = valid('needs_review');
  r.limitations = [{ code: 'text_layout_may_differ', message: 'Some text layout may differ from Microsoft Publisher.' }];
  const report = projectCanonicalReport(r, sha);
  assert.equal(report?.limitations.length, 1);
  assert.equal(report?.contentSummary?.page_count, 3);
  assert.equal('private_text' in report.contentSummary, false);
});
