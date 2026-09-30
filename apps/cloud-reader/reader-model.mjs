import { assertReaderSceneSourceNeutral } from "./render-v1.mjs";

export const EMU_PER_CSS_PX = 9525;

export function orderedPages(scene) {
  assertReaderSceneSourceNeutral(scene);
  if (scene?.protocol_version !== "chaptera.reader-scene.v1") {
    throw new Error("scene_protocol_mismatch");
  }
  if (!Array.isArray(scene.pages) || !Array.isArray(scene.nodes) || !Array.isArray(scene.stories)) {
    throw new Error("scene_protocol_mismatch");
  }
  const ids = new Set();
  const orders = new Set();
  for (const page of scene.pages) {
    if (!page || typeof page.page_id !== "string" || !page.page_id || ids.has(page.page_id)
        || !Number.isSafeInteger(page.order) || page.order < 0 || orders.has(page.order)
        || !Number.isSafeInteger(page.width_emu) || page.width_emu <= 0
        || !Number.isSafeInteger(page.height_emu) || page.height_emu <= 0) {
      throw new Error("scene_protocol_mismatch");
    }
    ids.add(page.page_id);
    orders.add(page.order);
  }
  const stories = new Set();
  for (const story of scene.stories) {
    if (!story || typeof story.story_id !== "string" || !story.story_id
        || stories.has(story.story_id) || typeof story.text !== "string") {
      throw new Error("scene_protocol_mismatch");
    }
    stories.add(story.story_id);
  }
  for (const key of ["resources", "fonts", "diagnostics"]) {
    if (scene[key] !== undefined && (!Array.isArray(scene[key])
        || scene[key].some((item) => !item || typeof item !== "object"))) {
      throw new Error("scene_protocol_mismatch");
    }
  }
  if (scene.nodes.some((node) => !node || typeof node !== "object")
      || (scene.fidelity?.reasons !== undefined && !Array.isArray(scene.fidelity.reasons))) {
    throw new Error("scene_protocol_mismatch");
  }
  return [...scene.pages].sort((left, right) => left.order - right.order);
}

export function searchStories(scene, query, limit = 200) {
  const matches = [];
  if (!query || query.length > 200) return { matches, truncated: false };
  const literal = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(literal, "giu");
  for (const story of scene.stories ?? []) {
    if (typeof story.text !== "string") continue;
    let previousEnd = 0;
    let scalarCursor = 0;
    for (const match of story.text.matchAll(pattern)) {
      if (matches.length === limit) return { matches, truncated: true };
      scalarCursor += Array.from(story.text.slice(previousEnd, match.index)).length;
      const start = scalarCursor;
      const end = start + Array.from(match[0]).length;
      matches.push({
        story_id: story.story_id,
        scalar_start: start,
        scalar_end: end,
        utf16_start: match.index,
        utf16_end: match.index + match[0].length,
        snippet: story.text.slice(Math.max(0, match.index - 40), match.index + match[0].length + 70)
      });
      previousEnd = match.index + match[0].length;
      scalarCursor = end;
    }
  }
  return { matches, truncated: false };
}

export function guestRequestPath(value, action, origin) {
  const url = new URL(value, origin);
  if (url.origin !== origin || url.search || url.hash
      || !/^\/v1\/reader\/guest-sessions\/[A-Za-z0-9_:-]+\/(content|open|scene|contribution-capability|contribute)$/.test(url.pathname)
      || !url.pathname.endsWith("/" + action)) {
    throw new Error("guest_path_invalid");
  }
  return url.pathname;
}

export function contributionEligible(failureClassification) {
  return failureClassification?.protocol_version === "chaptera.failure-classifier.v1"
    && ["PUB_HIGH_VALUE", "PUB_DAMAGED"].includes(failureClassification.class);
}

export function extractableImages(scene) {
  let totalBytes = 0;
  return (scene.resources ?? []).filter((resource) => {
    if (resource.availability !== "inline_data_url" || typeof resource.inline_data_url !== "string") return false;
    const match = /^data:(image\/(?:png|jpeg|jpg|gif));base64,([A-Za-z0-9+/]*={0,2})$/.exec(resource.inline_data_url);
    if (!match || match[1] !== resource.mime || !match[2] || match[2].length % 4 !== 0) return false;
    const padding = match[2].endsWith("==") ? 2 : match[2].endsWith("=") ? 1 : 0;
    const bytes = match[2].length / 4 * 3 - padding;
    if (bytes > 4 * 1024 * 1024 || totalBytes + bytes > 8 * 1024 * 1024) return false;
    totalBytes += bytes;
    return true;
  });
}

export function classificationMessage(classification, failureClass = null) {
  if (classification === "unsupported") {
    const terminal = ({
      PUB_DAMAGED: "This Publisher file appears damaged and could not be opened completely. Keep the original; a recovery path may still help.",
      NOT_PUB: "This file is not a Publisher document. Choose a .PUB file.",
      PUB_HIGH_VALUE: "This Publisher file is not supported yet. Keep the original; it may help Chaptera improve compatibility.",
      PUB_POSSIBLE: "This file may be Publisher-related, but the service cannot verify or open it yet.",
      ARCHIVE_WITH_PUB: "This archive contains Publisher material, but the online Reader opens Publisher files directly."
    })[failureClass];
    if (terminal) return terminal;
  }
  return ({
    supported: "Opened read-only.",
    partial: "Opened with display limitations. Check the details before relying on the page appearance.",
    unsupported: "This Publisher file is not supported yet. Choose another file to continue.",
    damaged: "This Publisher file could not be opened completely. Keep the original; choose another file or use a recovery tool.",
    not_pub: "This file is not a Publisher document. Choose a .PUB file.",
    NOT_PUB: "This file is not a Publisher document. Choose a .PUB file.",
    security_rejected: "The file could not be accepted safely. Choose another file.",
    rejected: "The file could not be accepted safely. Choose another file."
  })[classification] ?? "The service could not open this file. Choose another file or retry.";
}

export function errorMessage(error) {
  if (error?.name === "AbortError") return "Opening cancelled. Choose a file to try again.";
  if (error?.status === 413) return "This file is too large for the viewing service. Choose a smaller file.";
  if (error?.status === 429) return "The service is busy or your viewing limit was reached. Wait a little and retry.";
  if ([401, 403].includes(error?.status)) return "Access is unavailable or has expired. Reopen the file or check access to the saved document.";
  if ([404, 410].includes(error?.status)) return "The document or temporary viewing session is no longer available. Open the file again.";
  if (error?.status >= 500) return "The service is temporarily unavailable. Keep your original file and retry shortly.";
  if (["scene_protocol_mismatch", "guest_path_invalid", "guest_protocol_mismatch"].includes(error?.code ?? error?.message)) {
    return "The website and viewing service are incompatible. Reload the page and retry.";
  }
  return "Opening failed. Check your connection and try again; your original file is unchanged.";
}
