#!/usr/bin/env node
// Reopens the exact imported document after a real Chaptera process restart.
import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { chromium } from "playwright";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const receiptPath = process.env.CHAPTERA_SECURE_RECEIPT;
const storageStatePath = process.env.CHAPTERA_BROWSER_STORAGE_STATE;
const layoutEnvironmentId = "sha256:" + createHash("sha256")
  .update("chaptera-cloud-real-pub-export-layout-v1")
  .digest("hex");
if (!origin || !receiptPath || !storageStatePath) {
  throw new Error("restart reopen requires secure origin, receipt and storage state");
}

const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
if (!receipt.document_id || !receipt.current_revision_id || !receipt.real_pub_sha256 ||
    !receipt.real_move_commit || !receipt.move_commit?.node_id) {
  throw new Error("initial ingress receipt lacks persistent edited document identity");
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
  const movedNode = scene.nodes.find(node => node.node_id === receipt.move_commit.node_id);
  if (!movedNode ||
      movedNode.bounds?.x !== receipt.move_commit.after.x_emu ||
      movedNode.bounds?.y !== receipt.move_commit.after.y_emu) {
    throw new Error("persisted MoveNode geometry was not recovered after restart: " +
      JSON.stringify({ movedNode, expected: receipt.move_commit }));
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

  const exportResult = await page.evaluate(async input => {
    const sessionResponse = await fetch("/v1/session", {
      credentials: "same-origin", cache: "no-store",
    });
    const session = await sessionResponse.json().catch(() => null);
    if (!sessionResponse.ok || !session?.csrf_token) {
      throw new Error("exact revision export could not obtain CSRF session");
    }
    const clientRequestId = "real-pub-export-001";
    const createResponse = await fetch("/v1/exports", {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        "content-type": "application/json",
        "x-csrf-token": session.csrf_token,
      },
      body: JSON.stringify({
        protocol_version: "chaptera.export-create.v1",
        document_id: input.document_id,
        revision_id: input.revision_id,
        target_profile: "idml:bounded-editable",
        layout_environment_id: input.layout_environment_id,
        client_request_id: clientRequestId,
      }),
    });
    const created = await createResponse.json().catch(() => null);
    if (!createResponse.ok ||
        created?.protocol_version !== "chaptera.export-job-http.v1" ||
        created?.document_id !== input.document_id ||
        created?.revision_id !== input.revision_id ||
        created?.target_profile !== "idml:bounded-editable" ||
        !created?.job_id) {
      throw new Error("exact revision export create failed: " +
        JSON.stringify({ status: createResponse.status, created }));
    }

    const deadline = Date.now() + 90000;
    while (Date.now() < deadline) {
      const statusResponse = await fetch(
        "/v1/exports/" + encodeURIComponent(created.job_id),
        { credentials: "same-origin", cache: "no-store" },
      );
      const status = await statusResponse.json().catch(() => null);
      if (!statusResponse.ok) {
        throw new Error("exact revision export status failed: " +
          JSON.stringify({ status: statusResponse.status, body: status }));
      }
      if (status?.status === "ready") {
        if (status.document_id !== input.document_id ||
            status.revision_id !== input.revision_id ||
            status.target_profile !== "idml:bounded-editable" ||
            !status.artifact_id || !status.loss_report_id) {
          throw new Error("ready export identity differs from edited revision: " +
            JSON.stringify(status));
        }
        return {
          job_id: status.job_id,
          document_id: status.document_id,
          revision_id: status.revision_id,
          target_profile: status.target_profile,
          layout_environment_id: status.layout_environment_id,
          artifact_id: status.artifact_id,
          loss_report_id: status.loss_report_id,
          client_request_id: clientRequestId,
        };
      }
      if (status?.status === "failed" || status?.status === "cancelled") {
        throw new Error("exact revision export terminated: " + JSON.stringify(status));
      }
      await new Promise(resolve => setTimeout(resolve, 250));
    }
    throw new Error("exact revision export did not reach ready before timeout");
  }, {
    document_id: receipt.document_id,
    revision_id: receipt.current_revision_id,
    layout_environment_id: layoutEnvironmentId,
  });

  receipt.server_restart_reopen_claim = true;
  receipt.server_restart_reopen = {
    document_id: current.document_id,
    revision_id: current.revision_id,
    source_hash: scene.source_hash,
    page_count: scene.pages.length,
    node_count: scene.nodes.length,
    moved_node_id: movedNode.node_id,
    moved_x_emu: movedNode.bounds.x,
    moved_y_emu: movedNode.bounds.y,
  };
  receipt.move_and_export_claim = true;
  receipt.exact_revision_export = exportResult;
  receipt.export_download_claim = false;
  await writeFile(receiptPath, JSON.stringify(receipt, null, 2) + "\n");
  process.stdout.write(JSON.stringify({
    server_restart_reopen: receipt.server_restart_reopen,
    exact_revision_export: exportResult,
  }) + "\n");
} finally {
  await browser.close();
}
