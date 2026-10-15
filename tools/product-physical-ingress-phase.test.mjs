import assert from "node:assert/strict";
import test from "node:test";
import { classifyNavigationFailure, NAVIGATION_FAILURE_CODES } from "./product-physical-ingress-phase.mjs";

test("known existing file-entry states get only fixed code", () => {
  for (const [status, code] of Object.entries({
    "Uploading": "ingress_uploading_timeout",
    "Validating": "ingress_validation_timeout",
    "Preparing project": "ingress_project_timeout",
    "Opening": "ingress_open_timeout",
    "Ready": "ingress_ready_without_navigation",
    "Needs attention": "ingress_file_entry_rejected",
  })) assert.equal(classifyNavigationFailure(status, false), code);
});

test("unknown/private DOM strings are redacted", () => {
  assert.equal(classifyNavigationFailure("secret book title and private path", false), "ingress_navigation_timeout");
  assert.equal(classifyNavigationFailure(null, false), "ingress_navigation_timeout");
  assert.equal(classifyNavigationFailure("secret book title", true), "ingress_file_entry_rejected");
  assert.ok(NAVIGATION_FAILURE_CODES.every(v => /^[a-z0-9_]+$/.test(v)));
});
