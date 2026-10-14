const ID_RE = /^[a-zA-Z0-9._:-]{1,192}$/;

function requireId(value, label) {
  if (typeof value !== "string" || !ID_RE.test(value)) {
    throw new TypeError("invalid " + label);
  }
  return value;
}

function requireSafeGeneration(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(label + " must be a non-negative safe integer");
  }
  return value;
}

function catalogError(code, message = code) {
  const error = new Error(message);
  error.code = code;
  error.retryable = false;
  return error;
}

function normalizeRow(row, workspaceId) {
  if (!row || typeof row !== "object" || Array.isArray(row)) {
    throw catalogError("project_catalog_row_invalid");
  }
  const projectId = requireId(row.project_id, "project_id");
  const documentId = requireId(row.document_id, "document_id");
  const rowWorkspaceId = requireId(row.workspace_id, "workspace_id");
  const currentRevisionId = requireId(row.current_revision_id, "current_revision_id");
  if (rowWorkspaceId !== workspaceId) {
    throw catalogError("project_catalog_workspace_mismatch");
  }
  if (row.lifecycle_state !== "active") {
    throw catalogError("project_catalog_lifecycle_invalid");
  }
  if (typeof row.name !== "string") {
    throw catalogError("project_catalog_name_invalid");
  }
  const lifecycleGeneration = requireSafeGeneration(
    row.lifecycle_generation,
    "lifecycle_generation",
  );
  const metadataVersion = requireSafeGeneration(row.metadata_version, "metadata_version");
  if (!Number.isSafeInteger(row.created_at_ms) || row.created_at_ms < 0) {
    throw catalogError("project_catalog_created_at_invalid");
  }
  return Object.freeze({
    project_id: projectId,
    document_id: documentId,
    name: row.name,
    lifecycle_state: "active",
    lifecycle_generation: lifecycleGeneration,
    metadata_version: metadataVersion,
    workspace_id: rowWorkspaceId,
    current_revision_id: currentRevisionId,
    created_at_ms: row.created_at_ms,
  });
}

// Thin browser adapter over the canonical Chaptera workspace project catalog.
// It owns no project cache, no ACL copy and no revision authority.
export class ChapteraCloudProjectCatalogV1 {
  constructor({ workspaceSession } = {}) {
    if (!workspaceSession ||
        typeof workspaceSession.prepare !== "function" ||
        typeof workspaceSession.request !== "function") {
      throw new TypeError("workspaceSession must implement prepare() and request()");
    }
    this.workspaceSession = workspaceSession;
  }

  async listProjects({ signal } = {}) {
    const workspaceId = requireId(
      await this.workspaceSession.prepare({ signal }),
      "workspace_id",
    );
    const path = "/v1/workspaces/" + encodeURIComponent(workspaceId) + "/projects";
    const response = await this.workspaceSession.request(path, { signal });
    if (!response || typeof response !== "object" || !Array.isArray(response.projects)) {
      throw catalogError("project_catalog_response_invalid");
    }
    if (response.projects.length > 100) {
      throw catalogError("project_catalog_response_unbounded");
    }
    return response.projects.map(row => normalizeRow(row, workspaceId));
  }
}
