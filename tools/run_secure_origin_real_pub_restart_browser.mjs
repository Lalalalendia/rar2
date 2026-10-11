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
        created?.layout_environment_id !== input.layout_environment_id ||
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
            status.layout_environment_id !== input.layout_environment_id ||
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

  const downloadedArtifact = await page.evaluate(async input => {
    const response = await fetch(
      "/v1/exports/" + encodeURIComponent(input.job_id) +
        "/artifacts/" + encodeURIComponent(input.artifact_id),
      { credentials: "same-origin", cache: "no-store" },
    );
    const contentType = response.headers.get("content-type");
    const disposition = response.headers.get("content-disposition");
    const contentLengthRaw = response.headers.get("content-length");
    const nosniff = response.headers.get("x-content-type-options");
    const cacheControl = response.headers.get("cache-control");
    const buffer = await response.arrayBuffer();
    const bytes = new Uint8Array(buffer);
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", buffer));
    const sha256 = [...digest].map(byte => byte.toString(16).padStart(2, "0")).join("");
    return {
      status: response.status,
      content_type: contentType,
      content_disposition: disposition,
      content_length: contentLengthRaw === null ? null : Number(contentLengthRaw),
      nosniff,
      cache_control: cacheControl,
      byte_len: bytes.length,
      sha256,
      zip_magic: bytes.length >= 4 &&
        bytes[0] === 0x50 && bytes[1] === 0x4b &&
        bytes[2] === 0x03 && bytes[3] === 0x04,
    };
  }, {
    job_id: exportResult.job_id,
    artifact_id: exportResult.artifact_id,
  });
  if (downloadedArtifact.status !== 200 ||
      downloadedArtifact.content_type !== "application/vnd.adobe.indesign-idml-package" ||
      !downloadedArtifact.content_disposition?.startsWith("attachment;") ||
      !downloadedArtifact.content_disposition.includes(".idml") ||
      !Number.isSafeInteger(downloadedArtifact.content_length) ||
      downloadedArtifact.content_length <= 0 ||
      downloadedArtifact.content_length !== downloadedArtifact.byte_len ||
      downloadedArtifact.nosniff !== "nosniff" ||
      downloadedArtifact.cache_control !== "private, no-store" ||
      !downloadedArtifact.zip_magic ||
      !/^[0-9a-f]{64}$/.test(downloadedArtifact.sha256)) {
    throw new Error("physical exact-revision artifact download failed representation/integrity checks: " +
      JSON.stringify(downloadedArtifact));
  }

  // Keep the already-accepted edited IDML proof above. Then test the real
  // browser Ctrl/Cmd+Z on that exact durable revision through the same
  // configured OIDC/HTTPS Chaptera route, without issuing a fabricated
  // reverse MoveNode command.
  const historyPosts = [];
  const historyResponses = [];
  const historyPath =
    "/v1/documents/" + encodeURIComponent(receipt.document_id) + "/history";
  page.on("request", request => {
    if (request.method() === "POST" &&
        new URL(request.url()).pathname === historyPath) {
      historyPosts.push({
        csrfToken: request.headers()["x-csrf-token"] ?? null,
        forged: !!request.headers()["x-chaptera-principal-id"],
        body: request.postData(),
      });
    }
  });
  page.on("response", response => {
    if (response.request().method() === "POST" &&
        new URL(response.url()).pathname === historyPath) {
      historyResponses.push(response.json().then(body => ({
        status: response.status(),
        code: typeof body?.error === "string" ? body.error : (body?.error?.code ?? null),
      })).catch(() => ({
        status: response.status(),
        code: null,
      })));
    }
  });
  await page.keyboard.press("Control+z");
  await page.waitForFunction(previousRevision => {
    const status = document.querySelector("#status");
    return status?.classList.contains("ok") &&
      status.textContent.includes("Revision sha256:") &&
      !status.textContent.includes(previousRevision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length > 0;
  }, receipt.current_revision_id, { timeout: 60000 });
  const undoCurrent = await page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin", cache: "no-store",
    });
    const body = await response.json().catch(() => null);
    return {
      status: response.status,
      document_id: body?.document_id ?? null,
      revision_id: body?.revision_id ?? null,
      source_hash: body?.source_hash ?? null,
    };
  }, receipt.document_id);
  const historyReplies = await Promise.all(historyResponses);
  // A restarted browser may hold a stale CSRF token. Product mutation logic
  // explicitly permits ONE retry after a typed csrf_invalid 403, with the
  // exact same idempotent request body and a newly fetched CSRF token.
  // Reject extra POSTs, changed operation IDs, or any other failed request.
  const safeSingle = historyPosts.length === 1 &&
    historyReplies.length === 1 &&
    historyReplies[0].status === 200;
  const safeCsrfRefresh = historyPosts.length === 2 &&
    historyReplies.length === 2 &&
    historyReplies[0].status === 403 &&
    historyReplies[0].code === "csrf_invalid" &&
    historyReplies[1].status === 200 &&
    historyPosts[0].body === historyPosts[1].body &&
    historyPosts[0].csrfToken !== historyPosts[1].csrfToken;
  if (!(safeSingle || safeCsrfRefresh) ||
      !historyPosts.every(post => post.csrfToken && !post.forged) ||
      undoCurrent.status !== 200 || undoCurrent.document_id !== receipt.document_id ||
      !undoCurrent.revision_id || undoCurrent.revision_id === receipt.current_revision_id ||
      undoCurrent.source_hash !== receipt.real_pub_sha256) {
    // Do not copy the ephemeral CSRF tokens, raw source/doc identities or
    // canonical mutation JSON into retained security telemetry.
    throw new Error("real keyboard Undo did not preserve authenticated idempotent history: " +
      JSON.stringify({
        post_count: historyPosts.length,
        responses: historyReplies,
        valid_single: safeSingle,
        valid_csrf_refresh: safeCsrfRefresh,
        current_ok: undoCurrent.status === 200,
        revision_advanced: undoCurrent.revision_id !== receipt.current_revision_id,
      }));
  }
  const undoScene = await page.evaluate(async id => {
    const response = await fetch("/v1/reader/documents/" + encodeURIComponent(id) + "/scene", {
      credentials: "same-origin", cache: "no-store",
    });
    if (!response.ok) throw new Error("Undo Reader scene HTTP " + response.status);
    return response.json();
  }, receipt.document_id);
  const undoneNode = undoScene.nodes?.find(node =>
    node.node_id === receipt.move_commit.node_id);
  if (undoScene.revision_id !== undoCurrent.revision_id ||
      undoScene.source_hash !== receipt.real_pub_sha256 ||
      undoneNode?.bounds?.x !== receipt.move_commit.before.x_emu ||
      undoneNode?.bounds?.y !== receipt.move_commit.before.y_emu) {
    throw new Error("real keyboard Undo failed to restore authoritative original geometry");
  }
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction(revision => {
    const status = document.querySelector("#status");
    return status?.classList.contains("ok") &&
      status.textContent.includes(revision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length > 0;
  }, undoCurrent.revision_id, { timeout: 30000 });
  const undoAfterReload = await page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin", cache: "no-store",
    });
    const body = await response.json().catch(() => null);
    return { status: response.status, revision_id: body?.revision_id ?? null };
  }, receipt.document_id);
  if (undoAfterReload.status !== 200 ||
      undoAfterReload.revision_id !== undoCurrent.revision_id) {
    throw new Error("browser reload failed to recover the immutable Undo revision");
  }

  // Acceptance completes with a SECOND exact export of the durable Undo
  // revision. A successful edited export before Undo does not prove that
  // materialization and export work for a history-transition child revision.
  const undoExport = await page.evaluate(async input => {
    const sessionResponse = await fetch("/v1/session", {
      credentials: "same-origin", cache: "no-store",
    });
    const session = await sessionResponse.json().catch(() => null);
    if (!sessionResponse.ok || !session?.csrf_token) {
      throw new Error("Undo revision export lacked an authenticated CSRF session");
    }
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
        client_request_id: "real-pub-undo-export-002",
      }),
    });
    const created = await createResponse.json().catch(() => null);
    if (!createResponse.ok || created?.protocol_version !== "chaptera.export-job-http.v1" ||
        created.document_id !== input.document_id ||
        created.revision_id !== input.revision_id ||
        created.target_profile !== "idml:bounded-editable" ||
        created.layout_environment_id !== input.layout_environment_id ||
        !created.job_id) {
      throw new Error("durable Undo revision export create rejected: " +
        JSON.stringify({
          status: createResponse.status,
          code: typeof created?.error === "string" ?
            created.error : (created?.error?.code ?? null),
        }));
    }
    const deadline = Date.now() + 90000;
    while (Date.now() < deadline) {
      const response = await fetch(
        "/v1/exports/" + encodeURIComponent(created.job_id),
        { credentials: "same-origin", cache: "no-store" },
      );
      const status = await response.json().catch(() => null);
      if (!response.ok) {
        throw new Error("durable Undo export status HTTP " + response.status);
      }
      if (status?.status === "ready") {
        if (status.document_id !== input.document_id ||
            status.revision_id !== input.revision_id ||
            status.target_profile !== "idml:bounded-editable" ||
            status.layout_environment_id !== input.layout_environment_id ||
            !status.artifact_id || !status.loss_report_id) {
          throw new Error("Undo export artifact identity mismatched accepted revision");
        }
        return {
          job_id: status.job_id,
          artifact_id: status.artifact_id,
          revision_id: status.revision_id,
          target_profile: status.target_profile,
          loss_report_id: status.loss_report_id,
        };
      }
      if (status?.status === "failed" || status?.status === "cancelled") {
        throw new Error("durable Undo revision export terminated " + status.status);
      }
      await new Promise(resolve => setTimeout(resolve, 250));
    }
    throw new Error("durable Undo revision export did not reach ready");
  }, {
    document_id: receipt.document_id,
    revision_id: undoCurrent.revision_id,
    layout_environment_id: layoutEnvironmentId,
  });

  const undoExportDownload = await page.evaluate(async input => {
    const response = await fetch(
      "/v1/exports/" + encodeURIComponent(input.job_id) +
        "/artifacts/" + encodeURIComponent(input.artifact_id),
      { credentials: "same-origin", cache: "no-store" },
    );
    const buffer = await response.arrayBuffer();
    const bytes = new Uint8Array(buffer);
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", buffer));
    return {
      status: response.status,
      content_type: response.headers.get("content-type"),
      content_disposition: response.headers.get("content-disposition"),
      content_length: Number(response.headers.get("content-length")),
      nosniff: response.headers.get("x-content-type-options"),
      cache_control: response.headers.get("cache-control"),
      byte_len: bytes.length,
      sha256: [...digest].map(byte => byte.toString(16).padStart(2, "0")).join(""),
      zip_magic: bytes.length >= 4 &&
        bytes[0] === 0x50 && bytes[1] === 0x4b &&
        bytes[2] === 0x03 && bytes[3] === 0x04,
    };
  }, { job_id: undoExport.job_id, artifact_id: undoExport.artifact_id });

  if (undoExportDownload.status !== 200 ||
      undoExportDownload.content_type !== "application/vnd.adobe.indesign-idml-package" ||
      !undoExportDownload.content_disposition?.startsWith("attachment;") ||
      !undoExportDownload.content_disposition.includes(".idml") ||
      !Number.isSafeInteger(undoExportDownload.content_length) ||
      undoExportDownload.content_length <= 0 ||
      undoExportDownload.content_length !== undoExportDownload.byte_len ||
      undoExportDownload.nosniff !== "nosniff" ||
      undoExportDownload.cache_control !== "private, no-store" ||
      !undoExportDownload.zip_magic ||
      !/^[0-9a-f]{64}$/.test(undoExportDownload.sha256) ||
      undoExportDownload.sha256 === downloadedArtifact.sha256) {
    throw new Error("Undo revision IDML physical artifact failed integrity or delta proof");
  }
  const unchangedHeadAfterExport = await page.evaluate(async documentId => {
    const response = await fetch(
      "/v1/documents/" + encodeURIComponent(documentId) + "/current",
      { credentials: "same-origin", cache: "no-store" },
    );
    const data = await response.json().catch(() => null);
    return { status: response.status, revision_id: data?.revision_id ?? null };
  }, receipt.document_id);
  if (unchangedHeadAfterExport.status !== 200 ||
      unchangedHeadAfterExport.revision_id !== undoCurrent.revision_id ||
      undoExport.revision_id !== undoCurrent.revision_id) {
    throw new Error("Undo revision export advanced or misbound authoritative revision head");
  }
  receipt.undo_revision_export_claim = true;
  receipt.undo_revision_export = {
    revision_id: undoExport.revision_id,
    target_profile: undoExport.target_profile,
    artifact_bytes: undoExportDownload.byte_len,
    artifact_sha256: undoExportDownload.sha256,
    downloadable_idml: true,
    differs_from_moved_revision_export: true,
    original_undo_revision_preserved: true,
  };

  receipt.real_browser_authoritative_undo_claim = true;
  receipt.real_browser_authoritative_undo = {
    base_revision_id: receipt.current_revision_id,
    revision_id: undoCurrent.revision_id,
    history_post_count: historyPosts.length,
    accepted_history_post_count: 1,
    csrf_refresh_retry: safeCsrfRefresh,
    csrf_present: true,
    source_hash_unchanged: true,
    baseline_geometry_restored: true,
    browser_reload_persisted: true,
  };


  // Redo is a real browser command after the durable Undo/Reload/export path.
  // No client-supplied geometry and no synthetic history authority.
  const redoRequestOffset = historyPosts.length;
  const redoReplyOffset = historyResponses.length;
  await page.keyboard.press("Control+Shift+z");
  await page.waitForFunction(undoneRevision => {
    const status = document.querySelector("#status");
    return status?.classList.contains("ok") &&
      status.textContent.includes("Revision sha256:") &&
      !status.textContent.includes(undoneRevision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length > 0;
  }, undoCurrent.revision_id, { timeout: 60000 });
  const redoCurrent = await page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin", cache: "no-store",
    });
    const body = await response.json().catch(() => null);
    return {
      status: response.status,
      document_id: body?.document_id ?? null,
      revision_id: body?.revision_id ?? null,
      source_hash: body?.source_hash ?? null,
    };
  }, receipt.document_id);
  const redoPosts = historyPosts.slice(redoRequestOffset);
  const redoReplies = await Promise.all(historyResponses.slice(redoReplyOffset));
  const redoIntent = redoPosts.length ? JSON.parse(redoPosts[0].body) : null;
  const oneRedo = redoPosts.length === 1 &&
    redoReplies.length === 1 && redoReplies[0].status === 200;
  const csrfRetryRedo = redoPosts.length === 2 &&
    redoReplies.length === 2 &&
    redoReplies[0].status === 403 &&
    redoReplies[0].code === "csrf_invalid" &&
    redoReplies[1].status === 200 &&
    redoPosts[0].body === redoPosts[1].body &&
    redoPosts[0].csrfToken !== redoPosts[1].csrfToken;
  const undoIntent = JSON.parse(historyPosts[0].body);
  if (!(oneRedo || csrfRetryRedo) ||
      !redoPosts.every(post => post.csrfToken && !post.forged) ||
      redoIntent?.protocol_version !== "chaptera.history-transition-intent.v1" ||
      redoIntent?.command?.kind !== "redo" ||
      Object.keys(redoIntent.command).length !== 1 ||
      redoIntent.source_hash !== receipt.real_pub_sha256 ||
      redoIntent.base_revision_id !== undoCurrent.revision_id ||
      redoIntent.client_operation_id === undoIntent.client_operation_id ||
      redoCurrent.status !== 200 || redoCurrent.document_id !== receipt.document_id ||
      redoCurrent.source_hash !== receipt.real_pub_sha256 ||
      !redoCurrent.revision_id ||
      redoCurrent.revision_id === undoCurrent.revision_id ||
      redoCurrent.revision_id === receipt.current_revision_id) {
    throw new Error("real Chromium Redo did not produce an authenticated immutable successor: " +
      JSON.stringify({
        requests: redoPosts.length,
        responses: redoReplies,
        csrf_retry: csrfRetryRedo,
        revision_advanced: redoCurrent.revision_id !== undoCurrent.revision_id,
      }));
  }
  const redoScene = await page.evaluate(async id => {
    const response = await fetch("/v1/reader/documents/" + encodeURIComponent(id) + "/scene", {
      credentials: "same-origin", cache: "no-store",
    });
    if (!response.ok) throw new Error("Redo Reader scene HTTP " + response.status);
    return response.json();
  }, receipt.document_id);
  const redoneNode = redoScene.nodes?.find(node =>
    node.node_id === receipt.move_commit.node_id);
  if (redoScene.revision_id !== redoCurrent.revision_id ||
      redoScene.source_hash !== receipt.real_pub_sha256 ||
      redoneNode?.bounds?.x !== receipt.move_commit.after.x_emu ||
      redoneNode?.bounds?.y !== receipt.move_commit.after.y_emu) {
    throw new Error("real Chromium Redo did not restore moved canonical Reader geometry");
  }
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction(revision => {
    const status = document.querySelector("#status");
    return status?.classList.contains("ok") &&
      status.textContent.includes(revision) &&
      document.querySelectorAll("#canvas svg.page[data-page-id]").length > 0;
  }, redoCurrent.revision_id, { timeout: 30000 });
  const redoAfterReload = await page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin", cache: "no-store",
    });
    const body = await response.json().catch(() => null);
    return { status: response.status, revision_id: body?.revision_id ?? null };
  }, receipt.document_id);
  if (redoAfterReload.status !== 200 ||
      redoAfterReload.revision_id !== redoCurrent.revision_id) {
    throw new Error("real Chromium Redo was not durable after browser reload");
  }

  // Exact-revision Redo IDML export. Reusing the same source keeps the export
  // geometry comparison meaningful while requiring a distinct immutable RevId.
  const redoExport = await page.evaluate(async input => {
    const sessionResponse = await fetch("/v1/session", {
      credentials: "same-origin", cache: "no-store",
    });
    const session = await sessionResponse.json().catch(() => null);
    if (!sessionResponse.ok || !session?.csrf_token) {
      throw new Error("Redo export requires authenticated CSRF");
    }
    const createResponse = await fetch("/v1/exports", {
      method: "POST", credentials: "same-origin", cache: "no-store",
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
        client_request_id: "real-pub-redo-export-003",
      }),
    });
    const created = await createResponse.json().catch(() => null);
    if (!createResponse.ok ||
        created?.protocol_version !== "chaptera.export-job-http.v1" ||
        created.document_id !== input.document_id ||
        created.revision_id !== input.revision_id ||
        created.target_profile !== "idml:bounded-editable" ||
        created.layout_environment_id !== input.layout_environment_id ||
        !created.job_id) {
      throw new Error("Redo export create failed, HTTP " + createResponse.status);
    }
    const deadline = Date.now() + 90000;
    while (Date.now() < deadline) {
      const response = await fetch("/v1/exports/" + encodeURIComponent(created.job_id), {
        credentials: "same-origin", cache: "no-store",
      });
      const status = await response.json().catch(() => null);
      if (!response.ok) throw new Error("Redo export status HTTP " + response.status);
      if (status?.status === "ready") {
        if (status.document_id !== input.document_id ||
            status.revision_id !== input.revision_id ||
            status.target_profile !== "idml:bounded-editable" ||
            status.layout_environment_id !== input.layout_environment_id ||
            !status.artifact_id || !status.loss_report_id) {
          throw new Error("Redo export artifact identity mismatch");
        }
        return { job_id: status.job_id, artifact_id: status.artifact_id,
          revision_id: status.revision_id };
      }
      if (status?.status === "failed" || status?.status === "cancelled") {
        throw new Error("Redo export terminated " + status.status);
      }
      await new Promise(resolve => setTimeout(resolve, 250));
    }
    throw new Error("Redo export did not complete within its acceptance deadline");
  }, {
    document_id: receipt.document_id,
    revision_id: redoCurrent.revision_id,
    layout_environment_id: layoutEnvironmentId,
  });
  const redoDownload = await page.evaluate(async input => {
    const response = await fetch(
      "/v1/exports/" + encodeURIComponent(input.job_id) +
      "/artifacts/" + encodeURIComponent(input.artifact_id),
      { credentials: "same-origin", cache: "no-store" },
    );
    const contentLength = response.headers.get("content-length");
    const data = await response.arrayBuffer();
    const bytes = new Uint8Array(data);
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", data));
    return {
      status: response.status,
      content_type: response.headers.get("content-type"),
      disposition: response.headers.get("content-disposition"),
      nosniff: response.headers.get("x-content-type-options"),
      cache_control: response.headers.get("cache-control"),
      content_length: contentLength === null ? null : Number(contentLength),
      bytes: bytes.length,
      hash: [...digest].map(byte => byte.toString(16).padStart(2, "0")).join(""),
      zip_magic: bytes.length >= 4 &&
        bytes[0] === 0x50 && bytes[1] === 0x4b &&
        bytes[2] === 0x03 && bytes[3] === 0x04,
    };
  }, { job_id: redoExport.job_id, artifact_id: redoExport.artifact_id });
  if (redoDownload.status !== 200 ||
      redoDownload.content_type !== "application/vnd.adobe.indesign-idml-package" ||
      !redoDownload.disposition?.startsWith("attachment;") ||
      !redoDownload.disposition.includes(".idml") ||
      redoDownload.nosniff !== "nosniff" ||
      redoDownload.cache_control !== "private, no-store" ||
      !Number.isSafeInteger(redoDownload.content_length) ||
      redoDownload.content_length !== redoDownload.bytes ||
      redoDownload.bytes <= 0 || !redoDownload.zip_magic ||
      !/^[0-9a-f]{64}$/.test(redoDownload.hash) ||
      redoDownload.hash === undoExportDownload.sha256) {
    throw new Error("Redo IDML exact-revision artifact failed content or integrity checks");
  }
  const headAfterRedoExport = await page.evaluate(async id => {
    const response = await fetch("/v1/documents/" + encodeURIComponent(id) + "/current", {
      credentials: "same-origin", cache: "no-store",
    });
    const body = await response.json().catch(() => null);
    return { status: response.status, revision_id: body?.revision_id ?? null };
  }, receipt.document_id);
  if (headAfterRedoExport.status !== 200 ||
      headAfterRedoExport.revision_id !== redoCurrent.revision_id) {
    throw new Error("Redo export unexpectedly advanced or changed the document HEAD");
  }
  receipt.real_browser_authoritative_redo_claim = true;
  receipt.real_browser_authoritative_redo = {
    base_revision_id: undoCurrent.revision_id,
    revision_id: redoCurrent.revision_id,
    post_count: redoPosts.length,
    csrf_refresh_retry: csrfRetryRedo,
    source_hash_unchanged: true,
    restored_moved_geometry: true,
    reload_persisted: true,
  };
  receipt.redo_revision_export_claim = true;
  receipt.redo_revision_export = {
    revision_id: redoExport.revision_id,
    artifact_bytes: redoDownload.bytes,
    artifact_sha256: redoDownload.hash,
    downloadable_idml: true,
    differs_from_undo_export: true,
  };

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
  receipt.export_download_claim = true;
  receipt.exact_revision_export_download = downloadedArtifact;
  await writeFile(receiptPath, JSON.stringify(receipt, null, 2) + "\n");
  process.stdout.write(JSON.stringify({
    server_restart_reopen: receipt.server_restart_reopen,
    exact_revision_export: exportResult,
    exact_revision_export_download: downloadedArtifact,
  }) + "\n");
} finally {
  await browser.close();
}
