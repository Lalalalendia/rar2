// Renderer scale evidence on synthetic scenes, not real-PUB reference parity.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { createHash } from "node:crypto";
import { chromium } from "playwright";

const root = dirname(fileURLToPath(import.meta.url));
const output = resolve(process.env.READER_RENDER_OUTPUT ?? join(root, "../../target/cloud-reader-render"));
const emu = (px) => px * 9525;
const rectangle = (x, y, width, height) => ({ x: emu(x), y: emu(y), width: emu(width), height: emu(height) });
const pixelPng = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==";
// The existing server fallback provider pins these public bytes; this fixture
// exercises a loaded font, without adding fonts to the static release.
const fontId = "chaptera.desktop.fallback-font.ubuntu-light.v1";
const fontSha = "80307b8da7649aa4ee4d484b232140e3ce1ec0ca093073d3c53c8f5a5ced7a70";
let fontBytes;
if (process.env.READER_FALLBACK_FONT) fontBytes = await readFile(process.env.READER_FALLBACK_FONT);
else {
  const response = await fetch("https://raw.githubusercontent.com/emilk/egui/1669e52a7ccfc3489c1b0999b9ed48894a0b3887/crates/epaint_default_fonts/fonts/Ubuntu-Light.ttf");
  assert.equal(response.ok, true, "pinned fallback font acquisition must succeed");
  fontBytes = Buffer.from(await response.arrayBuffer());
}
assert.equal(fontBytes.length, 361676);
assert.equal(createHash("sha256").update(fontBytes).digest("hex"), fontSha);
const scene = {
  protocol_version: "chaptera.reader-scene.v1",
  pages: [{ page_id: "p", order: 0, width_emu: emu(600), height_emu: emu(400) }],
  nodes: [
    { node_id: "text", page_id: "p", kind: "text", bounds: rectangle(30, 30, 240, 60), text: "Visible preview text\nSecond line.",
      preview_text_style: { font_resource_id: fontId } },
    { node_id: "table", page_id: "p", kind: "table", bounds: rectangle(30, 130, 500, 80), table: {
      story_id: "story", rows: 1, columns: 2, cells: [
        { cell_id: "a", row: 0, column: 0, bounds: rectangle(30, 130, 240, 80), text: "Visible table cell" },
        { cell_id: "b", row: 0, column: 1, bounds: rectangle(290, 130, 240, 80), text: "Second cell" }
      ]
    } },
    { node_id: "border", page_id: "p", kind: "shape", bounds: rectangle(350, 250, 200, 100),
      paint: { line: { rgb: [255, 0, 0], width_emu: emu(4) } },
      decorative_border: { placements: [
        { slot: "top_left", resource_id: "border-r", bounds: rectangle(350, 250, 20, 20) }
      ] } },
    { node_id: "unresolved-font", page_id: "p", kind: "text", bounds: rectangle(30, 260, 300, 80), text: "Fallback after unavailable font", text_layout: {
      disposition: "shared_resolved", font_resource_id: "missing", font_size_emu: emu(16), line_height_emu: emu(20),
      lines: [{ line_index: 0, text: "Fallback after unavailable font", measured_width_emu: emu(240), line_height_emu: emu(20) }]
    } },
    { node_id: "resolved", page_id: "p", kind: "text", bounds: rectangle(350, 30, 220, 80),
      transform: { a: 1, b: 0, c: 0, d: 1, tx: emu(5), ty: emu(7) }, text: "Server line A\nServer line B", text_layout: {
        disposition: "shared_resolved", font_resource_id: fontId, font_size_emu: emu(16), line_height_emu: emu(20),
        lines: [{ line_index: 0, text: "Server line A", measured_width_emu: emu(120), line_height_emu: emu(20) },
          { line_index: 1, text: "Server line B", measured_width_emu: emu(120), line_height_emu: emu(20) }]
      }
    }
  ], stories: [], resources: [
    { resource_id: "border-r", mime: "image/png", availability: "inline_data_url", inline_data_url: pixelPng }
  ], fonts: [{ resource_id: fontId, expected_sha256: fontSha,
    availability: "inline_data_url", inline_data_url: "data:font/ttf;base64," + fontBytes.toString("base64") }]
};
const server = createServer(async (request, response) => {
  if (request.url === "/render-v1.mjs") {
    response.writeHead(200, { "content-type": "text/javascript" });
    response.end(await readFile(join(root, "render-v1.mjs")));
  } else {
    response.writeHead(200, { "content-type": "text/html" });
    response.end('<!doctype html><html lang="en"><title>Renderer scale fixture</title><body style="margin:0;background:#e9edf3"><main id="pages" style="padding:20px"></main></body></html>');
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  await mkdir(output, { recursive: true });
  browser = await chromium.launch({ headless: true, ...(process.env.READER_UI_BROWSER ? { executablePath: process.env.READER_UI_BROWSER } : {}) });
  const page = await browser.newPage({ viewport: { width: 680, height: 460 } });
  await page.goto("http://127.0.0.1:" + server.address().port);
  await page.evaluate(async (scene) => {
    const { renderReaderScene } = await import("/render-v1.mjs");
    await renderReaderScene(document.querySelector("#pages"), scene);
  }, scene);
  const measurements = await page.locator("foreignObject").evaluateAll((elements) => elements.map((element) => {
    const div = element.firstElementChild;
    const range = document.createRange();
    range.selectNodeContents(div);
    const text = range.getBoundingClientRect();
    const frame = element.getBoundingClientRect();
    return { node_id: element.closest("[data-node-id]").getAttribute("data-node-id"), cell_id: element.getAttribute("data-table-cell-id"),
      authority: element.getAttribute("data-text-authority"), text_height_px: text.height, text_width_px: text.width,
      frame: [frame.x, frame.y, frame.width, frame.height],
      inside_frame: text.x >= frame.x - 0.1 && text.right <= frame.right + 0.1 && text.y >= frame.y - 0.1 && text.bottom <= frame.bottom + 0.1 };
  }));
  await page.screenshot({ path: join(output, "preview-text.png") });
  assert.equal(measurements.length, 4);
  const expectedFrames = [[50, 50, 240, 60], [50, 150, 240, 80], [310, 150, 240, 80], [50, 280, 300, 80]];
  measurements.forEach((measurement, index) => {
    measurement.frame.forEach((value, component) => assert.ok(Math.abs(value - expectedFrames[index][component]) < 0.1,
      "preview CSS conversion must preserve canonical page-space bounds"));
  });
  for (const measurement of measurements) {
    assert.ok(measurement.text_height_px >= 8, "preview text must remain readable in screen pixels: " + JSON.stringify(measurement));
    assert.ok(measurement.text_width_px >= 30, "text must not collapse to an EMU-sized speck: " + JSON.stringify(measurement));
    assert.equal(measurement.inside_frame, true);
    assert.equal(measurement.authority, "browser-preview-only");
  }
  const previewMetadata = await page.locator('[data-text-authority="browser-preview-only"]')
    .evaluateAll((elements) => elements.map((element) => ({
      node: element.closest("[data-node-id]")?.getAttribute("data-node-id"),
      kind: element.getAttribute("data-preview-kind"),
      reason: element.getAttribute("data-preview-reason"),
      sizeSource: element.getAttribute("data-preview-size-source")
    })));
  assert.deepEqual(previewMetadata, [
    { node: "text", kind: "other_node_text", reason: "scene_layout_missing", sizeSource: "generic_9pt" },
    { node: "table", kind: "table_cell", reason: "table_cell_preview", sizeSource: "generic_9pt" },
    { node: "table", kind: "table_cell", reason: "table_cell_preview", sizeSource: "generic_9pt" },
    { node: "unresolved-font", kind: "other_node_text", reason: "base_font_unavailable",
      sizeSource: "shared_resolved_plan" }
  ]);
  const replacementPreview = await page.locator('[data-node-id="text"] [data-text-authority="browser-preview-only"]').evaluate((element) => ({
    resource_id: element.getAttribute("data-preview-font-resource-id"),
    authority: element.getAttribute("data-preview-font-authority"),
    font_family: getComputedStyle(element.firstElementChild).fontFamily
  }));
  assert.equal(replacementPreview.resource_id, fontId);
  assert.equal(replacementPreview.authority, "configured-replacement");
  assert.ok(replacementPreview.font_family.includes("ChapteraReader_80307b8da7649aa4"),
    "paint-only replacement must affect browser preview glyphs without a shared layout");
  const shared = await page.locator('[data-text-authority="server-shared-resolved"]').evaluateAll((lines) => lines.map((line) => {
    const bounds = line.getBoundingClientRect();
    return { text: line.textContent, font_size_px: parseFloat(getComputedStyle(line).fontSize),
      font_family: getComputedStyle(line).fontFamily, x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height };
  }));
  assert.deepEqual(shared.map((line) => line.text), ["Server line A", "Server line B"]);
  for (const line of shared) {
    assert.equal(line.font_size_px, 16, "SVG font-size must not hit Chromium's 10000px clamp");
    assert.ok(line.font_family.includes("ChapteraReader_80307b8da7649aa4"));
    assert.ok(line.height >= 12 && line.height <= 24, "loaded shared text must have a visible physical size");
    assert.ok(line.width > 60 && line.width < 200);
    assert.ok(Math.abs(line.x - 375) < 0.1, "canonical x plus node transform must be preserved");
  }
  assert.ok(Math.abs(shared[1].y - shared[0].y - 20) < 0.1, "server line-height, not browser reflow, places lines");
  const textViewport = await page.locator('[data-node-id="resolved"] [data-text-viewport="fixed-frame"]').evaluate((element) => ({
    frame: ["x", "y", "width", "height"].map((key) => Number(element.getAttribute(key))),
    view_box: element.getAttribute("viewBox"),
    overflow: element.getAttribute("overflow")
  }));
  assert.deepEqual(
    textViewport.frame,
    [emu(350), emu(30), emu(220), emu(80)],
    "shared-text viewport remains in canonical node space"
  );
  assert.equal(textViewport.view_box, "0 0 220 80", "shared-text clip uses local CSS-pixel coordinates");
  assert.equal(textViewport.overflow, "hidden", "shared-text viewport remains a fixed-frame clip");
  const borderArt = page.locator('[data-node-id="border"] [data-decorative-border-slot="top_left"]');
  assert.equal(await borderArt.count(), 1, "source-backed decorative border placement must paint as an image");
  assert.deepEqual(
    await borderArt.evaluate((element) => ["x", "y", "width", "height"].map((key) => Number(element.getAttribute(key)))),
    [emu(350), emu(250), emu(20), emu(20)]
  );
  assert.equal(
    await page.locator('[data-node-id="border"] rect').count(),
    0,
    "ordinary line stroke must not paint when decorative BorderArt is present"
  );

  const before = await page.locator("#pages > svg.page").getAttribute("viewBox");
  const sharedMatrixBefore = await page.locator('[data-text-authority="server-shared-resolved"]').first().evaluate((element) => {
    const matrix = element.getScreenCTM(); return [matrix.a, matrix.b, matrix.c, matrix.d];
  });
  await page.locator("#pages > svg.page").evaluate((svg) => { svg.setAttribute("width", 300); svg.setAttribute("height", 200); });
  const scaledHeight = await page.locator("foreignObject").first().evaluate((element) => {
    const range = document.createRange(); range.selectNodeContents(element.firstElementChild); return range.getBoundingClientRect().height;
  });
  assert.ok(Math.abs(scaledHeight * 2 - measurements[0].text_height_px) < 0.1);
  const scaledSharedHeight = await page.locator('[data-text-authority="server-shared-resolved"]').first().evaluate((element) => element.getBoundingClientRect().height);
  const sharedMatrixAfter = await page.locator('[data-text-authority="server-shared-resolved"]').first().evaluate((element) => {
    const matrix = element.getScreenCTM(); return [matrix.a, matrix.b, matrix.c, matrix.d];
  });
  sharedMatrixAfter.forEach((value, index) => assert.ok(Math.abs(value * 2 - sharedMatrixBefore[index]) < 0.00001,
    "shared line paint must follow the exact SVG page transform"));
  // geometricPrecision keeps glyph extents in continuous SVG coordinates,
  // including headless Chromium, which otherwise hints these extents.
  assert.ok(Math.abs(scaledSharedHeight * 2 - shared[0].height) < 0.1,
    "shared glyph geometry must follow zoom: " + JSON.stringify({ before: shared[0].height, after: scaledSharedHeight }));
  assert.equal(await page.locator("#pages > svg.page").getAttribute("viewBox"), before);
  const receipt = { protocol: "chaptera.cloud-reader-preview-scale.v1", scope: "synthetic renderer readability only; excludes source typography and real-PUB reference parity",
    repository_commit_sha: process.env.REPOSITORY_COMMIT_SHA ?? "local-uncommitted", browser: await browser.version(), measurements,
    shared_lines: shared, loaded_fallback_font_sha256: fontSha, shared_zoom: { before_height_px: shared[0].height,
      after_height_px: scaledSharedHeight, matrix_before: sharedMatrixBefore, matrix_after: sharedMatrixAfter }, zoom_preserves_geometry: true };
  await writeFile(join(output, "receipt.json"), JSON.stringify(receipt, null, 2) + "\n");
  console.log(JSON.stringify({ readable_preview_frames: measurements.length, readable_shared_lines: shared.length, zoom_preserves_geometry: true, receipt: join(output, "receipt.json") }));
} finally {
  if (browser) await browser.close();
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}
