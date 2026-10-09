#!/usr/bin/env node
// Sample3 real UI acceptance: exact-source Reader Scene -> pointer selection ->
// Chaptera StoryRange -> RevisionKernel -> native Writer -> actual browser Download.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE_SHA = "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc";
const OUTPUT_SHA = "a92543b6f2b6ac3a8ae2481e15a2188a338ddc2a92832580f8987079fa4f70f8";
const MARKER = "345678";
const [source, baseline, graph, viewer, producer, output] = process.argv.slice(2);
if (![source, baseline, graph, viewer, producer, output].every(Boolean)) {
  throw new Error("usage: node test_sample3_editor_ui_acceptance.mjs SOURCE BASELINE GRAPH VIEWER PRODUCER OUTPUT");
}
fs.mkdirSync(output, { recursive: true });
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
if (digest(fs.readFileSync(source)) !== SOURCE_SHA) throw new Error("wrong source fixture");

function startStatic() {
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      try {
        const file = path.resolve(ROOT, "." + decodeURIComponent(new URL(req.url, "http://localhost").pathname));
        if (!file.startsWith(ROOT + path.sep)) {
          res.writeHead(403).end();
          return;
        }
        const body = fs.readFileSync(file);
        const type = file.endsWith(".html") ? "text/html; charset=utf-8"
          : file.endsWith(".mjs") || file.endsWith(".js") ? "text/javascript; charset=utf-8"
          : "application/octet-stream";
        res.writeHead(200, { "content-type": type, "cache-control": "no-store" }).end(body);
      } catch {
        res.writeHead(404).end();
      }
    });
    server.listen(0, "127.0.0.1", () => resolve({ server, port: server.address().port }));
  });
}

async function waitReady(child, getOutput) {
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error("real service exited before ready: " + getOutput());
    for (const line of getOutput().split("\n")) {
      if (!line.trim().startsWith("{")) continue;
      try {
        const item = JSON.parse(line);
        if (item.ready === true && Number.isInteger(item.port)) return item.port;
      } catch {}
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("real service readiness timed out: " + getOutput());
}

function framePoints(scene, frameNode) {
  const page = scene.pages.find((p) => p.page_id === frameNode.page_id);
  if (!page) throw new Error("selected frame has no backed page");
  const sorted = [...scene.pages].sort((a, b) => a.order - b.order);
  const earlier = sorted.filter((p) => p.order < page.order);
  const pageY = 24 + earlier.reduce((v, p) => v + p.height_emu / 12700 + 32, 0);
  const points = [];
  for (const [fx, fy] of [[0.5, 0.5], [0.15, 0.15], [0.85, 0.15], [0.15, 0.85], [0.85, 0.85]]) {
    points.push({
      x: 24 + (frameNode.bounds.x + frameNode.bounds.width * fx) / 12700,
      y: pageY + (frameNode.bounds.y + frameNode.bounds.height * fy) / 12700,
    });
  }
  return points;
}

async function selectByPointer(page, points) {
  for (const point of points) {
    const location = await page.evaluate((p) => {
      const wrap = document.getElementById("host-wrap");
      wrap.scrollTop = Math.max(0, Math.floor(p.y) - 200);
      const host = document.getElementById("host").getBoundingClientRect();
      return { x: host.left + p.x, y: host.top + p.y };
    }, point);
    const viewport = page.viewportSize();
    if (location.x < 0 || location.x >= viewport.width || location.y < 0 ||
        location.y >= viewport.height) continue;
    await page.mouse.move(location.x, location.y);
    await page.mouse.down();
    const selected = await page.locator("#edit-text").isEnabled();
    // Selection is a real DOM pointer gesture. A cancel prevents an
    // unrequested MoveNode mutation on pointer release.
    await page.locator("#host").dispatchEvent("pointercancel");
    await page.mouse.up();
    if (selected) return true;
  }
  return false;
}

async function main() {
  let stdout = "", stderr = "";
  const proc = spawn("python3", [
    "services/editor-api/web_real_acceptance_service.py",
    "--port", "0", "--interactive", "--fixture-profile", "sample3",
    "--fixture", source,
    "--baseline-project", baseline,
    "--resolved-graph", graph,
    "--viewer-receipt", viewer,
    "--exporter", producer,
    "--work-dir", output,
  ], { cwd: ROOT, stdio: ["ignore", "pipe", "pipe"] });
  proc.stdout.on("data", (chunk) => { stdout += chunk.toString(); });
  proc.stderr.on("data", (chunk) => { stderr += chunk.toString(); });
  let staticServer, browser;
  try {
    const port = await waitReady(proc, () => stdout + "\n" + stderr);
    const base = "http://127.0.0.1:" + port;
    const headers = { "x-chaptera-principal-id": "synthetic-editor" };
    const read = async (route) => {
      const response = await fetch(base + route, { headers });
      const payload = await response.json();
      if (!response.ok) throw new Error(route + " HTTP " + response.status + " " + JSON.stringify(payload));
      return payload;
    };
    const pre = await read("/v1/pub-save/preview");
    if (pre.can_download || pre.can_serialize || pre.blocker_code !== "editor_pub_story_mutation_count") {
      throw new Error("unedited Sample3 must not be downloadable: " + JSON.stringify(pre));
    }
    const scene = await read("/v1/scenes/current");
    const caps = await read("/v1/editor/capabilities");
    const stories = scene.stories.filter((s) => s.text.includes(MARKER));
    if (stories.length !== 1 || stories[0].text.split(MARKER).length !== 2) {
      throw new Error("no unique source-backed controlled Story");
    }
    const story = stories[0];
    if (!(caps.editable_story_ids || []).includes(story.story_id)) {
      throw new Error("Story lacks current Rust edit capability");
    }
    const bindings = scene.story_frames.filter((frame) => frame.story_id === story.story_id);
    const frames = bindings.map((frame) => scene.nodes.find((node) =>
      node.node_id === frame.node_id && node.editable !== false && node.kind === "text_frame"
    )).filter(Boolean);
    if (!frames.length) throw new Error("source-backed Story has no selectable page-owned TextFrame");

    staticServer = await startStatic();
    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext({ acceptDownloads: true, viewport: { width: 1280, height: 920 } });
    const page = await context.newPage();
    const url = "http://127.0.0.1:" + staticServer.port + "/apps/web/local-editor.html?api=" +
      encodeURIComponent(base) + "&emu_per_css_px=12700&pan_y_css_px=0";
    await page.goto(url, { waitUntil: "networkidle" });
    await page.waitForFunction(() => document.getElementById("state").textContent.startsWith("revision "));
    if (await page.locator("#save-pub").isEnabled()) throw new Error("initial Download PUB must be disabled");

    let selected = false;
    for (const frame of frames) {
      if (await selectByPointer(page, framePoints(scene, frame))) { selected = true; break; }
    }
    if (!selected) throw new Error("real pointer cannot select canonical Story TextFrame");
    await page.locator("#edit-text").click();
    const editor = page.locator("#text-value");
    const previous = await editor.inputValue();
    if (previous.split(MARKER).length !== 2) throw new Error("real Story editor did not expose exact source text");
    await editor.fill(previous.replace(MARKER, ""));
    const revisionBefore = scene.revision_id;
    await page.locator("#apply-text").click();
    await page.waitForFunction(() => {
      const state = document.getElementById("state").textContent;
      return state.startsWith("revision ") &&
        document.getElementById("save-pub").disabled === false;
    }, null, { timeout: 120000 });
    const approved = await read("/v1/pub-save/preview");
    if (approved.output_hash !== OUTPUT_SHA || approved.can_download !== true ||
        approved.native_publisher_authorized !== true ||
        approved.revision_id === revisionBefore) {
      throw new Error("UI Story edit is not exact Publisher-approved output: " + JSON.stringify(approved));
    }
    const stateAfterEdit = await read("/v1/harness/state");
    if (stateAfterEdit.executor_calls !== 1 || stateAfterEdit.commit_requests !== 1) {
      throw new Error("browser Story UI did not create exactly one canonical edit");
    }
    const editedScene = await read("/v1/scenes/current");
    const editedStory = editedScene.stories.find((item) => item.story_id === story.story_id);
    if (!editedStory || editedStory.text !== story.text.replace(MARKER, "")) {
      throw new Error("Scene Story text differs from canonical accepted edit");
    }

    const downloadWait = page.waitForEvent("download");
    await page.locator("#save-pub").click();
    const download = await downloadWait;
    const bytes = fs.readFileSync(await download.path());
    if (bytes.length !== 72192 || digest(bytes) !== OUTPUT_SHA) {
      throw new Error("actual Chromium downloaded file is not approved Sample3");
    }
    fs.writeFileSync(path.join(output, "ui-edited.pub"), bytes);

    await page.locator("#undo").click();
    await page.waitForFunction(() => document.getElementById("save-pub").disabled === true);
    const undo = await read("/v1/pub-save/preview");
    if (undo.can_download) throw new Error("Undo must revoke native PUB download");
    await page.locator("#redo").click();
    await page.waitForFunction(() => document.getElementById("save-pub").disabled === false);
    const redo = await read("/v1/pub-save/preview");
    if (redo.output_hash !== OUTPUT_SHA) throw new Error("Redo exact Writer bytes changed");
    await page.locator("#reopen").click();
    await page.waitForFunction(() => document.getElementById("save-pub").disabled === false);
    const reloaded = await read("/v1/harness/state");
    if (reloaded.reopen_count < 1 || reloaded.fixture_sha256 !== SOURCE_SHA) {
      throw new Error("fresh project replay/source immutability not verified");
    }
    await page.reload({ waitUntil: "networkidle" });
    await page.waitForFunction(() => document.getElementById("save-pub").disabled === false);
    const finalScene = await read("/v1/scenes/current");
    if (finalScene.stories.find((item) => item.story_id === story.story_id)?.text !== editedStory.text) {
      throw new Error("real browser reload lost canonical Story edit");
    }
    if (digest(fs.readFileSync(source)) !== SOURCE_SHA) throw new Error("original Sample3 mutated");

    const receipt = {
      result: "PASS_real_ui_story_to_native_pub_download",
      scope: "exact Sample3/Story deletion/Publisher-accepted SHA pair only",
      source_sha256: SOURCE_SHA,
      pub_sha256: OUTPUT_SHA,
      byte_len: bytes.length,
      browser_version: browser.version(),
      selected_text_frame: true,
      canonical_story_commits: stateAfterEdit.commit_requests,
      undo_revoked_download: !undo.can_download,
      redo_restored_exact_bytes: redo.output_hash === OUTPUT_SHA,
      fresh_reopen: reloaded.reopen_count >= 1,
      browser_reload_preserved_story: true,
      immutable_source: true,
      release_ready: false,
    };
    fs.writeFileSync(path.join(output, "sample3-ui-pub-download.json"), JSON.stringify(receipt, null, 2) + "\n");
    console.log(JSON.stringify(receipt));
  } finally {
    if (browser) await browser.close();
    if (staticServer) await new Promise((done) => staticServer.server.close(done));
    if (proc.exitCode === null) {
      proc.kill("SIGTERM");
      await new Promise((done) => { proc.once("exit", done); setTimeout(done, 2000); });
    }
    if (stderr.trim()) console.error("Sample3 service stderr:", stderr.slice(-10000));
  }
}
main().catch((error) => { console.error(error); process.exitCode = 1; });
