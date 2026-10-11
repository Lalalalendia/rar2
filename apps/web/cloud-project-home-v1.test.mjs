import test from "node:test";
import assert from "node:assert/strict";
import { editorPathForDocument } from "./cloud-project-home-v1.mjs";

test("project home opens canonical SourceIngress document route", () => {
  assert.equal(
    editorPathForDocument("document:0123456789abcdef01234567"),
    "/editor/doc/document%3A0123456789abcdef01234567",
  );
});

test("project home rejects forged document paths", () => {
  assert.throws(() => editorPathForDocument("document:../../bad"), /invalid canonical/);
});
