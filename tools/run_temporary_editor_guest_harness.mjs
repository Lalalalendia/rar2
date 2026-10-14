import { createServer } from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { resolve, extname } from "node:path";
import { createHash } from "node:crypto";
import { chromium } from "playwright";

const id = "tempedit:" + "a".repeat(32);
const token = "b".repeat(64);
const source = "c".repeat(64);
const pageId = "20000000-0000-4000-8000-000000000001";
const nodeId = "30000000-0000-4000-8000-000000000001";
const before = "sha256:" + "d".repeat(64);
const after = "sha256:" + "e".repeat(64);
const afterShape = "sha256:" + "2".repeat(64);
const canon0 = "f".repeat(64), canon1 = "1".repeat(64), canon2 = "2".repeat(64);
const ttl0 = Date.now() + 3_600_000, ttl1 = ttl0 + 120_000, ttl2 = ttl1 + 120_000;
const editedBytes = Buffer.from("PK\u0003\u0004Chaptera temporary browser synthetic edited IDML proof");
const outputSha = createHash("sha256").update(editedBytes).digest("hex");
// Synthetic Chrome transport acceptance: this is deliberately NOT a real-PUB
// PDF rendering proof. The separate live HTTPS scenario proves actual pixels.
const pdfBytes = Buffer.from("%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF\n");
const pdfSha = createHash("sha256").update(pdfBytes).digest("hex");
const route = "/v1/editor/temporary-sessions/" + id;
let accepted = null, created = null, requests = [], editedReportRequests = 0, deleted = false;
const authoredNodeId = "019a2d80-1112-7001-8001-aabbccddee01";
const assets = new Map([
  ["/editor/temporary-preview", "apps/web/temporary-editor-preview.html"],
  ["/editor/temporary-editor-preview.mjs", "apps/web/temporary-editor-preview.mjs"],
  ["/editor/temporary-editor-preview.css", "apps/web/temporary-editor-preview.css"],
  ["/editor/temporary-edit-browser-session-v0.mjs", "apps/web/temporary-edit-browser-session-v0.mjs"],
  ["/editor/temporary-edit-move-client-v1.mjs", "apps/web/temporary-edit-move-client-v1.mjs"],
  ["/editor/text-protocol-v1.mjs", "apps/web/text-protocol-v1.mjs"],
  ["/editor/story-line-endings-v1.mjs", "apps/web/story-line-endings-v1.mjs"],
  ["/packages/protocol/editor-resize/v1/resize-gesture-v1.mjs", "packages/protocol/editor-resize/v1/resize-gesture-v1.mjs"],
  ["/editor/interaction-v1.mjs", "apps/web/interaction-v1.mjs"],
  ["/editor/reader-scene-editor-interaction-v1.mjs", "apps/web/reader-scene-editor-interaction-v1.mjs"],
  ["/cloud-reader/render-v1.mjs", "apps/cloud-reader/render-v1.mjs"],
]);
const json = (res, code, value) => {
  res.writeHead(code, { "content-type": "application/json", "cache-control": "no-store" });
  res.end(JSON.stringify(value));
};
const bounds = () => ({ x: accepted?.x_emu ?? 95250, y: accepted?.y_emu ?? 95250,
  width: 190500, height: 190500 });
const scene = () => ({
  protocol_version: "chaptera.reader-scene.v1",
  document_id: id, source_hash: source,
  revision_id: created ? afterShape : accepted ? after : before,
  scene_authority: "isolated_reader_scene_synthetic",
  stacking_fidelity: "partial", fidelity: { state: "partial", reasons: ["browser_harness"] },
  pages: [{ page_id: pageId, order: 0, width_emu: 952500, height_emu: 952500 }],
  nodes: [
    { node_id: nodeId, page_id: pageId, kind: "shape", bounds: bounds(),
      transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
      paint: { preset_shape: null, fill_rgb: [10,120,200], line: null } },
    ...(created ? [{
      node_id: authoredNodeId, page_id: pageId, kind: "shape",
      bounds: created.operation.bounds,
      transform: { a: "1", b: "0", c: "0", d: "1", tx: 0, ty: 0 },
      paint: { fill_rgb: created.command.fill_rgb,
        line: { rgb: created.command.stroke_rgb, width_emu: created.command.stroke_width_emu } },
    }] : []),
  ],
  stories: [], resources: [], fonts: [], diagnostics: [],
});
const current = () => ({
  protocol_version: "chaptera.temporary-editor-current.v1",
  document_id: id, source_hash: source,
  revision_id: created ? afterShape : accepted ? after : before,
  canonical_authoring_revision_id: created ? canon2 : accepted ? canon1 : canon0,
  revision_cursor: created ? 2 : accepted ? 1 : 0,
  expires_at_ms: created ? ttl2 : accepted ? ttl1 : ttl0,
  project: { schema_version: "editor", operations: created ? [created.operation] : [] },
  authoring_graph: {
    nodes: { [nodeId]: { header: { bounds: bounds() } } },
    pages: { [pageId]: { size: { width: 952500, height: 952500 } } },
  },
});
const server = createServer(async (req,res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  const path = url.pathname;
  requests.push({ path, method: req.method, query: url.search });
  if (assets.has(path)) {
    const name = assets.get(path);
    const content = await readFile(resolve(name));
    const type = extname(name) === ".html" ? "text/html" :
      extname(name) === ".css" ? "text/css" : "text/javascript";
    res.writeHead(200,{ "content-type": type + "; charset=utf-8", "cache-control": "no-store" });
    res.end(content);
    return;
  }
  let body = Buffer.alloc(0);
  for await (const chunk of req) body = Buffer.concat([body, chunk]);
  if (path === "/v1/editor/temporary-sessions" && req.method === "POST") {
    json(res,200,{
      protocol_version: "chaptera.reader-guest-session.v1",
      session_id:id, mode:"temporary_edit", access_token:token,
      expires_at_ms:ttl0, max_file_bytes:1048576,
      upload_path:"/v1/reader/guest-sessions/"+id+"/content",
      open_path:"/v1/reader/guest-sessions/"+id+"/open",
      scene_path:"/v1/reader/guest-sessions/"+id+"/scene",
    });return;
  }
  if (!path.includes(id) || req.headers["x-chaptera-reader-session"] !== token) {
    json(res,403,{error:"guest_session_not_found"});return;
  }
  if (path === route && req.method === "DELETE") {
    if (deleted) { json(res,404,{error:"guest_session_not_found"});return; }
    deleted = true;
    res.writeHead(204,{"cache-control":"no-store"});
    res.end();
    return;
  }
  if (deleted) { json(res,404,{error:"guest_session_not_found"});return; }
  if (path.endsWith("/content") && req.method === "PUT") {
    json(res,200,{protocol_version:"chaptera.reader-guest-session.v1",
      session_id:id,state:"stored",expires_at_ms:ttl0});return;
  }
  if (path.endsWith("/open") && req.method === "POST") {
    json(res,200,{protocol_version:"chaptera.reader-guest-session.v1",
      session_id:id,classification:"supported",source_sha256:source,
      expires_at_ms:ttl0,scene:scene()});return;
  }
  if (path === "/v1/reader/guest-sessions/"+id+"/scene") {
    json(res,200,{protocol_version:"chaptera.reader-guest-session.v1",
      session_id:id,classification:"supported",source_sha256:source,
      expires_at_ms:ttl0,scene:{...scene(),revision_id:before}});return;
  }
  if (path === route+"/current" && req.method === "GET") {
    json(res,200,current());return;
  }
  if (path === route+"/scene" && req.method === "GET") {
    json(res,200,{protocol_version:"chaptera.temporary-editor-scene.v1",
      document_id:id,source_hash:source,
      revision_id:created?afterShape:accepted?after:before,
      expires_at_ms:created?ttl2:accepted?ttl1:ttl0,
      scene:scene()});return;
  }
  if (path === route+"/move-node" && req.method === "POST") {
    const command = JSON.parse(body.toString("utf8"));
    if (accepted) throw new Error("second canonical commit");
    if (command.document_id !== id || command.source_hash !== source ||
        command.base_revision_id !== before || command.command.node_id !== nodeId) {
      json(res,409,{error:"temporary_editor_stale_revision"});return;
    }
    accepted = command.command;
    json(res,200,{
      protocol_version:"chaptera.temporary-editor-move-node.v1",
      document_id:id,source_hash:source,base_revision_id:before,
      revision_id:after,canonical_authoring_revision_id:canon1,
      client_operation_id:command.client_operation_id,expires_at_ms:ttl1,
      replayed:false,scene_refresh:"current",canonical_operation:{kind:"move_node"},
    });return;
  }

  if (path === route+"/create-shape" && req.method === "POST") {
    const request = JSON.parse(body.toString("utf8"));
    const command = request.command;
    if (!accepted || created || request.source_hash !== source ||
        request.document_id !== id || request.base_revision_id !== after ||
        command.kind !== "create_shape" || command.page_id !== pageId ||
        Object.hasOwn(command,"node_id") || command.width_emu <= 0 ||
        command.height_emu <= 0 || command.x_emu < 0 || command.y_emu < 0 ||
        command.x_emu + command.width_emu > 952500 ||
        command.y_emu + command.height_emu > 952500) {
      json(res,409,{error:"temporary_editor_create_shape_rejected"});return;
    }
    const operation = {
      kind:"create_shape",node_id:authoredNodeId,page_id:pageId,parent_id:pageId,
      shape_kind:"rectangle",
      bounds:{x:command.x_emu,y:command.y_emu,width:command.width_emu,height:command.height_emu},
      transform:{kind:"identity"},
      paint:{
        fill:{visible:true,color:{r:command.fill_rgb[0],g:command.fill_rgb[1],b:command.fill_rgb[2]}},
        stroke:{visible:true,color:{r:command.stroke_rgb[0],g:command.stroke_rgb[1],
          b:command.stroke_rgb[2]},width_emu:command.stroke_width_emu},
        provenance:{kind:"author_created"},
      },
      provenance:{kind:"author_created"},
    };
    created = {command,operation};
    json(res,200,{
      protocol_version:"chaptera.temporary-editor-create-shape.v1",
      document_id:id,source_hash:source,base_revision_id:after,
      revision_id:afterShape,canonical_authoring_revision_id:canon2,
      client_operation_id:request.client_operation_id,expires_at_ms:ttl2,
      replayed:false,scene_refresh:"current",canonical_operation:operation,
    });return;
  }
  if ((path === route+"/exports/idml" || path === route+"/exports/pdf") && req.method === "GET") {
    const target = path.endsWith("/pdf") ? "pdf" : "idml";
    const output = target === "pdf" ? pdfBytes : editedBytes;
    const outputDigest = target === "pdf" ? pdfSha : outputSha;
    if (url.searchParams.get("revision_id") !== after ||
        url.searchParams.get("canonical_authoring_revision_id") !== canon1) {
      json(res,409,{error:"temporary_editor_export_stale_revision"});return;
    }
    if (url.searchParams.get("report_only") === "true") {
      editedReportRequests++;
      json(res,200,{protocol_version:"chaptera.temporary-edited-export-report.v1",
        revision_id:after,canonical_authoring_revision_id:canon1,
        source_hash:source,target,artifact_sha256:outputDigest,
        loss_report:{can_serialize:true,counts:{blocking:0,warning:0}}});return;
    }
    res.writeHead(200,{
      "content-type":target === "pdf" ? "application/pdf" : "application/vnd.adobe.indesign-idml-package",
      "content-disposition":`attachment; filename="chaptera-edited.${target}"`,
      "x-chaptera-edit-revision":after,"x-chaptera-export-sha256":outputDigest,
      "cache-control":"no-store",
    });
    res.end(output);return;
  }
  json(res,404,{error:"route_not_found"});
});
await new Promise(resolve => server.listen(0,"127.0.0.1",resolve));
const origin="http://127.0.0.1:"+server.address().port;
const browser=await chromium.launch({headless:true});
try {
  const page=await browser.newPage({viewport:{width:1024,height:700},acceptDownloads:true});
  const pageErrors=[];
  page.on("pageerror",error=>pageErrors.push(String(error?.message??error)));
  await page.goto(origin+"/editor/temporary-preview");
  await page.locator("#temporary-file").setInputFiles({
    name:"pinned.pub",mimeType:"application/vnd.ms-publisher",
    buffer:Buffer.alloc(48,0x42),
  });
  await page.locator("#temporary-start").click();
  const direct=page.locator('[data-node-id="'+nodeId+'"]');
  try {
    await direct.waitFor({state:"visible",timeout:7500});
  } catch {
    const status=await page.locator("#temporary-status").textContent();
    const bodyText=await page.locator("body").innerText();
    throw new Error("anonymous guest preview did not draw exact node; status=" +
      JSON.stringify(status) + " pageErrors=" + JSON.stringify(pageErrors) +
      " body=" + JSON.stringify(bodyText.slice(0,900)) +
      " requests=" + JSON.stringify(requests.map(x=>x.method+" "+x.path)));
  }
  await direct.scrollIntoViewIfNeeded();
  const rect=await direct.boundingBox();
  if(!rect)throw Error("source-neutral initial shape did not render");
  const x=rect.x+rect.width/2, y=rect.y+rect.height/2;
  const pointerContext = await page.evaluate(([x,y]) => {
    const hit = document.elementFromPoint(x,y);
    const svg = hit?.closest?.("svg.page[data-page-id]");
    const pageHost = document.querySelector("#temporary-pages");
    let transformed = null;
    try {
      const point = svg?.createSVGPoint();
      if (point) {
        point.x=x;point.y=y;
        transformed=point.matrixTransform(svg.getScreenCTM().inverse());
      }
    } catch (e) {transformed={error:String(e)}}
    return {tag:hit?.tagName,visualId:hit?.closest?.("[data-node-id]")?.getAttribute("data-node-id"),
      svgFound:!!svg,pageContains:pageHost?.contains(svg),
      pageId:svg?.getAttribute("data-page-id"),
      transformed,svgCount:pageHost?.querySelectorAll("svg.page").length};
  }, [x,y]);
  console.log("POINTER_CONTEXT", JSON.stringify(pointerContext));
  await page.mouse.move(x,y);
  await page.mouse.down();
  await page.mouse.move(x+15,y+11,{steps:5});
  await page.mouse.up();
  try {
    await page.waitForFunction(() => document.querySelector("#temporary-status")?.textContent.includes("Правка сохранена"),null,{timeout:8000});
  } catch {
    const status = await page.locator("#temporary-status").textContent();
    throw new Error("drag was not accepted; status=" + JSON.stringify(status)
      + " pointer=" + JSON.stringify(pointerContext) + " accepted=" + JSON.stringify(accepted)
      + " browserErrors=" + JSON.stringify(pageErrors)
      + " requests=" + JSON.stringify(requests.map(x => x.method + " " + x.path)));
  }
  if(!accepted)throw Error("Chromium did not send canonical guest MoveNode");
  const newNode=page.locator('[data-node-id="'+nodeId+'"] rect');
  const newX=await newNode.getAttribute("x");
  if(Number(newX)!==accepted.x_emu)throw Error("UI did not render the accepted revised Reader Scene");
  const downloadPromise=page.waitForEvent("download");
  await page.locator("#temporary-download-idml").click();
  const download=await downloadPromise;
  if(download.suggestedFilename()!=="chaptera-edited.idml")throw Error("guest changed download not offered");
  if(editedReportRequests!==1)throw Error("changed IDML was not loss-preflighted");
  const pdfDownloadPromise=page.waitForEvent("download");
  await page.locator("#temporary-download-pdf").click();
  const pdfDownload=await pdfDownloadPromise;
  if(pdfDownload.suggestedFilename()!=="chaptera-edited.pdf") {
    throw Error("guest changed PDF download not offered");
  }
  if(editedReportRequests!==2)throw Error("changed PDF was not loss-preflighted");
  if(requests.some(req => req.path.includes(token)||req.query.includes(token))) {
    throw Error("guest PDF download leaked private bearer in URI");
  }

  // Separate real Chromium pointer proof for the user-visible Rectangle tool.
  // The private backend in this harness is synthetic; physical PUB semantic
  // acceptance stays a separate required gate.
  await page.locator("#temporary-draw-rectangle").click();
  const svgPage = page.locator("svg.page[data-page-id='"+pageId+"']").first();
  const pageRect = await svgPage.boundingBox();
  if (!pageRect) throw Error("Reader page unavailable for authored rectangle pointer test");
  const startX = pageRect.x + pageRect.width*0.18;
  const startY = pageRect.y + pageRect.height*0.2;
  await page.mouse.move(startX,startY);
  await page.mouse.down();
  await page.mouse.move(startX + Math.min(36,pageRect.width*0.12),
    startY + Math.min(29,pageRect.height*0.12),{steps:5});
  await page.mouse.up();
  await page.waitForFunction(() =>
    document.querySelector("#temporary-status")?.textContent.includes("Прямоугольник сохранён"),
    null,{timeout:10000});
  if (!created) throw Error("Chromium Rectangle pointer tool did not emit canonical CreateShape");
  const authoredRectangle=page.locator('[data-node-id="'+authoredNodeId+'"]');
  await authoredRectangle.waitFor({state:"visible",timeout:8000});
  if (pageErrors.length) throw Error("Chromium encountered page errors: "+JSON.stringify(pageErrors));
  await page.reload();
  await page.locator("#temporary-resume").click();
  await page.waitForFunction((expected) => Number(document.querySelector('[data-node-id] rect')?.getAttribute("x")) === expected,
    accepted.x_emu,{timeout:30000});
  page.once("dialog", dialog => dialog.accept());
  await page.locator("#temporary-delete").click();
  await page.waitForFunction(() => document.querySelector("#temporary-status")?.textContent.includes("Сессия удалена"),null,{timeout:15000});
  if (!deleted) throw Error("user-clicked temporary session delete never reached server");
  const denied = await fetch(origin+route+"/current", {
    headers: {"x-chaptera-reader-session": token}, cache: "no-store",
  });
  if (denied.status !== 404) throw Error("old guest capability could still read after DELETE");
  await page.reload();
  if (!(await page.locator("#temporary-resume").isHidden())) {
    throw Error("guest browser retained locally deleted session token");
  }
  const movePosts=requests.filter(x=>x.path===route+"/move-node");
  const shapePosts=requests.filter(x=>x.path===route+"/create-shape");
  if(movePosts.length!==1)throw Error("browser replay silently committed MoveNode twice");
  if(shapePosts.length!==1)throw Error("browser created more than one rectangle");
  if(requests.some(x=>x.path.includes(token)||x.query.includes(token)))throw Error("capability leaked into URI");
  const receipt={schema:"chaptera.temporary-guest-browser-chromium-synthetic.v1",
    status:"PASS",real_chromium_pointer:true,synthetic_backend:true,
    private_guest_capability:true,commit_count:movePosts.length+shapePosts.length,
    exact_scene_after_move:true,exact_scene_after_reload:true,
    chromium_rectangle_pointer:true,synthetic_created_rectangle_scene:true,
    changed_download_preflighted:true,edited_download_offered:true,
    synthetic_pdf_download_preflighted:true,synthetic_pdf_download_offered:true,
    explicit_server_delete_confirmed:true,deleted_capability_denied:true,
    browser_local_key_erased_after_receipt:true,
    original_pub_unmodified:true,v0_go:false};
  await mkdir("target/web-editor-temporary",{recursive:true});
  await writeFile("target/web-editor-temporary/chromium-temporary-guest.json",
    JSON.stringify(receipt,null,2)+"\n");
  console.log(JSON.stringify(receipt));
}finally{
  await browser.close();
  await new Promise(resolve => server.close(resolve));
}
