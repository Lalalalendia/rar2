// A browser failure may disclose only a fixed stage enum, never DOM error text.
// These exact labels belong to the existing WebFileEntryControllerV1.
const PENDING_PHASE_CODES = Object.freeze({
  Uploading: "ingress_uploading_timeout",
  Validating: "ingress_validation_timeout",
  "Preparing project": "ingress_project_timeout",
  Opening: "ingress_open_timeout",
  Ready: "ingress_ready_without_navigation",
});

export const NAVIGATION_FAILURE_CODES = Object.freeze([
  ...Object.values(PENDING_PHASE_CODES),
  "ingress_file_entry_rejected",
  "ingress_navigation_timeout",
]);

export function classifyNavigationFailure(status, hasError) {
  if (hasError === true || status === "Needs attention") return "ingress_file_entry_rejected";
  if (typeof status !== "string") return "ingress_navigation_timeout";
  return PENDING_PHASE_CODES[status.trim()] ?? "ingress_navigation_timeout";
}
