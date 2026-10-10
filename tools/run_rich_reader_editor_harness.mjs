import { createServer } from "node:http";
import { readFile, stat, mkdir, writeFile } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { chromium } from "playwright";

const root=resolve(".");
const mime=new Map([
  [".html","text/html; charset=utf-8"],
  [".mjs","text/javascript; charset=utf-8"],
  [".js","text/javascript; charset=utf-8"],
  [".css","text/css; charset=utf-8"],
  [".json","application/json; charset=utf-8"],
]);

const server=createServer(async(req,res)=>{
  try{
    const pathname=decodeURIComponent(new URL(req.url,"http://127.0.0.1").pathname);
    const file=resolve(root,"."+pathname);
    if(file!==root && !file.startsWith(root+sep)) throw new Error("path escape");
    const info=await stat(file);
    if(!info.isFile()) throw new Error("not a file");
    res.writeHead(200,{"content-type":mime.get(extname(file))??"application/octet-stream"});
    res.end(await readFile(file));
  }catch{
    res.writeHead(404,{"content-type":"text/plain"});
    res.end("not found");
  }
});
await new Promise((resolveReady)=>server.listen(0,"127.0.0.1",resolveReady));
const address=server.address();
const origin="http://127.0.0.1:"+address.port;

const browser=await chromium.launch({headless:true});
try{
  const page=await browser.newPage({viewport:{width:800,height:500}});
  await page.goto(origin+"/apps/web/rich-reader-editor-harness.html");
  await page.waitForFunction(()=>window.__richReaderEditorReady===true);

  const ids=await page.evaluate(()=>window.__ids);
  const direct=page.locator('[data-node-id="'+ids.DIRECT+'"]');
  const projected=page.locator('[data-node-id="'+ids.PROJECTED+'"]');
  await direct.waitFor();
  await projected.waitFor();

  const directFill=await direct.locator("rect").first().getAttribute("fill");
  if(directFill!=="rgb(10 120 200)") throw new Error("rich Reader paint was not rendered: "+directFill);

  const projectedBox=await projected.boundingBox();
  if(!projectedBox) throw new Error("projected visual has no browser bounds");
  await page.mouse.click(projectedBox.x+projectedBox.width/2,projectedBox.y+projectedBox.height/2);
  const afterProjected=await page.evaluate(()=>({
    selected:window.__shell.stateReceipt().selected_node_id,
    commits:window.__serviceCalls.length
  }));
  if(afterProjected.selected!==null||afterProjected.commits!==0){
    throw new Error("projected visual became editable");
  }

  const directBox=await direct.boundingBox();
  if(!directBox) throw new Error("direct visual has no browser bounds");
  const startX=directBox.x+directBox.width/2;
  const startY=directBox.y+directBox.height/2;
  await page.mouse.move(startX,startY);
  await page.mouse.down();
  await page.mouse.move(startX+15,startY+10,{steps:4});
  await page.mouse.up();
  await page.waitForFunction((child)=>window.__shell.stateReceipt().revision_id===child,ids.CHILD);

  const result=await page.evaluate(()=>({
    state:window.__shell.stateReceipt(),
    calls:window.__serviceCalls,
    directX:document.querySelector('[data-node-id="'+window.__ids.DIRECT+'"] rect')?.getAttribute("x"),
    directY:document.querySelector('[data-node-id="'+window.__ids.DIRECT+'"] rect')?.getAttribute("y"),
    overlay:document.querySelector('[data-layer="editor-transient-overlay"]')!==null
  }));
  if(result.calls.length!==1) throw new Error("expected one canonical MoveNode commit");
  const request=result.calls[0];
  const expectedX=238125;
  const expectedY=190500;
  if(request.command.kind!=="move_node_to"||request.command.node_id!==ids.DIRECT
    ||request.command.x_emu!==expectedX||request.command.y_emu!==expectedY){
    throw new Error("unexpected canonical move request: "+JSON.stringify(request));
  }
  if(result.directX!==String(expectedX)||result.directY!==String(expectedY)){
    throw new Error("child rich Reader scene did not re-render committed bounds");
  }
  if(!result.overlay||result.state.selected_node_id!==ids.DIRECT){
    throw new Error("selection overlay was not restored on child revision");
  }
  if(result.state.visual_scene_is_edit_authority!==false
    ||result.state.browser_interaction_is_durable_authority!==false){
    throw new Error("browser shell claimed durable authority");
  }

  const receipt={
    schema_version:"chaptera.rich-reader-editor-browser-smoke.v1",
    status:"PASS",
    base_revision_id:ids.BASE,
    child_revision_id:ids.CHILD,
    direct_node_id:ids.DIRECT,
    projected_node_id:ids.PROJECTED,
    projected_visual_read_only:true,
    canonical_move_request_count:result.calls.length,
    committed_x_emu:request.command.x_emu,
    committed_y_emu:request.command.y_emu,
    rich_reader_paint_visible:true,
    child_scene_rerendered:true,
    visual_scene_is_edit_authority:false,
    browser_interaction_is_durable_authority:false
  };
  await mkdir("target/web-editor-rich",{recursive:true});
  await writeFile("target/web-editor-rich/chromium-rich-editor-smoke.json",JSON.stringify(receipt,null,2)+"\n");
  process.stdout.write(JSON.stringify(receipt,null,2)+"\n");
}finally{
  await browser.close();
  await new Promise((resolveClose)=>server.close(resolveClose));
}
