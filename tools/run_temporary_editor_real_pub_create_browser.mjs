// Real Publisher/CreateShape acceptance: Chromium pointer -> Caddy TLS ->
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
const observed = { current:[],scenes:[],accepted:[],reports:[],requests:[],responses:[] };
const pending = [],errors=[];
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
  if (response.status() !== 200) return;
  let bucket;
  if (/\/v1\/editor\/temporary-sessions\/[^/]+\/current$/.test(path)) {
    bucket=observed.current;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/scene$/.test(path)) {
    bucket=observed.scenes;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/create-shape$/.test(path)) {
    bucket=observed.accepted;
  } else if (/\/v1\/editor\/temporary-sessions\/[^/]+\/exports\/(?:idml|odg)$/.test(path)
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

try {
  const entry = await page.goto(origin+"/editor/temporary-preview",{
    waitUntil:"domcontentloaded",timeout:30000,
  });
  assert.equal(entry?.status(),200,"real TLS guest editor page unavailable");
  await page.locator("#temporary-file").setInputFiles(fixture);
  await page.locator("#temporary-start").click();
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
  const drawButton=page.locator("#temporary-draw-rectangle");
  await drawButton.waitFor({state:"visible",timeout:10000});
  assert.equal(await drawButton.isEnabled(),true,"Guest Rectangle tool did not admit source page");
  await drawButton.click();
  assert.equal(await drawButton.getAttribute("aria-pressed"),"true");

  const svg = page.locator('svg.page[data-page-id="'+eligible.page_id+'"]').first();
  await svg.scrollIntoViewIfNeeded({timeout:10000});
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
    const start=screen(.2,.22),finish=screen(.33,.32);
    if (![start.x,start.y,finish.x,finish.y].every(Number.isFinite) ||
        Math.max(start.x,finish.x)>=innerWidth ||
        Math.max(start.y,finish.y)>=innerHeight || start.x<=0 || start.y<=0) return null;
    const element=document.elementFromPoint(start.x,start.y);
    return element?.closest?.("svg.page")===svg ? {start,finish} : null;
  },eligible.page_id);
  assert.ok(positions,"real Publisher page-local pointer coordinates are unavailable");
  await page.mouse.move(positions.start.x,positions.start.y);
  await page.mouse.down();
  await page.mouse.move(positions.finish.x,positions.finish.y,{steps:7});
  await page.mouse.up();
  try {
    await page.waitForFunction(() =>
      document.querySelector("#temporary-status")?.textContent.includes(
        "Прямоугольник сохранён сервером"),null,{timeout:60000});
  } catch {
    const status=await page.locator("#temporary-status").textContent();
    throw new Error("real CreateShape rejected: "+String(status).slice(0,200)+
      " http="+JSON.stringify(observed.responses.slice(-14))+
      " page_errors="+JSON.stringify(errors));
  }
  await settle();
  const ack=observed.accepted.at(-1),now=observed.current.at(-1),envelope=observed.scenes.at(-1);
  assert.equal(ack?.protocol_version,"chaptera.temporary-editor-create-shape.v1");
  assert.equal(ack.source_hash,expectedSourceHash);
  assert.equal(ack.document_id,baseline.document_id);
  assert.equal(ack.base_revision_id,baseline.revision_id);
  assert.equal(ack.replayed,false);
  assert.equal(now?.revision_id,ack.revision_id);
  assert.equal(now.revision_cursor,1);
  assert.equal(now.expires_at_ms,ack.expires_at_ms);
  assert.equal(envelope?.revision_id,ack.revision_id);
  const operation=ack.canonical_operation;
  assert.equal(operation?.kind,"create_shape");
  assert.match(operation.node_id,/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.equal(operation.page_id,eligible.page_id);
  assert.equal(operation.parent_id,eligible.page_id);
  assert.equal(operation.shape_kind,"rectangle");
  assert.equal(operation.paint.fill.visible,true);
  assert.equal(operation.paint.stroke.visible,true);
  assert.deepEqual(operation.paint.fill.color,{r:18,g:52,b:86});
  assert.deepEqual(operation.paint.stroke.color,{r:170,g:187,b:204});
  assert.equal(operation.paint.stroke.width_emu,12700);
  assert.ok(operation.bounds.width>0 && operation.bounds.height>0);
  const canonicalLast=now.project?.operations?.at(-1);
  assert.deepEqual(canonicalLast,operation,"reopened project must carry exact created operation");
  const scene1=envelope.scene;
  const node=scene1.nodes.find(n=>n.node_id===operation.node_id);
  assert.ok(node,"real source-neutral ReaderScene omitted authored Rectangle");
  assert.equal(node.page_id,eligible.page_id);
  assert.deepEqual(node.bounds,operation.bounds);
  assert.deepEqual(node.paint?.fill_rgb,[18,52,86]);
  assert.deepEqual(node.paint?.line?.rgb,[170,187,204]);
  assert.equal(node.paint?.line?.width_emu,12700);
  assert.equal(scene1.fidelity?.state,"partial");
  assert.equal(scene1.stacking_fidelity,"unknown");
  assert.ok(scene1.fidelity.reasons.includes("authored_rectangle_mixed_order_unsupported"));

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
  assert.equal(report?.revision_id,ack.revision_id);
  assert.equal(report?.canonical_authoring_revision_id,ack.canonical_authoring_revision_id);
  assert.equal(report?.target,target);
  assert.equal(report?.artifact_sha256,sha256(artifact));
  assert.equal(report.loss_report?.can_serialize,true);
  assert.equal(report.loss_report?.counts?.blocking,0);

  const consumerCheck=spawnSync("python3",[
    "tools/verify_temporary_guest_created_shape_export.py",
    "--artifact",artifactPath,
    "--target",target,
    "--node-id",operation.node_id,
    "--bounds-json",JSON.stringify(operation.bounds),
  ],{encoding:"utf-8",timeout:230000});
  assert.equal(consumerCheck.status,0,
    "independent " +target+" Rectangle geometry/native reader failed: "+
    (consumerCheck.stderr||consumerCheck.stdout).slice(-1400));
  const consumer=JSON.parse(consumerCheck.stdout.trim().split("\n").at(-1));
  assert.equal(consumer.schema,"chaptera.guest-created-shape-independent-consumer.v1");
  assert.equal(consumer.status,"PASS");
  assert.equal(consumer.artifact_sha256,sha256(artifact));
  assert.equal(consumer.node_id,operation.node_id);
  assert.equal(consumer.consumer?.reopened,true);

  await page.reload({waitUntil:"domcontentloaded"});
  await page.locator("#temporary-resume").click();
  await page.waitForFunction(nodeId=>
    !!document.querySelector('[data-node-id="'+nodeId+'"]'),operation.node_id,
    {timeout:80000});
  await settle();
  const reopened=observed.scenes.at(-1)?.scene;
  assert.equal(reopened?.revision_id,ack.revision_id);
  assert.deepEqual(reopened.nodes.find(n=>n.node_id===operation.node_id)?.bounds,operation.bounds);
  assert.equal(observed.current.at(-1)?.expires_at_ms,ack.expires_at_ms,
    "GET/reload must not renew the 60 minute sliding TTL");
  assert.equal(sha256(await readFile(fixture)),expectedSourceHash,
    "the Guest authored rectangle changed the immutable source PUB");
  const writes=observed.requests.filter(req=>
    req.path===path("create-shape",baseline.document_id) && req.method==="POST");
  assert.equal(writes.length,1,"pointer created more than one canonical RevisionStream operation");
  const cap=await page.evaluate(()=>{
    const record=sessionStorage.getItem("chaptera.temporary-edit.v0");
    return record ? JSON.parse(record).access_token : null;
  });
  assert.match(cap,/^[0-9a-f]{64}$/);
  assert.ok(observed.requests.every(req=>!req.url.includes(cap)),
    "private bearer leaked into a request URL");
  assert.equal(errors.length,0,"Chromium emitted an uncaught page error");

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
  const receipt={
    schema:"chaptera.tempedit-real-pub-create-shape-chromium-https.v1",
    status:"PASS",source_sha256:expectedSourceHash,source_byte_len:original.length,
    real_pub:true,real_chromium:true,local_tls_caddy:true,
    real_isolated_rust_guest:true,authoring_node_uuidv7:true,
    browser_pointer_create_commits:writes.length,
    exact_current_reopened:true,exact_reader_scene_created_node:true,
    partial_stacking_truthful:true,physical_target_xml_consumer:true,
    target_application_reopened:consumer.consumer.reopened,
    target_application_consumer:consumer.consumer.application,
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
} finally {
  await browser.close();
}
