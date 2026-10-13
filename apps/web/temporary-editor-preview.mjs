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

export async function bootTemporaryEditorPreview({
  documentObject = globalThis.document,
  cryptoObject = globalThis.crypto,
  storage = defaultStorage(),
  fetchImpl = globalThis.fetch,
  renderer = renderReaderScene,
} = {}) {
  const ids = [
    "temporary-preview-root", "temporary-upload-form", "temporary-file",
    "temporary-start", "temporary-resume", "temporary-forget",
    "temporary-status", "temporary-expiry", "temporary-pages",
  ];
  const elements = Object.fromEntries(ids.map((id) => [id, documentObject?.getElementById(id)]));
  if (ids.some((id) => !elements[id])) {
    throw new Error("temporary preview HTML contract incomplete");
  }
  const form = elements["temporary-upload-form"];
  const input = elements["temporary-file"];
  const startButton = elements["temporary-start"];
  const resumeButton = elements["temporary-resume"];
  const forgetButton = elements["temporary-forget"];
  const status = elements["temporary-status"];
  const expiry = elements["temporary-expiry"];
  const pages = elements["temporary-pages"];
  const client = new TemporaryEditBrowserSessionV0({
    csrfToken: newCsrfToken(cryptoObject), storage, fetchImpl,
  });
  let busy = false;
  const buttons = [startButton, resumeButton, forgetButton];

  function controls() {
    for (const button of buttons) button.disabled = busy;
    resumeButton.hidden = !client.hasSavedSession();
    forgetButton.hidden = !client.hasSavedSession();
  }

  async function show(view) {
    const description = describeTemporaryPreview(view);
    pages.replaceChildren();
    expiry.textContent = temporaryExpiryText(view.expires_at_ms);
    if (description.renderable) {
      try {
        await renderer(pages, view.scene, {
          maxPageWidth: Math.max(320, Math.min(920, pages.clientWidth || 920)),
        });
      } catch {
        pages.replaceChildren();
        status.textContent = "Не удалось безопасно отобразить страницы этого PUB. Оригинал сохранён без изменений.";
        controls();
        return;
      }
    }
    status.textContent = description.message;
    controls();
  }

  async function guarded(operation) {
    if (busy) return;
    busy = true;
    controls();
    status.textContent = "Обработка временной сессии…";
    try {
      const view = await operation();
      await show(view);
    } catch (error) {
      pages.replaceChildren();
      expiry.textContent = "";
      const code = SAFE_ERROR_CODES.has(error?.code)
        ? error.code : "temporary_request_failed";
      status.textContent = "Временный просмотр не завершён (" + code + "). Исходный PUB не изменён.";
    } finally {
      busy = false;
      controls();
    }
  }

  async function start(file) {
    if (!file || !/\.pub$/i.test(file.name ?? "") || !Number.isSafeInteger(file.size) || file.size <= 0) {
      status.textContent = "Выберите непустой файл с расширением .pub.";
      return;
    }
    await guarded(() => client.start(file));
  }

  async function resume() {
    await guarded(() => client.resume());
  }

  function forget() {
    if (busy) return;
    client.clear();
    pages.replaceChildren();
    expiry.textContent = "";
    input.value = "";
    status.textContent = "Ключ удалён из этой вкладки. Это не досрочное удаление файла с сервера.";
    controls();
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    await start(input.files?.[0] ?? null);
  });
  resumeButton.addEventListener("click", resume);
  forgetButton.addEventListener("click", forget);
  controls();
  return { start, resume, forget };
}

if (typeof document !== "undefined" && document.getElementById("temporary-preview-root")) {
  bootTemporaryEditorPreview().catch(() => {
    const status = document.getElementById("temporary-status");
    if (status) status.textContent = "Предварительный просмотр недоступен в этом браузере.";
  });
}
