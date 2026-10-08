function clone(value) {
  return value == null ? value : structuredClone(value);
}

function requireMethod(object, name, label) {
  if (!object || typeof object[name] !== "function") {
    throw new TypeError(label + "." + name + "() is required");
  }
}

function requireIdentity(value, label) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 256 ||
    !/^[A-Za-z0-9_.:@/-]+$/.test(value)
  ) {
    throw new TypeError(label + " must be a bounded identity");
  }
  return value;
}

function normalizeTarget(value, name) {
  if (!value || typeof value !== "object") {
    throw new TypeError(name + " migration target assessment is required");
  }
  const state = String(value.state ?? "");
  if (
    ![
      "available_with_declared_losses",
      "unavailable",
      "not_verified",
    ].includes(state)
  ) {
    throw new TypeError(name + " migration target state is unsupported");
  }
  const declared = Number(value.declared_loss_count ?? 0);
  const blocking = Number(value.blocking_loss_count ?? 0);
  if (
    !Number.isSafeInteger(declared) ||
    declared < 0 ||
    !Number.isSafeInteger(blocking) ||
    blocking < 0
  ) {
    throw new TypeError(name + " migration loss counts are invalid");
  }
  return Object.freeze({
    state,
    reason_code:
      typeof value.reason_code === "string" ? value.reason_code : "unknown",
    declared_loss_count: declared,
    blocking_loss_count: blocking,
    available: state === "available_with_declared_losses" && blocking === 0,
  });
}

export function normalizeMigrationCapabilityV1(value) {
  if (
    !value ||
    value.protocol_version !== "chaptera.migration-editable-route-response.v1"
  ) {
    throw new TypeError("migration capability response is required");
  }
  requireIdentity(value.document_id, "document_id");
  requireIdentity(value.source_sha256, "source_sha256");
  if (!Number.isSafeInteger(value.source_byte_len) || value.source_byte_len <= 0) {
    throw new TypeError("source_byte_len must be positive");
  }
  const idml = normalizeTarget(value.idml, "idml");
  const odg = normalizeTarget(value.odg, "odg");
  return Object.freeze({
    protocol_version: value.protocol_version,
    document_id: value.document_id,
    source_sha256: value.source_sha256,
    source_byte_len: value.source_byte_len,
    open_state: String(value.open_state ?? "not_admitted"),
    idml,
    odg,
    allowed_targets: Object.freeze(
      [
        idml.available ? "idml" : null,
        odg.available ? "odg" : null,
      ].filter(Boolean),
    ),
  });
}

function normalizeExportJob(value) {
  if (!value || typeof value !== "object") throw new TypeError("export job required");
  requireIdentity(value.job_id, "job_id");
  requireIdentity(value.revision_id, "revision_id");
  const status = String(value.status ?? "");
  if (!["queued", "running", "ready", "failed", "cancelled"].includes(status)) {
    throw new TypeError("unsupported export job status");
  }
  return Object.freeze({
    job_id: value.job_id,
    document_id: value.document_id ?? null,
    revision_id: value.revision_id,
    target_profile: value.target_profile ?? null,
    status,
    artifact_id: status === "ready" ? value.artifact_id ?? null : null,
    loss_report_id: status === "ready" ? value.loss_report_id ?? null : null,
    error_code: status === "failed" ? value.error_code ?? "export_failed" : null,
  });
}

export class WebMigrationUxV1 {
  constructor({ service, requestIdFactory = null, onState = null }) {
    for (const name of [
      "currentDocument",
      "migrationEditableRoutes",
      "createMigrationExport",
      "exportStatus",
      "authorizeExportDownload",
      "authorizeLossReportDownload",
    ]) {
      requireMethod(service, name, "service");
    }
    this.service = service;
    this.requestIdFactory =
      requestIdFactory ?? (() => globalThis.crypto.randomUUID());
    this.onState = onState;
    this._attempt = null;
    this._state = Object.freeze({
      protocol_version: "chaptera.web-migration-ux.v1",
      mode: "idle",
      source: null,
      capability: null,
      job: null,
      error_code: null,
    });
  }

  state() {
    return clone(this._state);
  }

  async inspect() {
    this._set({
      mode: "inspecting",
      source: null,
      capability: null,
      job: null,
      error_code: null,
    });
    try {
      const current = await this.service.currentDocument();
      requireIdentity(current.document_id, "document_id");
      requireIdentity(current.source_hash, "source_hash");
      const capability = normalizeMigrationCapabilityV1(
        await this.service.migrationEditableRoutes(current.source_hash),
      );
      if (
        capability.document_id !== current.document_id ||
        capability.source_sha256 !== current.source_hash
      ) {
        throw Object.assign(new Error("migration capability identity mismatch"), {
          code: "migration_capability_identity_mismatch",
        });
      }
      const source = Object.freeze({
        document_id: current.document_id,
        source_sha256: current.source_hash,
        revision_id: current.revision_id ?? null,
      });
      this._set({ mode: "capability", source, capability });
      return clone(capability);
    } catch (error) {
      this._fail(error);
      throw error;
    }
  }

  async start(target) {
    const capability = this._state.capability;
    const source = this._state.source;
    if (!capability || !source) {
      throw new Error("migration capability check required before start");
    }
    if (!capability.allowed_targets.includes(target)) {
      throw new Error(target + " migration route is not admitted");
    }
    const requestId = this.requestIdFactory("migration");
    requireIdentity(requestId, "requestId");
    this._attempt = Object.freeze({
      source_sha256: source.source_sha256,
      target,
      client_request_id: requestId,
    });
    return this._create(this._attempt);
  }

  async retryCreate() {
    if (!this._attempt) throw new Error("no migration create attempt to retry");
    return this._create(this._attempt);
  }

  async resume(jobId) {
    requireIdentity(jobId, "jobId");
    this._set({ mode: "loading_job", error_code: null });
    try {
      const job = normalizeExportJob(await this.service.exportStatus(jobId));
      this._set({ mode: "job", job });
      return clone(job);
    } catch (error) {
      this._fail(error);
      throw error;
    }
  }

  async refresh() {
    if (!this._state.job?.job_id) throw new Error("no migration job to refresh");
    return this.resume(this._state.job.job_id);
  }

  async downloadArtifact() {
    const job = this._state.job;
    if (!job || job.status !== "ready" || !job.artifact_id) {
      throw new Error("migration artifact is not ready");
    }
    return clone(
      await this.service.authorizeExportDownload(job.job_id, job.artifact_id),
    );
  }

  async downloadLossReport() {
    const job = this._state.job;
    if (!job || job.status !== "ready" || !job.loss_report_id) {
      throw new Error("migration loss report is not ready");
    }
    return clone(
      await this.service.authorizeLossReportDownload(
        job.job_id,
        job.loss_report_id,
      ),
    );
  }

  resumableState() {
    return Object.freeze({
      schema_version: "chaptera.web-migration-resume.v1",
      job_id: this._state.job?.job_id ?? null,
    });
  }

  async _create(attempt) {
    this._set({ mode: "creating", error_code: null });
    try {
      const created = await this.service.createMigrationExport({
        sourceSha256: attempt.source_sha256,
        target: attempt.target,
        clientRequestId: attempt.client_request_id,
      });
      requireIdentity(created.job_id, "job_id");
      if (
        created.source_sha256 !== attempt.source_sha256 ||
        created.target !== attempt.target
      ) {
        throw Object.assign(new Error("created migration changed exact source identity"), {
          code: "migration_export_identity_mismatch",
        });
      }
      const job = Object.freeze({
        job_id: created.job_id,
        document_id: created.document_id ?? null,
        revision_id: created.revision_id,
        target_profile: created.target_profile,
        status: created.status,
        artifact_id: null,
        loss_report_id: null,
        error_code: null,
      });
      this._set({ mode: "job", job });
      return clone(job);
    } catch (error) {
      this._fail(error);
      throw error;
    }
  }

  _fail(error) {
    this._set({
      mode: "error",
      error_code: error?.code ?? "migration_ux_failed",
    });
  }

  _set(patch) {
    this._state = Object.freeze({ ...this._state, ...patch });
    this.onState?.(this.state());
  }
}
