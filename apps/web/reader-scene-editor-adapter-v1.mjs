const READER_SCENE_V1 = "chaptera.reader-scene.v1";
const BROWSER_SCENE_V1 = "chaptera.scene.v1";

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(label + " must be an object");
  }
  return value;
}

function safeInteger(value, label) {
  if (!Number.isSafeInteger(value)) throw new RangeError(label + " must be a JavaScript-safe integer");
  return value;
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    const out = {};
    for (const key of Object.keys(value).sort()) out[key] = canonicalize(value[key]);
    return out;
  }
  return value;
}

async function hashId(value) {
  const bytes = new TextEncoder().encode(JSON.stringify(canonicalize(value)));
  const digest = await globalThis.crypto.subtle.digest("SHA-256", bytes);
  const hex = [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  return "sha256:" + hex;
}

function rgba(rgb) {
  if (!Array.isArray(rgb) || rgb.length !== 3) return null;
  const [r, g, b] = rgb.map((value) => safeInteger(value, "rgb"));
  if ([r, g, b].some((value) => value < 0 || value > 255)) return null;
  return { r, g, b, a: 255 };
}

function browserPaint(node) {
  const paint = node.paint ?? null;
  if (!paint) return null;
  const fill = rgba(paint.fill_rgb);
  const line = paint.line ?? null;
  const strokeColor = rgba(line?.rgb);
  const stroke = strokeColor && Number.isSafeInteger(line?.width_emu)
    ? { color: strokeColor, width_emu: line.width_emu }
    : null;
  if (!fill && !stroke) return null;
  return {
    paint_id: "reader-paint:" + node.node_id,
    fill,
    stroke,
  };
}

function resourceFingerprint(resources) {
  return resources.map((resource) => ({
    resource_id: resource.resource_id,
    mime: resource.mime,
    availability: resource.availability,
  }));
}

export async function adaptReaderSceneToEditorScene(readerScene) {
  object(readerScene, "reader scene");
  if (readerScene.protocol_version !== READER_SCENE_V1) {
    throw new Error("chaptera.reader-scene.v1 is required");
  }
  if (typeof readerScene.document_id !== "string" || !readerScene.document_id) {
    throw new TypeError("reader scene document_id is required");
  }
  if (typeof readerScene.source_hash !== "string" || !/^[0-9a-f]{64}$/.test(readerScene.source_hash)) {
    throw new TypeError("reader scene source_hash must be lowercase SHA-256");
  }
  if (typeof readerScene.revision_id !== "string" || !readerScene.revision_id.startsWith("sha256:")) {
    throw new TypeError("reader scene revision_id is required");
  }

  const pages = [...(readerScene.pages ?? [])].map((page) => ({
    page_id: page.page_id,
    order: safeInteger(page.order, "page.order"),
    width_emu: safeInteger(page.width_emu, "page.width_emu"),
    height_emu: safeInteger(page.height_emu, "page.height_emu"),
  }));
  const exactStacking = readerScene.stacking_fidelity === "source_back_to_front";
  const pageOrder = new Map();
  const paints = [];
  const stories = [];
  const storyFrames = [];
  const nodes = [];

  for (const node of readerScene.nodes ?? []) {
    const order = pageOrder.get(node.page_id) ?? 0;
    pageOrder.set(node.page_id, order + 1);
    const paint = browserPaint(node);
    if (paint) paints.push(paint);
    if (typeof node.text === "string") {
      const storyId = "reader-story:" + node.node_id;
      stories.push({
        story_id: storyId,
        text: node.text,
        text_fidelity: node.text_layout?.disposition === "shared_resolved" ? "supported" : "partial",
      });
      storyFrames.push({ story_id: storyId, frame_ordinal: 0, node_id: node.node_id });
    }
    nodes.push({
      node_id: node.node_id,
      page_id: node.page_id,
      parent_node_id: node.parent_node_id ?? null,
      kind: node.kind,
      bounds: {
        x: safeInteger(node.bounds?.x, "node.bounds.x"),
        y: safeInteger(node.bounds?.y, "node.bounds.y"),
        width: safeInteger(node.bounds?.width, "node.bounds.width"),
        height: safeInteger(node.bounds?.height, "node.bounds.height"),
      },
      z_order: exactStacking ? order : null,
      paint_order: exactStacking ? order : null,
      paint_id: paint?.paint_id ?? null,
      resource_id: node.resource_id ?? null,
      image_source_window: node.image_source_window ?? null,
      image_content_rotation_degrees: node.image_content_rotation_degrees ?? null,
      image_recolor: node.image_recolor ?? null,
      transform: node.transform ?? { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
      editable: node.origin_node_id == null,
      origin_node_id: node.origin_node_id ?? null,
      preview_text_style: node.preview_text_style ?? null,
      text_bounds: node.text_bounds ?? null,
      text_layout: node.text_layout ?? null,
      visual_authority: "reader_scene",
    });
  }

  const imageResources = [...(readerScene.resources ?? [])].map((resource) => ({
    resource_id: resource.resource_id,
    kind: "image",
    mime: resource.mime,
    content_hash: null,
    byte_len: null,
    availability: resource.availability,
    fetch_handle: null,
    inline_data_url: resource.inline_data_url ?? null,
  }));
  const fontResources = [...(readerScene.fonts ?? [])].map((font) => ({
    resource_id: font.resource_id,
    kind: "font",
    mime: font.mime,
    content_hash: font.expected_sha256 ?? null,
    byte_len: null,
    availability: font.availability === "inline_data_url" ? "available" : "unknown",
    fetch_handle: null,
    inline_data_url: font.inline_data_url ?? null,
    expected_sha256: font.expected_sha256 ?? null,
    family_name: font.family_name ?? null,
  }));
  const resources = [...imageResources, ...fontResources];
  const resourceIds = new Set();
  for (const resource of resources) {
    if (typeof resource.resource_id !== "string" || resource.resource_id.length === 0) {
      throw new TypeError("Reader resource_id is required");
    }
    if (resourceIds.has(resource.resource_id)) {
      throw new Error("Reader scene contains duplicate image/font resource identity");
    }
    resourceIds.add(resource.resource_id);
  }

  const diagnostics = [...(readerScene.diagnostics ?? [])].map((diagnostic) => ({
    severity: diagnostic.severity,
    code: diagnostic.code,
    origin_node_id: diagnostic.origin_id ?? null,
    message_key: "reader." + diagnostic.code.toLowerCase().replaceAll("_", "."),
  }));

  const textNodes = nodes.filter((node) => stories.some((story) => story.story_id === "reader-story:" + node.node_id));
  const capabilities = [
    { key: "render.geometry", state: "supported", note: null },
    { key: "render.paint", state: paints.length ? "supported" : "partial", note: paints.length ? null : "No explicit source paint was projected." },
    { key: "render.resources", state: resources.length ? "supported" : "partial", note: resources.length ? null : "No inline image resources were projected." },
    { key: "render.stacking", state: exactStacking ? "supported" : "partial", note: exactStacking ? null : "Reader stacking authority is partial." },
    {
      key: "render.text",
      state: textNodes.length && textNodes.every((node) => node.text_layout?.disposition === "shared_resolved") ? "supported" : "partial",
      note: "Editor visual text authority is inherited from the current Reader Scene.",
    },
  ];

  const layoutEnvironment = {
    environment_id: await hashId({
      scene_authority: readerScene.scene_authority ?? "reader-scene",
      revision_id: readerScene.revision_id,
      resources: resourceFingerprint(resources),
      fonts: (readerScene.fonts ?? []).map((font) => ({
        resource_id: font.resource_id,
        expected_sha256: font.expected_sha256,
      })),
    }),
    engine_revision: readerScene.scene_authority ?? "reader-scene",
    font_set_fingerprint: await hashId((readerScene.fonts ?? []).map((font) => font.expected_sha256)),
    resource_fingerprint: await hashId(resourceFingerprint(resources)),
  };

  const scene = {
    protocol_version: BROWSER_SCENE_V1,
    document_id: readerScene.document_id,
    source_hash: readerScene.source_hash,
    revision_id: readerScene.revision_id,
    snapshot_id: "sha256:" + "0".repeat(64),
    layout_environment: layoutEnvironment,
    pages,
    nodes,
    stories,
    story_frames: storyFrames,
    paints,
    resources,
    diagnostics,
    capabilities,
    fidelity: structuredClone(readerScene.fidelity ?? { state: "partial", reasons: ["reader_fidelity_unknown"] }),
    stacking_fidelity: exactStacking ? "exact" : "partial",
  };
  const withoutId = structuredClone(scene);
  delete withoutId.snapshot_id;
  scene.snapshot_id = await hashId(withoutId);
  return scene;
}
