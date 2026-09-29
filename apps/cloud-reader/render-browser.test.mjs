// Renderer scale evidence on synthetic scenes, not real-PUB reference parity.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { chromium } from "playwright";

const root = dirname(fileURLToPath(import.meta.url));
const output = resolve(process.env.READER_RENDER_OUTPUT ?? join(root, "../../target/cloud-reader-render"));
const emu = (px) => px * 9525;
const rectangle = (x, y, width, height) => ({ x: emu(x), y: emu(y), width: emu(width), height: emu(height) });
const scene = {
  protocol_version: "chaptera.reader-scene.v1",
  pages: [{ page_id: "p", order: 0, width_emu: emu(600), height_emu: emu(400) }],
  nodes: [
    { node_id: "text", page_id: "p", kind: "text", bounds: rectangle(30, 30, 240, 60), text: "Visible preview text\nSecond line." },
    { node_id: "table", page_id: "p", kind: "table", bounds: rectangle(30, 130, 500, 80), table: {
      story_id: "story", rows: 1, columns: 2, cells: [
        { cell_id: "a", row: 0, column: 0, bounds: rectangle(30, 130, 240, 80), text: "Visible table cell" },
        { cell_id: "b", row: 0, column: 1, bounds: rectangle(290, 130, 240, 80), text: "Second cell" }
      ]
    } },
    { node_id: "unresolved-font", page_id: "p", kind: "text", bounds: rectangle(30, 260, 300, 80), text: "Fallback after unavailable font", text_layout: {
      disposition: "shared_resolved", font_resource_id: "missing", font_size_emu: emu(16), line_height_emu: emu(20),
      lines: [{ line_index: 0, text: "Fallback after unavailable font", measured_width_emu: emu(240), line_height_emu: emu(20) }]
    } }
  ], stories: [], resources: []
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
  const before = await page.locator("svg").getAttribute("viewBox");
  await page.locator("svg").evaluate((svg) => { svg.setAttribute("width", 300); svg.setAttribute("height", 200); });
  const scaledHeight = await page.locator("foreignObject").first().evaluate((element) => {
    const range = document.createRange(); range.selectNodeContents(element.firstElementChild); return range.getBoundingClientRect().height;
  });
  assert.ok(Math.abs(scaledHeight * 2 - measurements[0].text_height_px) < 0.1);
  assert.equal(await page.locator("svg").getAttribute("viewBox"), before);
  const receipt = { protocol: "chaptera.cloud-reader-preview-scale.v1", scope: "synthetic renderer readability only; excludes source typography and real-PUB reference parity",
    repository_commit_sha: process.env.REPOSITORY_COMMIT_SHA ?? "local-uncommitted", browser: await browser.version(), measurements, zoom_preserves_geometry: true };
  await writeFile(join(output, "receipt.json"), JSON.stringify(receipt, null, 2) + "\n");
  console.log(JSON.stringify({ readable_preview_frames: measurements.length, zoom_preserves_geometry: true, receipt: join(output, "receipt.json") }));
} finally {
  if (browser) await browser.close();
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}
