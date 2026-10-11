#!/usr/bin/env node
// Physical cross-principal authorization proof for the real B02 Product path.
import { readFile, writeFile } from "node:fs/promises";
import { chromium } from "playwright";

const origin = process.env.CHAPTERA_SECURE_ORIGIN;
const receiptPath = process.env.CHAPTERA_SECURE_RECEIPT;
if (!origin || !receiptPath) {
  throw new Error("cross-principal acceptance requires secure origin and receipt");
}
const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
if (!receipt.document_id || !receipt.current_revision_id || !receipt.real_pub_sha256 ||
    !receipt.move_commit?.node_id || !receipt.move_commit?.after) {
  throw new Error("primary receipt lacks durable source, revision or MoveNode identity");
}

const browser = await chromium.launch({
  headless: true,
  args: ["--host-resolver-rules=MAP edge.test 127.0.0.1"],
});
const context = await browser.newContext({
  ignoreHTTPSErrors: true,
  viewport: { width: 900, height: 700 },
});
const page = await context.newPage();

try {
  // Fresh context has no primary user's cookie. The live OIDC fixture has
  // switched to another subject before this process is launched.
  await page.goto(origin + "/editor/new", { waitUntil: "domcontentloaded" });
  await page.waitForURL(url =>
    url.origin === origin && url.pathname === "/editor/new",
  { timeout: 30000, waitUntil: "domcontentloaded" });
  await page.waitForFunction(() =>
    document.querySelector("#status")?.textContent?.includes("Готов к загрузке"),
  null, { timeout: 30000 });

  const cookies = await context.cookies(origin);
  const session = cookies.find(cookie => cookie.name === "__Host-chaptera_session");
  if (!session || !session.secure || !session.httpOnly || session.sameSite !== "Lax") {
    throw new Error("secondary principal did not receive a hardened Chaptera session");
  }

  const result = await page.evaluate(async input => {
    const request = async (path, init = {}) => {
      const response = await fetch(path, {
        credentials: "same-origin",
        cache: "no-store",
        ...init,
      });
      const body = await response.json().catch(() => null);
      return {
        status: response.status,
        cache_control: response.headers.get("cache-control"),
        code: body?.error?.code ?? null,
        message: body?.error?.message ?? null,
        document_id: body?.document_id ?? null,
        revision_id: body?.revision_id ?? null,
        source_hash: body?.source_hash ?? null,
        job_id: body?.job_id ?? null,
        artifact_id: body?.artifact_id ?? null,
        pages: Array.isArray(body?.pages) ? body.pages.length : null,
        nodes: Array.isArray(body?.nodes) ? body.nodes.length : null,
      };
    };
    const sessionResponse = await fetch("/v1/session", {
      credentials: "same-origin",
      cache: "no-store",
    });
    const session = await sessionResponse.json().catch(() => null);
    if (!sessionResponse.ok || !session?.csrf_token) {
      throw new Error("secondary principal lacks a real mutation CSRF session");
    }
    const post = (path, body) => request(path, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-csrf-token": session.csrf_token,
      },
      body: JSON.stringify(body),
    });
    const docPath = "/v1/documents/" + encodeURIComponent(input.document_id);
    const moveAttempt = (sourceHash, operationId) => ({
      protocol_version: "chaptera.commit-request.v1",
      document_id: input.document_id,
      source_hash: sourceHash,
      base_revision_id: input.revision_id,
      client_operation_id: operationId,
      command: {
        kind: "move_node_to",
        node_id: input.node_id,
        x_emu: input.x_emu,
        y_emu: input.y_emu,
      },
    });
    const correctHashCommit = await post(
      docPath + "/commit",
      moveAttempt(input.source_sha256, "b02-cross-principal-correct-hash"),
    );
    const wrongHashCommit = await post(
      docPath + "/commit",
      moveAttempt("0".repeat(64), "b02-cross-principal-wrong-hash"),
    );
    const layoutDigest = new Uint8Array(await crypto.subtle.digest(
      "SHA-256", new TextEncoder().encode("chaptera-cloud-real-pub-export-layout-v1"),
    ));
    const layoutEnvironmentId = "sha256:" + [...layoutDigest]
      .map(byte => byte.toString(16).padStart(2, "0")).join("");
    const exportRequest = (revisionId, clientRequestId) => ({
      protocol_version: "chaptera.export-create.v1",
      document_id: input.document_id,
      revision_id: revisionId,
      target_profile: "idml:bounded-editable",
      layout_environment_id: layoutEnvironmentId,
      client_request_id: clientRequestId,
    });
    const exportCreate = await post("/v1/exports", exportRequest(
      input.revision_id, "b02-cross-principal-export-denied",
    ));
    // A valid but unbound exact revision must be indistinguishable from a
    // real revision to a principal without CAP_EXPORT. This physically
    // proves that authorization precedes the durable revision lookup.
    const guessedRevision = "sha256:" + (
      input.revision_id === "sha256:" + "0".repeat(64) ? "f" : "0"
    ).repeat(64);
    const exportGuessedRevision = await post("/v1/exports", exportRequest(
      guessedRevision, "b02-cross-principal-export-guessed-revision",
    ));
    return {
      current: await request(docPath + "/current"),
      scene: await request("/v1/reader/documents/" +
        encodeURIComponent(input.document_id) + "/scene"),
      commit_correct_hash: correctHashCommit,
      commit_wrong_hash: wrongHashCommit,
      export_create: exportCreate,
      export_guessed_revision: exportGuessedRevision,
    };
  }, {
    document_id: receipt.document_id,
    source_sha256: receipt.real_pub_sha256,
    revision_id: receipt.current_revision_id,
    node_id: receipt.move_commit.node_id,
    x_emu: receipt.move_commit.after.x_emu,
    y_emu: receipt.move_commit.after.y_emu,
  });

  for (const [name, response] of Object.entries(result)) {
    if (response.status !== 403 ||
        response.cache_control !== "no-store" ||
        response.code !== "grant_missing" ||
        response.document_id !== null ||
        response.revision_id !== null ||
        response.source_hash !== null ||
        response.job_id !== null ||
        response.artifact_id !== null ||
        response.pages !== null ||
        response.nodes !== null) {
      throw new Error("cross-principal " + name + " did not fail closed: " +
        JSON.stringify(response));
    }
  }

  // A forged source SHA must not create an authorization oracle: both
  // requests are denied before source-hash validation, with identical bodies.
  if (JSON.stringify(result.commit_correct_hash) !==
      JSON.stringify(result.commit_wrong_hash)) {
    throw new Error("unauthorized MoveNode response differs by guessed source hash");
  }

  // In addition to the ordinary denial matrix, the public response to an
  // unauthorized export must not depend on existence of a guessed revision.
  if (JSON.stringify(result.export_create) !==
      JSON.stringify(result.export_guessed_revision)) {
    throw new Error("foreign export responses expose exact revision existence");
  }

  receipt.cross_principal_isolation_claim = true;
  receipt.cross_principal_isolation = {
    current_status: result.current.status,
    current_code: result.current.code,
    scene_status: result.scene.status,
    scene_code: result.scene.code,
    commit_correct_hash_status: result.commit_correct_hash.status,
    commit_wrong_hash_status: result.commit_wrong_hash.status,
    commit_source_hash_oracle: false,
    export_create_status: result.export_create.status,
    export_create_code: result.export_create.code,
    export_guessed_revision_status: result.export_guessed_revision.status,
    export_guessed_revision_code: result.export_guessed_revision.code,
    export_revision_existence_oracle: false,
    cache_control: result.current.cache_control,
    document_identity_disclosed: false,
    revision_identity_disclosed: false,
    scene_payload_disclosed: false,
  };
  await writeFile(receiptPath, JSON.stringify(receipt, null, 2) + "\n");
  process.stdout.write(JSON.stringify(receipt.cross_principal_isolation) + "\n");
} finally {
  await browser.close();
}
