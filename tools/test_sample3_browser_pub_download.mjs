#!/usr/bin/env node
// Real Chromium fetch/download byte contract for the exact Publisher-accepted
// Sample3 Story candidate. UI Story editing and generalized PUB save remain out of scope.
import { spawn } from "node:child_process";
import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";
import { chromium } from "playwright";

const [source, baseline, edited, producer, outputDir] = process.argv.slice(2);
if (![source, baseline, edited, producer, outputDir].every(Boolean)) {
  throw new Error("usage: node test_sample3_browser_pub_download.mjs SOURCE BASELINE EDITED PRODUCER OUTDIR");
}
const ROOT = path.resolve(import.meta.dirname, "..");
const SHA = "a92543b6f2b6ac3a8ae2481e15a2188a338ddc2a92832580f8987079fa4f70f8";
const SRC = "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc";
mkdirSync(outputDir, { recursive: true });

const child = spawn("python3", [
  "tools/test_sample3_browser_pub_download_service.py",
  "--source", source, "--baseline", baseline, "--edited", edited,
  "--producer", producer, "--out", outputDir,
], { cwd: ROOT, stdio: ["ignore", "pipe", "pipe"] });
let stdout = "", stderr = "";
child.stdout.on("data", (b) => { stdout += b.toString(); });
child.stderr.on("data", (b) => { stderr += b.toString(); });
let browser;
try {
  const deadline = Date.now() + 20000;
  let config = null;
  while (!config && Date.now() < deadline) {
    const line = stdout.split("\n").find((l) => l.startsWith('{"ready":'));
    if (line) config = JSON.parse(line);
    if (child.exitCode !== null) throw new Error("native producer service failed: " + stderr);
    if (!config) await new Promise((r) => setTimeout(r, 100));
  }
  if (!config) throw new Error("native producer service unavailable: " + stderr);
  if (config.source_sha256 !== SRC || config.candidate_sha256 !== SHA) {
    throw new Error("unrecognized source/candidate identity");
  }

  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  await page.goto("http://127.0.0.1:" + config.port + "/health", { waitUntil: "domcontentloaded" });
  const result = await page.evaluate(async (port) => {
    const base = "http://127.0.0.1:" + port;
    const headers = { "x-chaptera-principal-id": "synthetic-editor" };
    const preview = await fetch(base + "/v1/pub-save/preview", { headers });
    const pv = await preview.json();
    const resp = await fetch(base + "/v1/pub-save/download", { headers });
    const bytes = new Uint8Array(await resp.arrayBuffer());
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    const hash = [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
    const forbidden = await fetch(base + "/v1/pub-save/download", {
      headers: { "x-chaptera-principal-id": "synthetic-viewer" },
    });
    return {
      status: resp.status,
      content_type: resp.headers.get("content-type"),
      disposition: resp.headers.get("content-disposition"),
      length: bytes.length,
      hash,
      preview: pv,
      forbidden_status: forbidden.status,
      forbidden: await forbidden.json(),
    };
  }, config.port);

  if (result.status !== 200 ||
      result.hash !== SHA ||
      result.preview.output_hash !== SHA ||
      result.preview.can_download !== true ||
      result.preview.native_publisher_authorized !== true ||
      result.preview.source_hash !== SRC ||
      result.preview.byte_len !== result.length ||
      result.content_type !== "application/x-mspublisher" ||
      !result.disposition.includes("chaptera-edited.pub") ||
      result.forbidden_status !== 403) {
    throw new Error("Chromium native PUB HTTP contract failed: " + JSON.stringify(result));
  }

  const local = readFileSync(path.join(outputDir, "accepted.pub"));
  if (createHash("sha256").update(local).digest("hex") !== SHA ||
      local.length !== result.length ||
      createHash("sha256").update(readFileSync(source)).digest("hex") !== SRC) {
    throw new Error("download does not equal permitted Rust-produced bytes");
  }
  // Real Reader source/semantic reopen was checked by the Rust producer and
  // will also be checked independently by the pub_story_native_handoff verifier.
  const receipt = {
    result: "PASS_browser_http_exact_sha_only",
    limits: "not a Story-edit UI proof or general native PUB save permission",
    source_sha256: SRC, output_sha256: result.hash, bytes: result.length,
    chromium_version: browser.version(), http_status: result.status,
    unauthorized_http_status: result.forbidden_status,
    rust_reopen_verified: result.preview.can_serialize === true,
  };
  writeFileSync(path.join(outputDir, "sample3-browser-pub-download.json"), JSON.stringify(receipt, null, 2) + "\n");
  process.stdout.write(JSON.stringify(receipt) + "\n");
} finally {
  if (browser) await browser.close();
  if (child.exitCode === null) {
    child.kill("SIGTERM");
    await new Promise((resolve) => {
      child.once("exit", resolve);
      setTimeout(resolve, 2000);
    });
  }
}
