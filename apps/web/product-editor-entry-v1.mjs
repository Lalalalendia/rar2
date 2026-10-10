import { BrowserObservabilityV1 } from "./observability-v1.mjs";
import { ChapteraProductEditorServiceV1 } from "./chaptera-product-editor-service-v1.mjs";
import { RichReaderEditorShellV1 } from "./rich-reader-editor-shell-v1.mjs";

const UUID_RE=/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
// SourceIngress persists project documents as `document:` plus 24 lowercase SHA-256 hex chars.
const SOURCE_INGRESS_DOCUMENT_RE=/^document:[0-9a-f]{24}$/;
const TRACE_ID_RE=/^[A-Za-z0-9._:-]{8,160}$/;

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

export function productEditorStatusView(state, readerScene = null, traceContext = null) {
  if (!state || typeof state !== "object") {
    throw new TypeError("editor state receipt is required");
  }
  const revision = state.revision_id ?? "unknown";
  const reasons = readerScene?.fidelity?.reasons;
  const fidelity = "Fidelity: " + (readerScene?.fidelity?.state ?? "unknown") +
    (Array.isArray(reasons) && reasons.length ? " — " + reasons.join(", ") : "");
  const traceRef = typeof traceContext?.trace_id === "string" && TRACE_ID_RE.test(traceContext.trace_id)
    ? " · Ref " + traceContext.trace_id
    : "";
  if (state.reason === "commit_sent") {
    return { status: "Saving change…", kind: "pending", fidelity };
  }
  if (state.reason === "commit_error") {
    return {
      status: "Change not confirmed — reload to reconcile before retrying" + traceRef,
      kind: "error",
      fidelity,
    };
  }
  if (state.reason === "commit_rejected") {
    return {
      status: "Change rejected — reload the document before retrying" + traceRef,
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
    operationIdFactory:()=> "product-move-"+cryptoObject.randomUUID(),
    onState:(state)=>{
      const view=productEditorStatusView(state,shell?.readerScene,service.lastCommitTraceContext);
      statusText(status,view.status,view.kind);
      fidelity.textContent=view.fidelity;
    },
  });
  await shell.start();
  return {kind:"ready",document_id:documentId,shell,service};
}

if(typeof window!=="undefined"&&typeof document!=="undefined"){
  bootProductEditor().catch((error)=>{
    const status=document.querySelector("#status");
    if(status) statusText(status,String(error?.message??error),"error");
  });
}
