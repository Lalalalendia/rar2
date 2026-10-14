import {
  TemporaryGuestMoveClientV1, temporaryPlainStoryCandidates,
  temporaryStoryTextForTextarea, temporaryStoryObservedToCanonicalStory,
} from "./temporary-edit-move-client-v1.mjs";
import { MoveGestureV1, hitTestSnapshot } from "./interaction-v1.mjs";
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

const SVG_NS = "http://www.w3.org/2000/svg";

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
  const odgButton = documentObject?.getElementById("temporary-download-odg");
  const pdfButton = documentObject?.getElementById("temporary-download-pdf");
  const pdf9upButton = documentObject?.getElementById("temporary-download-pdf9up");
  const sheetPreviewButton = documentObject?.getElementById("temporary-preview-pdf9up");
  const sheetPreview = documentObject?.getElementById("temporary-sheet-preview");
  const sheetFrame = documentObject?.getElementById("temporary-sheet-pdf-frame");
  const sheetPreviewEnabled = !!(sheetPreviewButton && sheetPreview && sheetFrame);
  const editorEnabled = !!(actions && idmlButton && odgButton);
  const storyPanel = documentObject?.getElementById("temporary-story-panel");
  const storySelect = documentObject?.getElementById("temporary-story-select");
  const storyText = documentObject?.getElementById("temporary-story-text");
  const storyApply = documentObject?.getElementById("temporary-story-apply");
  const storyEnabled = !!(editorEnabled && storyPanel && storySelect && storyText && storyApply);
  const csrfToken = newCsrfToken(cryptoObject);
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken, storage, fetchImpl,
  });

  let busy = false;
  let editBusy = false;
  let moveClient = null;
  let canonicalCurrent = null;
  let interaction = null;
  let partialFidelity = false;
  let storyCandidates = [];
  let composingStory = false;
  let gesture = null;
  let gesturePage = null;
  let gestureOverlay = null;
  let sheetPreviewUrl = null;
  const buttons = [startButton, resumeButton, forgetButton, deleteButton].filter(Boolean);

  function controls() {
    for (const button of buttons) button.disabled = busy || editBusy;
    resumeButton.hidden = !client.hasSavedSession();
    forgetButton.hidden = !client.hasSavedSession();
    if (deleteButton) deleteButton.hidden = !client.hasSavedSession();
    if (editorEnabled) {
      actions.hidden = !moveClient;
      const canDownload = !busy && !editBusy && moveClient &&
        !moveClient.status.pending_reconciliation &&
        !moveClient.status.refresh_required &&
        canonicalCurrent?.revision_cursor > 0;
      idmlButton.disabled = !canDownload;
      odgButton.disabled = !canDownload;
      if (pdfButton) pdfButton.disabled = !canDownload;
      if (pdf9upButton || sheetPreviewEnabled) {
        const validCard = interaction?.pages?.length === 1 &&
          interaction.pages[0]?.width_emu === 3060000 &&
          interaction.pages[0]?.height_emu === 1980000;
        if (pdf9upButton) {
          pdf9upButton.hidden = !validCard;
          pdf9upButton.disabled = !canDownload || !validCard;
        }
        if (sheetPreviewEnabled) {
          sheetPreviewButton.hidden = !validCard;
          sheetPreviewButton.disabled = !canDownload || !validCard;
        }
      }
    }
  }

  function clearSheetPreview() {
    if (!sheetPreviewEnabled) return;
    sheetFrame.src = "";
    sheetPreview.hidden = true;
    if (sheetPreviewUrl !== null) {
      URL.revokeObjectURL(sheetPreviewUrl);
      sheetPreviewUrl = null;
    }
    if (storyEnabled) {
      const locked = busy || editBusy || !moveClient ||
        moveClient.status.pending_reconciliation || moveClient.status.refresh_required;
      storyPanel.hidden = !moveClient || storyCandidates.length === 0;
      storySelect.disabled = locked;
      storyText.disabled = locked;
      storyApply.disabled = locked || composingStory || storyCandidates.length === 0;
    }
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
    storyPanel.hidden = !moveClient || !selected;
  }

  function clearGesture() {
    try { gestureOverlay?.remove?.(); } catch {}
    gestureOverlay = null;
    if (gesture && gesture.state === "active") gesture.cancel();
    gesture = null;
    gesturePage = null;
  }

  function clearEditor() {
    clearSheetPreview();
    clearGesture();
    moveClient = null;
    canonicalCurrent = null;
    interaction = null;
    storyCandidates = [];
    composingStory = false;
    refreshStoryEditor();
  }

  function overlayBounds(bounds) {
    if (!gesturePage || !bounds) return;
    if (!gestureOverlay) {
      gestureOverlay = documentObject.createElementNS(SVG_NS, "rect");
      gestureOverlay.setAttribute("class", "temporary-move-preview");
      gestureOverlay.setAttribute("pointer-events", "none");
      gesturePage.appendChild(gestureOverlay);
    }
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
          storage, csrfToken, fetchImpl, cryptoObject,
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
        await renderer(pages, effectiveScene, {
          maxPageWidth: Math.max(320, Math.min(920, pages.clientWidth || 920)),
        });
      } catch {
        pages.replaceChildren();
        clearEditor();
        status.textContent = "Не удалось безопасно отобразить страницы. Оригинал PUB сохранён без изменений.";
        controls();
        return;
      }
    }
    refreshStoryEditor();
    if (moveClient) {
      expiry.textContent = temporaryExpiryText(canonicalCurrent.expires_at_ms);
      status.textContent = partialFidelity
        ? "Открыта редактируемая ревизия — Needs review / Partial: отображение неполное. Перемещайте только поддерживаемые объекты, проверяйте LossReport перед использованием выгрузки."
        : "Открыта редактируемая ревизия. Перетащите поддерживаемый объект. Изменение подтверждает сервер.";
    } else {
      status.textContent = description.message;
    }
    controls();
  }

  async function guarded(operation) {
    if (busy || editBusy) return;
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
    client.clear();
    clearEditor();
    pages.replaceChildren();
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
    if (event.button !== 0 || !moveClient || busy || editBusy || gesture ||
        moveClient.status.pending_reconciliation || moveClient.status.refresh_required) return;
    const svg = pageFromEvent(event);
    if (!svg || !interaction) return;
    try {
      const point = temporaryEditorPagePoint(svg, event.clientX, event.clientY);
      const visualId = event.target?.closest?.("[data-node-id]")?.getAttribute?.("data-node-id");
      const hit = hitTestSnapshot(interaction, svg.getAttribute("data-page-id"), point);
      if (hit.kind !== "hit" || (visualId && hit.node_id !== visualId)) return;
      gesture = new MoveGestureV1(interaction, hit.node_id, point);
      gesturePage = svg;
      overlayBounds(gesture.previewBounds());
      pages.setPointerCapture?.(event.pointerId);
      event.preventDefault?.();
    } catch {
      clearGesture();
    }
  }

  function pointerMove(event) {
    if (!gesture || !gesturePage) return;
    try {
      gesture.update(temporaryEditorPagePoint(gesturePage, event.clientX, event.clientY));
      overlayBounds(gesture.previewBounds());
    } catch { clearGesture(); }
  }

  async function pointerUp(event) {
    if (!gesture || !moveClient || editBusy || busy) return;
    const active = gesture;
    const pageSvg = gesturePage;
    try {
      active.update(temporaryEditorPagePoint(pageSvg, event.clientX, event.clientY));
    } catch { clearGesture(); return; }
    const next = active.previewBounds();
    const old = interaction.nodes.find(n => n.node_id === active.nodeId)?.bounds;
    clearGesture();
    if (!old || !next || (old.x === next.x && old.y === next.y)) return;
    clearSheetPreview();
    editBusy = true;
    controls();
    status.textContent = "Сохраняется каноническая правка…";
    try {
      const result = await moveClient.moveNode({
        nodeId: active.nodeId, xEmu: next.x, yEmu: next.y,
      });
      canonicalCurrent = result.current;
      const exactScene = await moveClient.readerScene();
      const updated = exactScene.nodes.find(n => n.node_id === active.nodeId);
      if (!updated || updated.bounds?.x !== next.x || updated.bounds?.y !== next.y) {
        throw new Error("temporary_scene_geometry_mismatch");
      }
      interaction = projectReaderSceneToEditorInteractionScene(exactScene);
      await renderer(pages, exactScene, {
        maxPageWidth: Math.max(320, Math.min(920, pages.clientWidth || 920)),
      });
      refreshStoryEditor();
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


  async function applyStoryEdit() {
    if (!storyEnabled || busy || editBusy || composingStory || !moveClient ||
        moveClient.status.pending_reconciliation || moveClient.status.refresh_required) return;
    const storyId = storySelect.value;
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
      await renderer(pages, scene, {
        maxPageWidth: Math.max(320, Math.min(920, pages.clientWidth || 920)),
      });
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
    if (!moveClient || !canonicalCurrent || busy || editBusy) return;
    editBusy = true;
    controls();
    try {
      const printableName = target === "pdf9up" ? "листа А4 с девятью карточками (PDF)" :
        "изменённого " + target.toUpperCase();
      status.textContent = "Проверка потерь и подготовка " + printableName + "…";
      const report = await moveClient.exportEdited(target, { reportOnly: true });
      // Explicit disclosure before the response bytes are offered for download.
      const losses = report.loss_report?.counts;
      status.textContent = "Отчёт об экспорте: " +
        (losses && typeof losses === "object" ? JSON.stringify(losses) : "частично поддерживаемый формат") +
        ". Скачивание изменённой ревизии…";
      const result = await moveClient.exportEdited(target, {
        expectedSha256: report.artifact_sha256,
      });
      const blobUrl = URL.createObjectURL(result.blob);
      try {
        const anchor = documentObject.createElement("a");
        anchor.href = blobUrl;
        anchor.download = target === "pdf9up" ? "chaptera-edited-a4-9up.pdf" : "chaptera-edited." + target;
        anchor.hidden = true;
        documentObject.body.appendChild(anchor);
        anchor.click();
        anchor.remove();
      } finally {
        globalThis.setTimeout(() => URL.revokeObjectURL(blobUrl), 5000);
      }
      const downloadedName = target === "pdf9up" ? "лист А4 с девятью карточками (PDF)" :
        target.toUpperCase();
      status.textContent = "Скачан " + downloadedName + " из изменённой ревизии. Потери: " +
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
    storySelect.addEventListener("change", () => {
      const selected = storyCandidates.find(entry => entry.story_id === storySelect.value);
      storyText.value = selected ? temporaryStoryTextForTextarea(selected.text) : "";
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
    pages.addEventListener("pointerdown", pointerDown);
    pages.addEventListener("pointermove", pointerMove);
    pages.addEventListener("pointerup", pointerUp);
    pages.addEventListener("pointercancel", clearGesture);
    idmlButton.addEventListener("click", () => download("idml"));
    odgButton.addEventListener("click", () => download("odg"));
    pdfButton?.addEventListener("click", () => download("pdf"));
    pdf9upButton?.addEventListener("click", () => download("pdf9up"));
    if (sheetPreviewEnabled) sheetPreviewButton.addEventListener("click", () => previewNineUp());
  }
  controls();
  return { start, resume, forget, deleteServer };
}

if (typeof document !== "undefined" && document.getElementById("temporary-preview-root")) {
  bootTemporaryEditorPreview().catch(() => {
    const status = document.getElementById("temporary-status");
    if (status) status.textContent = "Предварительный просмотр недоступен в этом браузере.";
  });
}
