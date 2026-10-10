import test from "node:test";
import assert from "node:assert/strict";
import {prepareStoryTextForTextarea, restoreStoryTextFromTextarea,
  mapUnchangedTextareaSelectionToSourceScalarRange as map} from "./story-line-endings-v1.mjs";

const range=(source,a,b)=> {
  const profile=prepareStoryTextForTextarea(source);
  return map(source,profile,profile.value,a,b);
};

test("CRLF newline maps to both source scalars",()=>{
  assert.deepEqual(range("A😀\r\nB\r",3,4),{start_scalar:2,end_scalar:4});
  assert.deepEqual(range("A😀\r\nB\r",4,5),{start_scalar:4,end_scalar:5});
});

test("CRLF full textarea selection never consumes terminal Publisher CR",()=>{
  const source="A😀\r\nB\r";
  const profile=prepareStoryTextForTextarea(source);
  assert.equal(profile.value,"A😀\nB");
  assert.equal(restoreStoryTextFromTextarea(profile.value,profile),source);
  assert.deepEqual(range(source,0,profile.value.length),{start_scalar:0,end_scalar:5});
  assert.equal(Array.from(source).length,6);
});

test("CRLF scalar offset remains exact across multiple paragraphs",()=>{
  assert.deepEqual(range("ab\r\ncd\r\nef",4,6),{start_scalar:4,end_scalar:6});
  assert.deepEqual(range("ab\r\ncd\r\nef",6,8),{start_scalar:6,end_scalar:8});
});

test("bare CR, LF and no-separator Stories preserve ordinary scalar offsets",()=>{
  assert.deepEqual(range("A\rB\r",1,2),{start_scalar:1,end_scalar:2});
  assert.deepEqual(range("A\nB",1,2),{start_scalar:1,end_scalar:2});
  assert.deepEqual(range("abcd",1,3),{start_scalar:1,end_scalar:3});
});

test("surrogate pair is one source scalar and cannot be split",()=>{
  assert.deepEqual(range("A😀B",1,3),{start_scalar:1,end_scalar:2});
  assert.throws(()=>range("A😀B",2,3),/surrogate/);
  assert.throws(()=>range("A😀B",1,2),/surrogate/);
});

test("altered or stale textarea values refuse mapping",()=>{
  const original="One\r\nTwo\r";
  const p=prepareStoryTextForTextarea(original);
  assert.throws(()=>map(original,p,"One\nEvil",0,1),/unchanged exact/);
  assert.throws(()=>map("Change\r\nTwo\r",p,p.value,0,1),/unchanged exact/);
  assert.throws(()=>map(original,{...p,line_ending_kind:"lf"},p.value,0,1),/unchanged exact/);
  assert.throws(()=>map(original,{...p,terminal_cr:false},p.value,0,1),/unchanged exact/);
});

test("empty, reversed, outside and fractional offset ranges fail closed",()=>{
  const original="A\r\nB",p=prepareStoryTextForTextarea(original);
  for(const [a,b] of [[0,0],[3,2],[-1,1],[0,5],[.5,2],[0,2.5]]){
    assert.throws(()=>map(original,p,p.value,a,b),/nonempty exact/);
  }
});

test("unsupported mixed source separators remain rejected",()=>{
  assert.throws(()=>prepareStoryTextForTextarea("A\r\nB\nC"),/mixed Publisher Story/);
});
