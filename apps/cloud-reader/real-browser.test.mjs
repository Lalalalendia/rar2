// Actual canonical worker scenes over controlled HTTP. This does not prove
// live quarantine/scanning/TTL or Publisher-exact document appearance.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createHash } from "node:crypto";
import { readFile, writeFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = dirname(fileURLToPath(import.meta.url));
const repo = resolve(root, "../..");
const output = resolve(process.env.READER_REAL_OUTPUT ?? join(repo, "target/cloud-reader-real"));
const worker = resolve(process.env.READER_WORKER_BINARY ?? join(repo, "target/debug/chaptera"));
const run = promisify(execFile);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const defaultFixtures = [
  { name: "SampleNewsletter", sha256: "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf", bytes: 291840, pages: 4, require_render: true, require_shared_text: true },
  { name: "SampleBrochure", sha256: "ffed034ac87e679f0bd08ff9cf74ad11c0e0e510a42b1bc1a7502415f6c29c87", bytes: 161792, pages: 2, require_render: true, require_shared_text: true }
];
const manifestPath = process.env.READER_REAL_MANIFEST ? resolve(process.env.READER_REAL_MANIFEST) : null;
const fixtures = manifestPath
  ? JSON.parse(await readFile(manifestPath, "utf8")).fixtures
  : defaultFixtures;
assert.ok(Array.isArray(fixtures) && fixtures.length > 0, "real fixture manifest must contain fixtures");
for (const fixture of fixtures) {
  assert.match(fixture.name, /^[A-Za-z0-9._-]+$/, "fixture name must be path-safe");
  assert.match(fixture.sha256, /^[0-9a-f]{64}$/, "fixture SHA-256 must be canonical");
  assert.ok(Number.isSafeInteger(fixture.bytes) && fixture.bytes > 0, "fixture byte length must be positive");
  if (fixture.pages != null) assert.ok(Number.isSafeInteger(fixture.pages) && fixture.pages > 0, "fixture page count must be positive");
  if (fixture.require_render != null) assert.equal(typeof fixture.require_render, "boolean", "require_render must be boolean");
  if (fixture.require_shared_text != null) assert.equal(typeof fixture.require_shared_text, "boolean", "require_shared_text must be boolean");
}
function compatibilityReport(classification, sha, scene) {
  const supported = classification === "supported";
  assert.ok(supported || classification === "partial", "renderable fixture must be supported or partial");
  return {
    protocol_version: "chaptera.reader-compatibility-report.v1",
    source_sha256: sha,
    state: supported ? "opens_normally" : "needs_review",
    engine_classification: classification,
    content_summary: {
      page_count: scene.pages.length,
      text_frame_count: scene.nodes.filter((node) => node.kind === "text_frame").length,
      picture_frame_count: scene.nodes.filter((node) => node.kind === "picture_frame").length
    },
    limitations: supported ? [] : [{
      code: "preview_fidelity_warning",
      message: "The preview contains known display limitations."
    }],
    output_routes: {
      read_only_preview: supported ? "available" : "available_with_limitations",
      salvage_recovery: "not_applicable",
      editable_idml: "not_verified",
      editable_odg: "not_verified"
    },
    recommended_next_step: supported
      ? "migration_pilot_preview"
      : "review_preview_before_migration"
  };
}

const continueWorkerFailures = process.env.READER_CONTINUE_WORKER_FAILURES === "1";
const workerTimeoutSeconds = Number(process.env.READER_WORKER_TIMEOUT_SECONDS ?? "60");
const workerCpuSeconds = Number(process.env.READER_WORKER_CPU_SECONDS ?? "30");
const workerAddressSpaceMb = Number(process.env.READER_WORKER_ADDRESS_SPACE_MB ?? "512");
assert.ok(Number.isInteger(workerTimeoutSeconds) && workerTimeoutSeconds >= 30 && workerTimeoutSeconds <= 180, "worker timeout must be 30..180 seconds");
assert.ok(Number.isInteger(workerCpuSeconds) && workerCpuSeconds >= 15 && workerCpuSeconds <= workerTimeoutSeconds, "worker CPU limit must be bounded by timeout");
assert.ok(Number.isInteger(workerAddressSpaceMb) && workerAddressSpaceMb >= 256 && workerAddressSpaceMb <= 1024, "worker address-space limit must be 256..1024 MiB");

const referenceRasterDpi = Number(process.env.READER_REFERENCE_RASTER_DPI ?? "0");
const corpusDiagnosticMode = process.env.READER_CORPUS_DIAGNOSTIC === "1";
assert.ok(
  referenceRasterDpi === 0 || (Number.isInteger(referenceRasterDpi) && referenceRasterDpi >= 72 && referenceRasterDpi <= 300),
  "reference raster DPI must be 0 or an integer in 72..300"
);
const temporary = await mkdtemp(join(tmpdir(), "chaptera-real-scene-"));
const results = [];
const errors = [];
const foreign = [];
let active;
let browser;
const assets = new Set(["index.html", "reader.css", "reader-app.mjs", "reader-model.mjs", "render-v1.mjs", "observability-v1.mjs"]);
const server = createServer(async (req, res) => {
  try {
    if (req.url.startsWith("/v1/")) {
      const chunks = [];
      let length = 0;
      for await (const part of req) {
        length += part.length;
        assert.ok(length <= active.fixture.bytes);
        chunks.push(part);
      }
      const bytes = Buffer.concat(chunks);
      const session = active.receipt.session_id;
      const payload = { protocol_version: "chaptera.reader-guest-session.v1", session_id: session };
      if (req.url === "/v1/reader/guest-sessions") {
        Object.assign(payload, { access_token: "controlled-transport-only", upload_path: `/v1/reader/guest-sessions/${session}/content`, open_path: `/v1/reader/guest-sessions/${session}/open` });
      } else if (req.url === `/v1/reader/guest-sessions/${session}/content`) {
        assert.equal(sha256(bytes), active.fixture.sha256);
        Object.assign(payload, { state: "uploaded" });
      } else if (req.url === `/v1/reader/guest-sessions/${session}/open`) {
        Object.assign(payload, {
          classification: active.receipt.classification,
          source_sha256: active.fixture.sha256,
          compatibility_report: compatibilityReport(
            active.receipt.classification,
            active.fixture.sha256,
            active.receipt.scene
          ),
          scene: active.receipt.scene
        });
      } else throw new Error("unexpected_transport_path");
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
      res.end(JSON.stringify(payload));
      return;
    }
    const name = req.url === "/" ? "index.html" : req.url.slice(1);
    if (!assets.has(name)) { res.writeHead(404); res.end(); return; }
    const source = name === "observability-v1.mjs" ? join(root, "../web/observability-v1.mjs") : join(root, name);
    const bytes = await readFile(source);
    res.writeHead(200, { "content-type": name.endsWith(".html") ? "text/html" : name.endsWith(".css") ? "text/css" : "text/javascript" });
    res.end(bytes);
  } catch (error) { errors.push(String(error)); res.writeHead(500); res.end("controlled_transport_failure"); }
});

try {
  await mkdir(output, { recursive: true });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const origin = "http://127.0.0.1:" + server.address().port;
  browser = await chromium.launch({ headless: true, ...(process.env.READER_UI_BROWSER ? { executablePath: process.env.READER_UI_BROWSER } : {}) });
  const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
  page.on("pageerror", (error) => errors.push(String(error)));
  page.on("request", (request) => {
    if (!request.url().startsWith(origin) && !request.url().startsWith("data:")) foreign.push(request.url());
  });
  for (const [index, fixture] of fixtures.entries()) {
    const source = join(temporary, fixture.name + ".pub");
    let bytes;
    if (fixture.source_path) bytes = await readFile(resolve(fixture.source_path));
    else if (process.env.READER_REAL_FIXTURE_DIR) bytes = await readFile(join(process.env.READER_REAL_FIXTURE_DIR, fixture.name + ".pub"));
    else {
      const response = await fetch(`https://raw.githubusercontent.com/apache/poi/942d95d85b15d0dfdb3bc9ba1b4f273f277757c8/test-data/publisher/${fixture.name}.pub`);
      assert.equal(response.ok, true, "pinned public fixture must be acquired");
      bytes = Buffer.from(await response.arrayBuffer());
    }
    assert.equal(bytes.length, fixture.bytes);
    assert.equal(sha256(bytes), fixture.sha256);
    await writeFile(source, bytes);
    const workerOutput = join(temporary, fixture.name + "-worker");
    let isolation;
    try {
      const { stdout } = await run("python3", [join(repo, "tools/migration_pdf_worker_isolation.py"), "run",
        "--output-dir", workerOutput, "--input", source, "--timeout", String(workerTimeoutSeconds),
        "--address-space-mb", String(workerAddressSpaceMb),
        "--cpu-seconds", String(workerCpuSeconds), "--open-files", "64", "--output-file-mb", "32", "--clear-environment", "--",
        worker, "guest-reader-scene", "--session-id", "guest:" + String(index + 1).padStart(32, "0"),
        "--expected-sha256", fixture.sha256, "--expected-byte-len", String(fixture.bytes)],
        { cwd: repo, timeout: (workerTimeoutSeconds + 10) * 1000, maxBuffer: 1024 * 1024 });
      isolation = JSON.parse(stdout);
      assert.equal(isolation.status, "success");
      assert.equal(isolation.network_policy, "seccomp_default_deny");
    } catch (error) {
      if (!continueWorkerFailures || fixture.require_render === true) throw error;
      let failure = null;
      try {
        failure = JSON.parse(String(error.stdout ?? ""));
      } catch {}
      results.push({
        fixture: fixture.name,
        source_sha256: fixture.sha256,
        source_byte_len: fixture.bytes,
        classification: "unsupported",
        terminal_code: "reader_worker_isolation_failed",
        rendered: false,
        worker_failure: failure ? {
          status: failure.status ?? null,
          exit_code: failure.exit_code ?? null,
          timed_out: failure.timed_out ?? null,
        } : null,
        filesystem_confinement: null,
        network_policy: failure?.network_policy ?? "seccomp_default_deny",
        screenshots: []
      });
      console.log(JSON.stringify({
        fixture: fixture.name,
        classification: "unsupported",
        terminal_code: "reader_worker_isolation_failed",
        worker_exit_code: failure?.exit_code ?? null
      }));
      continue;
    }
    const receiptBytes = await readFile(join(workerOutput, "result.json"));
    const receipt = JSON.parse(receiptBytes);
    assert.equal(receipt.filesystem_confinement, true);
    assert.equal(receipt.source_sha256, fixture.sha256);
    assert.equal(receipt.source_byte_len, fixture.bytes);
    assert.ok(["partial", "supported", "unsupported"].includes(receipt.classification));
    if (receipt.classification === "unsupported") {
      results.push({
        fixture: fixture.name,
        source_sha256: fixture.sha256,
        source_byte_len: fixture.bytes,
        classification: receipt.classification,
        terminal_code: receipt.terminal_code ?? null,
        rendered: false,
        worker_receipt_sha256: sha256(receiptBytes),
        filesystem_confinement: true,
        network_policy: isolation.network_policy,
        screenshots: []
      });
      console.log(JSON.stringify({ fixture: fixture.name, classification: receipt.classification, terminal_code: receipt.terminal_code ?? null }));
      assert.equal(fixture.require_render === true, false, "required reference fixture must render");
      continue;
    }
    const scene = receipt.scene;
    assert.equal(scene.protocol_version, "chaptera.reader-scene.v1");
    if (fixture.pages != null) assert.equal(scene.pages.length, fixture.pages);
    const fixturePages = scene.pages.length;
    active = { fixture, receipt };
    await page.goto(origin);
    await page.locator("#pub-file").setInputFiles(source);
    await page.waitForFunction(() => document.querySelectorAll("#pages svg.page").length > 0);
    await page.evaluate(() => document.fonts.ready);
    assert.equal(await page.locator("#pages svg.page").count(), fixturePages);
    const expectedNodeOrderByPage = [...scene.pages]
      .sort((left, right) => left.order - right.order)
      .map((pageModel) => ({
        page_id: pageModel.page_id,
        node_ids: scene.nodes
          .filter((node) => node.page_id === pageModel.page_id)
          .map((node) => node.node_id)
      }));
    const paintedNodeOrderByPage = await page.locator("#pages svg.page").evaluateAll((pages) => pages.map((svg) => ({
      page_id: svg.dataset.pageId,
      node_ids: [...svg.querySelectorAll(":scope > g[data-node-id]")].map((node) => node.dataset.nodeId)
    })));
    assert.deepEqual(
      paintedNodeOrderByPage,
      expectedNodeOrderByPage,
      "browser must preserve server scene node order within each page"
    );
    const browserPreviewCensus = await page.locator('[data-text-authority="browser-preview-only"]').evaluateAll((elements) => {
      const kinds = new Set(["text_frame", "other_node_text", "table_cell"]);
      const reasons = new Set([
        "scene_layout_missing", "shared_plan_invalid", "server_layout_unavailable",
        "base_font_unavailable", "span_font_unavailable", "span_font_fingerprint_mismatch",
        "invalid_text_viewport", "table_cell_preview"
      ]);
      const sizeSources = new Set(["shared_resolved_plan", "source_uniform_preview", "generic_9pt"]);
      const byKind = {}, byReason = {}, bySizeSource = {}, byPage = {}, byPageCause = {};
      const pages = [...document.querySelectorAll("#pages svg.page")];
      for (const element of elements) {
        const kind = element.getAttribute("data-preview-kind");
        const reason = element.getAttribute("data-preview-reason");
        const sizeSource = element.getAttribute("data-preview-size-source");
        if (!kinds.has(kind) || !reasons.has(reason) || !sizeSources.has(sizeSource)) {
          throw new Error("browser preview census has an unknown paint decision");
        }
        const page = pages.indexOf(element.closest("svg.page")) + 1;
        if (page < 1) throw new Error("browser preview was not painted inside an SVG page");
        for (const [counts, key] of [
          [byKind, kind], [byReason, reason], [bySizeSource, sizeSource],
          [byPage, String(page)], [byPageCause, page + "|" + kind + "|" + reason + "|" + sizeSource]
        ]) counts[key] = (counts[key] ?? 0) + 1;
      }
      return { total: elements.length, by_kind: byKind, by_reason: byReason,
        by_size_source: bySizeSource, by_page: byPage, by_page_cause: byPageCause };
    });
    assert.equal(
      Object.values(browserPreviewCensus.by_kind).reduce((sum, count) => sum + count, 0),
      browserPreviewCensus.total,
      "browser preview census must count only actually painted preview elements"
    );
    const expectedLines = scene.nodes.flatMap((node) => node.text_layout?.disposition === "shared_resolved"
      ? node.text_layout.lines.map((line) => ({ node_id: node.node_id, index: line.line_index, text: line.text, font_size: node.text_layout.font_size_emu / 9525 })) : []);
    const painted = await page.locator('[data-text-authority="server-shared-resolved"]').evaluateAll((lines) => lines.map((line) => {
      const bounds = line.getBoundingClientRect();
      const node = line.closest("[data-node-id]");
      const svg = line.closest("svg.page");
      const pageBounds = svg?.getBoundingClientRect();
      const viewBox = svg?.viewBox?.baseVal ?? null;
      const pageScreenScale = pageBounds && viewBox && viewBox.width > 0 && viewBox.height > 0
        ? Math.min(pageBounds.width / viewBox.width, pageBounds.height / viewBox.height)
        : null;
      const normalizedWidthPx96 = pageScreenScale && pageScreenScale > 0
        ? bounds.width / pageScreenScale / 9525
        : bounds.width;
      const normalizedHeightPx96 = pageScreenScale && pageScreenScale > 0
        ? bounds.height / pageScreenScale / 9525
        : bounds.height;
      const ctm = typeof line.getCTM === "function" ? line.getCTM() : null;
      const screenCtm = typeof line.getScreenCTM === "function" ? line.getScreenCTM() : null;
      return {
        node_id: node.dataset.nodeId,
        index: Number(line.dataset.textLineIndex),
        text: line.textContent,
        font_size: parseFloat(getComputedStyle(line).fontSize),
        height: bounds.height,
        width: bounds.width,
        normalized_width_px_96: normalizedWidthPx96,
        normalized_height_px_96: normalizedHeightPx96,
        page_screen_scale_px_per_emu: pageScreenScale,
        node_transform: node.getAttribute("transform"),
        ctm: ctm ? { a: ctm.a, b: ctm.b, c: ctm.c, d: ctm.d, e: ctm.e, f: ctm.f } : null,
        screen_ctm: screenCtm ? { a: screenCtm.a, b: screenCtm.b, c: screenCtm.c, d: screenCtm.d, e: screenCtm.e, f: screenCtm.f } : null,
        page: svg ? {
          page_id: svg.dataset.pageId,
          width_px: pageBounds?.width ?? null,
          height_px: pageBounds?.height ?? null,
          width_attr: svg.getAttribute("width"),
          height_attr: svg.getAttribute("height"),
          view_box: svg.getAttribute("viewBox")
        } : null
      };
    }));
    assert.equal(painted.length, expectedLines.length, "actual SharedResolved frames must use loaded fonts");
    if (fixture.require_shared_text === true) {
      assert.ok(painted.some((line) => line.text.trim()), "fixture marked require_shared_text must exercise shared text");
    }
    const visualDegeneracies = [];
    for (const line of painted) {
      const expected = expectedLines.find((candidate) => candidate.node_id === line.node_id && candidate.index === line.index);
      assert.ok(expected);
      assert.equal(line.text, expected.text, "server line breaks/content must survive painting");
      assert.ok(Math.abs(line.font_size - expected.font_size) < 0.001, "real shared font must not be browser-clamped");
      // U+200B is the bounded render-only replacement for a projected Cmo
      // object marker. It is intentionally zero-width, so marker-only lines
      // are visually empty even though JavaScript trim() retains U+200B.
      const visibleText = line.text.replaceAll("\u200B", "").trim();
      if (visibleText && !(line.normalized_height_px_96 >= 5 && line.normalized_width_px_96 >= 1)) {
        const degeneracy = {
          code: "shared_text_tiny_speck",
          node_id: line.node_id,
          line_index: line.index,
          width_px: line.width,
          height_px: line.height,
          normalized_width_px_96: line.normalized_width_px_96,
          normalized_height_px_96: line.normalized_height_px_96,
          page_screen_scale_px_per_emu: line.page_screen_scale_px_per_emu,
          font_size_px: line.font_size,
          node_transform: line.node_transform,
          ctm: line.ctm,
          screen_ctm: line.screen_ctm,
          page: line.page
        };
        if (corpusDiagnosticMode) visualDegeneracies.push(degeneracy);
        else assert.fail("shared text must not collapse to tiny specks");
      }
    }
    await page.locator("#pages").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(output, fixture.name + "-ui.png") });
    // Pure page rasters remove only ancestor viewport clipping for capture.
    await page.locator(".viewer").evaluate((element) => { element.style.height = "auto"; element.style.overflow = "visible"; });
    const orderedPageGeometry = [...scene.pages]
      .sort((left, right) => left.order - right.order)
      .map((pageModel) => ({
        page_id: pageModel.page_id,
        page_identity_sha256: sha256(Buffer.from(pageModel.page_id, "utf8")),
        order: pageModel.order,
        width_emu: pageModel.width_emu,
        height_emu: pageModel.height_emu
      }));
    if (referenceRasterDpi > 0) {
      const emuPerPixel = 914400 / referenceRasterDpi;
      const referenceZoom = String(referenceRasterDpi / 96);
      assert.equal(
        await page.locator(`#zoom-select option[value="${referenceZoom}"]`).count(),
        1,
        "reference raster DPI must map to an exact supported browser zoom"
      );
      await page.locator("#zoom-select").selectOption(referenceZoom);
      await page.locator("#pages svg.page").evaluateAll((svgs, argument) => {
        const byId = new Map(argument.pages.map((entry) => [entry.page_id, entry]));
        for (const svg of svgs) {
          const geometry = byId.get(svg.dataset.pageId);
          if (!geometry) throw new Error("reference raster page geometry missing");
          svg.setAttribute("width", String(Math.round(geometry.width_emu / argument.emuPerPixel)));
          svg.setAttribute("height", String(Math.round(geometry.height_emu / argument.emuPerPixel)));
        }
      }, { pages: orderedPageGeometry, emuPerPixel });
      const capturedPageSizes = await page.locator("#pages svg.page").evaluateAll((svgs) =>
        svgs.map((svg) => {
          const bounds = svg.getBoundingClientRect();
          return { width: bounds.width, height: bounds.height };
        })
      );
      for (let i = 0; i < orderedPageGeometry.length; i++) {
        assert.ok(
          Math.abs(capturedPageSizes[i].width - orderedPageGeometry[i].width_emu / emuPerPixel) < 1,
          "reference raster page width must stay at requested DPI"
        );
        assert.ok(
          Math.abs(capturedPageSizes[i].height - orderedPageGeometry[i].height_emu / emuPerPixel) < 1,
          "reference raster page height must stay at requested DPI"
        );
      }
    }
    const screenshots = [];
    for (let i = 0; i < fixturePages; i++) {
      const filename = `${fixture.name}-page-${i + 1}.png`;
      const pageSvg = page.locator("#pages svg.page").nth(i);
      if (referenceRasterDpi > 0) {
        await pageSvg.evaluate((svg) => {
          svg.style.position = "fixed";
          svg.style.left = "0";
          svg.style.top = "0";
          svg.style.zIndex = "2147483647";
        });
      }
      const png = await pageSvg.screenshot({ path: join(output, filename) });
      if (referenceRasterDpi > 0) {
        await pageSvg.evaluate((svg) => {
          svg.style.removeProperty("position");
          svg.style.removeProperty("left");
          svg.style.removeProperty("top");
          svg.style.removeProperty("z-index");
        });
      }
      screenshots.push({ page: i + 1, filename, sha256: sha256(png) });
    }
    const nonempty = painted.filter((line) => line.text.trim());
    const fidelityReasons = [...(scene.fidelity?.reasons ?? [])].sort();
    if (scene.stacking_fidelity === "source_back_to_front") {
      assert.equal(
        fidelityReasons.includes("stacking_order_unavailable"),
        false,
        "grounded source stacking must remove the unavailable reason"
      );
    } else {
      assert.equal(scene.stacking_fidelity, "unknown");
      assert.equal(
        fidelityReasons.includes("stacking_order_unavailable"),
        scene.nodes.length > 0,
        "unknown non-empty scenes must keep stacking_order_unavailable explicit"
      );
    }
    const diagnosticCodes = [...new Set((scene.diagnostics ?? []).map((diagnostic) =>
      diagnostic.severity + ":" + diagnostic.code
    ))].sort();
    const nodeKindCounts = {};
    const textLayoutDispositionCounts = {};
    for (const node of scene.nodes) {
      nodeKindCounts[node.kind] = (nodeKindCounts[node.kind] ?? 0) + 1;
      const disposition = node.text_layout?.disposition ?? "none";
      textLayoutDispositionCounts[disposition] = (textLayoutDispositionCounts[disposition] ?? 0) + 1;
    }
    const descriptorOnlyResourceCount = (scene.resources ?? [])
      .filter((resource) => resource.availability !== "inline_data_url").length;
    results.push({ fixture: fixture.name, source_sha256: fixture.sha256, source_byte_len: fixture.bytes,
      classification: receipt.classification, rendered: true, fidelity: scene.fidelity, stacking_fidelity: scene.stacking_fidelity,
      fidelity_reasons: fidelityReasons, diagnostic_codes: diagnosticCodes, pages: fixturePages, nodes: scene.nodes.length,
      node_kind_counts: nodeKindCounts, text_layout_disposition_counts: textLayoutDispositionCounts,
      text_layout_fallback_counts: scene.text_layout_fallback_counts ?? {},
      browser_preview_census: browserPreviewCensus,
      descriptor_only_resource_count: descriptorOnlyResourceCount, browser_preserved_scene_node_order: true,
      reference_raster_dpi: referenceRasterDpi || null, page_geometry: orderedPageGeometry,
      stories: scene.stories.length, shared_lines: painted.length, nonempty_shared_lines: nonempty.length,
      visual_degeneracies: visualDegeneracies, visual_degeneracy_count: visualDegeneracies.length,
      shared_line_height_px: nonempty.length > 0
        ? { min: Math.min(...nonempty.map((line) => line.height)), max: Math.max(...nonempty.map((line) => line.height)) }
        : null,
      worker_receipt_sha256: sha256(receiptBytes), filesystem_confinement: true, network_policy: isolation.network_policy, screenshots });
    console.log(JSON.stringify({ fixture: fixture.name, classification: receipt.classification, pages: fixturePages, readable_shared_lines: nonempty.length }));
  }
  assert.deepEqual(errors, []);
  assert.deepEqual(foreign, []);
  if (!manifestPath) {
    assert.ok(
      results.some((result) => result.stacking_fidelity === "source_back_to_front"),
      "default pinned real PUB set must contain at least one fully grounded source stacking witness"
    );
  }
  await writeFile(join(output, "receipt.json"), JSON.stringify({ protocol: "chaptera.cloud-reader-real-scene-browser.v1",
    repository_commit_sha: process.env.REPOSITORY_COMMIT_SHA ?? "local-uncommitted", browser: await browser.version(),
    scope: "pinned public PUB -> existing isolated Scene worker -> controlled HTTP -> browser; no live scanning/storage/TTL or Publisher-reference parity claim",
    source_pub_bytes_emitted: false, results, errors, foreign }, null, 2) + "\n");
} finally {
  if (browser) await browser.close();
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  await rm(temporary, { recursive: true, force: true });
}
