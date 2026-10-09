import test from "node:test";
import assert from "node:assert/strict";

import {
  assertFontEnvironmentMatchesScene,
  browserTextMetricAuthority,
  resolveBrowserFont,
  projectAdmittedFontChoices,
  chooseAdmittedFontCandidate,
} from "./font-environment-v1.mjs";

const scene = {
  document_id: "11111111-1111-4111-8111-111111111111",
  revision_id: "sha256:" + "1".repeat(64),
  snapshot_id: "sha256:" + "2".repeat(64),
  layout_environment: {
    environment_id: "sha256:" + "3".repeat(64),
    font_set_fingerprint: "sha256:" + "4".repeat(64),
  },
  capabilities: [{ key: "render.text", state: "partial", note: null }],
};

function env() {
  return {
    protocol_version: "chaptera.font-environment.v1",
    document_id: scene.document_id,
    revision_id: scene.revision_id,
    scene_snapshot_id: scene.snapshot_id,
    layout_environment_id: scene.layout_environment.environment_id,
    font_set_fingerprint: scene.layout_environment.font_set_fingerprint,
    preview_authority: "server_frame_geometry_only",
    fonts: [],
    diagnostics: [],
  };
}

test("font environment is fenced to exact scene/layout identity", () => {
  assert.doesNotThrow(() => assertFontEnvironmentMatchesScene(scene, env()));
  const changed = env();
  changed.font_set_fingerprint = "sha256:" + "9".repeat(64);
  assert.throws(
    () => assertFontEnvironmentMatchesScene(scene, changed),
    /font_set_fingerprint mismatch/,
  );
});

test("partial text scene cannot be promoted to positioned-glyph authority", () => {
  const changed = env();
  changed.preview_authority = "server_positioned_glyphs";
  assert.throws(
    () => assertFontEnvironmentMatchesScene(scene, changed),
    /does not provide authoritative positioned text/,
  );
});

test("blocked and server-only fonts never invent host fallback", () => {
  const blocked = resolveBrowserFont({
    font_fingerprint: "sha256:" + "5".repeat(64),
    delivery: "blocked",
    reason_code: "delivery.blocked",
  });
  assert.deepEqual(blocked, {
    kind: "blocked",
    font_fingerprint: "sha256:" + "5".repeat(64),
    reason_code: "delivery.blocked",
  });

  const serverOnly = resolveBrowserFont({
    font_fingerprint: "sha256:" + "6".repeat(64),
    delivery: "server_render_only",
    reason_code: "delivery.not_authorized",
  });
  assert.equal(serverOnly.kind, "server_render_only");
  assert.ok(!("family" in serverOnly));
});

test("explicit substitution uses only the declared fingerprinted fallback", () => {
  const resolved = resolveBrowserFont({
    font_fingerprint: "sha256:" + "5".repeat(64),
    delivery: "substitute_explicit",
    reason_code: "source.missing",
    fallback: {
      font_fingerprint: "sha256:" + "7".repeat(64),
      content_hash: "8".repeat(64),
      face_index: 0,
      family: "Fallback Sans",
      style: "Regular",
      resource_id: "81111111-1111-4111-8111-111111111111",
      fetch_handle: "font_resource_fallback",
    },
  });
  assert.equal(resolved.kind, "explicit_substitute");
  assert.equal(resolved.font_fingerprint, "sha256:" + "7".repeat(64));
  assert.equal(resolved.family, "Fallback Sans");
});

test("browser text metrics are never canonical document authority", () => {
  assert.deepEqual(browserTextMetricAuthority(env()), {
    browser_metrics_authoritative: false,
    preview_authority: "server_frame_geometry_only",
    rule: "browser metrics are preview/composition only; server relayout wins",
  });
});


const FONT_RESOURCE_A = "81111111-1111-4111-8111-111111111111";
const FONT_RESOURCE_B = "82222222-2222-4222-8222-222222222222";

function exactPhysicalFont(resourceId = FONT_RESOURCE_A, faceIndex = 0) {
  return {
    font_fingerprint: "sha256:" + "a".repeat(64),
    content_hash: "b".repeat(64),
    face_index: faceIndex,
    family: "Example Sans",
    style: "Regular",
    delivery: "deliver_exact",
    reason_code: "delivery.allowed",
    resource_id: resourceId,
    fetch_handle: "font_resource_exact",
    fallback: null,
  };
}

function authoringAdmission(fonts) {
  return {
    protocol_version: "chaptera.font-authoring-admission.v1",
    document_id: scene.document_id,
    revision_id: scene.revision_id,
    scene_snapshot_id: scene.snapshot_id,
    layout_environment_id: scene.layout_environment.environment_id,
    font_set_fingerprint: scene.layout_environment.font_set_fingerprint,
    resources: fonts.map(({ resource_id, font_fingerprint, content_hash, face_index }) => ({
      resource_id, font_fingerprint, content_hash, face_index,
    })),
  };
}

test("font delivery alone never authorizes authoring options", () => {
  const delivery = env();
  delivery.fonts = [exactPhysicalFont()];
  assert.deepEqual(projectAdmittedFontChoices(scene, delivery), []);
  assert.deepEqual(projectAdmittedFontChoices(scene, delivery, authoringAdmission([])), []);
  assert.throws(
    () => chooseAdmittedFontCandidate(scene, delivery, null, FONT_RESOURCE_A),
    /selected by admitted resource identity/,
  );
});

test("exact independently admitted resource yields an identity-only candidate", () => {
  const delivery = env();
  const font = exactPhysicalFont();
  delivery.fonts = [font];
  const grant = authoringAdmission([font]);
  const originalDelivery = JSON.stringify(delivery);
  const originalGrant = JSON.stringify(grant);
  const choices = projectAdmittedFontChoices(scene, delivery, grant);
  assert.equal(choices.length, 1);
  assert.ok(Object.isFrozen(choices));
  assert.ok(Object.isFrozen(choices[0]));
  assert.equal(choices[0].kind, "admitted_exact_font");
  assert.equal(choices[0].display_family, "Example Sans");
  assert.equal(choices[0].resource_id, FONT_RESOURCE_A);
  assert.ok(!("mutation" in choices[0]));
  const candidate = chooseAdmittedFontCandidate(scene, delivery, grant, FONT_RESOURCE_A);
  assert.deepEqual(candidate, {
    protocol_version: "chaptera.font-replacement-candidate.v1",
    document_id: scene.document_id,
    expected_revision_id: scene.revision_id,
    scene_snapshot_id: scene.snapshot_id,
    layout_environment_id: scene.layout_environment.environment_id,
    font_set_fingerprint: scene.layout_environment.font_set_fingerprint,
    resource_id: FONT_RESOURCE_A,
    font_fingerprint: font.font_fingerprint,
    content_hash: font.content_hash,
    face_index: font.face_index,
    authority: "candidate_only_server_validation_required",
  });
  assert.equal(JSON.stringify(delivery), originalDelivery);
  assert.equal(JSON.stringify(grant), originalGrant);
});

test("two identically named fonts remain different exact physical choices", () => {
  const delivery = env();
  const a = exactPhysicalFont(FONT_RESOURCE_A, 0);
  const b = exactPhysicalFont(FONT_RESOURCE_B, 1);
  b.font_fingerprint = "sha256:" + "c".repeat(64);
  b.content_hash = "d".repeat(64);
  delivery.fonts = [b, a];
  const grant = authoringAdmission([b, a]);
  const choices = projectAdmittedFontChoices(scene, delivery, grant);
  assert.deepEqual(choices.map((choice) => choice.resource_id), [FONT_RESOURCE_A, FONT_RESOURCE_B]);
  assert.equal(choices[0].display_family, choices[1].display_family);
  assert.notEqual(
    chooseAdmittedFontCandidate(scene, delivery, grant, FONT_RESOURCE_A).font_fingerprint,
    chooseAdmittedFontCandidate(scene, delivery, grant, FONT_RESOURCE_B).font_fingerprint,
  );
  assert.throws(
    () => chooseAdmittedFontCandidate(scene, delivery, grant, "Example Sans"),
    /selected by admitted resource identity/,
  );
});

test("stale Scene and mismatched admission environment are rejected", () => {
  const delivery = env();
  const font = exactPhysicalFont();
  delivery.fonts = [font];
  const grant = authoringAdmission([font]);
  const staleScene = structuredClone(scene);
  staleScene.revision_id = "sha256:" + "9".repeat(64);
  assert.throws(
    () => projectAdmittedFontChoices(staleScene, delivery, grant),
    /revision_id mismatch/,
  );
  const staleGrant = structuredClone(grant);
  staleGrant.font_set_fingerprint = "sha256:" + "8".repeat(64);
  assert.throws(
    () => projectAdmittedFontChoices(scene, delivery, staleGrant),
    /font_set_fingerprint mismatch/,
  );
  staleGrant.font_set_fingerprint = grant.font_set_fingerprint;
  staleGrant.scene_snapshot_id = "sha256:" + "7".repeat(64);
  assert.throws(
    () => projectAdmittedFontChoices(scene, delivery, staleGrant),
    /scene_snapshot_id mismatch/,
  );
});

test("unadmitted, subset, substituted and blocked physical fonts cannot be selected", () => {
  const delivery = env();
  const font = exactPhysicalFont();
  delivery.fonts = [font];
  const grant = authoringAdmission([font]);
  for (const mode of ["deliver_subset", "substitute_explicit", "server_render_only", "blocked"]) {
    const changed = structuredClone(delivery);
    changed.fonts[0].delivery = mode;
    assert.throws(
      () => projectAdmittedFontChoices(scene, changed, grant),
      /not an explicitly delivered exact resource/,
    );
  }
  const extra = structuredClone(grant);
  extra.resources[0].resource_id = FONT_RESOURCE_B;
  assert.throws(
    () => projectAdmittedFontChoices(scene, delivery, extra),
    /not an explicitly delivered exact resource/,
  );
});

test("resource hash, face index and duplicates are all admission gates", () => {
  const delivery = env();
  const font = exactPhysicalFont();
  delivery.fonts = [font];
  const original = authoringAdmission([font]);
  const badFingerprint = structuredClone(original);
  badFingerprint.resources[0].font_fingerprint = "sha256:" + "c".repeat(64);
  assert.throws(
    () => projectAdmittedFontChoices(scene, delivery, badFingerprint),
    /font_fingerprint differs/,
  );
  const badHash = structuredClone(original);
  badHash.resources[0].content_hash = "c".repeat(64);
  assert.throws(
    () => projectAdmittedFontChoices(scene, delivery, badHash),
    /content_hash differs/,
  );
  const badFace = structuredClone(original);
  badFace.resources[0].face_index = 1;
  assert.throws(
    () => projectAdmittedFontChoices(scene, delivery, badFace),
    /face_index differs/,
  );
  const outOfRange = structuredClone(original);
  outOfRange.resources[0].face_index = 65536;
  assert.throws(
    () => projectAdmittedFontChoices(scene, delivery, outOfRange),
    /exact font identity and face/,
  );
  const duplicate = authoringAdmission([font, font]);
  assert.throws(
    () => projectAdmittedFontChoices(scene, delivery, duplicate),
    /duplicate authoring font grant/,
  );
  const duplicatePhysical = structuredClone(delivery);
  duplicatePhysical.fonts.push(structuredClone(font));
  assert.throws(
    () => projectAdmittedFontChoices(scene, duplicatePhysical, original),
    /duplicate physical font resource identity/,
  );
});
