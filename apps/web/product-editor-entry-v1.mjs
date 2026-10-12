import { BrowserObservabilityV1 } from "./observability-v1.mjs";
import { ChapteraProductEditorServiceV1 } from "./chaptera-product-editor-service-v1.mjs";
import { RichReaderEditorShellV1 } from "./rich-reader-editor-shell-v1.mjs";

const UUID_RE=/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
// SourceIngress persists project documents as `document:` plus 24 lowercase SHA-256 hex chars.
const SOURCE_INGRESS_DOCUMENT_RE=/^document:[0-9a-f]{24}$/;

export function documentIdFromEditorPath(pathname){
  if(typeof pathname!=="string") return null;
  const match=pathname.match(/^\/editor\/doc\/([^/]+)\/?$/);
  if(!match) return null;
  let value;
  try { value=decodeURIComponent(match[1]); } catch { return null; }
  return UUID_RE.test(value)||SOURCE_INGRESS_DOCUMENT_RE.test(value)?value:null;
}

export function loginUrlForReturnPath(returnPath){
  if(typeof returnPath!=="string"||!returnPath.startsWith("/")||returnPath.startsWith("//")){
    throw new TypeError("local return path is required");
  }
  return "/v1/auth/login?return_path="+encodeURIComponent(returnPath);
}

export function productEditorStatusView(state, readerScene = null) {
  if (!state || typeof state !== "object") {
    throw new TypeError("editor state receipt is required");
  }
  const revision = state.revision_id ?? "unknown";
  const reasons = readerScene?.fidelity?.reasons;
  const fidelity = "Fidelity: " + (readerScene?.fidelity?.state ?? "unknown") +
    (Array.isArray(reasons) && reasons.length ? " — " + reasons.join(", ") : "");
  if (state.commit_uncertain === true) {
    if (state.commit_recovery_in_flight === true) {
      return { status: "Checking exact server revision — editing locked…",
        kind: "pending", fidelity };
    }
    return { status: "Save outcome unknown — editing locked. Recheck server revision.",
      kind: "error", fidelity };
  }
  if (state.history_uncertain === true) {
    if (state.history_recovery_in_flight === true) {
      return { status: "Checking exact server history revision — editing locked…",
        kind: "pending", fidelity };
    }
    return { status: "Undo/Redo outcome unknown — editing locked. Recheck server revision.",
      kind: "error", fidelity };
  }
  if (state.reason === "history_authority_reconciled") {
    return { status: "Server history synchronized — verify the change before continuing",
      kind: "ok", fidelity };
  }
  if (state.reason === "commit_authority_reconciled") {
    return { status: "Server revision synchronized — verify the change before continuing",
      kind: "ok", fidelity };
  }
  if (state.reason === "commit_sent") {
    return { status: "Saving change…", kind: "pending", fidelity };
  }
  if (state.reason === "commit_error") {
    return {
      status: "Change not confirmed — reload to reconcile before retrying",
      kind: "error",
      fidelity,
    };
  }
  if (state.reason === "commit_rejected") {
    return {
      status: "Change rejected — reload the document before retrying",
      kind: "error",
      fidelity,
    };
  }
  if (state.reason === "history_blocked_during_save") return { status: "Save still pending — retry Undo/Redo after confirmation", kind: "pending", fidelity };
  const historyAction = state.history_command === "redo" ? "Redo" : "Undo";
  if (state.reason === "history_sent") {
    return { status: historyAction === "Redo" ? "Redoing change…" : "Undoing change…", kind: "pending", fidelity };
  }
  if (state.reason === "history_error") {
    return {
      status: historyAction + " not confirmed — reload to reconcile before retrying",
      kind: "error",
      fidelity,
    };
  }
  if (state.reason === "history_rejected") {
    return {
      status: historyAction + " rejected — current revision was not changed",
      kind: "error",
      fidelity,
    };
  }
  return {
    status: "Revision " + revision + (state.selected_node_id ? " · selected" : ""),
    kind: state.revision_id ? "ok" : "",
    fidelity,
  };
}

function statusText(element,text,kind=""){
  element.textContent=text;
  element.className="status"+(kind?" "+kind:"");
}

export async function bootProductEditor({
  locationObject=globalThis.location,
  documentObject=globalThis.document,
  cryptoObject=globalThis.crypto,
}={}){
  if(!locationObject||!documentObject) throw new TypeError("browser location/document are required");
  const documentId=documentIdFromEditorPath(locationObject.pathname);
  if(!documentId) throw new Error("invalid Chaptera Editor document URL");

  const status=documentObject.querySelector("#status");
  const fidelity=documentObject.querySelector("#fidelity");
  const identity=documentObject.querySelector("#document");
  const host=documentObject.querySelector("#canvas");
  const undoButton=documentObject.querySelector("#undo-action");
  const redoButton=documentObject.querySelector("#redo-action");
  const recheckButton=documentObject.querySelector("#reconcile-action");
  if(!status||!fidelity||!identity||!host) throw new Error("Chaptera Editor shell DOM is incomplete");
  identity.textContent=documentId;

  let seq=0;
  const observability=new BrowserObservabilityV1({
    sessionIncarnation:"session:product-editor",
    browserFamily:navigator.userAgent.includes("Firefox")?"firefox":"chromium",
    idFactory:(prefix)=>prefix+":editor-"+String(++seq).padStart(6,"0"),
  });
  const service=new ChapteraProductEditorServiceV1(locationObject.origin,{
    documentId,
    observability,
  });

  try{
    await service.session();
  }catch(error){
    if(/request failed: 401\b/.test(String(error?.message??error))){
      const returnPath=locationObject.pathname+locationObject.search;
      locationObject.assign(loginUrlForReturnPath(returnPath));
      return {kind:"login_redirect",document_id:documentId};
    }
    throw error;
  }

  let shell=null;
  shell=new RichReaderEditorShellV1({
    host,
    service,
    keyboardTarget:documentObject,
    operationIdFactory:()=> "product-move-"+cryptoObject.randomUUID(),
    historyOperationIdFactory:()=> "product-history-"+cryptoObject.randomUUID(),
    onState:(state)=>{
      const view=productEditorStatusView(state,shell?.readerScene);
      statusText(status,view.status,view.kind);
      fidelity.textContent=view.fidelity;
      const mutationLocked=state.history_in_flight===true||state.commit_in_flight===true||
        state.commit_uncertain===true||state.commit_recovery_in_flight===true||
        state.history_uncertain===true||state.history_recovery_in_flight===true;
      if(undoButton) undoButton.disabled=mutationLocked;
      if(redoButton) redoButton.disabled=mutationLocked;
      if(recheckButton){
        recheckButton.hidden=state.commit_uncertain!==true&&state.history_uncertain!==true;
        recheckButton.disabled=!mutationLocked||state.commit_recovery_in_flight===true||
          state.history_recovery_in_flight===true||
          (state.commit_uncertain!==true&&state.history_uncertain!==true);
      }
    },
  });
  await shell.start();
  recheckButton?.addEventListener("click",()=>{
    if(shell.commitUncertain && !shell.commitRecoveryInFlight){
      void shell.reconcileUncertainCommit().catch(()=>{});
    }else if(shell.historyUncertain && !shell.historyRecoveryInFlight){
      void shell.reconcileUncertainHistory().catch(()=>{});
    }
  });
  for(const [button,command] of [[undoButton,"undo"],[redoButton,"redo"]]){
    button?.addEventListener("click",()=>{
      if(shell.historyInFlight||shell.historyUncertain||shell.historyRecoveryInFlight) return;
      void shell[command]().catch(()=>{});
    });
  }
  return {kind:"ready",document_id:documentId,shell,service};
}

if(typeof window!=="undefined"&&typeof document!=="undefined"){
  bootProductEditor().catch((error)=>{
    const status=document.querySelector("#status");
    if(status) statusText(status,String(error?.message??error),"error");
  });
}
