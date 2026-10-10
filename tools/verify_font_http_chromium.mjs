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
  // The user-facing inspector is the primary font entry surface.
  // Diagnostics must not cover the workspace or masquerade as product UI.
  const userSurface=await page.evaluate(()=>{
    const workspace=document.querySelector("#workspace")?.getBoundingClientRect();
    const inspector=document.querySelector("#inspector")?.getBoundingClientRect();
    const canvas=document.querySelector("#host-wrap")?.getBoundingClientRect();
    const diagnostics=document.querySelector("#disclosure");
    return {
      asideVisible:!!inspector && inspector.width>=300 && inspector.height>=350,
      canvasVisible:!!canvas && canvas.width>500,
      separated:!!inspector && !!canvas && inspector.left>=canvas.right-2,
      diagnosticsCollapsed:diagnostics?.open===false,
      fidelitySummary:document.querySelector("#summary-fidelity")?.textContent??"",
      workspaceHeight:workspace?.height??0,
    };
  });
  if(!userSurface.asideVisible||!userSurface.canvasVisible||
     !userSurface.separated||!userSurface.diagnosticsCollapsed||
     !userSurface.fidelitySummary.includes("Partial")||
     userSurface.workspaceHeight<500){
    throw new Error("real font editor does not present a usable inspector and stage: "+
                    JSON.stringify(userSurface));
  }
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
  let selectionSurface="canvas_pointer";
  if(!selected){
    // Source stacking is deliberately unknown for this Publisher witness.
    // Canvas hit-test refuses overlap; resolve identity through an actual
    // user-facing native <select> that lists only server-authorized Stories.
    const picker=page.locator("#font-story-target");
    if(!await picker.isEnabled()){
      throw new Error("overlapping Publisher frames lack an explicit Story selector");
    }
    const values=await picker.locator("option").evaluateAll(nodes=>
      nodes.map(node=>node.value)
    );
    const targetIndex=values.indexOf(storyId);
    if(targetIndex<1){
      throw new Error("selected real Story missing from authorized chooser: "+JSON.stringify(values));
    }
    await picker.click();
    await picker.press("Home");
    for(let index=0;index<targetIndex;index++){
      await picker.press("ArrowDown");
    }
    await picker.press("Enter");
    if(await picker.inputValue()!==storyId){
      throw new Error("real keyboard picker failed to select server-admitted Story");
    }
    selected=await page.locator("#edit-text").isEnabled();
    selectionSurface="explicit_story_picker_for_ambiguous_canvas_hit";
  }
  if(!selected)throw new Error("real UI could not select font-editable Story through pointer or explicit list");
  await page.locator("#edit-text").click();
  // Initial source font names/indices are not actual physical glyph grants.
  await page.waitForFunction(()=>
    document.querySelector("#font-metrics")?.dataset.exactScalars==="0",
    null,{timeout:40_000}
  );
  const originalCoverage=await page.locator("#font-metrics").evaluate(el=>({
    exact:Number(el.dataset.exactScalars),
    unknown:Number(el.dataset.unresolvedScalars),
    glyphs:Number(el.dataset.exactGlyphs),
  }));
  if(originalCoverage.unknown<1||originalCoverage.glyphs!==0){
    throw new Error("unadmitted Publisher font was silently shaped: "+
                    JSON.stringify(originalCoverage));
  }
  await page.waitForFunction(()=>document.querySelector("#font-flow")?.dataset.flowState==="source_font_unresolved",null,{timeout:40_000});
  const sourceFlow=await page.locator("#font-flow").evaluate(el=>({
    state:el.dataset.flowState,
    gaps:Number(el.dataset.sourceGapScalars),
    lines:Number(el.dataset.lineCount),
  }));
  if(sourceFlow.gaps!==originalCoverage.unknown||sourceFlow.lines!==0){
    throw new Error("source font line fit invented geometry before authoring: "+
                    JSON.stringify(sourceFlow));
  }
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
  await page.waitForFunction(()=>
    document.querySelector("#font-metrics")?.dataset.exactScalars==="1",
    null,{timeout:40_000}
  );
  const physicalCoverage=await page.locator("#font-metrics").evaluate(el=>({
    admitted_scalars:Number(el.dataset.exactScalars),
    unresolved_scalars:Number(el.dataset.unresolvedScalars),
    exact_glyphs:Number(el.dataset.exactGlyphs),
    message:el.textContent,
  }));
  if(physicalCoverage.admitted_scalars!==1||
     physicalCoverage.unresolved_scalars!==originalCoverage.unknown-1||
     physicalCoverage.exact_glyphs<1||
     !physicalCoverage.message.includes("Line placement, overset and PDF not verified")){
    throw new Error("UI did not consume current native Rust glyph spans: "+
                    JSON.stringify(physicalCoverage));
  }
  await page.waitForFunction(()=>{
    const f=document.querySelector("#font-flow");
    return f?.dataset.flowState==="source_font_unresolved" &&
      f.dataset.sourceGapScalars!=="";
  },null,{timeout:40_000});
  const afterFlow=await page.locator("#font-flow").evaluate(el=>({
    state:el.dataset.flowState,
    gaps:Number(el.dataset.sourceGapScalars),
    lines:Number(el.dataset.lineCount),
    text:el.textContent,
  }));
  if(afterFlow.gaps!==physicalCoverage.unresolved_scalars||
     afterFlow.lines!==0||
     !afterFlow.text.includes("PDF blocked")){
    throw new Error("partially admitted Story yielded fake line-fit geometry: "+
                    JSON.stringify(afterFlow));
  }
  const after=await current();
  if(after.revision_id===before.revision_id ||
     after.snapshot_id===before.snapshot_id) {
    throw new Error("browser font Apply did not create a new real server Scene revision");
  }
  if(after.fidelity.state!=="partial"||
     !after.fidelity.reasons.includes("font_resource_layout_not_implemented")) {
    throw new Error("actual browser font change must remain fidelity Partial");
  }
  const scope=await fetch(api+"/v1/editor/font-format-scope",{headers}).then(r=>r.json());
  if(!scope.stories.some(x=>x.story_id===storyId)) {
    throw new Error("fresh browser FontResource override missing from actual Rust Story");
  }
  const native=await fetch(api+"/v1/pub-save/preview",{headers}).then(r=>r.json());
  if(native.can_download||native.can_serialize||native.blocker_code!=="font_layout_unverified"){
    throw new Error("font-changed real PUB is falsely downloadable");
  }
  // Real user screenshot must never present unresolved Publisher Story text
  // as a free-running single line across other objects. This is a rendering
  // safety claim only; it does NOT prove authoritative glyph placement.
  const clip=await page.evaluate(()=>{
    const viewports=[...document.querySelectorAll(
      '#host svg[data-text-viewport="unshaped-source-frame"]'
    )];
    const offFrame=[...document.querySelectorAll(
      '#host text[data-preview-reason="story_text_layout_not_implemented"]'
    )].filter(node=>!node.closest(
      '[data-text-viewport="unshaped-source-frame"]'
    ));
    const mismatched=viewports.filter(viewport=>{
      const width=Number(viewport.getAttribute("width"));
      const height=Number(viewport.getAttribute("height"));
      return viewport.getAttribute("overflow")!=="hidden"||
        viewport.getAttribute("data-layout-verified")!=="false"||
        viewport.getAttribute("data-preview-reason")!=="story_text_layout_not_implemented"||
        !Number.isFinite(width)||width<=0||
        !Number.isFinite(height)||height<=0;
    });
    return {
      unresolved_frames:viewports.length,
      escaped_text_nodes:offFrame.length,
      incorrectly_marked_frames:mismatched.length,
    };
  });
  if(clip.unresolved_frames===0||clip.escaped_text_nodes!==0||
     clip.incorrectly_marked_frames!==0){
    throw new Error("real Publisher Story text painted outside unshaped source frame: "+
                    JSON.stringify(clip));
  }
  fs.mkdirSync("target/font-http-acceptance",{recursive:true});
  const evidence={
    receipt_kind:"chaptera.real-chromium-font-range-apply.v1",
    browser:"chromium",
    user_pointer_selection:selectionSurface==="canvas_pointer",
    user_explicit_story_picker:selectionSurface!=="canvas_pointer",
    selection_surface:selectionSurface,
    user_keyboard_range_selection:true,
    story_id:storyId,
    selected_range:selection.end,
    original_revision_id:before.revision_id,
    accepted_revision_id:after.revision_id,
    native_output_blocked:true,
    original_story_text_unchanged:true,
    user_typography_inspector:true,
    diagnostics_collapsed_by_default:true,
    unresolved_story_frame_clip:clip,
    unshaped_layout_still_unverified:true,
    physical_glyph_coverage:physicalCoverage,
    source_unknown_before:originalCoverage.unknown,
    real_glyph_spans_consumed:true,
    physical_line_fit_before:sourceFlow,
    physical_line_fit_after:afterFlow,
    no_invented_lines_without_source_fonts:true,
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
