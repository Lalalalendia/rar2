import test from "node:test";
import assert from "node:assert/strict";
import { documentIdFromEditorPath, loginUrlForReturnPath } from "./product-editor-entry-v1.mjs";

const DOC="10000000-0000-4000-8000-000000000001";

test("editor path admits one canonical document identity",()=>{
  assert.equal(documentIdFromEditorPath("/editor/doc/"+DOC),DOC);
  assert.equal(documentIdFromEditorPath("/editor/doc/"+DOC+"/"),DOC);
  assert.equal(documentIdFromEditorPath("/editor/doc/not-a-document"),null);
  assert.equal(documentIdFromEditorPath("/reader/doc/"+DOC),null);
});

test("login return stays same-origin relative",()=>{
  assert.equal(
    loginUrlForReturnPath("/editor/doc/"+DOC+"?mode=move"),
    "/v1/auth/login?return_path=%2Feditor%2Fdoc%2F"+DOC+"%3Fmode%3Dmove",
  );
  assert.throws(()=>loginUrlForReturnPath("https://evil.example/"),/local return path/);
  assert.throws(()=>loginUrlForReturnPath("//evil.example/"),/local return path/);
});

test("real UUIDv7 DocumentId is accepted by the Product Editor entry", () => {
  const v7 = "0199c4b0-e9a2-7d31-8b42-1234567890ab";
  assert.equal(documentIdFromEditorPath("/editor/doc/" + v7), v7);
  assert.equal(documentIdFromEditorPath("/editor/doc/" + v7.toUpperCase()), null);
  assert.equal(documentIdFromEditorPath("/editor/doc/" + v7 + "/extra"), null);
});
