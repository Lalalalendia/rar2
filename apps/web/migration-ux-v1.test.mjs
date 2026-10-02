import test from "node:test";
import assert from "node:assert/strict";
import { WebMigrationUxV1 } from "./migration-ux-v1.mjs";

test("migration ux module loads", () => {
  assert.equal(typeof WebMigrationUxV1, "function");
});
