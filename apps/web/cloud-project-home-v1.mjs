import { ChapteraCloudSourceIngressV1 } from "./chaptera-cloud-source-ingress-v1.mjs";
import { ChapteraCloudProjectCatalogV1 } from "./chaptera-cloud-project-catalog-v1.mjs";
import { WebProjectHomeControllerV1 } from "./project-home-v1.mjs";

export function editorPathForDocument(documentId) {
  if (typeof documentId !== "string" ||
      !/^(document:[0-9a-f]{24}|[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})$/.test(documentId)) {
    throw new TypeError("invalid canonical document identity");
  }
  return "/editor/doc/" + encodeURIComponent(documentId);
}

export function renderProjectCardsV1(container, cards, locationObject) {
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
    renderProjectCardsV1(list, state.cards, locationObject);
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
