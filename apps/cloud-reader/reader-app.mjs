import { renderReaderScene } from "./render-v1.mjs";
import { BrowserObservabilityV1, traceHeadersV1 } from "./observability-v1.mjs";
import {
  EMU_PER_CSS_PX, orderedPages, searchStories, guestRequestPath, contributionEligible,
  extractableImages, assertSalvageObservation, classificationMessage, errorMessage
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
let contributionContext = null;
let contributionExpiry = null;

function browserFamily() {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("firefox/")) return "firefox";
  if (ua.includes("edg/") || ua.includes("chrome/") || ua.includes("chromium/")) return "chromium";
  if (ua.includes("safari/") && !ua.includes("chrome/")) return "webkit";
  return "other";
}

const observability = new BrowserObservabilityV1({
  sessionIncarnation: "reader-session:" + crypto.randomUUID(),
  browserFamily: browserFamily()
});

function clearContribution() {
  clearTimeout(contributionExpiry);
  contributionExpiry = null;
  contributionContext?.controller?.abort();
  if (contributionContext) contributionContext.accessToken = null;
  contributionContext = null;
  $("#contribution-panel").hidden = true;
  $("#contribution-filename").textContent = "";
  $("#contribution-status").textContent = "";
  $("#send-contribution").disabled = false;
  $("#cancel-contribution").disabled = false;
  if ($("#contribution-dialog").open) $("#contribution-dialog").close();
}

function offerContribution(opened, file, sessionId, accessToken) {
  if (opened.classification !== "unsupported"
      || !contributionEligible(opened.failure_classification)
      || !Number.isSafeInteger(opened.expires_at_ms)
      || opened.expires_at_ms <= Date.now()) return;
  contributionContext = {
    sessionId,
    accessToken,
    generation,
    expiresAt: opened.expires_at_ms,
    controller: null,
    retentionRequested: false,
    filename: String(file["name"] ?? "")
  };
  const context = contributionContext;
  contributionExpiry = setTimeout(() => {
    if (contributionContext !== context) return;
    clearContribution();
    message("The viewing session expired. Open the file again to contribute.", true);
  }, Math.min(2_147_483_647, context.expiresAt - Date.now()));
  $("#contribution-panel").hidden = false;
}

function currentContribution(context) {
  return contributionContext === context && context.generation === generation
    && !context.controller?.signal.aborted && context.expiresAt > Date.now();
}

function cancelContribution() {
  const retentionRequested = contributionContext?.retentionRequested;
  clearContribution();
  message(retentionRequested
    ? "Contribution request cancelled. The file may already have been received."
    : "Contribution cancelled. This file will continue to expire automatically.");
}

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
  clearContribution();
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

function beginOpen(operationClass = "open") {
  pending?.controller.abort();
  const operation = {
    generation: ++generation,
    controller: new AbortController(),
    traceContext: observability.createContext({ operationClass })
  };
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
    const issued = await observability.measure("reader.session_create", operation.traceContext, async () =>
      jsonResponse(await fetch("/v1/reader/guest-sessions", {
        method: "POST", credentials: "omit", cache: "no-store", redirect: "error", signal,
        headers: {
          "content-type": "application/json",
          "x-csrf-token": csrf,
          ...traceHeadersV1(operation.traceContext)
        },
        body: JSON.stringify({ expected_byte_len: file.size })
      }))
    );
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
    const headers = { "x-csrf-token": csrf, "x-chaptera-reader-session": accessToken, ...traceHeadersV1(operation.traceContext) };
    message("Uploading for temporary private processing…");
    const uploaded = await observability.measure("reader.upload", operation.traceContext, async () =>
      jsonResponse(await fetch(uploadPath, {
        method: "PUT", credentials: "omit", cache: "no-store", redirect: "error", signal,
        headers: { ...headers, "content-type": "application/octet-stream" }, body: file
      }))
    );
    if (!isCurrent(operation)) return;
    if (uploaded.protocol_version !== issued.protocol_version || uploaded.session_id !== issued.session_id) {
      throw new Error("guest_protocol_mismatch");
    }
    message("Scanning and opening…");
    const opened = await observability.measure("reader.open", operation.traceContext, async () =>
      jsonResponse(await fetch(openPath, {
        method: "POST", credentials: "omit", cache: "no-store", redirect: "error", signal,
        headers: { ...headers, "content-type": "application/json" }, body: "{}"
      }))
    );
    if (!isCurrent(operation)) return;
    if (opened.protocol_version !== issued.protocol_version || opened.session_id !== issued.session_id) {
      throw new Error("guest_protocol_mismatch");
    }
    if (["supported", "partial"].includes(opened.classification)) {
      if (!opened.scene || opened.salvage !== undefined) throw new Error("scene_protocol_mismatch");
      if (!await observability.measure("reader.render", operation.traceContext, () => render(opened.scene, operation))) return;
    } else if (opened.classification === "salvage") {
      if (opened.scene !== undefined || !opened.salvage) throw new Error("salvage_protocol_mismatch");
      const salvage = assertSalvageObservation(opened.salvage);
      if (salvage.source_sha256 !== opened.source_sha256) throw new Error("salvage_protocol_mismatch");
    }
    if (isCurrent(operation)) {
      message(
        classificationMessage(opened.classification, opened.failure_classification?.class ?? null),
        !["supported", "partial", "salvage"].includes(opened.classification)
      );
      offerContribution(opened, file, issued.session_id, accessToken);
    }
  } catch (error) {
    if (isCurrent(operation)) message(errorMessage(error), true);
  } finally {
    accessToken = null;
    if (operation.generation === generation) { pending = null; busy(false); }
  }
}

async function contributeCurrentFile() {
  const context = contributionContext;
  if (!context || !currentContribution(context) || context.controller) return;
  context.controller = new AbortController();
  const signal = context.controller.signal;
  const send = $("#send-contribution");
  send.disabled = true;
  $("#contribution-status").textContent = "Preparing a private one-time contribution…";
  let contributionToken = null;
  try {
    const base = "/v1/reader/guest-sessions/" + context.sessionId + "/";
    const capabilityPath = guestRequestPath(
      base + "contribution-capability",
      "contribution-capability",
      location.origin
    );
    const contributePath = guestRequestPath(base + "contribute", "contribute", location.origin);
    const sessionHeaders = {
      "x-csrf-token": csrf,
      "x-chaptera-reader-session": context.accessToken
    };
    const capability = await jsonResponse(await fetch(capabilityPath, {
      method: "POST",
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
      signal,
      headers: { ...sessionHeaders, "content-type": "application/json" },
      body: JSON.stringify({
        protocol_version: "chaptera.intake-capability-request.v1",
        consent_version: "chaptera-intake-consent-v1"
      })
    }));
    if (!currentContribution(context)) return;
    if (capability.protocol_version !== "chaptera.reader-contribution-capability.v1"
        || typeof capability.submission_id !== "string"
        || !/^[A-Za-z0-9_:-]{16,160}$/.test(capability.submission_id)
        || typeof capability.capability_token !== "string"
        || !/^[0-9a-f]{64}$/.test(capability.capability_token)
        || !Number.isSafeInteger(capability.expires_at_ms)
        || capability.expires_at_ms <= Date.now()
        || capability.expires_at_ms > context.expiresAt
        || capability.retention_policy !== "chaptera-intake-retention-v1") {
      throw new Error("guest_protocol_mismatch");
    }
    contributionToken = capability.capability_token;
    $("#contribution-status").textContent = "Sending the exact file for compatibility research…";
    context.retentionRequested = true;
    const receipt = await jsonResponse(await fetch(contributePath, {
      method: "POST",
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
      signal,
      headers: {
        ...sessionHeaders,
        "x-chaptera-reader-contribution": contributionToken
      }
    }));
    contributionToken = null;
    if (!currentContribution(context)) return;
    if (receipt.protocol_version !== "chaptera.intake-receipt.v1"
        || receipt.submission_id !== capability.submission_id
        || typeof receipt.server_sha256 !== "string"
        || !/^[0-9a-f]{64}$/.test(receipt.server_sha256)
        || !["new_exact_bytes", "duplicate_exact_bytes"].includes(receipt.exact_byte_disposition)
        || receipt.cluster_disposition !== "deferred"
        || receipt.retention_policy !== "chaptera-intake-retention-v1") {
      throw new Error("guest_protocol_mismatch");
    }
    clearContribution();
    message("Contribution received for compatibility research. Your original file is unchanged.");
  } catch (error) {
    contributionToken = null;
    if (!currentContribution(context)) return;
    $("#contribution-status").textContent =
      context.retentionRequested
        ? "We could not confirm receipt. The file may already have been received; retry while this session is active."
        : "The contribution could not start. Retry while this viewing session is active.";
    send.disabled = false;
  } finally {
    contributionToken = null;
    if (contributionContext === context) context.controller = null;
  }
}

async function openDocument() {
  const documentId = documentInput.value.trim();
  if (!documentId) { message("Enter the saved document ID first.", true); return; }
  const operation = beginOpen("scene_read");
  try {
    message("Opening saved document…");
    const payload = await observability.measure("reader.scene_read", operation.traceContext, async () =>
      jsonResponse(await fetch("/v1/reader/documents/" + encodeURIComponent(documentId) + "/scene", {
        credentials: "include",
        cache: "no-store",
        signal: operation.controller.signal,
        headers: traceHeadersV1(operation.traceContext)
      }))
    );
    if (await render(payload, operation)) message("Opened saved document read-only.");
  } catch (error) {
    if (isCurrent(operation)) message(errorMessage(error), true);
  } finally {
    if (operation.generation === generation) { pending = null; busy(false); }
  }
}

$("#open-contribution").addEventListener("click", () => {
  if (!contributionContext || !currentContribution(contributionContext)) {
    clearContribution();
    return;
  }
  $("#contribution-filename").textContent = contributionContext.filename;
  $("#contribution-status").textContent = "";
  $("#contribution-dialog").showModal();
});
$("#cancel-contribution").addEventListener("click", cancelContribution);
$("#send-contribution").addEventListener("click", contributeCurrentFile);
$("#contribution-dialog").addEventListener("cancel", cancelContribution);

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
