import test from 'node:test';
import assert from 'node:assert/strict';
import { WebResourceSchedulerV1, RESOURCE_SCHEDULER_SCHEMA, RESOURCE_PRIORITY_CLASSES } from './web-resource-scheduler-v1.mjs';
import { ResourceReadinessRuntimeV1 } from './resource-readiness-v1.mjs';



function demand(id, priority = 'VisibleBlocking', extras = {}) {
  return {
    page_id: 'page:1',
    resource_id: `image:${id}`,
    content_hash: `hash:${id}`,
    derivative_id: 'full',
    priority,
    estimated_network_bytes: 10,
    estimated_decode_bytes: 20,
    estimated_pixels: 2,
    ...extras,
  };
}

function deferred() {
  let resolve; let reject;
  const promise = new Promise((a,b) => {resolve=a; reject=b;});
  return {promise,resolve,reject};
}

function controlledHarness(config = {}) {
  const readiness = new ResourceReadinessRuntimeV1({ document_id: "doc:1", revision_id: "revision:1", snapshot_id: "snapshot:1" });
  const fetches = [];
  const decodes = [];
  const scheduler = new WebResourceSchedulerV1({
    readiness,
    config,
    fetch_resource({identity,signal}) {
      const gate = deferred();
      fetches.push({identity,signal,...gate});
      return gate.promise;
    },
    decode_resource({identity,fetched,signal}) {
      const gate = deferred();
      decodes.push({identity,fetched,signal,...gate});
      return gate.promise;
    },
  });
  function finishFetch(index, extra = {}) {
    fetches[index].resolve({ bytes: new Uint8Array([1]), encoded_bytes: 1, ...extra });
  }
  function finishDecode(index, extra = {}) {
    decodes[index].resolve({ binding_identity: `decoded:${decodes[index].identity.resource_id}`, decoded_bytes: 20, ...extra });
  }
  return {readiness,scheduler,fetches,decodes,finishFetch,finishDecode};
}

async function tick() {
  await new Promise(resolve => setImmediate(resolve));
}

async function settleAll(h, { maxRounds = 30 } = {}) {
  let fetched = 0;
  let decoded = 0;
  for (let i = 0; i < maxRounds; i++) {
    while (fetched < h.fetches.length) h.finishFetch(fetched++);
    await tick();
    while (decoded < h.decodes.length) h.finishDecode(decoded++);
    await tick();
    if (h.scheduler.snapshot().inflight_fetch === 0 &&
        h.scheduler.snapshot().inflight_decode === 0 &&
        h.scheduler.snapshot().queued_fetch === 0 &&
        h.scheduler.snapshot().queued_decode === 0) return;
  }
  throw new Error('settleAll exceeded maxRounds (possible deadlock)');
}

const all = ['VisibleBlocking','VisibleUpgrade','NearWindow','Speculative'];
const d = (id,priority='VisibleBlocking',props={}) => demand(id,priority,props);
const idOf = x => x.identity.resource_id;
const limits = {
  max_network_inflight: 2,
  max_decode_inflight: 2,
  max_network_bytes_inflight: 100,
  max_decode_bytes_reserved: 100,
  max_decoded_pixels_inflight: 100,
  unknown_network_bytes_reservation: 40,
  unknown_decode_bytes_reservation: 70,
  unknown_pixels_reservation: 50,
  max_queued_items: 100,
};

test('contract exports, initialized snapshot and zero authoring authority', () => {
  assert.equal(RESOURCE_SCHEDULER_SCHEMA,'chaptera.web-resource-scheduler.v1');
  assert.deepEqual([...RESOURCE_PRIORITY_CLASSES], all);
  const h = controlledHarness();
  const s = h.scheduler.snapshot();
  assert.equal(s.schema,RESOURCE_SCHEDULER_SCHEMA);
  assert.equal(s.metrics.canonical_revisions_emitted,0);
  assert.equal(s.metrics.scene_patches_emitted,0);
  assert.equal(s.metrics.authoring_operations_emitted,0);
  h.scheduler.dispose();
});

test('500-page publication: only demanded current-page resources enter fetch queue', async () => {
  const h = controlledHarness(limits);
  const manifest = Array.from({length:500},(_,i)=>d(i+1,'VisibleBlocking',{page_id:`page:${i+1}`}));
  h.scheduler.updateDemands({generation:1,demands:manifest.filter(x=>x.page_id==='page:1')});
  await tick();
  assert.equal(h.fetches.length,1);
  assert.equal(idOf(h.fetches[0]),'image:1');
  await settleAll(h);
  assert.equal(h.readiness.receipt().metrics.ready_transitions,1);
  assert.equal(h.scheduler.snapshot().metrics.canonical_revisions_emitted,0);
  h.scheduler.dispose();
});

test('urgent work begins ahead of a speculative demand listed first', async () => {
  const h = controlledHarness({...limits,max_network_inflight:1});
  h.scheduler.updateDemands({generation:1,demands:[d('background','Speculative'),d('critical','VisibleBlocking')]});
  await tick();
  assert.equal(h.fetches.length,1);
  assert.equal(idOf(h.fetches[0]),'image:critical');
  h.scheduler.dispose();
});

test('identical immutable resource demanded by multiple pages has one fetch and decode', async () => {
  const h = controlledHarness(limits);
  const logo=d('logo');
  h.readiness.registerCompiledDependency({cache_entry_id:'page:1:logo',page_id:'page:1',...logo});
  h.readiness.registerCompiledDependency({cache_entry_id:'page:2:logo',...logo,page_id:'page:2'});
  h.readiness.registerCompiledDependency({cache_entry_id:'page:3:other',page_id:'page:3',...d('other')});
  h.scheduler.updateDemands({generation:1,demands:[logo,{...logo,page_id:'page:2'}]});
  await tick();
  assert.equal(h.fetches.length,1);
  await settleAll(h);
  assert.equal(h.fetches.length,1);
  assert.equal(h.decodes.length,1);
  assert.deepEqual(h.readiness.drainRepaintQueue(),['page:1','page:2']);
  assert.equal(h.readiness.receipt().metrics.ready_transitions,1);
  h.scheduler.dispose();
});

test('same resource already fetching as neighbor gets promoted without duplicate fetch', async () => {
  const h = controlledHarness(limits);
  const near=d('logo','NearWindow',{page_id:'page:2'});
  h.scheduler.updateDemands({generation:1,demands:[near]});
  await tick();
  assert.equal(h.fetches.length,1);
  h.scheduler.updateDemands({generation:2,demands:[{...near,page_id:'page:2',priority:'VisibleBlocking'}]});
  await tick();
  assert.equal(h.fetches.length,1);
  assert.equal(h.fetches[0].signal.aborted,false);
  h.finishFetch(0);
  await tick();
  assert.equal(h.decodes.length,1);
  h.finishDecode(0);
  await tick();
  assert.equal(h.readiness.resourceState(near).state,'ready');
  assert.ok(h.scheduler.snapshot().metrics.promotions>=1);
  h.scheduler.dispose();
});

test('page jump: ignored AbortSignal cannot publish an obsolete late fetch', async () => {
  const h = controlledHarness({...limits,max_network_inflight:1});
  const old=d('old','Speculative',{page_id:'page:1'});
  const next=d('new','VisibleBlocking',{page_id:'page:80'});
  h.scheduler.updateDemands({generation:1,demands:[old]});
  await tick();
  assert.equal(idOf(h.fetches[0]),'image:old');
  h.scheduler.updateDemands({generation:2,demands:[next]});
  await tick();
  // The injected loader ignores abort; it completes successfully anyway.
  h.finishFetch(0);
  await tick();
  assert.notEqual(h.readiness.resourceState(old).state,'ready');
  assert.equal(h.decodes.some(x=>idOf(x)==='image:old'),false);
  assert.ok(h.fetches.some(x=>idOf(x)==='image:new'));
  const nextIndex=h.fetches.findIndex(x=>idOf(x)==='image:new');
  h.finishFetch(nextIndex);
  await tick();
  const decodeIndex=h.decodes.findIndex(x=>idOf(x)==='image:new');
  assert.ok(decodeIndex>=0);
  h.finishDecode(decodeIndex);
  await tick();
  assert.equal(h.readiness.resourceState(next).state,'ready');
  h.scheduler.dispose();
});

test('different content digest and derivative never alias each other', async () => {
  const h=controlledHarness({...limits,max_network_inflight:3});
  const a=d('shared');
  const b={...a,content_hash:'changed-content'};
  const c={...a,derivative_id:'preview'};
  h.scheduler.updateDemands({generation:1,demands:[a,b,c]});
  await tick();
  assert.equal(h.fetches.length,3);
  assert.equal(new Set(h.fetches.map(x=>JSON.stringify(x.identity))).size,3);
  await settleAll(h);
  assert.equal(h.readiness.receipt().metrics.ready_transitions,3);
  h.scheduler.dispose();
});

test('independent network and decode concurrency caps are enforced', async () => {
  const h=controlledHarness({...limits,max_network_inflight:2,max_decode_inflight:1});
  h.scheduler.updateDemands({generation:1,demands:[d('1'),d('2'),d('3')]});
  await tick();
  assert.equal(h.fetches.length,2);
  h.finishFetch(0);h.finishFetch(1);
  await tick();
  assert.equal(h.decodes.length,1);
  assert.ok(h.scheduler.snapshot().inflight_decode<=1);
  assert.ok(h.scheduler.snapshot().inflight_fetch<=2);
  await settleAll(h);
  assert.equal(h.readiness.receipt().metrics.ready_transitions,3);
  h.scheduler.dispose();
});

test('decode memory reservation prevents 80+80 under a 100-byte limit', async () => {
  const h=controlledHarness({...limits,max_decode_inflight:2,max_decode_bytes_reserved:100});
  const args={estimated_decode_bytes:80,estimated_pixels:10};
  h.scheduler.updateDemands({generation:1,demands:[d('a','VisibleBlocking',args),d('b','VisibleBlocking',args)]});
  await tick();
  h.finishFetch(0);h.finishFetch(1);
  await tick();
  assert.equal(h.decodes.length,1);
  assert.ok(h.scheduler.snapshot().reserved_decode_bytes<=100);
  h.finishDecode(0);
  await tick();
  assert.equal(h.decodes.length,2);
  assert.ok(h.scheduler.snapshot().reserved_decode_bytes<=100);
  h.finishDecode(1);
  await tick();
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes,0);
  h.scheduler.dispose();
});

test('unknown decode cost is charged conservatively rather than zero', async () => {
  const h=controlledHarness({...limits,max_decode_bytes_reserved:100,unknown_decode_bytes_reservation:70});
  const unknown={estimated_decode_bytes:undefined,estimated_pixels:undefined};
  h.scheduler.updateDemands({generation:1,demands:[d('a','VisibleBlocking',unknown),d('b','VisibleBlocking',unknown)]});
  await tick();
  h.finishFetch(0);h.finishFetch(1);
  await tick();
  assert.equal(h.decodes.length,1);
  assert.ok(h.scheduler.snapshot().reserved_decode_bytes>=70);
  assert.ok(h.scheduler.snapshot().metrics.unknown_cost_admissions>=1);
  h.finishDecode(0);
  await tick();
  assert.equal(h.decodes.length,2);
  h.finishDecode(1);
  await tick();
  h.scheduler.dispose();
});

test('oversized decode fails closed without blocking a smaller visible task', async () => {
  const h=controlledHarness({...limits,max_decode_bytes_reserved:100,max_decode_inflight:2});
  const tooLarge=d('huge','VisibleBlocking',{estimated_decode_bytes:200});
  const small=d('small');
  h.scheduler.updateDemands({generation:1,demands:[tooLarge,small]});
  await tick();
  h.finishFetch(0);h.finishFetch(1);
  await tick();
  assert.equal(h.readiness.resourceState(tooLarge).state,'blocked');
  assert.ok(h.decodes.some(x=>idOf(x)==='image:small'));
  assert.ok(h.scheduler.snapshot().reserved_decode_bytes<=100);
  const index=h.decodes.findIndex(x=>idOf(x)==='image:small');
  h.finishDecode(index);
  await tick();
  assert.equal(h.readiness.resourceState(small).state,'ready');
  h.scheduler.dispose();
});

test('hidden tab blocks new neighbor/speculative work but allows visible blocking work', async () => {
  const h=controlledHarness(limits);
  const important=d('important');
  const near=d('near','NearWindow');
  const spec=d('spec','Speculative');
  h.scheduler.updateDemands({generation:1,hidden:true,demands:[near,spec,important]});
  await tick();
  assert.deepEqual(h.fetches.map(idOf),['image:important']);
  h.scheduler.updateDemands({generation:2,hidden:false,demands:[near,spec,important]});
  await tick();
  assert.ok(h.fetches.some(x=>idOf(x)==='image:near'));
  h.scheduler.dispose();
});

test('memory pressure sheds speculative admission without blocking current-page needs', async () => {
  const h=controlledHarness(limits);
  h.scheduler.updateDemands({generation:1,memory_pressure:true,demands:[d('a','Speculative'),d('b','VisibleBlocking')]});
  await tick();
  assert.deepEqual(h.fetches.map(idOf),['image:b']);
  h.scheduler.dispose();
});

test('late decode from removed demand does not publish ready', async () => {
  const h=controlledHarness(limits);
  const old=d('old');
  h.scheduler.updateDemands({generation:1,demands:[old]});
  await tick();
  h.finishFetch(0);
  await tick();
  assert.equal(h.decodes.length,1);
  h.scheduler.updateDemands({generation:2,demands:[]});
  h.finishDecode(0);
  await tick();
  assert.notEqual(h.readiness.resourceState(old).state,'ready');
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes,0);
  h.scheduler.dispose();
});

test('fetch rejection creates explicit failure and no automatic retry loop', async () => {
  const h=controlledHarness(limits);
  const req=d('bad');
  h.scheduler.updateDemands({generation:1,demands:[req]});
  await tick();
  h.fetches[0].reject(new Error('network failed'));
  await tick();
  assert.equal(h.readiness.resourceState(req).state,'failed');
  assert.equal(h.scheduler.snapshot().metrics.failures,1);
  h.scheduler.updateDemands({generation:2,demands:[req]});
  await tick();
  assert.equal(h.fetches.length,1);
  h.scheduler.dispose();
});

test('invalid generation and invalid demand throw atomically without admitting work', async () => {
  const h=controlledHarness(limits);
  h.scheduler.updateDemands({generation:1,demands:[]});
  assert.throws(()=>h.scheduler.updateDemands({generation:1,demands:[d('x')]}));
  assert.throws(()=>h.scheduler.updateDemands({generation:2,demands:[d('x','Unrecognized')]}));
  assert.throws(()=>h.scheduler.updateDemands({generation:2,demands:[d('x','VisibleBlocking',{estimated_decode_bytes:-1})]}));
  assert.equal(h.scheduler.snapshot().generation,1);
  await tick();
  assert.equal(h.fetches.length,0);
  h.scheduler.dispose();
});

test('dispose protects readiness from all late successful callbacks', async () => {
  const h=controlledHarness(limits);
  const req=d('in-flight');
  h.scheduler.updateDemands({generation:1,demands:[req]});
  await tick();
  assert.equal(h.fetches.length,1);
  h.scheduler.dispose();
  h.finishFetch(0);
  await tick();
  assert.notEqual(h.readiness.resourceState(req).state,'ready');
  assert.equal(h.decodes.length,0);
});

test('whenIdle resolves and no resource scheduling changes canonical revision', async () => {
  const h=controlledHarness(limits);
  h.scheduler.updateDemands({generation:1,demands:[d('abc')]});
  await tick();
  h.finishFetch(0);
  await tick();
  h.finishDecode(0);
  await h.scheduler.whenIdle();
  const snapshot=h.scheduler.snapshot();
  assert.equal(snapshot.inflight_fetch,0);
  assert.equal(snapshot.inflight_decode,0);
  assert.equal(snapshot.reserved_decode_bytes,0);
  assert.equal(h.readiness.receipt().canonical_revision_id,'revision:1');
  assert.equal(h.readiness.receipt().metrics.canonical_revisions_emitted,0);
  assert.equal(h.readiness.receipt().metrics.scene_patches_emitted,0);
  assert.equal(h.readiness.receipt().metrics.authoring_operations_emitted,0);
  assert.ok(!JSON.stringify(snapshot.metrics).includes('image:abc'));
  h.scheduler.dispose();
});

test('existing ready material is reused; evicted material can be requested again', async () => {
  const h=controlledHarness(limits);
  const req=d('cached');
  const old=h.readiness.beginRequest(req);
  h.readiness.completeReady(old,{binding_identity:'immutable-cache-key'});
  h.scheduler.updateDemands({generation:1,demands:[req]});
  await tick();
  assert.equal(h.fetches.length,0);
  h.readiness.evict(req);
  h.scheduler.updateDemands({generation:2,demands:[req]});
  await tick();
  assert.equal(h.fetches.length,1);
  await settleAll(h);
  assert.equal(h.readiness.resourceState(req).state,'ready');
  h.scheduler.dispose();
});

test('paused speculative queue is quiescent for whenIdle and resumes only with latest demand', async () => {
  const h=controlledHarness(limits);
  h.scheduler.updateDemands({generation:1,demands:[d('old','Speculative')],hidden:true});
  await h.scheduler.whenIdle();
  assert.equal(h.fetches.length,0);
  h.scheduler.updateDemands({generation:2,demands:[d('new','Speculative')],hidden:false});
  await tick();
  assert.equal(h.fetches.some(x=>idOf(x)==='image:old'),false);
  assert.equal(h.fetches.some(x=>idOf(x)==='image:new'),true);
  h.scheduler.dispose();
});

test('invalid capacity settings are rejected before creating a scheduler', () => {
  const r=new ResourceReadinessRuntimeV1({ document_id: "doc:1", revision_id: "revision:1", snapshot_id: "snapshot:1" });
  const fetch_resource=async()=>({bytes:new Uint8Array([1])});
  const decode_resource=async()=>({binding_identity:'decoded'});
  assert.throws(()=>new WebResourceSchedulerV1({
    readiness:r,fetch_resource,decode_resource,
    config:{max_decode_bytes_reserved:0},
  }));
  assert.throws(()=>new WebResourceSchedulerV1({
    readiness:r,fetch_resource,decode_resource,
    config:{max_network_inflight:Infinity},
  }));
});

// --- Additional adversarial, race-safety, budget, and fairness tests ---

test('adversarial A-B-A fetch race keeps physical reservation and dedupes until stale callback settles', async () => {
  const h = controlledHarness({ ...limits, max_network_inflight: 1 });
  const item = d('hero', 'VisibleBlocking', { page_id: 'page:1', estimated_network_bytes: 30 });

  h.scheduler.updateDemands({ generation: 1, demands: [item] });
  await tick();
  h.scheduler.updateDemands({ generation: 2, demands: [] });
  await tick();
  assert.equal(h.fetches[0].signal.aborted, true);
  assert.equal(h.scheduler.snapshot().inflight_fetch, 1);
  assert.equal(h.scheduler.snapshot().reserved_network_bytes, 30);
  assert.equal(h.scheduler.snapshot().retiring_callbacks, 1);

  h.scheduler.updateDemands({ generation: 3, demands: [item] });
  await tick();
  assert.equal(h.fetches.length, 1, 'same immutable bytes cannot fetch again while old callback is live');

  h.finishFetch(0, { tag: 'stale' });
  await tick();
  assert.equal(h.decodes.length, 0);
  assert.equal(h.fetches.length, 2);
  assert.equal(h.scheduler.snapshot().inflight_fetch, 1);
  assert.equal(h.scheduler.snapshot().retiring_callbacks, 0);
  assert.ok(h.scheduler.snapshot().metrics.stale_completions >= 1);

  h.finishFetch(1, { tag: 'fresh' });
  await tick();
  assert.equal(h.decodes.length, 1);
  assert.equal(h.decodes[0].fetched.tag, 'fresh');
  h.finishDecode(0, { binding_identity: 'decoded:fresh' });
  await tick();
  assert.equal(h.readiness.resourceState(item).binding_identity, 'decoded:fresh');
  h.scheduler.dispose();
});

test('adversarial A-B-A decode race holds decoded working-set and same-key work until settlement', async () => {
  const h = controlledHarness({ ...limits, max_decode_inflight: 1 });
  const item = d('bitmap', 'VisibleBlocking', { page_id: 'page:1', estimated_decode_bytes: 60 });

  h.scheduler.updateDemands({ generation: 1, demands: [item] });
  await tick();
  h.finishFetch(0);
  await tick();
  h.scheduler.updateDemands({ generation: 2, demands: [] });
  await tick();
  assert.equal(h.decodes[0].signal.aborted, true);
  assert.equal(h.scheduler.snapshot().inflight_decode, 1);
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes, 60);

  h.scheduler.updateDemands({ generation: 3, demands: [item] });
  await tick();
  assert.equal(h.fetches.length, 1, 'do not refetch same key during a retiring decode');

  h.finishDecode(0, { binding_identity: 'decoded:stale' });
  await tick();
  assert.equal(h.fetches.length, 2);
  assert.notEqual(h.readiness.resourceState(item).state, 'ready');
  h.finishFetch(1);
  await tick();
  assert.equal(h.decodes.length, 2);
  assert.equal(h.scheduler.snapshot().inflight_decode, 1);
  h.finishDecode(1, { binding_identity: 'decoded:fresh' });
  await tick();
  assert.equal(h.readiness.resourceState(item).state, 'ready');
  assert.equal(h.readiness.resourceState(item).binding_identity, 'decoded:fresh');
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes, 0);
  h.scheduler.dispose();
});

test('shared resource survives when one of two consumer pages navigates away', async () => {
  const h = controlledHarness(limits);
  const p1 = d('shared-font', 'VisibleBlocking', { page_id: 'page:1' });
  const p2 = d('shared-font', 'NearWindow', { page_id: 'page:2' });

  h.scheduler.updateDemands({ generation: 1, demands: [p1, p2] });
  await tick();
  assert.equal(h.fetches.length, 1);

  h.scheduler.updateDemands({
    generation: 2,
    demands: [{ ...p2, priority: 'VisibleBlocking' }],
  });
  await tick();
  assert.equal(h.fetches.length, 1);
  assert.equal(h.fetches[0].signal.aborted, false);
  assert.equal(h.scheduler.snapshot().metrics.cancelled_obsolete, 0);

  await settleAll(h);
  assert.equal(h.readiness.resourceState(p2).state, 'ready');
  h.scheduler.dispose();
});

test('changing derivative_id or content_hash cancels obsolete derivative without cross-completing', async () => {
  const h = controlledHarness(limits);
  const preview = d('photo', 'VisibleUpgrade', { derivative_id: 'preview' });
  const full = d('photo', 'VisibleBlocking', { derivative_id: 'full' });

  h.scheduler.updateDemands({ generation: 1, demands: [preview] });
  await tick();
  assert.equal(h.fetches.length, 1);
  assert.equal(h.fetches[0].identity.derivative_id, 'preview');

  h.scheduler.updateDemands({ generation: 2, demands: [full] });
  await tick();
  assert.equal(h.fetches[0].signal.aborted, true);
  assert.equal(h.fetches.length, 2);
  assert.equal(h.fetches[1].identity.derivative_id, 'full');

  h.finishFetch(0);
  await tick();
  assert.equal(h.decodes.length, 0);
  assert.notEqual(h.readiness.resourceState(preview).state, 'ready');
  assert.notEqual(h.readiness.resourceState(full).state, 'ready');

  h.finishFetch(1);
  await tick();
  assert.equal(h.decodes.length, 1);
  h.finishDecode(0);
  await tick();
  assert.equal(h.readiness.resourceState(full).state, 'ready');
  assert.notEqual(h.readiness.resourceState(preview).state, 'ready');
  h.scheduler.dispose();
});

test('cost overflow, non-integer, and unsafe integer inputs are rejected atomically', () => {
  const h = controlledHarness(limits);
  h.scheduler.updateDemands({ generation: 1, demands: [] });

  const badInputs = [
    d('overflow-net', 'VisibleBlocking', { estimated_network_bytes: Number.MAX_SAFE_INTEGER + 1 }),
    d('overflow-dec', 'VisibleBlocking', { estimated_decode_bytes: Number.MAX_SAFE_INTEGER + 1 }),
    d('overflow-pix', 'VisibleBlocking', { estimated_pixels: Number.MAX_SAFE_INTEGER + 1 }),
    d('nan-cost', 'VisibleBlocking', { estimated_decode_bytes: NaN }),
    d('inf-cost', 'VisibleBlocking', { estimated_network_bytes: Infinity }),
    d('float-cost', 'VisibleBlocking', { estimated_pixels: 12.5 }),
    d('string-cost', 'VisibleBlocking', { estimated_decode_bytes: '100' }),
    d('empty-page', 'VisibleBlocking', { page_id: '' }),
    d('empty-hash', 'VisibleBlocking', { content_hash: '' }),
    d('empty-deriv', 'VisibleBlocking', { derivative_id: '' }),
  ];

  for (const bad of badInputs) {
    assert.throws(() => {
      h.scheduler.updateDemands({ generation: 2, demands: [bad] });
    });
    assert.equal(h.scheduler.snapshot().generation, 1);
  }

  h.scheduler.dispose();
});

test('oversized network bytes and oversized pixel count fail closed with blocked state', async () => {
  const h = controlledHarness(limits);
  const netHuge = d('net-huge', 'VisibleBlocking', { estimated_network_bytes: 500 });
  const pixHuge = d('pix-huge', 'VisibleBlocking', { estimated_pixels: 500 });
  const normal = d('normal', 'VisibleBlocking', { estimated_network_bytes: 10, estimated_decode_bytes: 20, estimated_pixels: 10 });

  h.scheduler.updateDemands({ generation: 1, demands: [netHuge, pixHuge, normal] });
  await tick();

  assert.equal(h.readiness.resourceState(netHuge).state, 'blocked');
  assert.equal(h.readiness.resourceState(netHuge).terminal_reason, 'network_budget_exceeded');
  assert.equal(h.fetches.length, 2);

  await settleAll(h);
  assert.equal(h.readiness.resourceState(pixHuge).state, 'blocked');
  assert.equal(h.readiness.resourceState(pixHuge).terminal_reason, 'decode_budget_exceeded');
  assert.equal(h.readiness.resourceState(normal).state, 'ready');
  h.scheduler.dispose();
});

test('decode rejection and empty binding_identity transition to failed and release reservations', async () => {
  const h = controlledHarness(limits);
  const badDecode = d('corrupt', 'VisibleBlocking', { estimated_decode_bytes: 50, estimated_pixels: 25 });
  const badBinding = d('empty-binding', 'VisibleBlocking', { estimated_decode_bytes: 40, estimated_pixels: 20 });

  h.scheduler.updateDemands({ generation: 1, demands: [badDecode, badBinding] });
  await tick();
  h.finishFetch(0);
  h.finishFetch(1);
  await tick();
  assert.equal(h.decodes.length, 2);
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes, 90);
  assert.equal(h.scheduler.snapshot().reserved_pixels, 45);

  h.decodes[0].reject(new Error('corrupt bitmap'));
  h.decodes[1].resolve({ binding_identity: '' });
  await tick();

  assert.equal(h.readiness.resourceState(badDecode).state, 'failed');
  assert.equal(h.readiness.resourceState(badBinding).state, 'failed');
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes, 0);
  assert.equal(h.scheduler.snapshot().reserved_pixels, 0);
  assert.equal(h.scheduler.snapshot().metrics.failures, 2);
  h.scheduler.dispose();
});

test('fairness under sustained small-task arrivals: large VisibleBlocking decode does not starve beyond max_visible_wait_turns', async () => {
  const h = controlledHarness({
    ...limits,
    max_network_inflight: 6,
    max_decode_inflight: 3,
    max_decode_bytes_reserved: 100,
    max_visible_wait_turns: 2,
  });

  const d0 = d('init-40', 'VisibleBlocking', { estimated_decode_bytes: 40 });
  const big80 = d('big-80', 'VisibleBlocking', { estimated_decode_bytes: 80 });
  const s1 = d('small-1', 'VisibleBlocking', { estimated_decode_bytes: 25 });
  const s2 = d('small-2', 'VisibleBlocking', { estimated_decode_bytes: 25 });
  const s3 = d('small-3', 'VisibleBlocking', { estimated_decode_bytes: 25 });

  h.scheduler.updateDemands({ generation: 1, demands: [d0, big80, s1, s2, s3] });
  await tick();
  for (let i = 0; i < h.fetches.length; i++) h.finishFetch(i);
  await tick();

  const startedDecodeIds = h.decodes.map(idOf);
  assert.equal(startedDecodeIds.includes('image:init-40'), true);
  assert.equal(startedDecodeIds.includes('image:small-1'), true);
  assert.equal(startedDecodeIds.includes('image:small-2'), true);
  assert.equal(startedDecodeIds.includes('image:small-3'), false);

  h.finishDecode(0);
  h.finishDecode(1);
  h.finishDecode(2);
  await tick();

  assert.equal(idOf(h.decodes[3]), 'image:big-80');
  await settleAll(h);
  assert.equal(h.readiness.resourceState(big80).state, 'ready');
  assert.equal(h.readiness.resourceState(s3).state, 'ready');
  h.scheduler.dispose();
});

test('fairness across interactive generations: re-ordering demands array does not starve older VisibleBlocking tasks', async () => {
  const h = controlledHarness({ ...limits, max_network_inflight: 1 });
  const v1 = d('v1', 'VisibleBlocking');
  const v2 = d('v2', 'VisibleBlocking');
  const v3 = d('v3', 'VisibleBlocking');

  h.scheduler.updateDemands({ generation: 1, demands: [v1, v2] });
  await tick();
  assert.equal(idOf(h.fetches[0]), 'image:v1');

  h.scheduler.updateDemands({ generation: 2, demands: [v1, v3, v2] });
  await tick();
  h.finishFetch(0);
  await tick();

  assert.equal(idOf(h.fetches[1]), 'image:v2');
  await settleAll(h);
  assert.equal(h.readiness.resourceState(v1).state, 'ready');
  assert.equal(h.readiness.resourceState(v2).state, 'ready');
  assert.equal(h.readiness.resourceState(v3).state, 'ready');
  h.scheduler.dispose();
});

test('memory pressure aborts speculative work but holds physical budget until its fetch settles', async () => {
  const h = controlledHarness({ ...limits, max_network_inflight: 1 });
  const spec = d('spec-inflight', 'Speculative');
  const urgent = d('urgent-visible', 'VisibleBlocking');

  h.scheduler.updateDemands({ generation: 1, demands: [spec] });
  await tick();
  assert.equal(h.fetches.length, 1);
  h.scheduler.updateDemands({ generation: 2, memory_pressure: true, demands: [spec, urgent] });
  await tick();
  assert.equal(h.fetches[0].signal.aborted, true);
  assert.equal(h.fetches.length, 1, 'cannot admit urgent until uncooperative old fetch stops');
  assert.equal(h.scheduler.snapshot().inflight_fetch, 1);
  h.finishFetch(0);
  await tick();
  assert.equal(h.fetches.length, 2);
  assert.equal(idOf(h.fetches[1]), 'image:urgent-visible');
  await settleAll(h);
  assert.equal(h.readiness.resourceState(urgent).state, 'ready');
  h.scheduler.dispose();
});

test('max_queued_items cap drops lowest-priority excess items with queue_capacity_exceeded', async () => {
  const h = controlledHarness({
    ...limits,
    max_network_inflight: 1,
    max_queued_items: 1,
  });

  const spec = d('drop-spec', 'Speculative');
  const near = d('keep-near', 'NearWindow');
  const vis = d('run-vis', 'VisibleBlocking');

  h.scheduler.updateDemands({ generation: 1, demands: [spec, near, vis] });
  await tick();

  assert.equal(h.readiness.resourceState(spec).state, 'blocked');
  assert.equal(h.readiness.resourceState(spec).terminal_reason, 'queue_capacity_exceeded');
  assert.equal(idOf(h.fetches[0]), 'image:run-vis');

  await settleAll(h);
  assert.equal(h.readiness.resourceState(vis).state, 'ready');
  assert.equal(h.readiness.resourceState(near).state, 'ready');
  h.scheduler.dispose();
});

test('non-cooperative fetch never exceeds a one-slot physical network budget during rapid page changes', async () => {
  const h = controlledHarness({ ...limits, max_network_inflight: 1, max_network_bytes_inflight: 30 });
  const page = n => d(`physical-${n}`, 'VisibleBlocking', { estimated_network_bytes: 30, page_id: `page:${n}` });
  h.scheduler.updateDemands({ generation: 1, demands: [page(1)] });
  await tick();
  h.scheduler.updateDemands({ generation: 2, demands: [page(80)] });
  h.scheduler.updateDemands({ generation: 3, demands: [page(81)] });
  await tick();
  assert.equal(h.fetches.length, 1);
  assert.equal(h.scheduler.snapshot().reserved_network_bytes, 30);
  assert.equal(h.scheduler.snapshot().inflight_fetch, 1);
  h.finishFetch(0);
  await tick();
  assert.equal(h.fetches.length, 2);
  assert.equal(h.fetches[1].identity.resource_id, 'image:physical-81');
  await settleAll(h);
  assert.equal(h.scheduler.snapshot().reserved_network_bytes, 0);
  assert.equal(h.readiness.resourceState(page(81)).state, 'ready');
  h.scheduler.dispose();
});

test('non-cooperative decode retains its reserved bytes and pixels after cancellation', async () => {
  const h = controlledHarness({ ...limits, max_decode_inflight: 1, max_decode_bytes_reserved: 80, max_decoded_pixels_inflight: 50 });
  const old = d('decode-old', 'Speculative', { estimated_decode_bytes: 80, estimated_pixels: 50 });
  const latest = d('decode-new', 'VisibleBlocking', { estimated_decode_bytes: 80, estimated_pixels: 50 });
  h.scheduler.updateDemands({ generation: 1, demands: [old] });
  await tick(); h.finishFetch(0); await tick();
  assert.equal(h.decodes.length, 1);
  h.scheduler.updateDemands({ generation: 2, demands: [latest] });
  await tick();
  assert.equal(h.decodes[0].signal.aborted, true);
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes, 80);
  assert.equal(h.scheduler.snapshot().reserved_pixels, 50);
  assert.equal(h.scheduler.snapshot().inflight_decode, 1);
  h.finishFetch(1); await tick();
  assert.equal(h.decodes.length, 1, 'second decode must wait for actual first decode settlement');
  h.finishDecode(0); await tick();
  assert.equal(h.decodes.length, 2);
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes, 80);
  h.finishDecode(1); await tick();
  assert.equal(h.scheduler.snapshot().reserved_decode_bytes, 0);
  assert.equal(h.readiness.resourceState(latest).state, 'ready');
  h.scheduler.dispose();
});

test('disposing an active scheduler does not invent free physical slots or premature idle', async () => {
  const h = controlledHarness({ ...limits, max_network_inflight: 1 });
  h.scheduler.updateDemands({ generation: 1, demands: [d('dispose')] });
  await tick();
  h.scheduler.dispose();
  assert.equal(h.fetches[0].signal.aborted, true);
  assert.equal(h.scheduler.snapshot().inflight_fetch, 1);
  let idle = false;
  const onIdle = h.scheduler.whenIdle().then(() => { idle = true; });
  await tick();
  assert.equal(idle, false);
  h.finishFetch(0);
  await onIdle;
  assert.equal(h.scheduler.snapshot().inflight_fetch, 0);
  assert.equal(h.scheduler.snapshot().reserved_network_bytes, 0);
  assert.equal(h.decodes.length, 0);
});

test('cancel before queued callback invocation prevents starting unnecessary network activity', async () => {
  const h = controlledHarness({ ...limits, max_network_inflight: 1 });
  h.scheduler.updateDemands({ generation: 1, demands: [d('not-started')] });
  h.scheduler.updateDemands({ generation: 2, demands: [] });
  await tick();
  assert.equal(h.fetches.length, 0);
  assert.equal(h.scheduler.snapshot().inflight_fetch, 0);
  assert.equal(h.scheduler.snapshot().reserved_network_bytes, 0);
  h.scheduler.dispose();
});

test('failed late fetch releases retirement without marking a new same-key request failed', async () => {
  const h = controlledHarness({ ...limits, max_network_inflight: 1 });
  const req = d('retry-after-cancel');
  h.scheduler.updateDemands({ generation: 1, demands: [req] });
  await tick();
  h.scheduler.updateDemands({ generation: 2, demands: [] });
  h.scheduler.updateDemands({ generation: 3, demands: [req] });
  await tick();
  assert.equal(h.fetches.length, 1);
  h.fetches[0].reject(new Error('ignored abort then rejected'));
  await tick();
  assert.equal(h.fetches.length, 2);
  assert.equal(h.scheduler.snapshot().retiring_callbacks, 0);
  h.finishFetch(1); await tick(); h.finishDecode(0); await tick();
  assert.equal(h.readiness.resourceState(req).state, 'ready');
  h.scheduler.dispose();
});
