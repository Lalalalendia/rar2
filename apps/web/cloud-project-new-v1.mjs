import { ChapteraCloudSourceIngressV1 } from "./chaptera-cloud-source-ingress-v1.mjs";
import { WebFileEntryControllerV1, bindWebFileEntryV1 } from "./file-entry-v1.mjs";

// This page deliberately composes the existing file-entry state machine with
// real Rust source ingress. It never supplies a tenant/principal as client truth.
export async function bootCloudProjectNew({
  documentObject = globalThis.document,
  locationObject = globalThis.location,
  sourceIngress = new ChapteraCloudSourceIngressV1(),
} = {}) {
  if (!documentObject || !locationObject) throw new TypeError("browser context required");
  const pick = (selector) => {
    const element = documentObject.querySelector(selector);
    if (!element) throw new Error("cloud project creation UI missing " + selector);
    return element;
  };
  const dropTarget = pick("#drop-zone");
  const fileInput = pick("#pub-file");
  const chooseButton = pick("#choose-pub");
  const retryButton = pick("#retry");
  const cancelButton = pick("#cancel");
  const progress = pick("#progress");
  const status = pick("#status");
  const errorLabel = pick("#error");

  try {
    await sourceIngress.prepare();
  } catch (error) {
    if (error?.status === 401) {
      locationObject.assign("/v1/auth/login?return_path=%2Feditor%2Fnew");
      return { kind: "login_redirect" };
    }
    status.textContent = "Не удалось подготовить личное пространство.";
    errorLabel.textContent = error?.code ?? "workspace_unavailable";
    return { kind: "error", error };
  }

  const controller = new WebFileEntryControllerV1({
    sourceIngress,
    openProject: async ({ document_id }) => {
      if (typeof document_id !== "string" ||
          !/^(document:[0-9a-f]{24}|[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})$/.test(document_id)) {
        throw new TypeError("invalid canonical document identity");
      }
      locationObject.assign("/editor/doc/" + encodeURIComponent(document_id));
    },
    onState: (state) => {
      const busy = ["uploading", "validating", "preparing_project", "opening"].includes(state.phase);
      status.textContent = state.label;
      errorLabel.textContent = state.error_code ?? "";
      chooseButton.disabled = busy;
      retryButton.disabled = !state.retryable || busy;
      cancelButton.disabled = !busy;
      progress.hidden = state.phase !== "uploading";
      progress.max = Math.max(1, state.bytes_total);
      progress.value = Math.min(progress.max, state.bytes_sent);
    },
  });
  const binding = bindWebFileEntryV1({
    controller,
    dropTarget,
    fileInput,
    openButton: chooseButton,
    keyboardTarget: globalThis.window ?? null,
    onMultiplePub: () => {
      status.textContent = "Выберите один PUB для создания проекта";
    },
    onUnsupported: () => {
      status.textContent = "Поддерживаются файлы .pub";
    },
  });
  retryButton.addEventListener("click", () => { void controller.retry(); });
  cancelButton.addEventListener("click", () => controller.cancel());
  status.textContent = "Готов к загрузке файла Publisher";
  return { kind: "ready", controller, binding };
}

if (typeof window !== "undefined" && typeof document !== "undefined") {
  bootCloudProjectNew().catch((error) => {
    const status = document.querySelector("#status");
    if (status) status.textContent = String(error?.code ?? error?.message ?? error);
  });
}
