import { ChapteraCloudSourceIngressV1 } from "./chaptera-cloud-source-ingress-v1.mjs";
import { ChapteraCloudProjectCatalogV1 } from "./chaptera-cloud-project-catalog-v1.mjs";
import { WebProjectHomeControllerV1 } from "./project-home-v1.mjs";
import { canonicalProjectRenameName, submitProjectRenameV1 } from "./cloud-project-rename-v1.mjs";

export function editorPathForDocument(documentId) {
  if (typeof documentId !== "string" ||
      !/^(document:[0-9a-f]{24}|[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})$/.test(documentId)) {
    throw new TypeError("invalid canonical document identity");
  }
  return "/editor/doc/" + encodeURIComponent(documentId);
}

export function renderProjectCardsV1(container, cards, locationObject, { onRename = null } = {}) {
  if (!container || typeof container.replaceChildren !== "function") {
    throw new TypeError("project list container required");
  }
  if (!Array.isArray(cards)) throw new TypeError("cards array required");
  const documentObject = container.ownerDocument;
  const fragment = documentObject.createDocumentFragment();
  for (const project of cards) {
    const card = documentObject.createElement("article");
    card.className = "project-card";
    card.dataset.projectId = project.project_id;
    card.dataset.documentId = project.document_id;

    const title = documentObject.createElement("h2");
    title.textContent = project.name;
    card.append(title);

    const meta = documentObject.createElement("p");
    meta.className = "project-meta";
    meta.textContent = "Revision " + project.current_revision_id.slice(0, 20) + "…";
    card.append(meta);

    const open = documentObject.createElement("button");
    open.type = "button";
    open.textContent = "Открыть";
    open.dataset.action = "open-project";
    open.addEventListener("click", () => {
      locationObject.assign(editorPathForDocument(project.document_id));
    });
    card.append(open);
    if (typeof onRename === "function") {
      const rename = documentObject.createElement("button");
      rename.type = "button";
      rename.textContent = "Переименовать";
      rename.dataset.action = "rename-project";

      const form = documentObject.createElement("form");
      form.hidden = true;
      form.className = "project-rename-form";
      form.dataset.action = "rename-form";

      const input = documentObject.createElement("input");
      input.type = "text";
      input.required = true;
      input.maxLength = 512;
      input.value = project.name;
      input.dataset.action = "rename-input";
      input.setAttribute("aria-label", "Новое название проекта");

      const save = documentObject.createElement("button");
      save.type = "submit";
      save.textContent = "Сохранить";
      save.dataset.action = "save-rename";

      const cancel = documentObject.createElement("button");
      cancel.type = "button";
      cancel.textContent = "Отмена";
      cancel.dataset.action = "cancel-rename";

      const message = documentObject.createElement("p");
      message.className = "project-rename-message";
      message.setAttribute("role", "alert");
      message.hidden = true;

      let pending = false;
      rename.addEventListener("click", () => {
        if (pending) return;
        input.value = project.name;
        message.hidden = true;
        form.hidden = false;
        rename.hidden = true;
        input.focus();
        input.select();
      });
      cancel.addEventListener("click", () => {
        if (pending) return;
        form.hidden = true;
        rename.hidden = false;
        message.hidden = true;
      });
      form.addEventListener("submit", async (event) => {
        event.preventDefault();
        if (pending) return;
        let name;
        try {
          name = canonicalProjectRenameName(input.value);
        } catch {
          message.textContent = "Введите название без управляющих символов (не более 512 байт).";
          message.hidden = false;
          return;
        }
        if (name === project.name) {
          form.hidden = true;
          rename.hidden = false;
          return;
        }
        pending = true;
        input.disabled = true;
        save.disabled = true;
        cancel.disabled = true;
        message.hidden = false;
        message.textContent = "Ожидаем подтверждение сервера…";
        try {
          await onRename(project, name);
        } catch (error) {
          message.textContent = error?.status === 403
            ? "Недостаточно прав для переименования."
            : error?.status === 409
              ? "Проект изменился в другой сессии. Обновите список."
              : error?.status === 401
                ? "Сессия истекла. Войдите снова."
                : "Переименование не подтверждено. Повторите — запрос безопасен при потере ответа.";
          message.hidden = false;
        } finally {
          pending = false;
          input.disabled = false;
          save.disabled = false;
          cancel.disabled = false;
        }
      });
      form.append(input, save, cancel, message);
      card.append(rename, form);
    }
    fragment.append(card);
  }
  container.replaceChildren(fragment);
}

export async function bootCloudProjectHome({
  documentObject = globalThis.document,
  locationObject = globalThis.location,
  workspaceSession = new ChapteraCloudSourceIngressV1(),
} = {}) {
  if (!documentObject || !locationObject) throw new TypeError("browser context required");
  const list = documentObject.querySelector("#project-list");
  const status = documentObject.querySelector("#status");
  const empty = documentObject.querySelector("#empty");
  const create = documentObject.querySelector("#new-project");
  if (!list || !status || !empty || !create) throw new Error("cloud project home DOM is incomplete");

  create.addEventListener("click", () => locationObject.assign("/editor/new"));
  const catalog = new ChapteraCloudProjectCatalogV1({ workspaceSession });
  const controller = new WebProjectHomeControllerV1({ catalog });
  // Retain the exact mutation ID across a lost response. Never synthesize
  // a successful rename locally or reuse one ID for changed input.
  const pendingRenames = new Map();
  async function renameProject(project, name) {
    const normalized = canonicalProjectRenameName(name);
    const binding = [
      project.project_id, project.lifecycle_generation,
      project.metadata_version, normalized,
    ].join("|");
    let request = pendingRenames.get(project.project_id);
    if (!request || request.binding !== binding) {
      request = { binding, clientRequestId: crypto.randomUUID() };
      pendingRenames.set(project.project_id, request);
    }
    try {
      const receipt = await submitProjectRenameV1({
        workspaceSession, project, name: normalized,
        clientRequestId: request.clientRequestId,
      });
      // Revalidate the durable catalog rather than changing a label in the DOM
      // based only on a mutation response.
      const fresh = await controller.loadProjects();
      const actual = fresh.cards?.find(card => card.project_id === project.project_id);
      if (fresh.mode !== "projects" || actual?.document_id !== project.document_id ||
          actual?.name !== receipt.name ||
          actual?.metadata_version !== receipt.metadata_version ||
          actual?.lifecycle_generation !== receipt.lifecycle_generation ||
          actual?.current_revision_id !== project.current_revision_id) {
        const error = new Error("renamed project not yet confirmed by durable catalog");
        error.code = "project_rename_catalog_unconfirmed";
        error.retryable = true;
        throw error;
      }
      pendingRenames.delete(project.project_id);
      renderProjectCardsV1(list, fresh.cards, locationObject, { onRename: renameProject });
      empty.hidden = fresh.cards.length !== 0;
      status.textContent = "Проектов: " + fresh.cards.length;
      return receipt;
    } catch (error) {
      // A transport/JSON/5xx/catalog failure can occur AFTER the durable
      // rename commits. Preserve the exact idempotency ID for those unknown
      // outcomes; clear it only on a definitive client-side rejection.
      const rejected = Number.isInteger(error?.status) &&
        error.status >= 400 && error.status < 500 && error.status !== 429;
      if (rejected) pendingRenames.delete(project.project_id);
      throw error;
    }
  }
  status.textContent = "Загружаем проекты…";

  try {
    const prepared = await workspaceSession.prepare();
    if (typeof prepared !== "string" || !prepared) throw new Error("workspace unavailable");
    const state = await controller.loadProjects();
    if (state.mode === "error") {
      const error = new Error(state.error_code ?? "project_catalog_failed");
      error.code = state.error_code ?? "project_catalog_failed";
      throw error;
    }
    renderProjectCardsV1(list, state.cards, locationObject, { onRename: renameProject });
    empty.hidden = state.cards.length !== 0;
    status.textContent = state.cards.length === 0 ? "Проектов пока нет" : "Проектов: " + state.cards.length;
    return { kind: "ready", controller, cards: state.cards };
  } catch (error) {
    if (error?.status === 401 || error?.code === "session_missing") {
      locationObject.assign("/v1/auth/login?return_path=%2Feditor");
      return { kind: "login_redirect" };
    }
    status.textContent = "Не удалось загрузить проекты";
    status.dataset.errorCode = error?.code ?? "project_catalog_failed";
    return { kind: "error", error };
  }
}

if (typeof window !== "undefined" && typeof document !== "undefined") {
  bootCloudProjectHome().catch((error) => {
    const status = document.querySelector("#status");
    if (status) status.textContent = String(error?.code ?? error?.message ?? error);
  });
}
