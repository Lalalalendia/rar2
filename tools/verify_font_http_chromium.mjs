#!/usr/bin/env node
// Real pointer selection + real keyboard text-range selection + click Apply font.
import fs from "node:fs";
import { chromium } from "playwright";

const [url, storyId] = process.argv.slice(2);
if (!url?.startsWith("http://127.0.0.1:") ||
    !/^[0-9a-f-]{36}$/.test(storyId ?? "")) {
  throw new Error("pinned local real-PUB URL and StoryId required");
}
const api = new URL(url).searchParams.get("api");
if (!api?.startsWith("http://127.0.0.1:")) throw new Error("invalid local service URL");
const headers = {"x-chaptera-principal-id":"synthetic-editor"};
async function current() {
  const response=await fetch(api+"/v1/scenes/current",{headers});
  if (!response.ok) throw new Error("current Scene unreachable");
  return response.json();
}
const browser=await chromium.launch({headless:true});
const errors=[];
try {
  const before=await current();
  const page=await browser.newPage({viewport:{width:1440,height:960}});
  page.on("pageerror",error=>errors.push(String(error)));
  await page.goto(url,{waitUntil:"domcontentloaded",timeout:60_000});
  await page.waitForFunction(()=>{
    const select=document.querySelector("#font-choice");
    return select && !select.disabled && select.options.length===1 &&
      !document.querySelector("#state")?.textContent?.includes("connecting");
  },null,{timeout:60_000});
  const frames=before.story_frames.filter(f=>f.story_id===storyId);
  if(!frames.length) throw new Error("real Editor has no canvas frame for chosen Story");
  let selected=false;
  for (const frame of frames) {
    const rect=page.locator('[data-node-id="'+frame.node_id+'"]').first();
    if(!await rect.count()) continue;
    try { await rect.scrollIntoViewIfNeeded({timeout:7000}); }
    catch { continue; }
    const bounds=await rect.boundingBox();
    if(!bounds||bounds.width<4||bounds.height<4) continue;
    const coords=[
      [0.5,0.5],[0.1,0.1],[0.8,0.5],[0.1,0.8],[0.5,0.1],
    ];
    for(const [fx,fy] of coords) {
      const x=bounds.x+bounds.width*fx,y=bounds.y+bounds.height*fy;
      if(x<0||y<0||x>1440||y>960) continue;
      await page.mouse.click(x,y);
      const state=await page.locator("#state").textContent();
      if(state?.includes("selected "+frame.node_id) &&
         await page.locator("#edit-text").isEnabled()){
        selected=true;break;
      }
    }
    if(selected)break;
  }
  if(!selected)throw new Error("real pointer click did not select a Story with admitted font range");
  await page.locator("#edit-text").click();
  const textarea=page.locator("#text-value");
  await textarea.waitFor({state:"visible"});
  const previous=await textarea.inputValue();
  if(!previous)throw new Error("empty real Publisher Story cannot select font range");
  await textarea.click();
  await textarea.press("Control+Home");
  await textarea.press("Shift+ArrowRight");
  const selection=await textarea.evaluate(el=>({
    start:el.selectionStart,end:el.selectionEnd,value:el.value,
  }));
  if(selection.start!==0||selection.end<1||selection.value!==previous){
    throw new Error("Chromium did not select an unmodified real Story text span");
  }
  if(!await page.locator("#apply-font").isEnabled()){
    throw new Error("live font Apply button disabled despite exact trusted Story scope");
  }
  await page.locator("#apply-font").click();
  await page.waitForFunction(()=>{
    const text=document.querySelector("#font-message")?.textContent??"";
    return text.includes("Font saved in canonical Project") ||
      document.querySelector("#state")?.classList.contains("bad");
  },null,{timeout:45_000});
  const ui=await page.evaluate(()=>({
    message:document.querySelector("#font-message")?.textContent,
    state:document.querySelector("#state")?.textContent,
    bad:document.querySelector("#state")?.classList.contains("bad"),
    pubDisabled:document.querySelector("#save-pub")?.disabled,
  }));
  if(ui.bad||!ui.message?.includes("layout and fixed PDF remain blocked")||
     !ui.pubDisabled)throw new Error("font UI cannot certify native format commit: "+JSON.stringify(ui));
  const after=await current();
  if(after.revision_id===before.revision_id ||
     after.snapshot_id===before.snapshot_id) {
    throw new Error("browser font Apply did not create a new real server Scene revision");
  }
  const scope=await fetch(api+"/v1/editor/font-format-scope",{headers}).then(r=>r.json());
  if(!scope.stories.some(x=>x.story_id===storyId)) {
    throw new Error("fresh browser FontResource override missing from actual Rust Story");
  }
  const native=await fetch(api+"/v1/pub-save/preview",{headers}).then(r=>r.json());
  if(native.can_download||native.can_serialize||native.blocker_code!=="font_layout_unverified"){
    throw new Error("font-changed real PUB is falsely downloadable");
  }
  fs.mkdirSync("target/font-http-acceptance",{recursive:true});
  const evidence={
    receipt_kind:"chaptera.real-chromium-font-range-apply.v1",
    browser:"chromium",
    user_pointer_selection:true,
    user_keyboard_range_selection:true,
    story_id:storyId,
    selected_range:selection.end,
    original_revision_id:before.revision_id,
    accepted_revision_id:after.revision_id,
    native_output_blocked:true,
    original_story_text_unchanged:true,
    page_errors:errors,
  };
  if(errors.length)throw Error("Chromium page errors: "+errors.join("; "));
  fs.writeFileSync("target/font-http-acceptance/browser.json",
    JSON.stringify(evidence,null,2)+"\n");
  await page.screenshot({path:"target/font-http-acceptance/browser.png"});
  console.log("REAL_FONT_HTTP_CHROMIUM_OK "+JSON.stringify(evidence));
} finally {
  await browser.close();
}
