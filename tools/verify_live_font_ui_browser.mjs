#!/usr/bin/env node
// Real Chromium exercise on the running task-local PUB Editor service.
// A successful preview MUST NOT claim or perform a document font mutation.
import fs from "node:fs";
import { chromium } from "playwright";

const url = process.argv[2];
if (!url?.startsWith("http://127.0.0.1:")) {
  throw new Error("local real-Editor browser URL required");
}
const api = new URL(url).searchParams.get("api");
if (!api?.startsWith("http://127.0.0.1:")) {
  throw new Error("local real-Editor API URL required");
}
const browser = await chromium.launch({ headless: true });
const problems = [];
const state = async () => {
  const response = await fetch(api + "/v1/harness/state");
  if (!response.ok) throw new Error("harness state unavailable");
  return response.json();
};
try {
  const page = await browser.newPage({ viewport: { width: 1365, height: 950 } });
  page.on("pageerror", (error) => problems.push(String(error)));
  await page.goto(url, {waitUntil:"domcontentloaded", timeout: 60_000});
  await page.waitForFunction(() => {
    const select = document.querySelector("#font-choice");
    return select && select.options.length === 1 && !select.disabled &&
      select.options[0].text.includes("Abel") &&
      document.querySelector("#preview-physical-font")?.disabled === false;
  }, null, { timeout: 90_000 });
  const before = await state();
  const choice = await page.locator("#font-choice").inputValue();
  if (choice !== "f27a8036-8492-480f-8fa6-d2e775cc9f12") {
    throw new Error("real Chromium did not expose exact admitted Abel resource");
  }
  await page.locator("#preview-physical-font").click();
  await page.waitForFunction(() => {
    const text = document.querySelector("#font-message")?.textContent ?? "";
    return text.includes("Verified byte preview") ||
      text.includes("SHA-256 mismatch") || text.includes("denied");
  }, null, { timeout: 30_000 });
  const ui = await page.evaluate(() => {
    const message = document.querySelector("#font-message")?.textContent ?? "";
    const fontFamily = getComputedStyle(document.querySelector("#font-sample")).fontFamily;
    return {message, fontFamily};
  });
  if (!ui.message.includes("Verified byte preview") ||
      !ui.message.includes("Story, layout, saved PUB and PDF unchanged") ||
      !ui.fontFamily.includes("ChapteraPreview_8809dcad25318225")) {
    throw new Error("real browser physical-font preview failed: " + JSON.stringify(ui));
  }
  const after = await state();
  for (const key of [
    "current_revision_id", "current_snapshot_id",
    "commit_requests", "executor_calls", "history_requests",
  ]) {
    if (after[key] !== before[key]) {
      throw new Error("font preview unexpectedly mutated Editor " + key);
    }
  }
  if (problems.length) throw new Error("Chromium page errors: " + problems.join("; "));
  const receipt = {
    receipt_kind: "chaptera.real-pub-authoring-font-preview-only.v1",
    real_pub: true,
    browser: "chromium",
    exact_resource_id: choice,
    integrity_verified_and_loaded: true,
    editable_state_unchanged: true,
    font_layout_pdf_not_implemented: true,
    revision_id: after.current_revision_id,
    snapshot_id: after.current_snapshot_id,
    browser_page_errors: problems,
  };
  fs.mkdirSync("target/local-font-ui", { recursive: true });
  fs.writeFileSync("target/local-font-ui/chromium.json", JSON.stringify(receipt, null, 2) + "\n");
  await page.screenshot({path:"target/local-font-ui/chromium.png",fullPage:false});
  process.stdout.write(JSON.stringify(receipt, null, 2) + "\n");
} finally {
  await browser.close();
}
