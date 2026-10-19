// A browser failure may disclose only a fixed stage enum, never DOM error text.
// These exact labels belong to the existing WebFileEntryControllerV1.
const PENDING_PHASE_CODES = Object.freeze({
  Uploading: "ingress_uploading_timeout",
  Validating: "ingress_validation_timeout",
  "Preparing project": "ingress_project_timeout",
  Opening: "ingress_open_timeout",
  Ready: "ingress_ready_without_navigation",
});

const SOURCE_ENTRY_ERRORS = new Set([
  "source_validation_timeout",
  "source_validation_failed",
  "source_validation_exhausted",
  "source_baseline_worker_failed",
  "source_baseline_worker_timeout",
  "source_baseline_editor_legacy_low_text",
  "source_baseline_editor_legacy_quill",
  "source_baseline_editor_mature_2c",
  "source_baseline_editor_unknown",
  "source_baseline_editor_assets",
  "source_baseline_worker_cpu_limit",
  "source_baseline_worker_terminated",
  "source_baseline_worker_sandbox_failed",
  "source_already_consumed",
  "source_not_validated_durable",
  "network_error",
  "invalid_ingress_response",
]);
export const NAVIGATION_FAILURE_CODES = Object.freeze([
  ...Object.values(PENDING_PHASE_CODES),
  "ingress_file_entry_rejected",
  "ingress_navigation_timeout",
  "ingress_reject_source_validation_timeout",
  "ingress_reject_source_validation_failed",
  "ingress_reject_source_validation_exhausted",
  "ingress_reject_source_baseline_worker_failed",
  "ingress_reject_source_baseline_worker_timeout",
  "ingress_reject_source_baseline_editor_legacy_low_text",
  "ingress_reject_source_baseline_editor_legacy_quill",
  "ingress_reject_source_baseline_editor_mature_2c",
  "ingress_reject_source_baseline_editor_unknown",
  "ingress_reject_source_baseline_editor_assets",
  "ingress_reject_source_baseline_worker_cpu_limit",
  "ingress_reject_source_baseline_worker_terminated",
  "ingress_reject_source_baseline_worker_sandbox_failed",
  "ingress_reject_source_already_consumed",
  "ingress_reject_source_not_validated_durable",
  "ingress_reject_network_error",
  "ingress_reject_invalid_ingress_response",
]);

export function classifyNavigationFailure(status, hasError, errorCode = null) {
  if (hasError === true || status === "Needs attention") {
    return SOURCE_ENTRY_ERRORS.has(errorCode)
      ? "ingress_reject_" + errorCode : "ingress_file_entry_rejected";
  }
  if (typeof status !== "string") return "ingress_navigation_timeout";
  return PENDING_PHASE_CODES[status.trim()] ?? "ingress_navigation_timeout";
}
