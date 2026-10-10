const READER_SCENE_V1 = "chaptera.reader-scene.v1";
export const EDITOR_INTERACTION_SCENE_V1 =
  "chaptera.editor-interaction-scene.v1";

const FORBIDDEN_SOURCE_KEYS = new Set([
  "raw_pub_bytes",
  "raw_bytes",
  "source_path",
  "filesystem_path",
  "cfb_path",
  "stream_path",
  "stream_name",
  "parser_record",
  "carrier",
  "source_ref",
  "byte_range",
]);

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(label + " must be an object");
  }
  return value;
}

function safeInteger(value, label) {
  if (!Number.isSafeInteger(value)) {
    throw new RangeError(label + " must be a JavaScript-safe integer");
  }
  return value;
}

function positiveSafeInteger(value, label) {
  safeInteger(value, label);
  if (value <= 0) throw new RangeError(label + " must be positive");
  return value;
}

function canonicalUuid(value, label) {
  if (
    typeof value !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value)
  ) {
    throw new TypeError(label + " must be a canonical lowercase UUID");
  }
  return value;
}

function assertSourceNeutral(value, at = "$") {
  if (Array.isArray(value)) {
    value.forEach((child, index) => assertSourceNeutral(child, at + "[" + index + "]"));
    return;
  }
  if (value && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      if (FORBIDDEN_SOURCE_KEYS.has(key)) {
        throw new Error("Reader scene contains forbidden source field " + key + " at " + at);
      }
      assertSourceNeutral(child, at + "." + key);
    }
  }
}

function bounds(value, label) {
  object(value, label);
  return {
    x: safeInteger(value.x, label + ".x"),
    y: safeInteger(value.y, label + ".y"),
    width: positiveSafeInteger(value.width, label + ".width"),
    height: positiveSafeInteger(value.height, label + ".height"),
  };
}

function directPageLocal(node) {
  return (node.origin_node_id === undefined || node.origin_node_id === null)
    && (node.parent_node_id === undefined || node.parent_node_id === null);
}

export function projectReaderSceneToEditorInteractionScene(readerScene) {
  object(readerScene, "Reader scene");
  assertSourceNeutral(readerScene);
  if (readerScene.protocol_version !== READER_SCENE_V1) {
    throw new Error("chaptera.reader-scene.v1 is required");
  }
  if (typeof readerScene.document_id !== "string" || !readerScene.document_id) {
    throw new TypeError("Reader scene document_id is required");
  }
  if (
    typeof readerScene.source_hash !== "string" ||
    !/^[0-9a-f]{64}$/.test(readerScene.source_hash)
  ) {
    throw new TypeError("Reader scene source_hash must be lowercase SHA-256");
  }
  if (
    typeof readerScene.revision_id !== "string" ||
    !readerScene.revision_id.startsWith("sha256:")
  ) {
    throw new TypeError("Reader scene revision_id must be a sha256 identity");
  }
  if (!Array.isArray(readerScene.pages) || !Array.isArray(readerScene.nodes)) {
    throw new TypeError("Reader scene pages/nodes are required");
  }

  const pages = readerScene.pages.map((raw, index) => {
    const page = object(raw, "Reader scene page");
    return {
      page_id: canonicalUuid(page.page_id, "page_id"),
      order: safeInteger(page.order, "page.order"),
      width_emu: positiveSafeInteger(page.width_emu, "page.width_emu"),
      height_emu: positiveSafeInteger(page.height_emu, "page.height_emu"),
    };
  });
  const pageIds = new Set(pages.map((page) => page.page_id));
  if (pageIds.size !== pages.length) throw new Error("Reader scene has duplicate pages");

  const nodeIds = new Set();
  const nodes = [];
  for (const raw of readerScene.nodes) {
    const node = object(raw, "Reader scene node");
    if (!directPageLocal(node)) continue;
    const nodeId = canonicalUuid(node.node_id, "direct node_id");
    const pageId = canonicalUuid(node.page_id, "direct node page_id");
    if (!pageIds.has(pageId)) throw new Error("direct node targets unknown page");
    if (nodeIds.has(nodeId)) throw new Error("Reader scene has duplicate direct node identity");
    nodeIds.add(nodeId);
    nodes.push({
      node_id: nodeId,
      page_id: pageId,
      parent_node_id: null,
      kind: typeof node.kind === "string" ? node.kind : "unknown",
      bounds: bounds(node.bounds, "direct node bounds"),
      z_order: null,
      paint_order: null,
    });
  }

  pages.sort((a, b) => a.order - b.order || a.page_id.localeCompare(b.page_id));
  nodes.sort((a, b) => {
    const ap = pages.findIndex((page) => page.page_id === a.page_id);
    const bp = pages.findIndex((page) => page.page_id === b.page_id);
    return ap - bp || a.node_id.localeCompare(b.node_id);
  });

  return {
    protocol_version: EDITOR_INTERACTION_SCENE_V1,
    document_id: readerScene.document_id,
    source_hash: readerScene.source_hash,
    revision_id: readerScene.revision_id,
    visual_protocol_version: READER_SCENE_V1,
    visual_scene_authority: readerScene.scene_authority ?? null,
    stacking_fidelity: "unknown",
    pages,
    nodes,
  };
}

export function assertRichInteractionIdentity(readerScene, interactionScene) {
  object(readerScene, "Reader scene");
  object(interactionScene, "interaction scene");
  if (interactionScene.protocol_version !== EDITOR_INTERACTION_SCENE_V1) {
    throw new Error("editor interaction scene protocol mismatch");
  }
  for (const field of ["document_id", "source_hash", "revision_id"]) {
    if (readerScene[field] !== interactionScene[field]) {
      throw new Error("rich/interaction identity mismatch at " + field);
    }
  }
  return true;
}
