import test from "node:test";
import assert from "node:assert/strict";
import { ChapteraCloudProjectCatalogV1 } from "./chaptera-cloud-project-catalog-v1.mjs";
import { WebProjectHomeControllerV1 } from "./project-home-v1.mjs";

const WORKSPACE = "workspace:personal:" + "a".repeat(64);
const REVISION = "sha256:" + "b".repeat(64);

function row(overrides = {}) {
  return {
    project_id: "project:" + "c".repeat(24),
    document_id: "document:" + "d".repeat(24),
    name: "Newsletter.pub",
    lifecycle_state: "active",
    lifecycle_generation: 2,
    metadata_version: 5,
    workspace_id: WORKSPACE,
    current_revision_id: REVISION,
    created_at_ms: 1_800_000_000_000,
    ...overrides,
  };
}

function session(response) {
  const calls = [];
  return {
    calls,
    async prepare({ signal } = {}) {
      calls.push({ kind: "prepare", signal });
      return WORKSPACE;
    },
    async request(path, { signal } = {}) {
      calls.push({ kind: "request", path, signal });
      return structuredClone(response);
    },
  };
}

test("lists exact durable project rows from the authenticated personal workspace", async () => {
  const authority = session({ projects: [row()] });
  const client = new ChapteraCloudProjectCatalogV1({ workspaceSession: authority });
  const projects = await client.listProjects();
  assert.equal(projects.length, 1);
  assert.deepEqual(projects[0], row());
  assert.equal(
    authority.calls.find(x => x.kind === "request").path,
    "/v1/workspaces/" + encodeURIComponent(WORKSPACE) + "/projects",
  );
  assert.equal(Object.isFrozen(projects[0]), true);
});

test("catalog adapter feeds the real Project Home durable-project mode without projections", async () => {
  const authority = session({ projects: [row()] });
  const catalog = new ChapteraCloudProjectCatalogV1({ workspaceSession: authority });
  const controller = new WebProjectHomeControllerV1({ catalog });
  const state = await controller.loadProjects();
  assert.equal(state.mode, "projects");
  assert.equal(state.cards.length, 1);
  assert.equal(state.cards[0].project_id, row().project_id);
  assert.equal(state.cards[0].document_id, row().document_id);
  assert.equal(state.cards[0].current_revision_id, REVISION);
  assert.equal(state.cards[0].thumbnail.freshness, "missing");
});

test("catalog rejects a row that escapes the prepared workspace", async () => {
  const authority = session({
    projects: [row({ workspace_id: "workspace:personal:" + "e".repeat(64) })],
  });
  const client = new ChapteraCloudProjectCatalogV1({ workspaceSession: authority });
  await assert.rejects(
    () => client.listProjects(),
    error => error.code === "project_catalog_workspace_mismatch",
  );
});

test("catalog rejects stale-shaped rows without an exact current revision", async () => {
  const bad = row();
  delete bad.current_revision_id;
  const client = new ChapteraCloudProjectCatalogV1({
    workspaceSession: session({ projects: [bad] }),
  });
  await assert.rejects(() => client.listProjects(), /current_revision_id/);
});

test("catalog keeps web concurrency generations exact", async () => {
  for (const override of [
    { lifecycle_generation: Number.MAX_SAFE_INTEGER + 1 },
    { metadata_version: -1 },
  ]) {
    const client = new ChapteraCloudProjectCatalogV1({
      workspaceSession: session({ projects: [row(override)] }),
    });
    await assert.rejects(() => client.listProjects(), /safe integer/);
  }
});

test("catalog never treats trashed or malformed response rows as visible projects", async () => {
  for (const response of [
    { projects: [row({ lifecycle_state: "trashed" })] },
    { projects: null },
    { projects: Array.from({ length: 101 }, () => row()) },
  ]) {
    const client = new ChapteraCloudProjectCatalogV1({
      workspaceSession: session(response),
    });
    await assert.rejects(() => client.listProjects());
  }
});
