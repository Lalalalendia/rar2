import {
  TemporaryGuestMoveClientV1, temporaryPlainStoryCandidates,
  temporaryStoryTextForTextarea, temporaryStoryObservedToCanonicalStory,
} from "./temporary-edit-move-client-v1.mjs";
import { MoveGestureV1, hitTestSnapshot } from "./interaction-v1.mjs";
import {
  RESIZE_HANDLES, ResizeTransactionV1, resizeHandleCenters,
} from "../../packages/protocol/editor-resize/v1/resize-gesture-v1.mjs";
import { projectReaderSceneToEditorInteractionScene } from "./reader-scene-editor-interaction-v1.mjs";
import { TemporaryEditBrowserSessionV0 } from "./temporary-edit-browser-session-v0.mjs";
import { renderReaderScene } from "../cloud-reader/render-v1.mjs";

const PREVIEW_STATES = new Set(["supported", "partial", "salvage", "unsupported", "rejected"]);
const SAFE_ERROR_CODES = new Set([
  "temporary_file_invalid", "temporary_operation_pending", "temporary_request_failed",
  "temporary_issue_contract_invalid", "temporary_response_invalid",
  "temporary_response_identity_mismatch", "temporary_source_changed",
  "guest_session_expired", "guest_session_not_found", "guest_upload_too_large",
  "guest_scene_not_ready", "guest_open_in_progress", "guest_upload_state_conflict",
  "guest_source_scan_failed", "guest_scene_worker_failed", "temporary_scene_invalid"
]);

export function describeTemporaryPreview(view) {
  if (view?.kind === "missing") {
    return { message: "В этой вкладке нет сохранённой временной сессии.", renderable: false };
  }
  if (view?.kind !== "temporary_preview" || !PREVIEW_STATES.has(view.classification)) {
    throw new TypeError("temporary preview state is invalid");
  }
  // This is deliberately a read-only readiness bridge, not an editor promise.
  if (view.can_edit !== false || view.can_download_changed_file !== false) {
    throw new Error("temporary preview must not claim edit/export support");
  }
  switch (view.classification) {
    case "supported":
      return { message: "PUB открыт для временного просмотра. Редактирование пока недоступно.", renderable: true };
    case "partial":
      return { message: "PUB открыт частично: отдельные объекты могут отображаться неточно. Редактирование пока недоступно.", renderable: true };
    case "salvage":
      return { message: "Доступны только результаты восстановления. Редактирование и просмотр страниц пока не поддерживаются.", renderable: false };
    case "unsupported":
      return { message: "Этот PUB пока не поддерживается. Оригинальный файл не изменён.", renderable: false };
    default:
      return { message: "PUB отклонён проверкой безопасности или формата. Оригинальный файл не изменён.", renderable: false };
  }
}

export function temporaryExpiryText(expiresAtMs) {
  if (!Number.isSafeInteger(expiresAtMs) || expiresAtMs <= 0) return "";
  const localTime = new Date(expiresAtMs).toLocaleString("ru-RU");
  return "Серверный срок действия сессии (местное отображение): " + localTime +
    ". Обновление страницы не продлевает срок.";
}

function defaultStorage() {
  try { return globalThis.sessionStorage ?? null; } catch { return null; }
}

function newCsrfToken(cryptoObject) {
  if (!cryptoObject || typeof cryptoObject.getRandomValues !== "function") {
    throw new Error("secure browser randomness is unavailable");
  }
  const bytes = cryptoObject.getRandomValues(new Uint8Array(16));
  return [...bytes].map((value) => value.toString(16).padStart(2, "0")).join("");
}

// The existing public Reader serves /observability-v1.mjs. Local Node
// tests resolve the exact same contract from the working-tree module.
// A missing trace asset never prevents an anonymous upload or edit.
async function optionalTemporaryGuestTracing(cryptoObject) {
  try {
    const modulePath = import.meta.url.startsWith("file:")
      ? "./observability-v1.mjs" : "/observability-v1.mjs";
    const { BrowserObservabilityV1, traceHeadersV1 } = await import(modulePath);
    return {
      recorder: new BrowserObservabilityV1({
        sessionIncarnation: "tab:" + newCsrfToken(cryptoObject),
        browserFamily: "unknown",
        maxSpans: 256,
        idFactory: prefix => prefix + ":" + newCsrfToken(cryptoObject),
      }),
      headersFor: traceHeadersV1,
      interactionId: "interaction:" + newCsrfToken(cryptoObject),
    };
  } catch {
    return null;
  }
}

const SVG_NS = "http://www.w3.org/2000/svg";
const TEMPORARY_NUDGE_STEP_EMU = 12_700; // One point, not screen pixels.

// Camera-only scaling. Page viewBox, Scene identity, NodeIds and EMU are immutable.
export const TEMP_ZOOM_MIN = 50;
export const TEMP_ZOOM_MAX = 200;
export const TEMP_ZOOM_STEP = 25;
const TEMP_EMU_PER_PX = 9525;
export function temporaryViewportPages(stage) {
  return Array.from(stage?.querySelectorAll?.("svg.page[data-page-id]") ?? []);
}
export function temporaryViewportZoom(zoom) {
  if (zoom?.mode === "fit") return {mode:"fit",percent:100};
  if (zoom?.mode !== "manual" || !Number.isInteger(zoom?.percent) ||
      zoom.percent < TEMP_ZOOM_MIN || zoom.percent > TEMP_ZOOM_MAX) {
    throw new RangeError("temporary_viewport_zoom_invalid");
  }
  return {mode:"manual",percent:zoom.percent};
}
export function applyTemporaryViewport(stage,zoom={mode:"fit"}) {
  const view=temporaryViewportZoom(zoom);
  const svgs=temporaryViewportPages(stage);
  const stageWidth=Number(stage?.clientWidth);
  const available=Number.isFinite(stageWidth) && stageWidth>0 ? Math.max(1,stageWidth-64) : 920;
  const fitWidth=Math.min(920,available);
  let applied=0;
  for (const svg of svgs) {
    const box=svg.getAttribute?.("viewBox")?.trim().split(/[\s,]+/).map(Number);
    if (!box || box.length!==4 || !box.every(Number.isSafeInteger) ||
        box[0]!==0 || box[1]!==0 || box[2]<=0 || box[3]<=0) continue;
    const naturalWidth=box[2]/TEMP_EMU_PER_PX;
    const width=view.mode==="fit" ? Math.min(naturalWidth,fitWidth) : naturalWidth*view.percent/100;
    const height=width*box[3]/box[2];
    if (!(width>0) || !(height>0) || !Number.isFinite(width) ||
        !Number.isFinite(height) || width>1e6 || height>1e6) continue;
    svg.setAttribute("width",String(width));
    svg.setAttribute("height",String(height));
    if (svg.style) {
      svg.style.width=String(width)+"px";
      svg.style.height=String(height)+"px";
      svg.style.maxWidth="none";
    }
    applied++;
  }
  return {total:svgs.length,applied,view};
}



export function temporaryEditorPagePoint(pageSvg, clientX, clientY) {
  if (typeof pageSvg?.createSVGPoint !== "function" ||
      typeof pageSvg?.getScreenCTM !== "function") {
    throw new Error("temporary_editor_page_transform_unavailable");
  }
  const matrix = pageSvg.getScreenCTM();
  if (!matrix || typeof matrix.inverse !== "function") {
    throw new Error("temporary_editor_page_transform_unavailable");
  }
  const p = pageSvg.createSVGPoint();
  p.x = clientX;
  p.y = clientY;
  const point = p.matrixTransform(matrix.inverse());
  const round = value => Math.sign(value) * Math.floor(Math.abs(value) + 0.5);
  const x_emu = round(point.x);
  const y_emu = round(point.y);
  if (!Number.isSafeInteger(x_emu) || !Number.isSafeInteger(y_emu)) {
    throw new Error("temporary_editor_page_point_invalid");
  }
  return { x_emu, y_emu };
}

// Pointer geometry is strictly a transient user request. The canonical
// Server ReaderScene decides whether the resulting rectangle exists.
export function temporaryRectangleDragBounds(start, end, page) {
  const values = [start?.x_emu, start?.y_emu, end?.x_emu, end?.y_emu,
    page?.width_emu, page?.height_emu];
  if (!values.every(Number.isSafeInteger) || page.width_emu <= 0 || page.height_emu <= 0) return null;
  const x = Math.min(start.x_emu, end.x_emu);
  const y = Math.min(start.y_emu, end.y_emu);
  const width = Math.abs(start.x_emu - end.x_emu);
  const height = Math.abs(start.y_emu - end.y_emu);
  if (x < 0 || y < 0 || width <= 0 || height <= 0 ||
      !Number.isSafeInteger(width) || !Number.isSafeInteger(height) ||
      x + width > page.width_emu || y + height > page.height_emu) return null;
  return {x, y, width, height};
}

// Canvas is only a selection surface; ownership and edit eligibility come from
// the authoritative authoring graph and the existing plain-Story gate.
export function temporaryCanvasStoryId(graph, candidates, nodeId, pageId) {
  if (!graph?.nodes || !graph?.pages ||
      !Object.prototype.hasOwnProperty.call(graph.pages, pageId)) return null;
  const node = graph.nodes[nodeId];
  if (!node || node.header?.parent_id !== pageId) return null;
  const storyId = node.payload?.story_frame?.story_id;
  if (typeof storyId !== "string" || !Array.isArray(candidates)) return null;
  return candidates.some(candidate => candidate.story_id === storyId) ? storyId : null;
}

export async function bootTemporaryEditorPreview({
  documentObject = globalThis.document,
  cryptoObject = globalThis.crypto,
  storage = defaultStorage(),
  fetchImpl = globalThis.fetch,
  renderer = renderReaderScene,
  confirmDelete = globalThis.confirm,
} = {}) {
  const ids = [
    "temporary-preview-root", "temporary-upload-form", "temporary-file",
    "temporary-start", "temporary-resume", "temporary-forget",
    "temporary-status", "temporary-expiry", "temporary-pages",
  ];
  const elements = Object.fromEntries(ids.map(id => [id, documentObject?.getElementById(id)]));
  if (ids.some(id => !elements[id])) throw new Error("temporary preview HTML contract incomplete");

  const form = elements["temporary-upload-form"];
  const input = elements["temporary-file"];
  const startButton = elements["temporary-start"];
  const resumeButton = elements["temporary-resume"];
  const forgetButton = elements["temporary-forget"];
  const deleteButton = documentObject?.getElementById("temporary-delete");
  const status = elements["temporary-status"];
  const expiry = elements["temporary-expiry"];
  const pages = elements["temporary-pages"];
  // An older read-only embedded template stays read-only; the editor is
  // enabled only when the dedicated download/action panel is present.
  const actions = documentObject?.getElementById("temporary-export-actions");
  const idmlButton = documentObject?.getElementById("temporary-download-idml");
  const drawButton = documentObject?.getElementById("temporary-draw-rectangle");
  const odgButton = documentObject?.getElementById("temporary-download-odg");
  const pdfButton = documentObject?.getElementById("temporary-download-pdf");
  const pdf9upButton = documentObject?.getElementById("temporary-download-pdf9up");
  const sheetPreviewButton = documentObject?.getElementById("temporary-preview-pdf9up");
  const sheetPreview = documentObject?.getElementById("temporary-sheet-preview");
  const sheetFrame = documentObject?.getElementById("temporary-sheet-pdf-frame");
  const sheetPreviewEnabled = !!(sheetPreviewButton && sheetPreview && sheetFrame);
  const editorEnabled = !!(actions && idmlButton && odgButton);
  const drawEnabled = editorEnabled && !!drawButton;
  const viewButtons = {
    prev: documentObject?.getElementById("temporary-page-prev"),
    next: documentObject?.getElementById("temporary-page-next"),
    position: documentObject?.getElementById("temporary-page-position"),
    out: documentObject?.getElementById("temporary-zoom-out"),
    fit: documentObject?.getElementById("temporary-zoom-fit"),
    actual: documentObject?.getElementById("temporary-zoom-actual"),
    in: documentObject?.getElementById("temporary-zoom-in"),
    percent: documentObject?.getElementById("temporary-zoom-percent"),
  };
  const storyPanel = documentObject?.getElementById("temporary-story-panel");
  const storySelect = documentObject?.getElementById("temporary-story-select");
  const storyText = documentObject?.getElementById("temporary-story-text");
  const storyApply = documentObject?.getElementById("temporary-story-apply");
  const storyDiscard = documentObject?.getElementById("temporary-story-discard");
  const storyEnabled = !!(editorEnabled && storyPanel && storySelect && storyText && storyApply);
  const csrfToken = newCsrfToken(cryptoObject);
  const trace = await optionalTemporaryGuestTracing(cryptoObject);
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, storage, fetchImpl, trace,
  });

  let busy = false;
  let editBusy = false;
  let moveClient = null;
  let canonicalCurrent = null;
  let interaction = null;
  let partialFidelity = false;
  let storyCandidates = [];
  let composingStory = false;
  let activeStoryId = null;
  let gesture = null;
  let gesturePage = null;
  let gestureOverlay = null;
  let rectangleToolActive = false;
  let rectangleDraft = null;
  let selectedResizeNodeId = null;
  let selectedResizePageId = null;
  let resizeHandleElements = [];
  let sheetPreviewUrl = null;
  let viewportZoom = {mode:"fit",percent:100};
  let activeViewportPageIndex = 0;
  const buttons = [startButton,resumeButton,forgetButton,deleteButton].filter(Boolean);

  function refreshViewportControls() {
    const count=temporaryViewportPages(pages).length;
    activeViewportPageIndex=Math.min(activeViewportPageIndex,Math.max(0,count-1));
    const available=count>0 && !busy && !editBusy;
    if (viewButtons.prev) viewButtons.prev.disabled=!available || activeViewportPageIndex===0;
    if (viewButtons.next) viewButtons.next.disabled=!available || activeViewportPageIndex+1>=count;
    if (viewButtons.position) viewButtons.position.textContent=
      count ? (activeViewportPageIndex+1)+" / "+count : "0 / 0";
    if (viewButtons.fit) {
      viewButtons.fit.disabled=!available;
      viewButtons.fit.setAttribute?.("aria-pressed",String(viewportZoom.mode==="fit"));
    }
    if (viewButtons.actual) viewButtons.actual.disabled=!available;
    if (viewButtons.out) viewButtons.out.disabled=!available ||
      (viewportZoom.mode==="manual" && viewportZoom.percent<=TEMP_ZOOM_MIN);
    if (viewButtons.in) viewButtons.in.disabled=!available ||
      (viewportZoom.mode==="manual" && viewportZoom.percent>=TEMP_ZOOM_MAX);
    if (viewButtons.percent) viewButtons.percent.textContent=
      viewportZoom.mode==="fit" ? "По ширине" : viewportZoom.percent+"%";
  }
  function resetViewport() {
    viewportZoom={mode:"fit",percent:100};
    activeViewportPageIndex=0;
    pages.scrollTop=0;
    pages.scrollLeft=0;
    refreshViewportControls();
  }
  async function renderCanvas(scene) {
    const top=pages.scrollTop, left=pages.scrollLeft;
    await renderer(pages,scene,{
      maxPageWidth:Math.max(320,Math.min(920,pages.clientWidth||920)),
    });
    applyTemporaryViewport(pages,viewportZoom);
    if (Number.isFinite(top)) pages.scrollTop=top;
    if (Number.isFinite(left)) pages.scrollLeft=left;
    refreshViewportControls();
  }
  function changeViewportZoom(zoom) {
    if (busy || editBusy || !temporaryViewportPages(pages).length) return;
    viewportZoom=temporaryViewportZoom(zoom);
    applyTemporaryViewport(pages,viewportZoom);
    redrawResizeHandles();
    refreshViewportControls();
  }
  function navigateViewport(delta) {
    if (busy || editBusy) return;
    const svgs=temporaryViewportPages(pages);
    if (!svgs.length) return;
    activeViewportPageIndex=Math.max(0,Math.min(activeViewportPageIndex+delta,svgs.length-1));
    svgs[activeViewportPageIndex].scrollIntoView?.({block:"start",inline:"nearest",behavior:"auto"});
    refreshViewportControls();
  }


  function controls() {
    for (const button of buttons) button.disabled = busy || editBusy;
    resumeButton.hidden = !client.hasSavedSession();
    forgetButton.hidden = !client.hasSavedSession();
    if (deleteButton) deleteButton.hidden = !client.hasSavedSession();
    if (editorEnabled) {
      actions.hidden = !moveClient;
      const canExport = !busy && !editBusy && moveClient &&
        !moveClient.status.pending_reconciliation &&
        !moveClient.status.refresh_required && canonicalCurrent !== null &&
        !storyDraftDirty() && !composingStory;
      const canDownloadEdited = canExport && canonicalCurrent.revision_cursor > 0;
      if (drawEnabled) {
        drawButton.disabled = !canExport || !interaction?.pages?.length;
        drawButton.setAttribute?.("aria-pressed", String(rectangleToolActive));
        pages.setAttribute?.("data-rectangle-tool", String(rectangleToolActive));
      }
      idmlButton.disabled = !canDownloadEdited;
      odgButton.disabled = !canDownloadEdited;
      if (pdfButton) {
        pdfButton.disabled = !canExport;
        pdfButton.textContent = canDownloadEdited ? "Скачать изменённый PDF" : "Скачать PDF";
      }
      if (pdf9upButton || sheetPreviewEnabled) {
        const validCard = interaction?.pages?.length === 1 &&
          interaction.pages[0]?.width_emu === 3060000 &&
          interaction.pages[0]?.height_emu === 1980000;
        if (pdf9upButton) {
          pdf9upButton.hidden = !validCard;
          pdf9upButton.disabled = !canDownloadEdited || !validCard;
        }
        if (sheetPreviewEnabled) {
          sheetPreviewButton.hidden = !validCard;
          sheetPreviewButton.disabled = !canDownloadEdited || !validCard;
        }
      }
    }
    if (storyEnabled) {
      const locked = busy || editBusy || !moveClient ||
        moveClient.status.pending_reconciliation || moveClient.status.refresh_required;
      storyPanel.hidden = !moveClient || storyCandidates.length === 0;
      storySelect.disabled = locked;
      storyText.disabled = locked;
      storyApply.disabled = locked || composingStory || storyCandidates.length === 0;
      if (storyDiscard) storyDiscard.disabled = locked || composingStory || !storyDraftDirty();
    }
    refreshViewportControls();
  }

  function clearSheetPreview() {
    if (!sheetPreviewEnabled) return;
    sheetFrame.src = "";
    sheetPreview.hidden = true;
    if (sheetPreviewUrl !== null) {
      URL.revokeObjectURL(sheetPreviewUrl);
      sheetPreviewUrl = null;
    }
  }

  function storyDraftDirty() {
    if (!storyEnabled || !activeStoryId) return false;
    const current = storyCandidates.find(entry => entry.story_id === activeStoryId);
    return !!current && storyText.value !== temporaryStoryTextForTextarea(current.text);
  }

  function activateStory(storyId) {
    if (!storyEnabled) return false;
    const selected = storyCandidates.find(entry => entry.story_id === storyId);
    if (!selected) {
      storySelect.value = activeStoryId ?? "";
      return false;
    }
    if (composingStory || (storyDraftDirty() && storyId !== activeStoryId)) {
      status.textContent = "Сначала сохраните или сбросьте несохранённый текст перед выбором другой рамки.";
      storySelect.value = activeStoryId ?? "";
      return false;
    }
    // Clicking the current Story must not wipe an unsaved in-progress draft.
    if (storyId !== activeStoryId) {
      storySelect.value = storyId;
      storyText.value = temporaryStoryTextForTextarea(selected.text);
      activeStoryId = storyId;
    }
    controls();
    return true;
  }

  function refreshStoryEditor(preferredStoryId = null) {
    if (!storyEnabled) return;
    const previous = preferredStoryId ?? storySelect.value;
    storyCandidates = moveClient
      ? temporaryPlainStoryCandidates(canonicalCurrent?.authoring_graph) : [];
    storySelect.replaceChildren();
    for (const [index, entry] of storyCandidates.entries()) {
      const option = documentObject.createElement("option");
      option.value = entry.story_id;
      option.textContent = "Рамка " + (index + 1) + ": " + entry.text.slice(0, 55);
      storySelect.appendChild(option);
    }
    const selected = storyCandidates.find(entry => entry.story_id === previous) ??
      storyCandidates[0];
    storySelect.value = selected?.story_id ?? "";
    storyText.value = selected ? temporaryStoryTextForTextarea(selected.text) : "";
    activeStoryId = selected?.story_id ?? null;
    storyPanel.hidden = !moveClient || !selected;
  }

  function clearResizeHandles({ forget = false } = {}) {
    for (const element of resizeHandleElements) {
      try { element.remove(); } catch {}
    }
    resizeHandleElements = [];
    if (forget) {
      selectedResizeNodeId = null;
      selectedResizePageId = null;
    }
  }

  function resizeCandidate(nodeId) {
    const graph = canonicalCurrent?.authoring_graph;
    const sourceNode = graph?.nodes?.[nodeId];
    const bounds = sourceNode?.header?.bounds;
    const target = interaction?.nodes?.find(node => node.node_id === nodeId);
    if (!sourceNode || !target || !bounds ||
        !["x", "y", "width", "height"].every(key => Number.isSafeInteger(bounds[key])) ||
        bounds.width <= 0 || bounds.height <= 0 ||
        !Number.isSafeInteger(bounds.x + bounds.width) ||
        !Number.isSafeInteger(bounds.y + bounds.height)) return null;
    if (graph?.pages && !Object.keys(graph.pages).some(key =>
      key === sourceNode.header.parent_id)) return null;
    const transform = sourceNode.header.transform;
    if (transform && (String(transform.a) !== "1" ||
        String(transform.b) !== "0" || String(transform.c) !== "0" ||
        String(transform.d) !== "1" || transform.tx !== 0 || transform.ty !== 0)) {
      return null;
    }
    return { nodeId, bounds };
  }

  function drawResizeHandles(svg, nodeId) {
    clearResizeHandles();
    const candidate = resizeCandidate(nodeId);
    if (!svg || !candidate || !documentObject?.createElementNS) return;
    const bounds = candidate.bounds;
    const viewBox = svg.getAttribute?.("viewBox")?.trim().split(/\s+/).map(Number);
    const pageWidth = viewBox?.length === 4 && viewBox.every(Number.isFinite)
      ? viewBox[2] : bounds.width * 6;
    const handleSize = Math.max(1, Math.round(Math.min(
      Math.max(10, Math.min(bounds.width, bounds.height) / 4),
      pageWidth / Math.max(svg.clientWidth || 700, 1) * 10,
    )));
    const centers = resizeHandleCenters(bounds);
    for (const handle of RESIZE_HANDLES) {
      const center = centers[handle];
      const rect = documentObject.createElementNS(SVG_NS, "rect");
      rect.setAttribute("x", String(center.x - handleSize / 2));
      rect.setAttribute("y", String(center.y - handleSize / 2));
      rect.setAttribute("width", String(handleSize));
      rect.setAttribute("height", String(handleSize));
      rect.setAttribute("class", "temporary-resize-handle");
      rect.setAttribute("data-temporary-resize-handle", handle);
      rect.setAttribute("data-node-id", nodeId);
      rect.setAttribute("role", "button");
      rect.setAttribute("tabindex", "0");
      rect.setAttribute("aria-label", "Изменить размер: " + handle);
      svg.appendChild(rect);
      resizeHandleElements.push(rect);
    }
    selectedResizeNodeId = nodeId;
    selectedResizePageId = svg.getAttribute("data-page-id");
  }

  function redrawResizeHandles() {
    if (!selectedResizeNodeId || !selectedResizePageId) return;
    const svg = pages.querySelector?.("svg.page[data-page-id='" + selectedResizePageId + "']");
    if (svg) drawResizeHandles(svg, selectedResizeNodeId);
    else clearResizeHandles({forget: true});
  }

  function clearGesture() {
    try { gestureOverlay?.remove?.(); } catch {}
    gestureOverlay = null;
    if (gesture && gesture.state === "active") gesture.cancel();
    gesture = null;
    gesturePage = null;
    rectangleDraft = null;
  }

  function clearEditor() {
    clearSheetPreview();
    clearGesture();
    clearResizeHandles({forget: true});
    rectangleToolActive = false;
    pages.setAttribute?.("data-rectangle-tool", "false");
    drawButton?.setAttribute?.("aria-pressed", "false");
    moveClient = null;
    canonicalCurrent = null;
    interaction = null;
    storyCandidates = [];
    composingStory = false;
    activeStoryId = null;
    refreshStoryEditor();
  }

  function overlayBounds(bounds) {
    if (!gesturePage || !bounds) return;
    if (!gestureOverlay) {
      gestureOverlay = documentObject.createElementNS(SVG_NS, "rect");
      gestureOverlay.setAttribute("pointer-events", "none");
      gesturePage.appendChild(gestureOverlay);
    }
    gestureOverlay.setAttribute("class", rectangleDraft
      ? "temporary-rectangle-preview" : "temporary-move-preview");
    for (const key of ["x", "y", "width", "height"]) {
      gestureOverlay.setAttribute(key, String(bounds[key]));
    }
  }

  async function show(view) {
    const description = describeTemporaryPreview(view);
    clearEditor();
    partialFidelity = view.classification === "partial";
    pages.replaceChildren();
    expiry.textContent = temporaryExpiryText(view.expires_at_ms);

    let effectiveScene = view.scene;
    if (description.renderable && ["supported", "partial"].includes(view.classification) && editorEnabled) {
      try {
        moveClient = TemporaryGuestMoveClientV1.fromTab({
          storage, csrfToken, fetchImpl, cryptoObject, trace,
        });
        canonicalCurrent = await moveClient.refresh();
        effectiveScene = await moveClient.readerScene();
        interaction = projectReaderSceneToEditorInteractionScene(effectiveScene);
      } catch {
        clearEditor();
        // Original stored scene is never shown as if it were the current
        // edited revision. Explicit recovery may retry after a 409/429.
        status.textContent = "Актуальная ревизия редактора недоступна. Восстановите временную сессию для повторной попытки.";
        controls();
        return;
      }
    }
    if (description.renderable) {
      try {
        await renderCanvas(effectiveScene);
      } catch {
        pages.replaceChildren();
        clearEditor();
        status.textContent = "Не удалось безопасно отобразить страницы. Оригинал PUB сохранён без изменений.";
        controls();
        return;
      }
    }
    refreshStoryEditor();
    redrawResizeHandles();
    if (moveClient) {
      expiry.textContent = temporaryExpiryText(canonicalCurrent.expires_at_ms);
      status.textContent = partialFidelity
        ? "Открыта редактируемая ревизия — Needs review / Partial: отображение неполное. Перемещайте только поддерживаемые объекты, проверяйте LossReport перед использованием выгрузки."
        : "Открыта редактируемая ревизия. PUB открыт: PDF можно скачать без правки. Для изменения переместите поддерживаемый объект; результат подтверждает сервер.";
    } else {
      status.textContent = description.message;
    }
    controls();
  }

  async function guarded(operation) {
    if (busy || editBusy) return;
    if (storyDraftDirty() || composingStory) {
      status.textContent = "Сначала сохраните или сбросьте несохранённый текст перед открытием другой сессии.";
      return;
    }
    busy = true;
    clearSheetPreview();
    controls();
    status.textContent = "Обработка временной сессии…";
    try {
      const view = await operation();
      await show(view);
    } catch (error) {
      pages.replaceChildren();
      clearEditor();
      expiry.textContent = "";
      const code = SAFE_ERROR_CODES.has(error?.code)
        ? error.code : "temporary_request_failed";
      status.textContent = "Временная сессия не открыта (" + code + "). Исходный PUB не изменён.";
    } finally {
      busy = false;
      controls();
    }
  }

  async function start(file) {
    if (!file || !/\.pub$/i.test(file.name ?? "") ||
        !Number.isSafeInteger(file.size) || file.size <= 0) {
      status.textContent = "Выберите непустой файл с расширением .pub.";
      return;
    }
    resetViewport();
    await guarded(() => client.start(file));
  }

  async function resume() {
    if (moveClient?.status.pending_reconciliation) {
      if (busy || editBusy) return;
      editBusy = true;
      controls();
      try {
        const receipt = await moveClient.reconcileUnknown();
        canonicalCurrent = receipt.current;
      } catch {
        status.textContent = "Подтверждение правки не восстановлено. Редактирование заблокировано; повторите восстановление.";
        editBusy = false;
        controls();
        return;
      }
      editBusy = false;
    }
    await guarded(() => client.resume());
  }

  async function deleteServer() {
    if (busy || editBusy || !deleteButton || !client.hasSavedSession()) return;
    if (storyDraftDirty() || composingStory) {
      status.textContent = "Сначала сохраните или сбросьте несохранённый текст перед удалением сессии.";
      return;
    }
    if (typeof confirmDelete !== "function" ||
        !confirmDelete("Удалить временный PUB и все изменения с сервера? Отменить удаление нельзя.")) {
      return;
    }
    busy = true;
    clearGesture();
    controls();
    status.textContent = "Удаляется временная сессия с сервера…";
    try {
      await client.deleteSession();
      clearEditor();
      pages.replaceChildren();
      resetViewport();
      expiry.textContent = "";
      input.value = "";
      status.textContent = "Сессия удалена с сервера. Для дальнейшей работы загрузите новый PUB.";
    } catch {
      // A lost receipt may conceal an already-revoked session. Preserve the
      // tab capability for recovery but do not leave a stale edit scene active.
      clearEditor();
      pages.replaceChildren();
      status.textContent = "Удаление не подтверждено сервером. Повторите попытку или дождитесь истечения TTL.";
    } finally {
      busy = false;
      controls();
    }
  }

  function forget() {
    if (busy || editBusy) return;
    if (storyDraftDirty() || composingStory) {
      status.textContent = "Сначала сохраните или сбросьте несохранённый текст перед удалением ключа вкладки.";
      return;
    }
    client.clear();
    clearEditor();
    pages.replaceChildren();
    resetViewport();
    expiry.textContent = "";
    input.value = "";
    status.textContent = "Ключ удалён из этой вкладки. Это не досрочное удаление файла с сервера.";
    controls();
  }

  function pageFromEvent(event) {
    const svg = event.target?.closest?.("svg.page[data-page-id]") ?? null;
    return svg && pages.contains?.(svg) ? svg : null;
  }

  function pointerDown(event) {
    if (storyDraftDirty() || composingStory) {
      status.textContent = "Сначала сохраните или сбросьте несохранённый текст перед работой с холстом.";
      return;
    }
    if (event.button !== 0 || !moveClient || busy || editBusy || gesture || rectangleDraft ||
        moveClient.status.pending_reconciliation || moveClient.status.refresh_required) return;
    const svg = pageFromEvent(event);
    if (!svg || !interaction) return;
    try {
      const point = temporaryEditorPagePoint(svg, event.clientX, event.clientY);
      if (drawEnabled && rectangleToolActive) {
        const pageId = svg.getAttribute("data-page-id");
        const page = interaction.pages.find(entry => entry.page_id === pageId);
        if (!page || !canonicalCurrent?.authoring_graph?.pages?.[pageId] ||
            point.x_emu < 0 || point.y_emu < 0 ||
            point.x_emu > page.width_emu || point.y_emu > page.height_emu) return;
        clearResizeHandles({forget: true});
        rectangleDraft = {pageId, page, start: point, pointerId: event.pointerId};
        gesturePage = svg;
        pages.setPointerCapture?.(event.pointerId);
        event.preventDefault?.();
        return;
      }
      const resizeHandle = event.target?.closest?.("[data-temporary-resize-handle]");
      if (resizeHandle) {
        const nodeId = resizeHandle.getAttribute("data-node-id");
        const handle = resizeHandle.getAttribute("data-temporary-resize-handle");
        const candidate = resizeCandidate(nodeId);
        if (!candidate || nodeId !== selectedResizeNodeId ||
            svg.getAttribute("data-page-id") !== selectedResizePageId) return;
        gesture = new ResizeTransactionV1(nodeId, candidate.bounds, handle, point);
      } else {
        const visualId = event.target?.closest?.("[data-node-id]")?.getAttribute?.("data-node-id");
        const pageId = svg.getAttribute("data-page-id");
        const hit = hitTestSnapshot(interaction, pageId, point);
        const selectedNodeId = hit.kind === "hit" &&
          (!visualId || hit.node_id === visualId) ? hit.node_id
          : hit.kind === "ambiguous" && visualId && hit.node_ids.includes(visualId)
            ? visualId : null;
        if (!selectedNodeId) return;
        const selectedStoryId = temporaryCanvasStoryId(
          canonicalCurrent?.authoring_graph, storyCandidates, selectedNodeId, pageId);
        if (selectedStoryId && activateStory(selectedStoryId)) {
          status.textContent = "Текстовая рамка выбрана на странице. Измените текст в панели и сохраните правку.";
        }
        // Visual SVG identity is enough for a non-mutating Story pick even
        // when stacking is ambiguous. It is never enough to authorize MoveNode.
        if (hit.kind !== "hit") {
          clearResizeHandles({forget: true});
          event.preventDefault?.();
          return;
        }
        gesture = new MoveGestureV1(interaction, hit.node_id, point);
        drawResizeHandles(svg, hit.node_id);
        // Keyboard authoring belongs to the canvas, not the Story textarea.
        pages.focus?.({preventScroll: true});
      }
      gesturePage = svg;
      overlayBounds(gesture.previewBounds());
      pages.setPointerCapture?.(event.pointerId);
      event.preventDefault?.();
    } catch {
      clearGesture();
    }
  }

  function pointerMove(event) {
    if (rectangleDraft && gesturePage) {
      if (event.pointerId !== rectangleDraft.pointerId) return;
      try {
        const end = temporaryEditorPagePoint(gesturePage, event.clientX, event.clientY);
        const bounds = temporaryRectangleDragBounds(rectangleDraft.start, end, rectangleDraft.page);
        if (bounds) overlayBounds(bounds);
        else {
          gestureOverlay?.remove?.();
          gestureOverlay = null;
        }
      } catch { clearGesture(); }
      return;
    }
    if (!gesture || !gesturePage) return;
    try {
      gesture.update(temporaryEditorPagePoint(gesturePage, event.clientX, event.clientY));
      overlayBounds(gesture.previewBounds());
    } catch { clearGesture(); }
  }

  async function pointerUp(event) {
    if (rectangleDraft) {
      if (event.pointerId !== rectangleDraft.pointerId || !moveClient || editBusy || busy) return;
      const draft = rectangleDraft;
      const svg = gesturePage;
      let bounds = null;
      try {
        bounds = temporaryRectangleDragBounds(
          draft.start, temporaryEditorPagePoint(svg, event.clientX, event.clientY), draft.page);
      } catch {}
      clearGesture();
      if (!bounds) {
        status.textContent = "Прямоугольник не создан: протяните область положительного размера внутри одной страницы.";
        return;
      }
      clearSheetPreview();
      editBusy = true;
      controls();
      status.textContent = "Создаётся прямоугольник в канонической ревизии…";
      try {
        const result = await moveClient.createRectangle({pageId: draft.pageId, bounds});
        canonicalCurrent = result.current;
        const scene = await moveClient.readerScene();
        const created = result.accepted.canonical_operation;
        const node = scene.nodes.find(item => item.node_id === created.node_id);
        const fill = node?.paint?.fill_rgb;
        const line = node?.paint?.line;
        const expectedPaint = created.paint;
        if (!node || node.kind !== "shape" || node.page_id !== draft.pageId ||
            !node.bounds || ["x", "y", "width", "height"].some(k => node.bounds[k] !== bounds[k]) ||
            !Array.isArray(fill) || fill.length !== 3 ||
            fill.some((value, index) => value !==
              [expectedPaint.fill.color.r, expectedPaint.fill.color.g, expectedPaint.fill.color.b][index]) ||
            !line || line.width_emu !== expectedPaint.stroke.width_emu ||
            !Array.isArray(line.rgb) ||
            line.rgb.some((value, index) => value !==
              [expectedPaint.stroke.color.r, expectedPaint.stroke.color.g, expectedPaint.stroke.color.b][index])) {
          throw new Error("temporary_create_shape_scene_mismatch");
        }
        interaction = projectReaderSceneToEditorInteractionScene(scene);
        partialFidelity ||= scene.fidelity?.state === "partial";
        await renderCanvas(scene);
        refreshStoryEditor();
        clearResizeHandles({forget: true});
        expiry.textContent = temporaryExpiryText(canonicalCurrent.expires_at_ms);
        status.textContent = "Прямоугольник сохранён сервером. Доступны изменённые IDML и ODG." +
          (partialFidelity ? " Needs review / Partial: порядок наложения и экспорт требуют проверки." : "");
      } catch {
        pages.replaceChildren();
        interaction = null;
        status.textContent = "Создание прямоугольника или повторное открытие сцены не подтверждено. Восстановите сессию перед правкой.";
        if (!moveClient?.status.pending_reconciliation) clearEditor();
      } finally {
        editBusy = false;
        controls();
      }
      return;
    }
    if (!gesture || !moveClient || editBusy || busy) return;
    const active = gesture;
    const pageSvg = gesturePage;
    let update;
    try {
      update = active.update(temporaryEditorPagePoint(pageSvg, event.clientX, event.clientY));
    } catch { clearGesture(); return; }
    const resizing = active instanceof ResizeTransactionV1;
    const next = active.previewBounds();
    const old = interaction.nodes.find(n => n.node_id === active.nodeId)?.bounds;
    clearGesture();
    if (!old || !next || update?.kind === "invalid" ||
        (resizing ? (old.width === next.width && old.height === next.height)
                  : (old.x === next.x && old.y === next.y))) return;
    clearSheetPreview();
    editBusy = true;
    controls();
    status.textContent = "Сохраняется каноническая правка…";
    try {
      const result = resizing
        ? await moveClient.resizeNode({nodeId: active.nodeId, bounds: next})
        : await moveClient.moveNode({
            nodeId: active.nodeId, xEmu: next.x, yEmu: next.y,
          });
      canonicalCurrent = result.current;
      const exactScene = await moveClient.readerScene();
      const updated = exactScene.nodes.find(n => n.node_id === active.nodeId);
      if (!updated || updated.bounds?.x !== next.x || updated.bounds?.y !== next.y ||
          (resizing && (updated.bounds?.width !== next.width ||
            updated.bounds?.height !== next.height))) {
        throw new Error("temporary_scene_geometry_mismatch");
      }
      interaction = projectReaderSceneToEditorInteractionScene(exactScene);
      await renderCanvas(exactScene);
      refreshStoryEditor();
      redrawResizeHandles();
      expiry.textContent = temporaryExpiryText(canonicalCurrent.expires_at_ms);
      status.textContent = "Правка сохранена в ревизии. Можно скачать изменённый IDML, ODG или PDF." +
        (partialFidelity ? " Needs review / Partial: результат необходимо проверить." : "");
    } catch {
      // Fail closed: never repaint the old stored scene as a new revision.
      pages.replaceChildren();
      interaction = null;
      status.textContent = "Подтверждение или обновлённая сцена недоступны. Восстановите сессию перед следующей правкой.";
      if (!moveClient.status.pending_reconciliation) {
        clearEditor();
      }
    } finally {
      editBusy = false;
      controls();
    }
  }


  // One Arrow key = one canonical MoveNode for a focused, selected, admitted
  // page-local object. Reuse the exact Guest mutation/reconciliation authority.
  async function canvasKeydown(event) {
    if (event.key === "Escape") {
      clearGesture();
      clearResizeHandles({forget: true});
      rectangleToolActive = false;
      controls();
      return;
    }
    const delta = {
      ArrowLeft: [-TEMPORARY_NUDGE_STEP_EMU, 0],
      ArrowRight: [TEMPORARY_NUDGE_STEP_EMU, 0],
      ArrowUp: [0, -TEMPORARY_NUDGE_STEP_EMU],
      ArrowDown: [0, TEMPORARY_NUDGE_STEP_EMU],
    }[event.key];
    if (!delta || storyDraftDirty() || composingStory || event.target !== pages || event.defaultPrevented ||
        event.isComposing || event.altKey || event.ctrlKey ||
        event.metaKey || event.shiftKey || gesture || busy || editBusy ||
        !moveClient || !canonicalCurrent || !interaction ||
        moveClient.status.pending_reconciliation || moveClient.status.refresh_required) return;
    const nodeId = selectedResizeNodeId;
    const pageId = selectedResizePageId;
    const candidate = resizeCandidate(nodeId);
    const projected = interaction.nodes.find(node => node.node_id === nodeId);
    if (!candidate || projected?.page_id !== pageId) return;
    const before = candidate.bounds;
    const next = {
      ...before, x: before.x + delta[0], y: before.y + delta[1],
    };
    if (![next.x, next.y, next.width, next.height].every(Number.isSafeInteger) ||
        !Number.isSafeInteger(next.x + next.width) ||
        !Number.isSafeInteger(next.y + next.height)) return;
    event.preventDefault?.();
    clearSheetPreview();
    editBusy = true;
    controls();
    status.textContent = "Сохраняется перемещение объекта…";
    try {
      const result = await moveClient.moveNode({
        nodeId, xEmu: next.x, yEmu: next.y,
      });
      canonicalCurrent = result.current;
      const scene = await moveClient.readerScene();
      const updated = scene.nodes.find(node => node.node_id === nodeId);
      if (!updated || updated.bounds?.x !== next.x ||
          updated.bounds?.y !== next.y ||
          updated.bounds?.width !== before.width ||
          updated.bounds?.height !== before.height) {
        throw new Error("temporary_nudge_scene_geometry_mismatch");
      }
      interaction = projectReaderSceneToEditorInteractionScene(scene);
      await renderCanvas(scene);
      refreshStoryEditor();
      redrawResizeHandles();
      expiry.textContent = temporaryExpiryText(canonicalCurrent.expires_at_ms);
      status.textContent = "Объект перемещён и сохранён в новой ревизии." +
        (partialFidelity ? " Needs review / Partial: результат необходимо проверить." : "");
    } catch {
      // Never show a speculative position or continue editing after a lost ACK.
      pages.replaceChildren();
      interaction = null;
      status.textContent = "Подтверждение перемещения недоступно. Восстановите сессию перед следующей правкой.";
      if (!moveClient.status.pending_reconciliation) clearEditor();
    } finally {
      editBusy = false;
      controls();
    }
  }

  async function applyStoryEdit() {
    if (!storyEnabled || busy || editBusy || composingStory || !moveClient ||
        moveClient.status.pending_reconciliation || moveClient.status.refresh_required) return;
    const storyId = activeStoryId;
    const candidate = storyCandidates.find(entry => entry.story_id === storyId);
    const replacement = storyText.value;
    try {
      if (!candidate) throw new Error("Story candidate unavailable");
      temporaryStoryObservedToCanonicalStory(candidate.text, replacement);
    } catch {
      status.textContent = "Поддерживается правка обычной текстовой рамки с сохранением существующих границ абзацев; новые разрывы и управляющие символы запрещены (до 8192 байт).";
      return;
    }
    clearSheetPreview();
    editBusy = true;
    controls();
    status.textContent = "Сохраняется текст в канонической ревизии…";
    try {
      const result = await moveClient.replaceStory({ storyId, newText: replacement });
      canonicalCurrent = result.current;
      const scene = await moveClient.readerScene();
      interaction = projectReaderSceneToEditorInteractionScene(scene);
      await renderCanvas(scene);
      refreshStoryEditor(storyId);
      expiry.textContent = temporaryExpiryText(canonicalCurrent.expires_at_ms);
      status.textContent = "Текст подтверждён сервером в изменённой ревизии. Доступен экспорт IDML, ODG или PDF." +
        (partialFidelity ? " Needs review / Partial: перед использованием проверьте экспорт." : "");
    } catch {
      // The POST may have committed despite a lost ACK or Scene failure.
      // Never claim an unsaved local textarea as authoritative.
      pages.replaceChildren();
      interaction = null;
      storyPanel.hidden = true;
      status.textContent = "Исход изменения текста неизвестен. Восстановите сессию перед новой правкой.";
      if (!moveClient?.status.pending_reconciliation) clearEditor();
    } finally {
      editBusy = false;
      controls();
    }
  }

  async function previewNineUp() {
    if (storyDraftDirty() || composingStory) {
      status.textContent = "Сначала сохраните или сбросьте несохранённый текст перед предпросмотром.";
      return;
    }
    if (!sheetPreviewEnabled || !moveClient || !canonicalCurrent ||
        sheetPreviewButton.disabled || busy || editBusy) return;
    editBusy = true;
    clearSheetPreview();
    controls();
    status.textContent = "Формируется и проверяется печатный лист А4…";
    try {
      const report = await moveClient.exportEdited("pdf9up", { reportOnly: true });
      const result = await moveClient.exportEdited("pdf9up", {
        expectedSha256: report.artifact_sha256,
      });
      // Never display bytes that disagree with the revision-bound export report.
      sheetPreviewUrl = URL.createObjectURL(result.blob);
      sheetFrame.src = sheetPreviewUrl;
      sheetPreview.hidden = false;
      status.textContent = "Печатный лист А4 проверен по редакции и контрольной сумме. Девять визиток готовы к просмотру.";
    } catch {
      clearSheetPreview();
      status.textContent = "Предпросмотр не подтверждён. Восстановите актуальную ревизию перед печатью.";
    } finally {
      editBusy = false;
      controls();
    }
  }

  async function download(target) {
    if (storyDraftDirty() || composingStory) {
      status.textContent = "Сначала сохраните или сбросьте несохранённый текст перед скачиванием файла.";
      return;
    }
    if (!moveClient || !canonicalCurrent || busy || editBusy) return;
    editBusy = true;
    controls();
    try {
      const isBaselinePdf = target === "pdf" && canonicalCurrent.revision_cursor === 0;
      const printableName = target === "pdf9up" ? "листа А4 с девятью карточками (PDF)" :
        isBaselinePdf ? "PDF исходной версии" : "изменённого " + target.toUpperCase();
      status.textContent = "Проверка потерь и подготовка " + printableName + "…";
      const report = await moveClient.exportEdited(target, { reportOnly: true });
      // Explicit disclosure before the response bytes are offered for download.
      const losses = report.loss_report?.counts;
      status.textContent = "Отчёт об экспорте: " +
        (losses && typeof losses === "object" ? JSON.stringify(losses) : "частично поддерживаемый формат") +
        (isBaselinePdf ? ". Скачивание PDF без правки…" : ". Скачивание изменённой ревизии…");
      const result = await moveClient.exportEdited(target, {
        expectedSha256: report.artifact_sha256,
      });
      const blobUrl = URL.createObjectURL(result.blob);
      try {
        const anchor = documentObject.createElement("a");
        anchor.href = blobUrl;
        anchor.download = target === "pdf9up" ? "chaptera-edited-a4-9up.pdf" :
          isBaselinePdf ? "chaptera-original.pdf" : "chaptera-edited." + target;
        anchor.hidden = true;
        documentObject.body.appendChild(anchor);
        anchor.click();
        anchor.remove();
      } finally {
        globalThis.setTimeout(() => URL.revokeObjectURL(blobUrl), 5000);
      }
      const downloadedName = target === "pdf9up" ? "лист А4 с девятью карточками (PDF)" :
        isBaselinePdf ? "PDF исходной версии" : target.toUpperCase();
      status.textContent = "Скачан " + downloadedName +
        (isBaselinePdf ? ". Потери: " : " из изменённой ревизии. Потери: ") +
        (losses && typeof losses === "object" ? JSON.stringify(losses) : "см. отчёт формата") +
        (partialFidelity ? ". Needs review / Partial: обязательно проверьте экспорт." : ".");
    } catch {
      clearSheetPreview();
      status.textContent = "Выгрузка текущей ревизии не подтверждена. Восстановите сессию и проверьте срок действия.";
    } finally {
      editBusy = false;
      controls();
    }
  }

  form.addEventListener("submit", async event => {
    event.preventDefault();
    await start(input.files?.[0] ?? null);
  });
  resumeButton.addEventListener("click", resume);
  forgetButton.addEventListener("click", forget);
  deleteButton?.addEventListener("click", () => deleteServer());
  if (storyEnabled) {
    storySelect.addEventListener("change", () => activateStory(storySelect.value));
    storyText.addEventListener("input", controls);
    storyDiscard?.addEventListener("click", () => {
      if (composingStory || busy || editBusy || !activeStoryId) return;
      const selected = storyCandidates.find(entry => entry.story_id === activeStoryId);
      if (!selected) return;
      storyText.value = temporaryStoryTextForTextarea(selected.text);
      status.textContent = "Локальный черновик сброшен; сохранённая ревизия не менялась.";
      controls();
    });
    storyText.addEventListener("compositionstart", () => {
      composingStory = true;
      controls();
    });
    storyText.addEventListener("compositionend", () => {
      composingStory = false;
      controls();
    });
    storyApply.addEventListener("click", applyStoryEdit);
  }
  if (editorEnabled) {
    if (drawEnabled) drawButton.addEventListener("click", () => {
      if (storyDraftDirty() || composingStory) {
        status.textContent = "Сначала сохраните или сбросьте несохранённый текст перед сменой инструмента.";
        return;
      }
      if (busy || editBusy || !moveClient ||
          moveClient.status.pending_reconciliation ||
          moveClient.status.refresh_required) return;
      clearGesture();
      clearResizeHandles({forget: true});
      rectangleToolActive = !rectangleToolActive;
      status.textContent = rectangleToolActive
        ? "Инструмент прямоугольника: нажмите на страницу и протяните область. Escape отменяет."
        : "Режим выбора: выделяйте, перемещайте и меняйте размер поддерживаемых объектов.";
      controls();
    });
    pages.addEventListener("pointerdown", pointerDown);
    pages.addEventListener("pointermove", pointerMove);
    pages.addEventListener("pointerup", pointerUp);
    pages.addEventListener("pointercancel", clearGesture);
    pages.addEventListener("keydown", canvasKeydown);
    idmlButton.addEventListener("click", () => download("idml"));
    odgButton.addEventListener("click", () => download("odg"));
    pdfButton?.addEventListener("click", () => download("pdf"));
    pdf9upButton?.addEventListener("click", () => download("pdf9up"));
    if (sheetPreviewEnabled) sheetPreviewButton.addEventListener("click", () => previewNineUp());
  }
  viewButtons.fit?.addEventListener("click",()=>changeViewportZoom({mode:"fit"}));
  viewButtons.actual?.addEventListener("click",()=>changeViewportZoom({mode:"manual",percent:100}));
  viewButtons.out?.addEventListener("click",()=>changeViewportZoom({mode:"manual",
    percent:Math.max(TEMP_ZOOM_MIN,(viewportZoom.mode==="fit"?100:viewportZoom.percent)-TEMP_ZOOM_STEP)}));
  viewButtons.in?.addEventListener("click",()=>changeViewportZoom({mode:"manual",
    percent:Math.min(TEMP_ZOOM_MAX,(viewportZoom.mode==="fit"?100:viewportZoom.percent)+TEMP_ZOOM_STEP)}));
  viewButtons.prev?.addEventListener("click",()=>navigateViewport(-1));
  viewButtons.next?.addEventListener("click",()=>navigateViewport(1));
  controls();
  return { start, resume, forget, deleteServer };
}

if (typeof document !== "undefined" && document.getElementById("temporary-preview-root")) {
  bootTemporaryEditorPreview().catch(() => {
    const status = document.getElementById("temporary-status");
    if (status) status.textContent = "Предварительный просмотр недоступен в этом браузере.";
  });
}
