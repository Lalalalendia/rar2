import { renderReaderScene } from "./render-v1.mjs";
import {
  EMU_PER_CSS_PX, orderedPages, searchStories, guestRequestPath,
  extractableImages, classificationMessage, errorMessage
} from "./reader-model.mjs";

const $ = (selector) => document.querySelector(selector);
const fileInput = $("#pub-file");
const fileButton = $("#open-file");
const documentInput = $("#document-id");
const documentButton = $("#open-document");
const status = $("#status");
const viewer = $("#viewer");
const pagesHost = $("#pages");
const pageSelect = $("#page-select");
const zoomSelect = $("#zoom-select");
const storySelect = $("#story-select");
const storyText = $("#story-text");
let scene = null;
let pages = [];
let generation = 0;
let pending = null;
let pageIndex = 0;

const csrfBytes = crypto.getRandomValues(new Uint8Array(16));
const csrf = [...csrfBytes].map((value) => value.toString(16).padStart(2, "0")).join("");

function message(text, isError = false) {
  status.textContent = text;
  status.className = isError ? "error" : "";
}

function busy(value) {
  fileButton.disabled = value || !fileInput.files?.length;
  fileInput.disabled = value;
  documentButton.disabled = value;
  $("#cancel-open").hidden = !value;
}

function clearReader() {
  scene = null;
  pages = [];
  pagesHost.replaceChildren();
  $("#reader").hidden = true;
  for (const id of ["#search-results", "#assets", "#limitations", "#story-select"]) $(id).replaceChildren();
  storyText.value = "";
  $("#search-query").value = "";
  $("#search-status").textContent = "";
  $("#copy-status").textContent = "";
}

function beginOpen() {
  pending?.controller.abort();
  const operation = { generation: ++generation, controller: new AbortController() };
  pending = operation;
  clearReader();
  busy(true);
  return operation;
}

function isCurrent(operation) {
  return operation.generation === generation && !operation.controller.signal.aborted;
}

async function jsonResponse(response) {
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error("reader_request_failed");
    error.status = response.status;
    // The UI intentionally does not display internal scanner/storage errors.
    throw error;
  }
  return payload;
}

function updatePageControls(index) {
  pageIndex = Math.max(0, Math.min(pages.length - 1, index));
  pageSelect.value = String(pageIndex);
  $("#previous-page").disabled = pageIndex === 0;
  $("#next-page").disabled = pageIndex >= pages.length - 1;
}

function goToPage(index) {
  if (!pages.length) return;
  updatePageControls(index);
  const element = pagesHost.children[pageIndex];
  viewer.scrollTop += element.getBoundingClientRect().top - viewer.getBoundingClientRect().top - 12;
}

function applyZoom() {
  if (!scene) return;
  const availableWidth = Math.max(120, viewer.clientWidth - (innerWidth < 600 ? 24 : 48));
  for (let index = 0; index < pages.length; index += 1) {
    const page = pages[index];
    const naturalWidth = page.width_emu / EMU_PER_CSS_PX;
    const scale = zoomSelect.value === "fit" ? Math.min(2, availableWidth / naturalWidth) : Number(zoomSelect.value);
    const svg = pagesHost.children[index];
    svg.setAttribute("width", naturalWidth * scale);
    svg.setAttribute("height", page.height_emu / EMU_PER_CSS_PX * scale);
  }
}

function showStory(index) {
  const story = scene?.stories?.[index];
  storyText.value = typeof story?.text === "string" ? story.text : "";
  storySelect.value = String(index);
  $("#copy-text").disabled = !storyText.value;
  $("#copy-status").textContent = "";
}

const reasonLabels = {
  stacking_order_unavailable: "The original stacking order is not fully supported.",
  node_kind_partial: "Some page objects do not yet have a supported visual representation.",
  image_resource_not_inline: "Some images are unavailable in this viewing session.",
  viewer_fidelity_warnings: "The document has display limitations described below.",
  shared_text_layout_unavailable: "Some text uses an approximate preview layout.",
  text_layout_partial: "Some text does not yet have fully supported page layout.",
  explicit_fallback_font_substitution: "Text uses a substitute font; its appearance can differ from Publisher."
};

function showDetails() {
  const stories = scene.stories ?? [];
  stories.forEach((story, index) => {
    const option = document.createElement("option");
    option.value = String(index);
    option.textContent = "Section " + (index + 1);
    storySelect.appendChild(option);
  });
  storySelect.disabled = !stories.length;
  if (stories.length) showStory(0);
  else { storyText.value = "No text was recovered for this document."; $("#copy-text").disabled = true; }

  const images = extractableImages(scene);
  $("#assets-summary").textContent = "Available images (" + images.length + ")";
  images.forEach((resource, index) => {
    const item = document.createElement("li");
    const preview = document.createElement("img");
    preview.src = resource.inline_data_url;
    preview.alt = "Recovered image " + (index + 1);
    preview.loading = "lazy";
    const link = document.createElement("a");
    const extension = resource.mime === "image/png" ? "png" : resource.mime === "image/gif" ? "gif" : "jpg";
    link.href = resource.inline_data_url;
    link.download = "chaptera-image-" + (index + 1) + "." + extension;
    link.textContent = "Save image " + (index + 1);
    item.append(preview, link);
    $("#assets").appendChild(item);
  });
  if (!images.length) {
    const item = document.createElement("li");
    item.textContent = "No extractable images are available in this session.";
    $("#assets").appendChild(item);
  }

  const limitations = new Set((scene.fidelity?.reasons ?? []).map((reason) => reasonLabels[reason] ?? String(reason).replaceAll("_", " ")));
  for (const diagnostic of scene.diagnostics ?? []) {
    if (typeof diagnostic.message === "string") limitations.add(diagnostic.message);
  }
  if (scene.nodes.some((node) => node.text && !node.text_layout)) {
    limitations.add("Some text is shown as an approximate preview; full recovered text is available in the text panel.");
  }
  if (scene.fonts?.length) limitations.add("A pinned substitute font is used. This is not a claim of Publisher-exact typography.");
  for (const text of limitations) {
    const item = document.createElement("li");
    item.textContent = text;
    $("#limitations").appendChild(item);
  }
  if (!limitations.size) {
    const item = document.createElement("li");
    item.textContent = "No limitations were reported for the evaluated scope.";
    $("#limitations").appendChild(item);
  }
  $("#diagnostics").textContent = "Display details (" + limitations.size + ")";
}

async function render(payload, operation) {
  const ordered = orderedPages(payload);
  if (!ordered.length) throw new Error("scene_protocol_mismatch");
  // Render off-screen. A slow font/resource activation from an older open
  // must never publish its result into the current reading session.
  const staging = document.createElement("div");
  await renderReaderScene(staging, payload, { emuPerCssPx: EMU_PER_CSS_PX, maxPageWidth: 920 });
  if (!isCurrent(operation)) return false;
  scene = payload;
  pages = ordered;
  pagesHost.replaceChildren(...staging.childNodes);
  pageSelect.replaceChildren();
  pages.forEach((page, index) => {
    const option = document.createElement("option");
    option.value = String(index);
    option.textContent = (index + 1) + " of " + pages.length;
    pageSelect.appendChild(option);
    pagesHost.children[index].setAttribute("aria-label", "Page " + (index + 1) + " of " + pages.length);
  });
  $("#fidelity").textContent = payload.fidelity?.state === "supported" ? "Opened" : "Partial display";
  $("#page-count").textContent = pages.length + (pages.length === 1 ? " page" : " pages");
  $("#revision").textContent = "Read-only document";
  $("#reader").hidden = false;
  showDetails();
  applyZoom();
  updatePageControls(0);
  viewer.scrollTop = 0;
  return true;
}

async function openFile(file) {
  if (!file) return;
  const operation = beginOpen();
  if (file.size <= 0) {
    pending = null;
    busy(false);
    message("The file is empty. Choose a Publisher (.PUB) document.", true);
    return;
  }
  const signal = operation.controller.signal;
  let accessToken = null;
  try {
    message("Creating private viewing session…");
    const issued = await jsonResponse(await fetch("/v1/reader/guest-sessions", {
      method: "POST", credentials: "omit", cache: "no-store", redirect: "error", signal,
      headers: { "content-type": "application/json", "x-csrf-token": csrf },
      body: JSON.stringify({ expected_byte_len: file.size })
    }));
    if (!isCurrent(operation)) return;
    if (issued.protocol_version !== "chaptera.reader-guest-session.v1"
        || typeof issued.session_id !== "string" || !/^[A-Za-z0-9_:-]+$/.test(issued.session_id)
        || typeof issued.access_token !== "string" || !issued.access_token) {
      throw new Error("guest_protocol_mismatch");
    }
    const uploadPath = guestRequestPath(issued.upload_path, "content", location.origin);
    const openPath = guestRequestPath(issued.open_path, "open", location.origin);
    const sessionPath = "/v1/reader/guest-sessions/" + issued.session_id + "/";
    if (uploadPath !== sessionPath + "content" || openPath !== sessionPath + "open") throw new Error("guest_path_invalid");
    accessToken = issued.access_token;
    const headers = { "x-csrf-token": csrf, "x-chaptera-reader-session": accessToken };
    message("Uploading for temporary private processing…");
    const uploaded = await jsonResponse(await fetch(uploadPath, {
      method: "PUT", credentials: "omit", cache: "no-store", redirect: "error", signal,
      headers: { ...headers, "content-type": "application/octet-stream" }, body: file
    }));
    if (!isCurrent(operation)) return;
    if (uploaded.protocol_version !== issued.protocol_version || uploaded.session_id !== issued.session_id) {
      throw new Error("guest_protocol_mismatch");
    }
    message("Scanning and opening…");
    const opened = await jsonResponse(await fetch(openPath, {
      method: "POST", credentials: "omit", cache: "no-store", redirect: "error", signal,
      headers: { ...headers, "content-type": "application/json" }, body: "{}"
    }));
    if (!isCurrent(operation)) return;
    if (opened.protocol_version !== issued.protocol_version || opened.session_id !== issued.session_id) {
      throw new Error("guest_protocol_mismatch");
    }
    if (["supported", "partial"].includes(opened.classification)) {
      if (!opened.scene) throw new Error("scene_protocol_mismatch");
      if (!await render(opened.scene, operation)) return;
    }
    if (isCurrent(operation)) message(classificationMessage(opened.classification), !["supported", "partial"].includes(opened.classification));
  } catch (error) {
    if (isCurrent(operation)) message(errorMessage(error), true);
  } finally {
    accessToken = null;
    if (operation.generation === generation) { pending = null; busy(false); }
  }
}

async function openDocument() {
  const documentId = documentInput.value.trim();
  if (!documentId) { message("Enter the saved document ID first.", true); return; }
  const operation = beginOpen();
  try {
    message("Opening saved document…");
    const payload = await jsonResponse(await fetch("/v1/reader/documents/" + encodeURIComponent(documentId) + "/scene", {
      credentials: "include", cache: "no-store", signal: operation.controller.signal
    }));
    if (await render(payload, operation)) message("Opened saved document read-only.");
  } catch (error) {
    if (isCurrent(operation)) message(errorMessage(error), true);
  } finally {
    if (operation.generation === generation) { pending = null; busy(false); }
  }
}

$("#cancel-open").addEventListener("click", () => {
  pending?.controller.abort();
  ++generation;
  pending = null;
  clearReader();
  busy(false);
  message("Opening cancelled. Choose a file to try again.");
});
fileInput.addEventListener("change", () => { fileButton.disabled = !fileInput.files?.length; });
fileButton.addEventListener("click", () => openFile(fileInput.files?.[0]));
documentButton.addEventListener("click", openDocument);
documentInput.addEventListener("keydown", (event) => { if (event.key === "Enter") openDocument(); });
for (const name of ["dragenter", "dragover", "dragleave", "drop"]) {
  $("#drop").addEventListener(name, (event) => {
    event.preventDefault();
    $("#drop").classList.toggle("drag", name === "dragenter" || name === "dragover");
    if (name === "drop") openFile(event.dataTransfer?.files?.[0]);
  });
}
pageSelect.addEventListener("change", () => goToPage(Number(pageSelect.value)));
$("#previous-page").addEventListener("click", () => goToPage(pageIndex - 1));
$("#next-page").addEventListener("click", () => goToPage(pageIndex + 1));
zoomSelect.addEventListener("change", () => { applyZoom(); goToPage(pageIndex); });
viewer.addEventListener("keydown", (event) => {
  if (["PageDown", "PageUp", "ArrowRight", "ArrowLeft"].includes(event.key)) {
    event.preventDefault();
    goToPage(pageIndex + (["PageDown", "ArrowRight"].includes(event.key) ? 1 : -1));
  }
});
viewer.addEventListener("scroll", () => {
  if (!pages.length) return;
  const top = viewer.getBoundingClientRect().top;
  let closest = 0;
  let distance = Infinity;
  [...pagesHost.children].forEach((element, index) => {
    const delta = Math.abs(element.getBoundingClientRect().top - top - 12);
    if (delta < distance) { closest = index; distance = delta; }
  });
  updatePageControls(closest);
});
new ResizeObserver(() => { if (zoomSelect.value === "fit") applyZoom(); }).observe(viewer);
storySelect.addEventListener("change", () => showStory(Number(storySelect.value)));
$("#copy-text").addEventListener("click", async () => {
  const currentGeneration = generation;
  const text = storyText.value;
  try {
    await navigator.clipboard.writeText(text);
    if (currentGeneration === generation) $("#copy-status").textContent = "Section text copied.";
  } catch {
    if (currentGeneration !== generation) return;
    storyText.focus();
    storyText.select();
    $("#copy-status").textContent = "Text selected. Press Ctrl+C or use your device's Copy action.";
  }
});
$("#search-form").addEventListener("submit", (event) => {
  event.preventDefault();
  if (!scene) return;
  const query = $("#search-query").value;
  const result = searchStories(scene, query);
  $("#search-results").replaceChildren();
  $("#search-status").textContent = !query ? "Enter text to search." : result.matches.length + (result.truncated ? "+ matches; showing the first 200." : " matches.");
  for (const match of result.matches) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.className = "secondary";
    button.type = "button";
    button.textContent = match.snippet;
    button.addEventListener("click", () => {
      const index = scene.stories.findIndex((story) => story.story_id === match.story_id);
      showStory(index);
      storyText.focus();
      storyText.setSelectionRange(match.utf16_start, match.utf16_end);
      $("#search-status").textContent = "Match selected in recovered text. Page placement may be unavailable.";
    });
    item.appendChild(button);
    $("#search-results").appendChild(item);
  }
});
document.addEventListener("keydown", (event) => {
  if (scene && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
    event.preventDefault();
    $("#text-panel").open = true;
    $("#search-query").focus();
  }
});
documentInput.value = new URLSearchParams(location.search).get("document_id") ?? "";
if (documentInput.value) openDocument();
