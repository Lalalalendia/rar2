export function assertFontEnvironmentMatchesScene(scene, fontEnvironment) {
  const checks = [
    ["document_id", scene.document_id, fontEnvironment.document_id],
    ["revision_id", scene.revision_id, fontEnvironment.revision_id],
    ["scene_snapshot_id", scene.snapshot_id, fontEnvironment.scene_snapshot_id],
    [
      "layout_environment_id",
      scene.layout_environment.environment_id,
      fontEnvironment.layout_environment_id,
    ],
    [
      "font_set_fingerprint",
      scene.layout_environment.font_set_fingerprint,
      fontEnvironment.font_set_fingerprint,
    ],
  ];
  for (const [label, expected, actual] of checks) {
    if (expected !== actual) {
      throw new Error(`${label} mismatch`);
    }
  }
  if (fontEnvironment.preview_authority === "server_positioned_glyphs") {
    const text = scene.capabilities.find((item) => item.key === "render.text");
    if (!text || text.state !== "supported") {
      throw new Error("scene does not provide authoritative positioned text");
    }
  }
}

export function resolveBrowserFont(fontDescriptor) {
  switch (fontDescriptor.delivery) {
    case "deliver_exact":
    case "deliver_subset":
      if (!fontDescriptor.resource_id || !fontDescriptor.fetch_handle) {
        throw new Error("font delivery requires explicit resource");
      }
      return {
        kind: "explicit_font_resource",
        font_fingerprint: fontDescriptor.font_fingerprint,
        content_hash: fontDescriptor.content_hash,
        face_index: fontDescriptor.face_index,
        resource_id: fontDescriptor.resource_id,
        fetch_handle: fontDescriptor.fetch_handle,
      };

    case "substitute_explicit":
      if (!fontDescriptor.fallback) {
        throw new Error("explicit substitution requires fallback");
      }
      return {
        kind: "explicit_substitute",
        source_font_fingerprint: fontDescriptor.font_fingerprint,
        ...fontDescriptor.fallback,
      };

    case "server_render_only":
      return {
        kind: "server_render_only",
        font_fingerprint: fontDescriptor.font_fingerprint,
        reason_code: fontDescriptor.reason_code,
      };

    case "blocked":
      return {
        kind: "blocked",
        font_fingerprint: fontDescriptor.font_fingerprint,
        reason_code: fontDescriptor.reason_code,
      };

    default:
      throw new Error("unknown font delivery mode");
  }
}

export function browserTextMetricAuthority(fontEnvironment) {
  return {
    browser_metrics_authoritative: false,
    preview_authority: fontEnvironment.preview_authority,
    rule:
      fontEnvironment.preview_authority === "server_positioned_glyphs"
        ? "render server positions; browser font metrics do not relayout"
        : "browser metrics are preview/composition only; server relayout wins",
  };
}

// EDITOR-FONT-FAMILY-REPLACE-01: client-only projection of explicitly admitted
// physical resources. Delivery permission alone is NOT authoring permission.
// This model creates no EditOperation; the authoritative Editor service must
// independently validate the target Story/range and exact resource on commit.
const FONT_SHA256_ID = /^sha256:[0-9a-f]{64}$/;
const FONT_CONTENT_SHA256 = /^[0-9a-f]{64}$/;
const FONT_RESOURCE_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

function exactFontIdentity(value) {
  return Boolean(
    value && typeof value === "object" &&
    FONT_RESOURCE_UUID.test(value.resource_id) &&
    FONT_SHA256_ID.test(value.font_fingerprint) &&
    FONT_CONTENT_SHA256.test(value.content_hash) &&
    Number.isInteger(value.face_index) &&
    value.face_index >= 0 && value.face_index <= 65535
  );
}

// The optional admission comes from a future authoritative server endpoint;
// callers must not manufacture it from the BrowserFontEnvironment. Absence
// produces no choices. These checks protect the UI against stale responses,
// not against an untrusted client; the server remains the sole edit authority.
export function projectAdmittedFontChoices(scene, fontEnvironment, admission = null) {
  assertFontEnvironmentMatchesScene(scene, fontEnvironment);
  if (admission === null) return Object.freeze([]);
  if (!admission || admission.protocol_version !== "chaptera.font-authoring-admission.v1") {
    throw new Error("explicit font authoring admission is required");
  }
  for (const [key, expected] of [
    ["document_id", scene.document_id],
    ["revision_id", scene.revision_id],
    ["scene_snapshot_id", scene.snapshot_id],
    ["layout_environment_id", fontEnvironment.layout_environment_id],
    ["font_set_fingerprint", fontEnvironment.font_set_fingerprint],
  ]) {
    if (admission[key] !== expected) {
      throw new Error("font authoring admission " + key + " mismatch");
    }
  }
  if (!Array.isArray(admission.resources) || !Array.isArray(fontEnvironment.fonts)) {
    throw new Error("font authoring resource catalogs must be arrays");
  }

  const delivered = new Map();
  for (const descriptor of fontEnvironment.fonts) {
    if (descriptor.resource_id === null) continue;
    if (!exactFontIdentity(descriptor)) {
      throw new Error("font environment resource identity is incomplete");
    }
    if (delivered.has(descriptor.resource_id)) {
      throw new Error("duplicate physical font resource identity");
    }
    delivered.set(descriptor.resource_id, descriptor);
  }

  const choices = [];
  const seen = new Set();
  for (const grant of admission.resources) {
    if (!exactFontIdentity(grant)) {
      throw new Error("authoring admission requires exact font identity and face");
    }
    if (seen.has(grant.resource_id)) {
      throw new Error("duplicate authoring font grant");
    }
    seen.add(grant.resource_id);
    const physical = delivered.get(grant.resource_id);
    // A subset may not contain glyphs required after a text edit. A fallback
    // is never promoted merely because it has a matching family name.
    if (!physical || physical.delivery !== "deliver_exact" || !physical.fetch_handle) {
      throw new Error("authoring font is not an explicitly delivered exact resource");
    }
    for (const key of ["font_fingerprint", "content_hash", "face_index"]) {
      if (physical[key] !== grant[key]) {
        throw new Error("authoring font " + key + " differs from delivered resource");
      }
    }
    choices.push(Object.freeze({
      kind: "admitted_exact_font",
      resource_id: grant.resource_id,
      font_fingerprint: grant.font_fingerprint,
      content_hash: grant.content_hash,
      face_index: grant.face_index,
      // Display metadata only; family/style cannot identify the font.
      display_family: physical.family,
      display_style: physical.style,
    }));
  }
  return Object.freeze(choices.sort((a, b) =>
    a.resource_id.localeCompare(b.resource_id)
  ));
}

export function chooseAdmittedFontCandidate(scene, fontEnvironment, admission, resourceId) {
  const choices = projectAdmittedFontChoices(scene, fontEnvironment, admission);
  const chosen = choices.find((choice) => choice.resource_id === resourceId);
  if (!chosen) {
    throw new Error("font candidate must be selected by admitted resource identity");
  }
  return Object.freeze({
    protocol_version: "chaptera.font-replacement-candidate.v1",
    document_id: scene.document_id,
    expected_revision_id: scene.revision_id,
    scene_snapshot_id: scene.snapshot_id,
    layout_environment_id: fontEnvironment.layout_environment_id,
    font_set_fingerprint: fontEnvironment.font_set_fingerprint,
    resource_id: chosen.resource_id,
    font_fingerprint: chosen.font_fingerprint,
    content_hash: chosen.content_hash,
    face_index: chosen.face_index,
    authority: "candidate_only_server_validation_required",
  });
}
