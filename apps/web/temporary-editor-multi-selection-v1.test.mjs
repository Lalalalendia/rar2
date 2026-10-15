import test from "node:test";
import assert from "node:assert/strict";
import { TemporaryEditorMultiSelectionV1 } from "./temporary-editor-multi-selection-v1.mjs";

const DOC = "guest-edit:test";
const SOURCE = "f".repeat(64);
const PAGE = "11111111-1111-4111-8111-111111111111"; // imported PUB page; authored shapes are v7
const OTHER_PAGE = "22222222-2222-4222-8222-222222222222";
const A = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa";
const B = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb";
const C = "cccccccc-cccc-7ccc-8ccc-cccccccccccc";
const IMPORTED = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";

function sync(s, opts={}) {
  return s.sync({documentId:DOC,sourceHash:SOURCE,pageId:PAGE,admittedNodeIds:[A,B,C],...opts});
}

test("single-click replaces, Shift toggles, primary fallback is canonical", () => {
  const s=new TemporaryEditorMultiSelectionV1();sync(s);
  assert.deepEqual(s.click(B),{pageId:PAGE,primary:B,selectedNodeIds:[B],count:1});
  assert.deepEqual(s.click(A,{shiftKey:true}),{pageId:PAGE,primary:A,selectedNodeIds:[A,B],count:2});
  assert.deepEqual(s.click(C,{shiftKey:true}),{pageId:PAGE,primary:C,selectedNodeIds:[A,B,C],count:3});
  assert.deepEqual(s.click(C,{shiftKey:true}),{pageId:PAGE,primary:A,selectedNodeIds:[A,B],count:2});
  assert.deepEqual(s.click(B),{pageId:PAGE,primary:B,selectedNodeIds:[B],count:1});
  assert.deepEqual(s.click(null),{pageId:PAGE,primary:null,selectedNodeIds:[],count:0});
});

test("same-page Current refresh preserves survivor selection; page/source swap clears",()=>{
  const s=new TemporaryEditorMultiSelectionV1();sync(s);s.click(A);s.click(B,{shiftKey:true});
  assert.deepEqual(sync(s,{admittedNodeIds:[A,C]}),{pageId:PAGE,primary:A,selectedNodeIds:[A],count:1});
  assert.equal(sync(s,{pageId:OTHER_PAGE}).count,0);
  s.click(A);
  assert.equal(sync(s,{pageId:OTHER_PAGE,sourceHash:"a".repeat(64)}).count,0);
});

test("unknown, projected, imported and stale candidates never enter multi selection",()=>{
  const s=new TemporaryEditorMultiSelectionV1();sync(s);
  s.click(A);
  assert.equal(s.click(IMPORTED,{shiftKey:true}).count,1);
  assert.equal(s.click("3".repeat(36),{shiftKey:true}).count,1);
  assert.equal(s.click(IMPORTED).count,0);
  assert.equal(s.click(B,{shiftKey:true}).count,1);
});

test("fail-closed malformed identity/duplicate/limit never changes previous state",()=>{
  const s=new TemporaryEditorMultiSelectionV1();sync(s);s.click(A);
  assert.throws(()=>sync(s,{admittedNodeIds:[A,A]}),/duplicate/);
  assert.equal(s.primary,A);
  assert.throws(()=>sync(s,{admittedNodeIds:[IMPORTED]}),/UUIDv7/);
  assert.equal(s.primary,A);
  assert.throws(()=>sync(s,{admittedNodeIds:Array(16385).fill(A)}),/bounded/);
  assert.equal(s.primary,A);
  assert.throws(()=>sync(s,{sourceHash:"invalid"}),/SHA-256/);
  assert.equal(s.primary,A);
  assert.throws(()=>s.click(A,{shiftKey:1}),/boolean/);
  assert.equal(s.primary,A);
  assert.throws(()=>JSON.stringify(s),/not durable/);
});

test("selection is view-only and never invents document revisions or operations",()=>{
  const s=new TemporaryEditorMultiSelectionV1();const snap=sync(s);s.click(A);s.click(B,{shiftKey:true});
  const result=s.snapshot();
  assert.deepEqual(Object.keys(result).sort(),["count","pageId","primary","selectedNodeIds"]);
  assert.equal("revision_id" in result,false);
  assert.equal("operation" in result,false);
  assert.equal("sourceHash" in snap,false);
});

test("selected batch cannot silently exceed MAX_MOVE_NODES_V1=1024", () => {
  const ids = Array.from({ length: 1025 }, (_, i) =>
    `abcd0000-0000-7000-8000-${i.toString(16).padStart(12, "0")}`);
  const s = new TemporaryEditorMultiSelectionV1();
  sync(s, { admittedNodeIds: ids });
  s.click(ids[0]);
  for (const id of ids.slice(1, 1024)) s.click(id, { shiftKey: true });
  assert.equal(s.size, 1024);
  assert.throws(() => s.click(ids[1024], { shiftKey: true }), /batch limit/);
  assert.equal(s.size, 1024);
  assert.equal(s.primary, ids[1023]);
});
