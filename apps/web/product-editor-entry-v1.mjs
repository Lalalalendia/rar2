import { BrowserObservabilityV1 } from "./observability-v1.mjs";
import { ChapteraProductEditorServiceV1 } from "./chaptera-product-editor-service-v1.mjs";
import { RichReaderEditorShellV1 } from "./rich-reader-editor-shell-v1.mjs";
import { ProductSafePreviewImageAdmissionV1 } from "./web-resource-image-activation-v1.mjs";
import { diffObservedTextToScalarReplacement } from "./text-protocol-v1.mjs";
import {
  canonicalStorySelectionFromTextarea,
  prepareStoryTextForTextarea,
  restoreStoryTextFromTextarea,
  textareaSelectionFromCanonicalStory,
} from "./story-line-endings-v1.mjs";


const HAS_CONTROL = /\p{Cc}/u;

export function storyTextForTextarea(canonicalText) {
  const profile = prepareStoryTextForTextarea(canonicalText);
  if (!profile.value.length) throw new Error("first Story writer requires non-empty text");
  return profile.value;
}

export function textareaTextToCanonicalStory(canonicalText, observedText) {
  if (typeof observedText !== "string") throw new TypeError("observed Story text is required");
  if ([...observedText].some(char => char !== "\n" && HAS_CONTROL.test(char))) {
    throw new Error("first Story writer admits no new paragraphs/control characters");
  }
  const profile = prepareStoryTextForTextarea(canonicalText);
  if (!profile.value.length) throw new Error("first Story writer requires non-empty text");
  const newlineCount = text => [...text].filter(char => char === "\n").length;
  if (newlineCount(observedText) !== newlineCount(profile.value)) {
    throw new Error("first Story writer admits no new paragraphs/control characters");
  }
  return restoreStoryTextFromTextarea(observedText, profile);
}

export function sourcePlainStoryCandidates(graph) {
  if (!graph || typeof graph !== "object") return [];
  const frames = new Set();
  const tableOwned = new Set();
  for (const node of Object.values(graph.nodes ?? {})) {
    const frame = node?.payload?.story_frame?.story_id;
    if (typeof frame === "string") frames.add(frame);
    for (const owner of [node?.payload?.table_story, node?.payload?.table]) {
      if (typeof owner?.story_id === "string") tableOwned.add(owner.story_id);
    }
  }
  return Object.entries(graph.stories ?? {}).filter(([id, story]) =>
    typeof id === "string" && frames.has(id) && !tableOwned.has(id) &&
    typeof story?.text === "string" && (() => {
      try { storyTextForTextarea(story.text); return true; } catch { return false; }
    })() &&
    ["runs", "paragraphs", "fields", "hyperlinks"].every((part) =>
      !Array.isArray(story[part]) || story[part].length === 0
    )
  ).map(([story_id, story]) => ({story_id, text: story.text}))
    .sort((a, b) => a.story_id.localeCompare(b.story_id));
}

export function oneShotStoryIntent({
  documentId, sourceHash, baseRevisionId, storyId, canonicalText, observedText,
  clientOperationId,
}) {
  const desiredCanonicalText = textareaTextToCanonicalStory(canonicalText, observedText);
  const range = diffObservedTextToScalarReplacement(canonicalText, desiredCanonicalText);
  if (canonicalText === desiredCanonicalText) throw new Error("Story has no text change to save");
  if (range.replacement_text.length > 8192 || HAS_CONTROL.test(range.replacement_text)) {
    throw new Error("first Story writer admits no new paragraphs/control characters");
  }
  return {
    protocol_version: "chaptera.story-range-intent.v1",
    document_id: documentId,
    source_hash: sourceHash,
    base_revision_id: baseRevisionId,
    client_operation_id: clientOperationId,
    command: {
      kind: "replace_story_range",
      story_id: storyId,
      start_scalar: range.start_scalar,
      end_scalar: range.end_scalar,
      replacement_text: range.replacement_text,
    },
  };
}

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
  if (state.reason === "nudge_blocked_during_save") return { status: "Save still pending — retry the arrow key after confirmation", kind: "pending", fidelity };
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
  const scrollContainer=documentObject.querySelector("#canvas-wrap");
  const undoButton=documentObject.querySelector("#undo-action");
  const redoButton=documentObject.querySelector("#redo-action");
  const recheckButton=documentObject.querySelector("#reconcile-action");
  if(!status||!fidelity||!identity||!host||!scrollContainer) throw new Error("Chaptera Editor shell DOM is incomplete");
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

  // Admit host-verified inline SafePreview images only for the current page.
  // Without a verified derivative identity, the Reader painter stays unchanged.
  const resourceImages=new ProductSafePreviewImageAdmissionV1({
    host, scroll_container:scrollContainer, document_object:documentObject,
    crypto_object:cryptoObject,
  });
  globalThis.window?.addEventListener?.("pagehide",(event)=>{
    if(event.persisted!==true) resourceImages.dispose();
  });
  let shell=null;
  shell=new RichReaderEditorShellV1({
    resourceImages,
    host,
    service,
    keyboardTarget:documentObject,
    operationIdFactory:()=> "product-move-"+cryptoObject.randomUUID(),
    historyOperationIdFactory:()=> "product-history-"+cryptoObject.randomUUID(),
    onState:(state)=>{
      const view=productEditorStatusView(state,shell?.readerScene);
      statusText(status,view.status,view.kind);
      fidelity.textContent=view.fidelity;
      const mutationLocked=state.external_mutation_locked===true||
        state.history_in_flight===true||state.commit_in_flight===true||
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
      void shell[command]().then(()=>{
        // History owns a new canonical revision. Re-read the Story panel from
        // that revision instead of leaving browser-local text visible.
        documentObject.querySelector("#story-load-action")?.click();
      }).catch(()=>{});
    });
  }

  // User-visible first Story lane: a single server-backed text edit, not an
  // independent browser layout engine. It never modifies Reader SVG text.
  const storyLoad=documentObject.querySelector("#story-load-action");
  const storySelect=documentObject.querySelector("#story-select");
  const storyInput=documentObject.querySelector("#story-input");
  const storyApply=documentObject.querySelector("#story-apply-action");
  const storyReload=documentObject.querySelector("#story-reload-action");
  const storyStatus=documentObject.querySelector("#story-status");
  if(storyLoad&&storySelect&&storyInput&&storyApply&&storyStatus&&storyReload){
    let context=null;
    let candidates=[];
    let textBusy=false;
    let textUnconfirmed=false;
    let composing=false;

    const textState=(message,kind="")=>{
      storyStatus.textContent=message;
      storyStatus.className=kind;
    };
    const selectStory=(storyId)=>{
      const selected=candidates.find((candidate)=>candidate.story_id===storyId);
      if(!selected||!context) return;
      context={...context,story_id:selected.story_id,canonical_text:selected.text};
      storyInput.value=storyTextForTextarea(selected.text);
      storyInput.disabled=false;
      storyApply.disabled=false;
    };
    const lockDocument=(locked)=>{
      shell.setExternalMutationLocked(locked);
      storyLoad.disabled=locked;
      storySelect.disabled=locked||candidates.length===0;
      storyInput.disabled=locked||!context?.story_id;
      storyApply.disabled=locked||!context?.story_id;
      if(undoButton) undoButton.disabled=locked;
      if(redoButton) redoButton.disabled=locked;
    };

    storyInput.addEventListener("compositionstart",()=>{composing=true;});
    storyInput.addEventListener("compositionend",()=>{composing=false;});
    storySelect.addEventListener("change",()=>{
      if(textBusy||textUnconfirmed) return;
      selectStory(storySelect.value);
    });

    storyLoad.addEventListener("click",()=>{
      if(textBusy||textUnconfirmed||shell.commitInFlight||shell.historyInFlight||
          shell.commitUncertain||shell.historyUncertain||
          shell.commitRecoveryInFlight||shell.historyRecoveryInFlight) return;
      void (async()=>{
        textBusy=true;
        storyLoad.disabled=true;
        textState("Loading canonical Story list…");
        try{
          const current=await service.currentDocument();
          if(current.document_id!==documentId ||
              current.source_hash!==shell.readerScene?.source_hash ||
              current.revision_id!==shell.readerScene?.revision_id){
            throw new Error("Server revision differs from current Reader Scene; reload");
          }
          candidates=sourcePlainStoryCandidates(current.authoring_graph);
          context={
            document_id:documentId,
            source_hash:current.source_hash,
            revision_id:current.revision_id,
          };
          storySelect.replaceChildren();
          for(const candidate of candidates){
            const option=documentObject.createElement("option");
            option.value=candidate.story_id;
            option.textContent=candidate.story_id;
            storySelect.append(option);
          }
          if(candidates.length){
            selectStory(candidates[0].story_id);
            storySelect.disabled=false;
            textState("Canonical Story loaded; one plain edit per revision.");
          }else{
            storyInput.value="";
            storyInput.disabled=true;
            storyApply.disabled=true;
            textState("No supported plain Story with an ordinary frame in this PUB.");
          }
        }catch(error){
          context=null;
          candidates=[];
          storyInput.disabled=true;
          storyApply.disabled=true;
          storySelect.disabled=true;
          textState("Story load not confirmed: "+String(error?.message??error),"error");
        }finally{
          textBusy=false;
          if(!textUnconfirmed) storyLoad.disabled=false;
        }
      })();
    });

    storyApply.addEventListener("click",()=>{
      if(textBusy||textUnconfirmed||!context?.story_id||composing) return;
      let intent;
      let canonicalSelection;
      try{
        canonicalSelection=canonicalStorySelectionFromTextarea(
          context.canonical_text,
          storyInput.value,
          storyInput.selectionStart,
          storyInput.selectionEnd,
          storyInput.selectionDirection||"none",
        );
        intent=oneShotStoryIntent({
          documentId,
          sourceHash:context.source_hash,
          baseRevisionId:context.revision_id,
          storyId:context.story_id,
          canonicalText:context.canonical_text,
          observedText:storyInput.value,
          clientOperationId:"product-story-"+cryptoObject.randomUUID(),
        });
        lockDocument(true);
      }catch(error){
        textState(String(error?.message??error),"error");
        return;
      }
      void (async()=>{
        textBusy=true;
        textState("Submitting text intent and checking exact server revision…");
        statusText(status,"Saving Story text…","pending");
        try{
          const accepted=await service.commitStoryRange(intent);
          if(accepted.protocol_version!=="chaptera.commit-accepted.v1"){
            throw new Error("Story edit rejected: "+String(accepted.code??"unknown"));
          }
          const current=await service.currentDocument();
          if(current.document_id!==documentId ||
              current.source_hash!==intent.source_hash ||
              current.revision_id!==accepted.revision_id){
            throw new Error("Story ACK differs from exact durable current revision");
          }
          const scene=await service.readerSceneForRevision(accepted.revision_id);
          const actual=current.authoring_graph?.stories?.[context.story_id]?.text;
          const desired=textareaTextToCanonicalStory(context.canonical_text,storyInput.value);
          if(actual!==desired){
            throw new Error("Reopened Story text differs from submitted text");
          }
          await shell.loadScenes(scene,undefined,{preserveSelection:false});
          candidates=sourcePlainStoryCandidates(current.authoring_graph);
          const confirmed=candidates.find((item)=>item.story_id===context.story_id);
          if(!confirmed) throw new Error("Reopened Story is no longer editable");
          context={
            document_id:documentId,source_hash:current.source_hash,
            revision_id:current.revision_id,story_id:confirmed.story_id,
            canonical_text:confirmed.text,
          };
          storyInput.value=storyTextForTextarea(confirmed.text);
          lockDocument(false);
          const selection=textareaSelectionFromCanonicalStory(
            confirmed.text,
            canonicalSelection.start_scalar,
            canonicalSelection.end_scalar,
            canonicalSelection.direction,
          );
          storyInput.focus();
          storyInput.setSelectionRange(
            selection.start_utf16,selection.end_utf16,selection.direction,
          );
          textState(
            "Exact server revision and reopened Story text confirmed; canonical selection restored.",
            "ok",
          );
        }catch(error){
          // A POST or post-ACK Scene failure may have happened after a durable
          // commit. Preserve the operation and disallow all new mutations.
          textUnconfirmed=true;
          storyReload.hidden=false;
          storySelect.disabled=true;
          storyInput.disabled=true;
          storyApply.disabled=true;
          storyLoad.disabled=true;
          textState("Text outcome not confirmed — editor locked. Reload to reconcile. "+
            String(error?.message??error),"error");
          statusText(status,"Story save not confirmed — editing locked","error");
        }finally{
          textBusy=false;
        }
      })();
    });
    storyReload.addEventListener("click",()=>{
      // The page's fresh authenticated open owns recovery; no duplicate POST.
      locationObject.reload?.();
    });
  }
  return {kind:"ready",document_id:documentId,shell,service,resourceImages};
}

if(typeof window!=="undefined"&&typeof document!=="undefined"){
  bootProductEditor().catch((error)=>{
    const status=document.querySelector("#status");
    if(status) statusText(status,String(error?.message??error),"error");
  });
}
