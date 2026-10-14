import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sendResultEmail } from '../lib/email.ts';
import { projectCanonicalReport } from '../lib/canonical-report.ts';

const SHA = 'a'.repeat(64);
function receipt() {
  return {
    protocol_version: 'chaptera.reader-compatibility-report.v1',
    source_sha256: SHA, state: 'needs_review', engine_classification: 'partial',
    content_summary: {page_count: 5, story_count: 2, secret_story: '<PRIVATE STORY>'},
    limitations: [{code:'text_layout_may_differ',message:'Some text layout may differ from Microsoft Publisher.'}],
    output_routes: {read_only_preview:'available_with_limitations',salvage_recovery:'not_applicable',
      editable_idml:'not_verified',editable_odg:'not_verified'},
    recommended_next_step:'review_preview_before_migration',
    scene:{stories:[{text:'SECRET CONTENT'}]},
  };
}
function record(locale) {
  const canonical=projectCanonicalReport(receipt(),SHA);
  assert.ok(canonical);
  return {
    schema:'chaptera.pub-check.v1',id:'test-fake-id',email:'tester@example.com',locale,
    result:{canonical},status:'complete',
  };
}

test('canonical email contains server verdict and routes, not customer source/story', async () => {
  const oldKey=process.env.RESEND_API_KEY,oldFrom=process.env.REPORT_FROM_EMAIL,oldFetch=globalThis.fetch;
  process.env.RESEND_API_KEY='test-not-real';
  process.env.REPORT_FROM_EMAIL='Chaptera <reports@example.com>';
  let payload;
  globalThis.fetch=async (url,options)=>{
    assert.equal(url,'https://api.resend.com/emails');
    payload=JSON.parse(options.body);
    return {ok:true,status:202};
  };
  try {
    assert.equal(await sendResultEmail(record('ru-RU')),'sent');
    assert.match(payload.text,/Нужна проверка/);
    assert.match(payload.text,/Не подтверждено/);
    assert.match(payload.text,/5/);
    for(const forbidden of ['PRIVATE STORY','SECRET CONTENT','secret_story','<PRIVATE','reader-scene','resource_key']) {
      assert.equal(JSON.stringify(payload).includes(forbidden),false,forbidden);
    }
    assert.equal(payload.to[0],'tester@example.com');
  } finally {
    globalThis.fetch=oldFetch;
    if(oldKey===undefined)delete process.env.RESEND_API_KEY;else process.env.RESEND_API_KEY=oldKey;
    if(oldFrom===undefined)delete process.env.REPORT_FROM_EMAIL;else process.env.REPORT_FROM_EMAIL=oldFrom;
  }
});
