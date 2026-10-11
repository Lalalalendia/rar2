// Canonical project metadata mutation. The server resolves tenant/workspace and
// performs capability checks; the browser supplies only the selected ProjectId.
const ID_RE = /^[a-zA-Z0-9._:-]{1,160}$/;
const MAX_WEB_SAFE_INTEGER = 9007199254740991;
const MAX_NAME_BYTES = 512;
const encoder = new TextEncoder();

export function canonicalProjectRenameName(name) {
  if (typeof name !== "string") throw new TypeError("project name must be text");
  const normalized = name.trim();
  if (!normalized || encoder.encode(normalized).length > MAX_NAME_BYTES ||
      /[\p{Cc}]/u.test(normalized)) {
    throw new TypeError("project name must be bounded, non-empty and free of controls");
  }
  return normalized;
}

export function buildProjectRenameIntentV1(project, name, clientRequestId) {
  if (!project || typeof project !== "object" ||
      typeof project.project_id !== "string" || !ID_RE.test(project.project_id) ||
      project.lifecycle_state !== "active") {
    throw new TypeError("canonical active project required");
  }
  for (const key of ["lifecycle_generation", "metadata_version"]) {
    if (!Number.isSafeInteger(project[key]) || project[key] < 0 ||
        project[key] >= MAX_WEB_SAFE_INTEGER) {
      throw new TypeError("canonical project generation required");
    }
  }
  if (typeof clientRequestId !== "string" || !ID_RE.test(clientRequestId) ||
      clientRequestId.length < 8) {
    throw new TypeError("stable client request identity required");
  }
  return Object.freeze({
    protocol_version: "chaptera.project-rename.v1",
    expected_lifecycle_generation: project.lifecycle_generation,
    expected_metadata_version: project.metadata_version,
    name: canonicalProjectRenameName(name),
    client_request_id: clientRequestId,
  });
}

export async function submitProjectRenameV1({
  workspaceSession, project, name, clientRequestId,
}) {
  if (!workspaceSession || typeof workspaceSession.request !== "function") {
    throw new TypeError("authenticated same-origin workspace session required");
  }
  const intent = buildProjectRenameIntentV1(project, name, clientRequestId);
  const path = "/v1/projects/" + encodeURIComponent(project.project_id) + "/rename";
  const response = await workspaceSession.request(path, {
    method: "POST",
    json: intent,
  });
  const receipt = response?.receipt;
  if (response?.protocol_version !== "chaptera.project-rename-receipt.v1" ||
      !receipt || receipt.project_id !== project.project_id ||
      receipt.lifecycle_generation !== project.lifecycle_generation ||
      receipt.metadata_version !== project.metadata_version + 1 ||
      receipt.name !== intent.name || typeof receipt.replayed !== "boolean") {
    const error = new Error("project rename receipt not bound to exact request");
    error.code = "project_rename_receipt_invalid";
    error.retryable = false;
    throw error;
  }
  return Object.freeze({ ...receipt });
}
