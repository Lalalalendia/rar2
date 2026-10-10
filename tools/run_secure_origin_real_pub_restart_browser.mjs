#!/usr/bin/env node
// Reopens the exact imported document after a real Chaptera process restart.
import { readFile, writeFile } from "node:fs/promises";
import { chromium } from "playwright";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const receiptPath = process.env.CHAPTERA_SECURE_RECEIPT;
const storageStatePath = process.env.CHAPTERA_BROWSER_STORAGE_STATE;
if (!origin || !receiptPath || !storageStatePath) {
  throw new Error("restart reopen requires secure origin, receipt and storage state");
}

const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
if (!receipt.document_id || !receipt.current_revision_id || !receipt.real_pub_sha256) {
  throw new Error("initial ingress receipt lacks persistent document identity");
}

const browser = await chromium.launch({
  headless: true,
  args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"],
});
const context = await browser.newContext({
  ignoreHTTPSErrors: true,
  storageState: storageStatePath,
  viewport: { width: 1180, height: 830 },
});
const page = await context.newPage();
const failures = [];
page.on("response", response => {
  if (response.url().startsWith(origin + "/v1/") && response.status() >= 400) {
    failures.push(response.status() + " " + new URL(response.url()).pathname);
  }
});

try {
  await page.goto(origin + "/editor/doc/" + encodeURIComponent(receipt.document_id), {
    waitUntil: "domcontentloaded",
  });
  const current = await page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin",
      cache: "no-store",
    });
    const data = await response.json().catch(() => null);
    return {
      status: response.status,
      document_id: data?.document_id ?? null,
      revision_id: data?.revision_id ?? null,
    };
  }, receipt.document_id);
  if (current.status !== 200 || current.document_id !== receipt.document_id ||
      current.revision_id !== receipt.current_revision_id) {
    throw new Error("persistent current revision not recovered after restart: " +
      JSON.stringify({ current, expected: receipt.current_revision_id, failures }));
  }

  const scene = await page.evaluate(async id => {
    const response = await fetch("/v1/reader/documents/" + encodeURIComponent(id) + "/scene", {
      credentials: "same-origin",
      cache: "no-store",
    });
    if (!response.ok) throw new Error("Reader scene HTTP " + response.status);
    return response.json();
  }, receipt.document_id);
  if (scene.protocol_version !== "chaptera.reader-scene.v1" ||
      scene.document_id !== receipt.document_id ||
      scene.revision_id !== receipt.current_revision_id ||
      scene.source_hash !== receipt.real_pub_sha256 ||
      !scene.pages?.length || !scene.nodes?.length) {
    throw new Error("persistent Reader scene not recovered after restart");
  }

  await page.waitForFunction(revision => {
    const status = document.querySelector("#status");
    return status?.classList.contains("ok") && status.textContent.includes(revision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length > 0;
  }, receipt.current_revision_id, { timeout: 30000 });

  const paint = await page.evaluate(() => ({
    document_id: document.querySelector("#document")?.textContent ?? null,
    page_ids: [...document.querySelectorAll("#canvas svg.page[data-page-id]")]
      .map(node => node.getAttribute("data-page-id")),
    node_ids: [...document.querySelectorAll("#canvas g[data-node-id]")]
      .map(node => node.getAttribute("data-node-id")).sort(),
  }));
  const expectedPages = [...scene.pages].sort((a, b) => a.order - b.order)
    .map(item => item.page_id);
  const expectedNodes = scene.nodes
    .filter(node => node.bounds.width > 0 && node.bounds.height > 0)
    .map(node => node.node_id).sort();
  if (paint.document_id !== receipt.document_id ||
      JSON.stringify(paint.page_ids) !== JSON.stringify(expectedPages) ||
      JSON.stringify(paint.node_ids) !== JSON.stringify(expectedNodes)) {
    throw new Error("editor paint not recovered from persisted Reader scene after restart");
  }

  receipt.server_restart_reopen_claim = true;
  receipt.server_restart_reopen = {
    document_id: current.document_id,
    revision_id: current.revision_id,
    source_hash: scene.source_hash,
    page_count: scene.pages.length,
    node_count: scene.nodes.length,
  };
  await writeFile(receiptPath, JSON.stringify(receipt, null, 2) + "\n");
  process.stdout.write(JSON.stringify(receipt.server_restart_reopen) + "\n");
} finally {
  await browser.close();
}
