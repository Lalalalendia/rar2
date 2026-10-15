// Real Publisher/CreateTextBox acceptance: Chromium pointer -> Caddy TLS ->
// Chaptera Guest HTTP -> isolated Rust EditorSession -> physical IDML/ODG.
// Independent native consumers read the exact bytes downloaded by Chromium.
// No synthetic backend, no browser-minted NodeId, no native .pub writer.
import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {createHash} from "node:crypto";
import {readFile, writeFile, mkdir} from "node:fs/promises";
import {dirname, resolve} from "node:path";
import {chromium} from "playwright";

function arg(name) {
  const i = process.argv.indexOf("--" + name);
  if (i < 0 || !process.argv[i+1]) throw new Error("missing --" + name);
  return process.argv[i+1];
}
const origin = arg("origin");
const fixture = resolve(arg("fixture"));
const expectedSourceHash = arg("fixture-sha256");
const receiptPath = resolve(arg("out"));
const target = arg("target");
assert.match(origin, /^https:\/\/reader\.chaptera\.test:\d+$/);
assert.match(expectedSourceHash,/^[0-9a-f]{64}$/);
assert.ok(["idml","odg"].includes(target));
const sha256 = x => createHash("sha256").update(x).digest("hex");
const original = await readFile(fixture);
assert.equal(original.length, 291840, "pinned real Publisher source byte length changed");
assert.equal(sha256(original),expectedSourceHash,"real PUB input source SHA differs");
const browser = await chromium.launch({
  headless:true, args:[
    "--no-proxy-server",
    "--host-resolver-rules=MAP reader.chaptera.test 127.0.0.1",
  ],
});
const page = await browser.newPage({
  viewport:{width:1280,height:900}, acceptDownloads:true, ignoreHTTPSErrors:true,
});
const observed = { current:[],scenes:[],accepted:[],storyAccepted:[],reports:[],requests:[],responses:[] };
const pending = [],errors=[],safeSceneFailureCodes=[];
let phase = "ui_open";
let storyConsumerFailure = null;
page.on("pageerror",error => errors.push(error.name || "PageError"));
page.on("request",request => {
  if (!request.url().startsWith(origin)) return;
  observed.requests.push({
    url:request.url(),path:new URL(request.url()).pathname,method:request.method(),
  });
});
page.on("response", response => {
  if (!response.url().startsWith(origin)) return;
  const url = new URL(response.url()),path=url.pathname;
  observed.responses.push({
    path:path.replace(/tempedit:[0-9a-f]{32}/g,"{session}"),
    status:response.status(),
  });
  if (response.status() !== 200) {
    // Only narrow server error codes for the Scene endpoint are admissible
    // as public CI diagnostics; never copy URLs, headers, arbitrary error
    // strings, auth tokens or source/story payloads into artifacts.
    if (/\/v1\/editor\/temporary-sessions\/[^/]+\/scene$/.test(path)) {
      pending.push(response.json().then(body => {
        const code=body?.error?.code;
        if (typeof code==="string" &&
            /^temporary_editor_[a-z0-9_]{1,72}$/.test(code)) {
          safeSceneFailureCodes.push(code);
        }
      }).catch(()=>{}));
    }
    return;
  }
  let bucket;
  if (/\/v1\/editor\/temporary-sessions\/[^/]+\/current$/.test(path)) {
    bucket=observed.current;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/scene$/.test(path)) {
    bucket=observed.scenes;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/create-textbox$/.test(path)) {
    bucket=observed.accepted;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/story-range$/.test(path)) {
    bucket=observed.storyAccepted;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/exports\/(?:idml|odg|pdf)$/.test(path)
      && url.searchParams.get("report_only")==="true") {
    bucket=observed.reports;
  }
  if(bucket)pending.push(response.json().then(data=>bucket.push(data)));
});
async function settle() {
  const results=await Promise.allSettled(pending.splice(0));
  assert.ok(results.every(r=>r.status==="fulfilled"),"real Caddy guest response JSON invalid");
}
function path(action,id) {
  return "/v1/editor/temporary-sessions/" + id + "/" + action;
}

const SAFE_STORY_CONSUMER_CODES = new Set([
  "zip_header_or_size", "zip_member_limits", "zip_path_traversal", "zip_crc",
  "idml_story_parts_missing", "odg_parts_missing", "odg_mimetype",
  "story_text_not_in_package", "native_apps_unavailable",
  "scribus_importer_missing", "scribus_import_timeout",
  "scribus_reimport_failed", "scribus_story_missing",
  "libreoffice_import_timeout", "libreoffice_reimport_failed",
  "libreoffice_story_missing", "native_story_unconfirmed",
  "consumer_assertion_other", "malformed_export_xml_or_zip",
  "consumer_unexpected_error",
]);
function safeStoryConsumerFailure(output) {
  // Parse the *allowlisted* error code only. No raw Python/Scribus stderr can
  // leave the private GitHub worker, even when an application importer fails.
  try {
    const data = JSON.parse(String(output).trim().split("\n").at(-1));
    if (data.schema === "chaptera.tempedit-source-neutral-story-consumer-failure.v1" &&
        data.status === "FAIL" && SAFE_STORY_CONSUMER_CODES.has(data.failure_code)) {
      return data.failure_code;
    }
  } catch {}
  return "consumer_unclassified";
}
function classifiedCreateReopenFailure() {
  // Only safe contract booleans are returned. Never persist raw response
  // objects, Story text, capability, source paths, IDs or request URLs.
  const ack=observed.accepted.at(-1);
  if (!ack) return "create_ack_missing";
  // Later StoryRange edits advance the revision and the latest operation
  // legitimately becomes replace_story_range. Classify against the exact
  // CreateTextBox revision, not whichever revision failed downstream.
  const current=observed.current.find(item=>item.revision_id===ack.revision_id);
  if (!current) return "reopened_create_revision_missing";
  const op=ack.canonical_operation,latest=current.project?.operations?.at(-1);
  if (!op || !latest) return "canonical_operation_missing";
  if (latest.kind!=="create_text_box" || op.kind!=="create_text_box") {
    return "operation_kind_mismatch";
  }
  if (current.revision_id!==ack.revision_id) return "revision_identity_mismatch";
  if (current.canonical_authoring_revision_id!==ack.canonical_authoring_revision_id) {
    return "authoring_revision_mismatch";
  }
  if (current.revision_cursor!==1) return "cursor_mismatch";
  if (current.expires_at_ms!==ack.expires_at_ms) return "ttl_identity_mismatch";
  if (latest.node_id!==op.node_id || latest.story_id!==op.story_id) {
    return "operation_id_mismatch";
  }
  if (latest.page_id!==op.page_id) return "operation_page_mismatch";
  if (JSON.stringify(latest.bounds)!==JSON.stringify(op.bounds)) {
    return "operation_bounds_mismatch";
  }
  if (JSON.stringify(latest.text_preset)!==JSON.stringify(op.text_preset)) {
    return "operation_font_mismatch";
  }
  if (current.authoring_graph?.nodes?.[op.node_id]?.payload?.story_frame?.story_id!==op.story_id) {
    return "authored_frame_story_link_missing";
  }
  if (current.authoring_graph?.stories?.[op.story_id]?.text!=="") {
    return "authored_empty_story_missing";
  }
  return "client_validation_or_scene_pending";
}

try {
  const entry = await page.goto(origin+"/editor/temporary-preview",{
    waitUntil:"domcontentloaded",timeout:30000,
  });
  assert.equal(entry?.status(),200,"real TLS guest editor page unavailable");
  await page.locator("#temporary-file").setInputFiles(fixture);
  await page.locator("#temporary-start").click();
  phase = "pub_ingress";
  try {
    await page.waitForFunction(() =>
      /(?:PUB открыт|Открыта редактируемая ревизия)/.test(
        document.querySelector("#temporary-status")?.textContent ?? ""),
      null,{timeout:170000});
  } catch {
    const status=await page.locator("#temporary-status").textContent();
    throw new Error("real PUB open failed: UI "+String(status).slice(0,200)+
      " http="+JSON.stringify(observed.responses.slice(-12))+
      " page_errors="+JSON.stringify(errors));
  }
  await settle();
  const baseline=observed.current.at(-1),scene0=observed.scenes.at(-1)?.scene;
  assert.equal(baseline?.protocol_version,"chaptera.temporary-editor-current.v1");
  assert.equal(baseline.source_hash,expectedSourceHash);
  assert.equal(baseline.revision_cursor,0);
  assert.equal(scene0?.source_hash,expectedSourceHash);
  assert.equal(scene0?.revision_id,baseline.revision_id);
  const eligible=scene0.pages.find(p=>
    baseline.authoring_graph?.pages?.[p.page_id] &&
    p.width_emu>2_000_000 && p.height_emu>2_000_000);
  assert.ok(eligible,"real Publisher source offers no canonically authorable page");
  phase = "draw_tool";
  const drawButton=page.locator("#temporary-draw-textbox");
  await drawButton.waitFor({state:"visible",timeout:10000});
  assert.equal(await drawButton.isEnabled(),true,"Guest TextFrame tool did not admit source page");
  await drawButton.click();
  assert.equal(await drawButton.getAttribute("aria-pressed"),"true");

  const svg = page.locator('svg.page[data-page-id="'+eligible.page_id+'"]').first();
  await svg.scrollIntoViewIfNeeded({timeout:10000});
  phase = "pointer_geometry";
  const positions=await page.evaluate(pageId=>{
    const svg=[...document.querySelectorAll("#temporary-pages svg.page")]
      .find(s=>s.getAttribute("data-page-id")===pageId);
    if (!svg||!svg.getScreenCTM()) return null;
    const view=svg.getAttribute("viewBox").trim().split(/\s+/).map(Number);
    if (view.length!==4||view[2]<=0||view[3]<=0) return null;
    function screen(fx,fy) {
      const point=svg.createSVGPoint();point.x=view[2]*fx;point.y=view[3]*fy;
      const pos=point.matrixTransform(svg.getScreenCTM());
      return {x:pos.x,y:pos.y};
    }
    const start=screen(.18,.20),finish=screen(.78,.41);
    if (![start.x,start.y,finish.x,finish.y].every(Number.isFinite) ||
        Math.max(start.x,finish.x)>=innerWidth ||
        Math.max(start.y,finish.y)>=innerHeight || start.x<=0 || start.y<=0) return null;
    const element=document.elementFromPoint(start.x,start.y);
    return element?.closest?.("svg.page")===svg ? {start,finish} : null;
  },eligible.page_id);
  assert.ok(positions,"real Publisher page-local pointer coordinates are unavailable");
  phase = "pointer_commit";
  await page.mouse.move(positions.start.x,positions.start.y);
  await page.mouse.down();
  await page.mouse.move(positions.finish.x,positions.finish.y,{steps:7});
  await page.mouse.up();
  try {
    await page.waitForFunction(() =>
      document.querySelector("#temporary-status")?.textContent.includes(
        "Новая рамка подтверждена сервером"),null,{timeout:60000});
  } catch {
    const status=await page.locator("#temporary-status").textContent();
    throw new Error("real CreateTextBox rejected: "+String(status).slice(0,200)+
      " http="+JSON.stringify(observed.responses.slice(-14))+
      " page_errors="+JSON.stringify(errors));
  }
  await settle();
  phase = "create_contract";
  const ack=observed.accepted.at(-1),now=observed.current.at(-1),envelope=observed.scenes.at(-1);
  assert.equal(ack?.protocol_version,"chaptera.temporary-editor-create-textbox.v1");
  assert.equal(ack.source_hash,expectedSourceHash);
  assert.equal(ack.document_id,baseline.document_id);
  assert.equal(ack.base_revision_id,baseline.revision_id);
  assert.equal(ack.replayed,false);
  assert.equal(now?.revision_id,ack.revision_id);
  assert.equal(now.revision_cursor,1);
  assert.equal(now.expires_at_ms,ack.expires_at_ms);
  assert.equal(envelope?.revision_id,ack.revision_id);
  const operation=ack.canonical_operation;
  assert.equal(operation?.kind,"create_text_box");
  assert.match(operation.node_id,/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.match(operation.story_id,/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.notEqual(operation.node_id,operation.story_id);
  assert.equal(operation.page_id,eligible.page_id);
  assert.ok(operation.bounds.width>0 && operation.bounds.height>0);
  assert.equal(operation.text_preset?.font_size_emu,114300);
  assert.deepEqual(now.project?.operations?.at(-1),operation);
  assert.equal(now.authoring_graph?.nodes?.[operation.node_id]?.payload?.story_frame?.story_id,
    operation.story_id,"canonical new TextFrame is not connected to its Story");
  assert.equal(now.authoring_graph?.stories?.[operation.story_id]?.text,"");
  const scene1=envelope.scene;
  const node=scene1.nodes.find(n=>n.node_id===operation.node_id);
  assert.ok(node,"real source-neutral ReaderScene omitted authored TextFrame");
  assert.equal(node.page_id,eligible.page_id);
  assert.deepEqual(node.bounds,operation.bounds);
  const select=page.locator("#temporary-story-select");
  assert.equal(await select.inputValue(),operation.story_id,
    "new Story is not selected in the actual browser UI");
  const editor=page.locator("#temporary-story-text");
  assert.equal(await editor.inputValue(),"","new Story textarea is not initially empty");

  // Physical user edit: exactly one additional source-preserving StoryRange
  // commit to the freshly authored empty Story. No direct browser API POST.
  phase = "story_edit";
  const textMarker="ChapteraNewTextBoxProof42";
  await page.waitForTimeout(5400);
  await editor.fill(textMarker);
  const textAckResponse=page.waitForResponse(response=>
    response.request().method()==="POST" &&
    new URL(response.url()).pathname===path("story-range",baseline.document_id),
    {timeout:120000});
  await page.locator("#temporary-story-apply").click();
  const confirmedText=await textAckResponse;
  assert.equal(confirmedText.status(),200,
    "real authored StoryRange request was rejected: "+confirmedText.status());
  await page.waitForFunction(() =>
    document.querySelector("#temporary-status")?.textContent.includes(
      "Текст подтверждён сервером"),null,{timeout:120000});
  await settle();
  phase = "story_contract";
  const storyAck=observed.storyAccepted.at(-1);
  const textCurrent=observed.current.at(-1);
  const textScene=observed.scenes.at(-1)?.scene;
  assert.equal(storyAck?.protocol_version,"chaptera.temporary-editor-story-range.v1");
  assert.equal(storyAck.base_revision_id,ack.revision_id);
  assert.equal(storyAck.canonical_operation?.kind,"replace_story_range");
  assert.equal(storyAck.canonical_operation.story_id,operation.story_id);
  assert.equal(storyAck.canonical_operation.replacement_text,textMarker);
  assert.equal(textCurrent.revision_id,storyAck.revision_id);
  assert.equal(textCurrent.revision_cursor,2);
  assert.equal(textCurrent.authoring_graph?.stories?.[operation.story_id]?.text,textMarker);
  assert.ok(textScene?.stories?.some(story=>story.story_id===operation.story_id &&
    story.text===textMarker),"real Reader Scene did not project new Story text");

  // Existing trusted temporary exports consume the edited current revision.
  await page.waitForTimeout(7500);
  phase = "export";
  const button=page.locator("#temporary-download-"+target);
  await button.waitFor({state:"visible"});
  assert.equal(await button.isEnabled(),true,"created authoring did not unlock export");
  const downloadWait=page.waitForEvent("download",{timeout:90000});
  await button.click();
  const download=await downloadWait;
  await settle();
  assert.equal(download.suggestedFilename(),"chaptera-edited."+target);
  const artifactPath=await download.path(),artifact=await readFile(artifactPath);
  assert.ok(artifact.subarray(0,2).equals(Buffer.from("PK")),
    "real edited IDML/ODG did not have a ZIP header");
  const report=observed.reports.at(-1);
  assert.equal(report?.source_hash,expectedSourceHash);
  assert.equal(report?.revision_id,storyAck.revision_id);
  assert.equal(report?.canonical_authoring_revision_id,storyAck.canonical_authoring_revision_id);
  assert.equal(report?.target,target);
  assert.equal(report?.artifact_sha256,sha256(artifact));
  assert.equal(report.loss_report?.can_serialize,true);
  assert.equal(report.loss_report?.counts?.blocking,0);

  phase = "physical_consumer";
  const consumerCheck=spawnSync("python3",[
    "tools/verify_temporary_guest_story_export.py",
    "--artifact",artifactPath,
    "--target",target,
    "--expected-text",textMarker,
    "--real-application-consumer",
  ],{encoding:"utf-8",timeout:230000});
  if (consumerCheck.status !== 0) {
    storyConsumerFailure = safeStoryConsumerFailure(consumerCheck.stderr);
    throw new Error("source-neutral independent Story consumer failed");
  }
  const consumer=JSON.parse(consumerCheck.stdout.trim().split("\n").at(-1));
  assert.equal(consumer.schema,"chaptera.tempedit-independent-story-xml-consumer.v1");
  assert.equal(consumer.status,"PASS");
  assert.equal(consumer.artifact_sha256,sha256(artifact));
  assert.equal(consumer.edited_text_recovered,true);
  assert.equal(consumer.target_application_consumer?.reopened,true);
  assert.equal(consumer.target_application_consumer?.story_marker_retained,true);


  // IDML/ODG import cannot prove the newly authored Story is painted in the
  // PDF exported from this exact canonical R2 revision. Inspect the actual
  // Chromium-downloaded PDF through an independent physical text consumer.
  phase = "physical_pdf_consumer";
  const pdfDownloadWait=page.waitForEvent("download",{timeout:90000});
  await page.locator("#temporary-download-pdf").click();
  const pdfDownload=await pdfDownloadWait;
  await settle();
  assert.equal(pdfDownload.suggestedFilename(),"chaptera-edited.pdf");
  const pdfPath=await pdfDownload.path(),pdfBytes=await readFile(pdfPath);
  assert.ok(pdfBytes.subarray(0,5).equals(Buffer.from("%PDF-")),
    "new Story export did not have a physical PDF header");
  const pdfReport=observed.reports.at(-1);
  assert.equal(pdfReport?.target,"pdf","new Story PDF report missing");
  assert.equal(pdfReport.source_hash,expectedSourceHash);
  assert.equal(pdfReport.revision_id,storyAck.revision_id);
  assert.equal(pdfReport.canonical_authoring_revision_id,
    storyAck.canonical_authoring_revision_id);
  assert.equal(pdfReport.artifact_sha256,sha256(pdfBytes));
  assert.equal(pdfReport.loss_report?.can_serialize,true);
  assert.equal(pdfReport.loss_report?.counts?.blocking,0);
  const authoredPdf=spawnSync("python3",[
    "tools/verify_temporary_guest_story_pdf_export.py",
    "--artifact",pdfPath,"--expected-text",textMarker,
  ],{encoding:"utf-8",timeout:120000});
  assert.equal(authoredPdf.status,0,
    "new Story physically absent in same R2 PDF (independent text-paint proof): "+
    String(authoredPdf.stderr||authoredPdf.stdout).slice(-400));
  const pdfConsumer=JSON.parse(authoredPdf.stdout.trim().split("\n").at(-1));
  assert.equal(pdfConsumer.schema,
    "chaptera.tempedit-independent-authored-text-pdf-consumer.v1");
  assert.equal(pdfConsumer.status,"PASS");
  assert.equal(pdfConsumer.artifact_sha256,sha256(pdfBytes));
  assert.equal(pdfConsumer.authored_text_recovered,true);
  assert.equal(pdfConsumer.marker_page_count,1);

  phase = "reopen";
  await page.reload({waitUntil:"domcontentloaded"});
  await page.locator("#temporary-resume").click();
  await page.waitForFunction(nodeId=>
    !!document.querySelector('[data-node-id="'+nodeId+'"]'),operation.node_id,
    {timeout:80000});
  await settle();
  const reopened=observed.scenes.at(-1)?.scene;
  assert.equal(reopened?.revision_id,storyAck.revision_id);
  assert.deepEqual(reopened.nodes.find(n=>n.node_id===operation.node_id)?.bounds,operation.bounds);
  assert.ok(reopened.stories?.some(story=>story.story_id===operation.story_id &&
    story.text===textMarker),"reopened Scene lost authored Story");
  assert.equal(observed.current.at(-1)?.expires_at_ms,storyAck.expires_at_ms,
    "GET/reload must not renew the 60 minute sliding TTL");
  assert.equal(sha256(await readFile(fixture)),expectedSourceHash,
    "the Guest authored textbox changed the immutable source PUB");
  const writes=observed.requests.filter(req=>
    req.path===path("create-textbox",baseline.document_id) && req.method==="POST");
  assert.equal(writes.length,1,"pointer created more than one canonical CreateTextBox operation");
  const storyWrites=observed.requests.filter(req=>req.path===path("story-range",baseline.document_id) &&
    req.method==="POST");
  assert.equal(storyWrites.length,1,"typing caused more than one canonical StoryRange edit");
  const cap=await page.evaluate(()=>{
    const record=sessionStorage.getItem("chaptera.temporary-edit.v0");
    return record ? JSON.parse(record).access_token : null;
  });
  assert.match(cap,/^[0-9a-f]{64}$/);
  assert.ok(observed.requests.every(req=>!req.url.includes(cap)),
    "private bearer leaked into a request URL");
  assert.equal(errors.length,0,"Chromium emitted an uncaught page error");

  phase = "revoke";
  page.once("dialog",dialog=>dialog.accept());
  await page.locator("#temporary-delete").click();
  await page.waitForFunction(()=>
    document.querySelector("#temporary-status")?.textContent.includes(
      "Сессия удалена с сервера"),null,{timeout:45000});
  const denied=await page.evaluate(async ({id,cap})=>{
    let code=429;
    for (let k=0;k<8;k++){
      const response=await fetch("/v1/editor/temporary-sessions/"+id+"/current",{
        method:"GET",credentials:"omit",cache:"no-store",
        headers:{"x-chaptera-reader-session":cap},
      });
      code=response.status;
      if(code!==429)break;
      await new Promise(done=>setTimeout(done,2200));
    }
    return code;
  },{id:baseline.document_id,cap});
  assert.ok(denied===404||denied===410,"deleted capability can still access current");
  await page.reload({waitUntil:"domcontentloaded"});
  assert.equal(await page.evaluate(()=>
    sessionStorage.getItem("chaptera.temporary-edit.v0")),null);
  assert.equal(await page.locator("#temporary-resume").isHidden(),true);
  phase = "receipt";
  const receipt={
    schema:"chaptera.tempedit-real-pub-create-textbox-chromium-https.v1",
    status:"PASS",source_sha256:expectedSourceHash,source_byte_len:original.length,
    real_pub:true,real_chromium:true,local_tls_caddy:true,
    real_isolated_rust_guest:true,browser_pointer_create_commits:writes.length,
    exact_current_reopened:true,exact_reader_scene_created_node:true,
    authoring_node_uuidv7:true,authoring_story_uuidv7:true,
    browser_story_commits:storyWrites.length,story_selected_after_create:true,
    created_story_exact_reopened:true,physical_target_xml_consumer:true,
    physical_new_story_pdf_text_consumer:true,changed_pdf_sha256:sha256(pdfBytes),
    target_application_reopened:consumer.target_application_consumer.reopened,
    target_application_consumer:consumer.target_application_consumer.application,
    export_target:target,changed_artifact_sha256:sha256(artifact),
    original_pub_immutable:true,read_replay_export_no_ttl_extension:true,
    private_bearer_not_in_urls:true,same_browser_delete_and_revoke:true,
    loss_report_checked:true,v0_go:false,
  };
  await mkdir(dirname(receiptPath),{recursive:true});
  await writeFile(receiptPath,JSON.stringify(receipt,null,2)+"\n");
  console.log(JSON.stringify({schema:receipt.schema,status:"PASS",
    target,real_pub_browser_https:true,physical_target_consumer:true,
    artifact_sha256:receipt.changed_artifact_sha256}));
} catch (error) {
  // Fail-closed physical evidence: do not expose browser stderr, server URLs,
  // private bearer, Story text, raw headers or foreign source identifiers.
  const endpointStatuses = suffix => observed.responses
    .filter(x => x.path?.endsWith(suffix) && Number.isInteger(x.status))
    .slice(-8).map(x => x.status);
  const diagnostic = {
    schema: "chaptera.tempedit-physical-textbox-failure.v1",
    status: "FAIL",
    phase,
    export_target: target,
    failure_class: error?.name === "AssertionError" ? "assertion"
      : error?.name === "TimeoutError" ? "timeout" : "other",
    create_reopen_contract: classifiedCreateReopenFailure(),
    story_consumer_failure_code: storyConsumerFailure,
    browser_page_error_count: Math.min(100, errors.length),
    current_success_replies: observed.current.length,
    scene_success_replies: observed.scenes.length,
    create_ack_replies: observed.accepted.length,
    story_ack_replies: observed.storyAccepted.length,
    create_request_count: observed.requests.filter(x =>
      x.method === "POST" && x.path.endsWith("/create-textbox")).length,
    story_request_count: observed.requests.filter(x =>
      x.method === "POST" && x.path.endsWith("/story-range")).length,
    create_statuses: endpointStatuses("/create-textbox"),
    story_statuses: endpointStatuses("/story-range"),
    scene_statuses: endpointStatuses("/scene"),
    current_statuses: endpointStatuses("/current"),
    scene_request_count: observed.requests.filter(x =>
      x.method === "GET" && x.path.endsWith("/scene")).length,
    safe_scene_failure_codes: safeSceneFailureCodes.slice(-8),
  };
  try {
    await mkdir(dirname(receiptPath), {recursive:true});
    await writeFile(receiptPath, JSON.stringify(diagnostic,null,2)+"\n");
  } catch {
    // Artifact publication has its own fail-closed gate; no raw fallback.
  }
  throw new Error("source-neutral real CreateTextBox acceptance failed at "+phase);
} finally {
  await browser.close();
}
