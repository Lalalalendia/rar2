import test from "node:test";
import assert from "node:assert/strict";

import {
  WebMigrationUxV1,
  normalizeMigrationCapabilityV1,
} from "./migration-ux-v1.mjs";

const SOURCE = "a".repeat(64);

function capability({
  idml = {
    state: "available_with_declared_losses",
    reason_code: "serializable",
    declared_loss_count: 4,
    blocking_loss_count: 0,
  },
  odg = {
    state: "unavailable",
    reason_code: "blocking_losses",
    declared_loss_count: 2,
    blocking_loss_count: 1,
  },
} = {}) {
  return {
    protocol_version: "chaptera.migration-editable-route-response.v1",
    document_id: "doc:1",
    source_sha256: SOURCE,
    source_byte_len: 12345,
    open_state: "admitted",
    idml,
    odg,
  };
}

function setup() {
  const createCalls = [];
  const jobs = new Map();
  const service = {
    async currentDocument() {
      return {
        protocol_version: "chaptera.current-document.v1",
        document_id: "doc:1",
        source_hash: SOURCE,
        revision_id: "rev:baseline",
      };
    },
    async migrationEditableRoutes(sourceSha256) {
      assert.equal(sourceSha256, SOURCE);
      return capability();
    },
    async createMigrationExport(input) {
      createCalls.push(structuredClone(input));
      return {
        protocol_version: "chaptera.migration-export-job.v1",
        document_id: "doc:1",
        source_sha256: input.sourceSha256,
        target: input.target,
        target_profile:
          input.target === "idml"
            ? "idml:bounded-editable"
            : "odg:bounded-editable",
        revision_id: "rev:baseline",
        job_id: "export-job:1",
        status: "queued",
        declared_loss_count: 4,
        blocking_loss_count: 0,
      };
    },
    async exportStatus(jobId) {
      return structuredClone(jobs.get(jobId));
    },
    async authorizeExportDownload(jobId, artifactId) {
      return {
        protocol_version: "chaptera.export-download.v1",
        job_id: jobId,
        artifact_id: artifactId,
        download_handle: "https://download.invalid/artifact",
      };
    },
    async authorizeLossReportDownload(jobId, lossReportId) {
      return {
        protocol_version: "chaptera.export-loss-download.v1",
        job_id: jobId,
        loss_report_id: lossReportId,
        download_handle: "https://download.invalid/loss",
      };
    },
  };
  const ux = new WebMigrationUxV1({
    service,
    requestIdFactory: () => "migration-request-0001",
  });
  return { ux, service, jobs, createCalls };
}

test("capability normalization exposes only zero-blocker admitted routes", () => {
  const normalized = normalizeMigrationCapabilityV1(capability());
  assert.deepEqual(normalized.allowed_targets, ["idml"]);
  assert.equal(normalized.idml.available, true);
  assert.equal(normalized.odg.available, false);
});

test("inspect binds migration capability to exact current source hash", async () => {
  const { ux } = setup();
  const result = await ux.inspect();
  assert.equal(result.source_sha256, SOURCE);
  assert.deepEqual(result.allowed_targets, ["idml"]);
  assert.equal(ux.state().mode, "capability");
});

test("start refuses an unavailable target and never asks server to materialize it", async () => {
  const { ux, createCalls } = setup();
  await ux.inspect();
  await assert.rejects(() => ux.start("odg"), /not admitted/);
  assert.equal(createCalls.length, 0);
});

test("start preserves exact source identity and admitted target", async () => {
  const { ux, createCalls } = setup();
  await ux.inspect();
  const job = await ux.start("idml");
  assert.equal(job.job_id, "export-job:1");
  assert.equal(job.revision_id, "rev:baseline");
  assert.deepEqual(createCalls, [
    {
      sourceSha256: SOURCE,
      target: "idml",
      clientRequestId: "migration-request-0001",
    },
  ]);
});

test("retry create reuses the same logical request id", async () => {
  const { ux, service, createCalls } = setup();
  let calls = 0;
  const original = service.createMigrationExport;
  service.createMigrationExport = async (input) => {
    calls += 1;
    if (calls === 1) {
      createCalls.push(structuredClone(input));
      const error = new Error("network");
      error.code = "network";
      throw error;
    }
    return original(input);
  };
  await ux.inspect();
  await assert.rejects(() => ux.start("idml"), /network/);
  await ux.retryCreate();
  assert.equal(createCalls[0].clientRequestId, createCalls[1].clientRequestId);
});

test("ready migration exposes independently authorized artifact and loss report", async () => {
  const { ux, jobs } = setup();
  jobs.set("export-job:ready", {
    protocol_version: "chaptera.export-job-http.v1",
    job_id: "export-job:ready",
    document_id: "doc:1",
    revision_id: "rev:baseline",
    target_profile: "idml:bounded-editable",
    layout_environment_id: "sha256:" + "b".repeat(64),
    status: "ready",
    artifact_id: "binding:artifact",
    loss_report_id: "binding:loss",
    error_code: null,
    progress_percent: null,
  });

  const job = await ux.resume("export-job:ready");
  assert.equal(job.status, "ready");

  const artifact = await ux.downloadArtifact();
  assert.equal(artifact.download_handle, "https://download.invalid/artifact");

  const loss = await ux.downloadLossReport();
  assert.equal(loss.download_handle, "https://download.invalid/loss");
});

test("non-ready migration cannot download either resource", async () => {
  const { ux, jobs } = setup();
  jobs.set("export-job:run", {
    protocol_version: "chaptera.export-job-http.v1",
    job_id: "export-job:run",
    document_id: "doc:1",
    revision_id: "rev:baseline",
    target_profile: "idml:bounded-editable",
    layout_environment_id: "sha256:" + "b".repeat(64),
    status: "running",
    artifact_id: "binding:premature",
    loss_report_id: "binding:premature-loss",
    error_code: null,
    progress_percent: null,
  });
  await ux.resume("export-job:run");
  await assert.rejects(() => ux.downloadArtifact(), /not ready/);
  await assert.rejects(() => ux.downloadLossReport(), /not ready/);
});

test("resumable state stores only durable job identity", async () => {
  const { ux } = setup();
  await ux.inspect();
  await ux.start("idml");
  assert.deepEqual(ux.resumableState(), {
    schema_version: "chaptera.web-migration-resume.v1",
    job_id: "export-job:1",
  });
});
