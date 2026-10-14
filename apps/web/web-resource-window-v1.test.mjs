import test from 'node:test';
import assert from 'node:assert/strict';
import { WebResourceWindowIndexV1, WebResourceViewportObserverV1, WEB_RESOURCE_WINDOW_SCHEMA } from './web-resource-window-v1.mjs';
import { WebResourceSchedulerV1 } from './web-resource-scheduler-v1.mjs';
import { ResourceReadinessRuntimeV1 } from './resource-readiness-v1.mjs';

const hex = n => n.toString(16).padStart(64, '0');
const page = n => `p${n}`;
const desc = n => ({ resource_id: `img${n}`, content_hash: hex(n), byte_len: 32, estimated_decode_bytes: 64, estimated_pixels: 16 });
function scene(n = 500) {
  return {
    protocol_version: 'chaptera.reader-scene.v1', document_id: 'doc:one', revision_id: 'rev:one',
    pages: Array.from({length:n}, (_,i) => ({ page_id:page(i+1), order:i })),
    resources: Array.from({length:n}, (_,i) => desc(i+1)),
    nodes: Array.from({length:n}, (_,i) => ({ page_id:page(i+1), resource_id:`img${i+1}` })),
  };
}
function spy() { return { calls:[], updateDemands(options) { this.calls.push(options); } }; }
function defer() { let resolve, reject; const promise=new Promise((a,b)=>{resolve=a;reject=b});return {promise,resolve,reject}; }
const tick = () => new Promise(resolve => setImmediate(resolve));

// Do not admit a manifest. Demand must be bounded by the observed page window.
test('500 page scene admits only current page and explicit neighbors', () => {
  const index = new WebResourceWindowIndexV1(scene());
  const s = spy();
  const result = index.update(s,{current_page_id:'p1',near_page_ids:['p2','p3']});
  assert.equal(result.schema,WEB_RESOURCE_WINDOW_SCHEMA);
  assert.deepEqual(s.calls[0].demands.map(x=>x.page_id),['p1','p2','p3']);
  assert.deepEqual(s.calls[0].demands.map(x=>x.priority),['VisibleBlocking','NearWindow','NearWindow']);
  assert.equal(result.selected_page_count,3);
  assert.equal(index.pages.length,500);
  assert.equal(result.all_visible_resources_schedulable,true);
});

test('direct page 1 -> 80 jump replaces obsolete neighbor demands', () => {
  const index=new WebResourceWindowIndexV1(scene());const s=spy();
  index.update(s,{current_page_id:'p1',near_page_ids:['p2']});
  index.update(s,{current_page_id:'p80',near_page_ids:['p79','p81']});
  assert.deepEqual(s.calls[1].demands.map(x=>x.page_id),['p79','p80','p81']);
  assert.equal(s.calls[1].generation,2);
  assert.equal(s.calls[1].demands[1].priority,'VisibleBlocking');
  assert.equal(s.calls[1].demands.some(x=>x.page_id==='p2'),false);
});

test('current visible resources outrank near and speculative regardless of input ordering', () => {
  const index=new WebResourceWindowIndexV1(scene(5));
  const x=index.select({current_page_id:'p3',visible_page_ids:['p2'],near_page_ids:['p2','p4'],speculative_page_ids:['p1','p4','p5']});
  const p=new Map(x.demands.map(x=>[x.page_id,x.priority]));
  assert.equal(p.get('p2'),'VisibleBlocking'); assert.equal(p.get('p3'),'VisibleBlocking');
  assert.equal(p.get('p4'),'NearWindow'); assert.equal(p.get('p1'),'Speculative');
});

test('an explicit current-page quality upgrade is VisibleUpgrade only for that resource', () => {
  const x=new WebResourceWindowIndexV1(scene(2)).select({current_page_id:'p1',upgrade_resource_ids:['img1','img2'],near_page_ids:['p2']});
  assert.deepEqual(x.demands.map(d=>d.priority),['VisibleUpgrade','NearWindow']);
});

test('missing content hash is reported and never invented or admitted', () => {
  const s=scene(1);s.resources[0].content_hash=null;s.resources[0].inline_data_url='data:image/png;base64,YQ==';
  const index=new WebResourceWindowIndexV1(s);const spySched=spy();
  const res=index.update(spySched,{current_page_id:'p1'});
  assert.deepEqual(spySched.calls[0].demands,[]);
  assert.equal(res.unsupported_visible_count,1);
  assert.equal(res.all_visible_resources_schedulable,false);
});

test('SHA-256 format is strict, including uppercase and prefixed hashes',()=>{
  for(const hash of ['A'.repeat(64),'sha256:'+hex(1),'001','']) {
    const s=scene(1);s.resources[0].content_hash=hash;
    assert.equal(new WebResourceWindowIndexV1(s).select({current_page_id:'p1'}).unsupported_visible_count,1);
  }
});

test('non-current unsupported resource does not pretend current page is blocked',()=>{
  const s=scene(2);s.resources[1].content_hash=null;
  const res=new WebResourceWindowIndexV1(s).select({current_page_id:'p1',speculative_page_ids:['p2']});
  assert.equal(res.unsupported_visible_count,0);
  assert.equal(res.unsupported_resource_count,1);
  assert.equal(res.all_visible_resources_schedulable,true);
});

test('fonts from resolved layout and decorative images are included, global unused fonts are not',()=>{
  const s=scene(2);
  s.resources.push({resource_id:'ornament',content_hash:hex(200)});
  s.fonts=[{resource_id:'font-a',expected_sha256:hex(201)},{resource_id:'unused',expected_sha256:hex(202)}];
  s.nodes[0].text_layout={font_resource_id:'font-a',lines:[{spans:[{font_resource_id:'font-a'}]}]};
  s.nodes[0].decorative_border={placements:[{resource_id:'ornament'}]};
  const res=new WebResourceWindowIndexV1(s).select({current_page_id:'p1'});
  assert.deepEqual(res.demands.map(d=>d.resource_id),['img1','ornament','font-a']);
  assert.ok(res.demands.every(d=>d.content_hash.length===64));
});

test('shared resource yields per-page consumers, not duplicated within a page',()=>{
  const s=scene(2);s.nodes[1].resource_id='img1';
  s.nodes.push({page_id:'p1',resource_id:'img1'});
  const d=new WebResourceWindowIndexV1(s).select({current_page_id:'p1',near_page_ids:['p2']}).demands;
  assert.equal(d.length,2); assert.ok(d.every(x=>x.resource_id==='img1'));
});

test('resource identity includes derivative and exact hash; estimates are conservatively optional',()=>{
  const s=scene(2);
  s.resources[0]={...s.resources[0],derivative_id:'small',byte_len:null,estimated_pixels:0,estimated_decode_bytes:0};
  const d=new WebResourceWindowIndexV1(s).select({current_page_id:'p1'}).demands[0];
  assert.equal(d.derivative_id,'small');
  assert.equal(d.estimated_network_bytes,undefined);
  assert.equal(d.estimated_decode_bytes,0);
  assert.equal(d.estimated_pixels,0);
});

test('reject duplicate page ids, resource ids and nodes with invalid page reference',()=>{
  for(const variant of ['pages','resources','unknownNode']) {
    const s=scene(2);
    if(variant==='pages')s.pages[1].page_id='p1';
    if(variant==='resources')s.resources[1].resource_id='img1';
    if(variant==='unknownNode')s.nodes[0].page_id='missing';
    assert.throws(()=>new WebResourceWindowIndexV1(s));
  }
});

test('reject unknown current or requested page, invalid inputs and stale generations before admitting work',()=>{
  const i=new WebResourceWindowIndexV1(scene(2));const s=spy();
  assert.throws(()=>i.update(s,{current_page_id:'p9'}));
  assert.throws(()=>i.update(s,{current_page_id:'p1',near_page_ids:['p9']}));
  assert.throws(()=>i.update(s,{current_page_id:'p1',visible_page_ids:['p2',null]}));
  assert.equal(s.calls.length,0);
  i.update(s,{current_page_id:'p1',generation:3});
  assert.throws(()=>i.update(s,{current_page_id:'p1',generation:3}));
  assert.equal(s.calls.length,1);
});

test('passes visibility and pressure to scheduler without corrupting demand priority',()=>{
  const i=new WebResourceWindowIndexV1(scene(3));const s=spy();
  i.update(s,{current_page_id:'p1',near_page_ids:['p2'],hidden:true,memory_pressure:true});
  assert.equal(s.calls[0].hidden,true); assert.equal(s.calls[0].memory_pressure,true);
  assert.equal(s.calls[0].demands[0].priority,'VisibleBlocking');
});

test('actual scheduler promotes in-flight neighbor on page navigation without a second fetch',async()=>{
  const reader=new ResourceReadinessRuntimeV1({document_id:'doc:one',revision_id:'rev:one',snapshot_id:'snap:one'});
  const pending=new Map();const fetches=[];
  const scheduler=new WebResourceSchedulerV1({readiness:reader,fetch_resource:({identity})=>{
    fetches.push(identity.resource_id);const d=defer();pending.set(identity.resource_id,d);return d.promise;
  },decode_resource:async({identity})=>({binding_identity:'decoded:'+identity.resource_id}),config:{max_network_inflight:2}});
  const index=new WebResourceWindowIndexV1(scene(2));
  index.update(scheduler,{current_page_id:'p1',near_page_ids:['p2']});
  await tick();assert.equal(fetches.length,2);
  index.update(scheduler,{current_page_id:'p2',near_page_ids:['p1']});
  assert.equal(scheduler.snapshot().metrics.promotions>=1,true);
  await tick();assert.equal(fetches.length,2);
  for(const v of pending.values())v.resolve({bytes:1});
  await scheduler.whenIdle();
  assert.equal(reader.resourceState({resource_id:'img2',content_hash:hex(2),derivative_id:'original'}).state,'ready');
  assert.equal(reader.receipt().metrics.canonical_revisions_emitted,0);
  scheduler.dispose();
});

test('real scheduler cancels uncooperative old fetch on page jump and retains physical cap',async()=>{
  const reader=new ResourceReadinessRuntimeV1({document_id:'doc:one',revision_id:'rev:one',snapshot_id:'snap:one'});
  const pending=new Map();const calls=[];
  const scheduler=new WebResourceSchedulerV1({readiness:reader,fetch_resource:({identity})=>{
    calls.push(identity.resource_id);const d=defer();pending.set(identity.resource_id,d);return d.promise;
  },decode_resource:async({identity})=>({binding_identity:'decoded:'+identity.resource_id}),config:{max_network_inflight:1}});
  const index=new WebResourceWindowIndexV1(scene(80));
  index.update(scheduler,{current_page_id:'p1'});await tick();assert.deepEqual(calls,['img1']);
  index.update(scheduler,{current_page_id:'p80'});await tick();
  assert.deepEqual(calls,['img1']); assert.equal(scheduler.snapshot().inflight_fetch,1);
  pending.get('img1').resolve({bytes:1});await tick();await tick();
  assert.deepEqual(calls,['img1','img80']);
  pending.get('img80').resolve({bytes:1});await scheduler.whenIdle();
  assert.equal(reader.resourceState({resource_id:'img1',content_hash:hex(1)}).state,'fetching');
  assert.equal(reader.resourceState({resource_id:'img80',content_hash:hex(80)}).state,'ready');
  scheduler.dispose();
});

class FakeContainer extends EventTarget { constructor(rect){super();this.rect=rect;}getBoundingClientRect(){return this.rect;} }
class FakePage { constructor(id,rect){this.id=id;this.rect=rect;}getAttribute(k){return k==='data-page-id'?this.id:null;}getBoundingClientRect(){return this.rect;} }
const bounds=(t,b)=>({top:t,bottom:b,left:0,right:100});
function observerHarness() {
  const container=new FakeContainer(bounds(0,100));const doc=new EventTarget();doc.visibilityState='visible';
  const pages=[new FakePage('p1',bounds(0,80)),new FakePage('p2',bounds(80,180)),new FakePage('p3',bounds(180,280))];
  const host={querySelectorAll:()=>pages};const pending=[];const updates=[];
  const obs=new WebResourceViewportObserverV1({host,scroll_container:container,document_object:doc,
    near_before:1,near_after:1,on_window:x=>updates.push(x),
    request_frame:cb=>{pending.push(cb);return pending.length;},cancel_frame:()=>{}});
  const frame=()=>{const cb=pending.shift();if(cb)cb();};
  obs.setIndex(new WebResourceWindowIndexV1(scene(3)));
  return {container,doc,pages,updates,obs,pending,frame};
}

test('viewport observer chooses largest visible page and bounded neighbors',()=>{
  const h=observerHarness();h.frame();
  assert.equal(h.updates[0].current_page_id,'p1');
  assert.deepEqual(h.updates[0].visible_page_ids,['p1','p2']);
  assert.deepEqual(h.updates[0].near_page_ids,[]);
  h.pages[0].rect=bounds(-170,-90);h.pages[1].rect=bounds(-80,20);h.pages[2].rect=bounds(20,100);
  h.container.dispatchEvent(new Event('scroll'));h.frame();
  assert.equal(h.updates[1].current_page_id,'p3');
  assert.deepEqual(h.updates[1].near_page_ids,[]);h.obs.dispose();
});

test('observer coalesces scroll events, communicates tab suspension and disposes safely',()=>{
  const h=observerHarness();h.frame();
  h.doc.visibilityState='hidden';h.doc.dispatchEvent(new Event('visibilitychange'));
  h.container.dispatchEvent(new Event('scroll'));h.container.dispatchEvent(new Event('scroll'));
  assert.equal(h.pending.length,1);h.frame();
  assert.equal(h.updates.length,2);assert.equal(h.updates[1].hidden,true);
  h.obs.dispose();h.container.dispatchEvent(new Event('scroll'));
  assert.equal(h.pending.length,0);
});

test('observer reports page order hints without universal neighbor radius',()=>{
  const h=observerHarness();h.pages[1].rect=bounds(200,300);h.pages[2].rect=bounds(300,400);
  h.frame();
  assert.deepEqual(h.updates[0].near_page_ids,['p2']);h.obs.dispose();
  const bare=new WebResourceViewportObserverV1({host:{querySelectorAll:()=>h.pages},scroll_container:h.container,
    on_window:x=>h.updates.push(x),request_frame:cb=>{cb();return 1},cancel_frame:()=>{}});
  bare.setIndex(new WebResourceWindowIndexV1(scene(3)));
  assert.deepEqual(h.updates.at(-1).near_page_ids,[]);bare.dispose();
});

test('IntersectionObserver window updates without scanning 500 DOM rects every scroll',()=>{
  let callback, observer;
  const observed=[];
  let rectReads=0;
  const pages=Array.from({length:500},(_,i)=>({
    getAttribute:key=>key==='data-page-id'?page(i+1):null,
    getBoundingClientRect:()=>{rectReads++;return bounds(0,100);}
  }));
  const container=new FakeContainer(bounds(0,100));
  const frames=[];const seen=[];
  const obj=new WebResourceViewportObserverV1({
    host:{querySelectorAll:()=>pages},scroll_container:container,
    on_window:x=>seen.push(x), near_after:1,
    request_frame:cb=>{frames.push(cb);return frames.length},cancel_frame:()=>{},
    intersection_observer_factory:cb=>{
      callback=cb;observer={observe:p=>observed.push(p),disconnect:()=>{}};return observer;
    },
  });
  obj.setIndex(new WebResourceWindowIndexV1(scene()));
  assert.equal(observed.length,500);
  assert.equal(frames.length,0); // No speculative p1 admission before IO sample.
  callback([{target:pages[79],isIntersecting:true,intersectionRect:{width:100,height:90}}]);
  frames.shift()();
  assert.equal(seen[0].current_page_id,'p80');
  assert.deepEqual(seen[0].near_page_ids,['p81']);
  for(let i=0;i<15;i++)container.dispatchEvent(new Event('scroll'));
  frames.shift()();
  assert.equal(rectReads,0);
  obj.dispose();
});

test('bindResourceWindowV1 projects viewport changes to the exact scheduler without inventing readiness', async()=>{
  const {bindResourceWindowV1}=await import('./web-resource-window-v1.mjs');
  const reader = new ResourceReadinessRuntimeV1({document_id:'doc:one',revision_id:'rev:one',snapshot_id:'snap:one'});
  const records=[];
  const scheduler = new WebResourceSchedulerV1({readiness:reader,
    fetch_resource:async({identity})=>({identity}),
    decode_resource:async({identity})=>({binding_identity:'test:'+identity.resource_id}),
    config:{max_network_inflight:2}});
  const container=new FakeContainer(bounds(0,100));
  const p1=new FakePage('p1',bounds(0,100));const p2=new FakePage('p2',bounds(101,201));
  const frame=[];
  const binding=bindResourceWindowV1({scene:scene(2),scheduler,host:{querySelectorAll:()=>[p1,p2]},
    scroll_container:container,request_frame:cb=>{frame.push(cb);return frame.length},cancel_frame:()=>{},
    on_projection:projection=>records.push(projection)});
  frame.shift()();await scheduler.whenIdle();
  assert.equal(records[0].current_page_id,'p1');
  assert.equal(reader.resourceState({resource_id:'img1',content_hash:hex(1)}).state,'ready');
  p1.rect=bounds(-120,-20);p2.rect=bounds(0,100);
  container.dispatchEvent(new Event('scroll'));frame.shift()();await scheduler.whenIdle();
  assert.equal(records[1].current_page_id,'p2');
  assert.equal(reader.resourceState({resource_id:'img2',content_hash:hex(2)}).state,'ready');
  assert.equal(reader.receipt().metrics.canonical_revisions_emitted,0);
  binding.dispose();scheduler.dispose();
});

test('index is immutable with respect to later Scene/descriptor mutation and does not retain inline payload',()=>{
  const source=scene(1);source.resources[0].inline_data_url='data:image/png;base64,THIS_IS_NOT_TO_BE_RETAINED';
  const idx=new WebResourceWindowIndexV1(source);
  source.resources[0].content_hash=hex(55);source.nodes[0].resource_id='hijacked';
  const result=idx.select({current_page_id:'p1'});
  assert.equal(result.demands[0].content_hash,hex(1));
  assert.equal(result.demands[0].resource_id,'img1');
  assert.equal('inline_data_url' in idx.descriptors.get('img1'),false);
});