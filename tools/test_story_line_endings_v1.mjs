#!/usr/bin/env node
import assert from "node:assert/strict";
import { prepareStoryTextForTextarea, restoreStoryTextFromTextarea } from
  "../apps/web/story-line-endings-v1.mjs";

function checkRoundTrip(original, mode) {
  const prepared = prepareStoryTextForTextarea(original);
  assert.equal(prepared.line_ending_kind, mode);
  assert.equal(restoreStoryTextFromTextarea(prepared.value, prepared), original);
  return prepared;
}

// Exact Sample3 Story neighborhood: the six digit deletion must NOT normalize
// all the surrounding native CR separators into LF bytes.
const sample3 = "This is the second page12345678\r\rIt is also times new roman, 10 point\rTable\r";
const s = checkRoundTrip(sample3, "cr");
assert.equal(
  restoreStoryTextFromTextarea(s.value.replace("345678", ""), s),
  sample3.replace("345678", ""),
);
assert.ok(restoreStoryTextFromTextarea(s.value, s).includes("\r\r"));

checkRoundTrip("one\r\ntwo\r\n", "crlf");
checkRoundTrip("one\ntwo\n", "lf");
checkRoundTrip("one\r", "none");
checkRoundTrip("single line", "none");

assert.throws(() => prepareStoryTextForTextarea("one\r\ntwo\rthree"),
  /mixed Publisher Story line endings/);
assert.throws(() => prepareStoryTextForTextarea("one\rtwo\nthree"),
  /mixed Publisher Story line endings/);
const unknown = prepareStoryTextForTextarea("single line");
assert.throws(() => restoreStoryTextFromTextarea("one\ntwo", unknown),
  /not grounded/);
assert.throws(() => restoreStoryTextFromTextarea("one\rtwo", s),
  /LF-only input/);

console.log(JSON.stringify({
  result: "PASS_source_line_ending_roundtrip",
  cases: ["sample3_cr", "crlf", "lf", "terminal_cr", "unknown", "mixed_rejected"],
  native_pub_authority: "unchanged",
}));
