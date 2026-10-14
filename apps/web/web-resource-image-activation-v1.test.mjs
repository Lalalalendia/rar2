import test from 'node:test';
import assert from 'node:assert/strict';
import {webcrypto,createHash} from 'node:crypto';
import {ProductSafePreviewImageAdmissionV1,verifySafePreviewInlineV1,PRODUCT_IMAGE_ADMISSION_SCHEMA} from './web-resource-image-activation-v1.mjs';
const b64=Buffer.from('small safe image').toString('base64');
const hash=createHash('sha256').update(Buffer.from('small safe image')).digest('hex');
const ident={resource_id:'img-1',encoded_artifact_sha256:hash,derivative_id:'safe-preview:'+'a'.repeat(64),byte_len:16,inline_data_url:'data:image/png;base64,'+b64,mime:'image/png',availability:'inline_data_url'};
const scene=()=>({protocol_version:'chaptera.reader-scene.v1',document_id:'doc:one',revision_id:'sha256:r',pages:[{page_id:'p1',order:0},{page_id:'p2',order:1}],nodes:[{page_id:'p1',resource_id:'img-1'},{page_id:'p2',resource_id:'img-1'}],resources:[ident]});
const wait=()=>new Promise(r=>setTimeout(r,15));
async function eventually(predicate){for(let i=0;i<200;i++){if(predicate())return;await new Promise(r=>setTimeout(r,5));}throw new Error('condition did not settle');}
function fakeImage(){return {
  attributes:new Map(), listeners:new Map(), previousElementSibling:{getAttribute:()=> 'img-1',setAttribute(...args){this.hidden=true},removeAttribute(){this.hidden=false}},
  setAttribute(k,v){this.attributes.set(k,v)},getAttribute(k){return this.attributes.get(k)},removeAttribute(k){this.attributes.delete(k)},
  addEventListener(k,fn){this.listeners.set(k,fn)},removeEventListener(k,fn){if(this.listeners.get(k)===fn)this.listeners.delete(k)},fire(k){this.listeners.get(k)?.()}
}}
function open(extra={}) {let observer,decodes=0;
 const admission=new ProductSafePreviewImageAdmissionV1({host:{},scroll_container:{},crypto_object:webcrypto,atob_fn:atob,
   create_image_bitmap:async()=>{decodes++;return {width:3,height:3,close(){}}},
   make_observer:(opts)=>{observer={...opts,setIndex(){},dispose(){this.disposed=true}};return observer}, ...extra});
 admission.prepare(scene());return {admission,observer:()=>observer,decodes:()=>decodes};}
test('verified SafePreview bytes; mismatch, size, and cancellation reject',async()=>{
 const meta={identity:{content_hash:hash},mime:'image/png',href:ident.inline_data_url,byte_len:ident.byte_len};
 const r=await verifySafePreviewInlineV1(meta,{subtle:webcrypto.subtle,atob_fn:atob});assert.equal(r.bytes.length,16);
 await assert.rejects(verifySafePreviewInlineV1({...meta,identity:{content_hash:'f'.repeat(64)}},{subtle:webcrypto.subtle,atob_fn:atob}),/sha256_mismatch/);
 await assert.rejects(verifySafePreviewInlineV1({...meta,byte_len:15},{subtle:webcrypto.subtle,atob_fn:atob}),/size_invalid|size_mismatch/);
 await assert.rejects(verifySafePreviewInlineV1(meta,{subtle:webcrypto.subtle,atob_fn:atob,signal:{aborted:true}}),/aborted/);
});
test('only admitted image is intercepted; legacy/unsupported remain eager',()=>{
 const {admission}=open();const image=fakeImage();assert.equal(admission.captureImageHref({image,resource:ident,page_id:'p1',href:ident.inline_data_url}),true);
 assert.equal(admission.captureImageHref({image,resource:{resource_id:'legacy'},page_id:'p1',href:'data:image/png;base64,AA=='}),false);
 assert.equal(admission.captureImageHref({image,resource:ident,page_id:'not-a-page',href:ident.inline_data_url}),false);
 admission.dispose();
});
test('current page admitted before other page; shared resource decodes once and paints when visible',async()=>{
 const {admission,observer,decodes}=open();const p1=fakeImage(),p2=fakeImage();
 for(const [id,image] of [['p1',p1],['p2',p2]])assert.equal(admission.captureImageHref({image,resource:ident,page_id:id,href:ident.inline_data_url}),true);
 admission.mount(); observer().on_window({current_page_id:'p1',visible_page_ids:['p1'],near_page_ids:[]});
 await eventually(()=>decodes()===1 && p1.getAttribute('href')===ident.inline_data_url);assert.equal(decodes(),1);assert.equal(p1.getAttribute('href'),ident.inline_data_url);assert.equal(p2.getAttribute('href'),undefined);
 p1.fire('load');assert.equal(p1.previousElementSibling.hidden,true);
 observer().on_window({current_page_id:'p2',visible_page_ids:['p2'],near_page_ids:[]});
 assert.equal(p1.getAttribute('href'),undefined);assert.equal(p1.previousElementSibling.hidden,false);
 assert.equal(p2.getAttribute('href'),ident.inline_data_url);assert.equal(decodes(),1);
 assert.equal(admission.snapshot().readiness.dependency_count,2);assert.equal(admission.snapshot().schema,PRODUCT_IMAGE_ADMISSION_SCHEMA);
 admission.dispose();assert.equal(observer().disposed,true);
});
test('bad advertised hash cannot activate an image; remains explicit pending',async()=>{
 const bad={...ident,encoded_artifact_sha256:'f'.repeat(64)};
 const {admission,observer}=open();admission.prepare({...scene(),resources:[bad]});
 const image=fakeImage();assert.equal(admission.captureImageHref({image,resource:bad,page_id:'p1',href:bad.inline_data_url}),true);
 admission.mount();observer().on_window({current_page_id:'p1'});await eventually(()=>admission.readiness.resourceState({resource_id:'img-1',content_hash:bad.encoded_artifact_sha256,derivative_id:bad.derivative_id}).state==='failed');
 assert.equal(image.getAttribute('href'),undefined);assert.equal(admission.readiness.resourceState({resource_id:'img-1',content_hash:bad.encoded_artifact_sha256,derivative_id:bad.derivative_id}).state,'failed');
 admission.dispose();
});
test('unsupported browser does not suppress visual image and does not initialize a scheduler',()=>{
 const {admission}=open({create_image_bitmap:null});const image=fakeImage();
 assert.equal(admission.captureImageHref({image,resource:ident,page_id:'p1',href:ident.inline_data_url}),false);
 admission.mount();assert.equal(admission.snapshot().enabled,false);admission.dispose();
});
test('old unabortable decode holds admission budget across Scene revisions',async()=>{
 let unblock;let started;let decodes=0;
 const gate=new Promise(r=>{unblock=r});const firstStarted=new Promise(r=>{started=r});
 const {admission,observer}=open({create_image_bitmap:async()=>{
   decodes++;if(decodes===1){started();await gate;}
   return {width:2,height:2,close(){}};
 }});
 const old=fakeImage();admission.captureImageHref({image:old,resource:ident,page_id:'p1',href:ident.inline_data_url});
 admission.mount();observer().on_window({current_page_id:'p1'});await firstStarted;
 admission.prepare(scene());const now=fakeImage();
 admission.captureImageHref({image:now,resource:ident,page_id:'p1',href:ident.inline_data_url});
 admission.mount();observer().on_window({current_page_id:'p1'});
 await wait();assert.equal(decodes,1,'second Scene must wait for physical old decode');
 assert.equal(now.getAttribute('href'),undefined);
 unblock();await eventually(()=>decodes===2 && now.getAttribute('href')===ident.inline_data_url);
 assert.equal(old.getAttribute('href'),undefined,'stale previous image must remain unpainted');
 admission.dispose();
});
test('image preflight enforces decoded pixel ceiling and closes bitmap',async()=>{
 let closed=false;const {admission,observer}=open({create_image_bitmap:async()=>({width:5000,height:5000,close(){closed=true}})});
 const image=fakeImage();admission.captureImageHref({image,resource:ident,page_id:'p1',href:ident.inline_data_url});admission.mount();observer().on_window({current_page_id:'p1'});await eventually(()=>closed);
 assert.equal(image.getAttribute('href'),undefined);assert.equal(closed,true);admission.dispose();
});

test('scheduler setup failure falls back to unchanged eager image semantics',()=>{
 const {admission}=open({make_observer:()=>{throw new Error('observation unavailable')}});
 const image=fakeImage();admission.captureImageHref({image,resource:ident,page_id:'p1',href:ident.inline_data_url});
 assert.throws(()=>admission.mount(),/observation unavailable/);
 admission.fallbackToEager();
 assert.equal(image.getAttribute('href'),ident.inline_data_url);
 assert.equal(image.getAttribute('data-resource-activation'),'legacy-fallback');
 assert.equal(admission.snapshot().enabled,false);
 admission.dispose();
});
test('hidden window removes SVG material and does not paint before resume',async()=>{
 const {admission,observer}=open();const image=fakeImage();
 admission.captureImageHref({image,resource:ident,page_id:'p1',href:ident.inline_data_url});admission.mount();
 observer().on_window({current_page_id:'p1'});
 await eventually(()=>image.getAttribute('href')===ident.inline_data_url);
 observer().on_window({current_page_id:'p1',hidden:true});
 assert.equal(image.getAttribute('href'),undefined);
 observer().on_window({current_page_id:'p1',hidden:false});
 assert.equal(image.getAttribute('href'),ident.inline_data_url);
 admission.dispose();
});
