// Private source-bound Chromium proof for the exact edited SBDC034 Guest revision.
// Consumes only a short-lived, mode-0600 input inside the CI runner. Never
// writes customer PUB, Scene, image, screenshots or bearer into artifacts.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { chromium } from "playwright";

const SHA = "0d2278960a0101ced0a1ba6d5922f5a26ea37b76b3ab00ef21e21873359d4a5a";
function argument(flag) {
  const i = process.argv.indexOf("--" + flag);
  if (i < 0 || !process.argv[i + 1]) throw Error("required private browser argument missing");
  return process.argv[i + 1];
}
const origin = argument("origin");
assert.match(origin, /^https:\/\/reader\.chaptera\.test:\d+$/);
const input = JSON.parse(await readFile(argument("private-input"), "utf8"));
assert.equal(input.schema, "chaptera.sbdc034-private-r1-browser-input.v1");
assert.equal(input.source_sha256, SHA);
const record = input.browser_record;
assert.equal(record.schema, "chaptera.temporary-edit.browser.v0");
assert.match(record.session_id, /^tempedit:[0-9a-f]{32}$/);
assert.match(record.access_token, /^[0-9a-f]{64}$/);
assert.equal(record.source_sha256, SHA);
assert.ok(Number.isSafeInteger(record.expires_at_ms));
const expected = input.expected;
assert.match(expected.revision_id, /^sha256:[0-9a-f]{64}$/);
assert.equal(typeof expected.star_resource_id, "string");
assert.equal(typeof expected.logo_resource_id, "string");
assert.notEqual(expected.star_resource_id, expected.logo_resource_id);
assert.ok(Array.isArray(expected.star_frames) && expected.star_frames.length === 4);
assert.equal(new Set(expected.star_frames.map(x => x.node_id)).size, 4);
assert.equal(new Set(expected.star_frames.map(x => x.page_id)).size, 1);
for (const frame of expected.star_frames) {
  assert.ok(typeof frame.node_id === "string" && typeof frame.page_id === "string");
  assert.ok(["x", "y", "width", "height"].every(k => Number.isSafeInteger(frame.bounds?.[k])));
  assert.ok(frame.bounds.width > 0 && frame.bounds.height > 0);
}

const browser = await chromium.launch({
  headless: true,
  args: [
    "--no-proxy-server",
    "--host-resolver-rules=MAP reader.chaptera.test 127.0.0.1",
  ],
});
try {
  const page = await browser.newPage({
    ignoreHTTPSErrors: true,
    viewport: { width: 1280, height: 1000 },
    deviceScaleFactor: 1,
  });
  const errors = [];
  const external = [];
  const currentReplies = [];
  const sceneReplies = [];
  const reads = [];
  page.on("pageerror", error => errors.push(error.name || "PageError"));
  page.on("request", request => {
    if (/^https?:\/\//.test(request.url()) && !request.url().startsWith(origin + "/")) {
      external.push(1);
    }
  });
  page.on("response", response => {
    if (response.status() !== 200 || !response.url().startsWith(origin + "/")) return;
    const path = new URL(response.url()).pathname;
    let destination = null;
    if (/\/v1\/editor\/temporary-sessions\/[^/]+\/scene$/.test(path)) destination = sceneReplies;
    if (/\/v1\/editor\/temporary-sessions\/[^/]+\/current$/.test(path)) destination = currentReplies;
    if (destination !== null) {
      reads.push(response.json().then(value => destination.push(value)));
    }
  });
  // Resume an EXISTING server-accepted R1 session. This script does not fake a
  // scene response or a MoveNode, does not upload a second copy of the source,
  // and only uses real read-only Guest current/scene requests.
  await page.addInitScript(({ record }) => {
    if (location.hostname === "reader.chaptera.test") {
      sessionStorage.setItem("chaptera.temporary-edit.v0", JSON.stringify(record));
    }
  }, { record });
  const navigation = await page.goto(origin + "/editor/temporary-preview", {
    waitUntil: "domcontentloaded",
    timeout: 30000,
  });
  assert.equal(navigation?.status(), 200);
  await page.locator("#temporary-resume").waitFor({ state: "visible", timeout: 15000 });
  await page.locator("#temporary-resume").click();
  await page.waitForFunction(() => {
    const status = document.querySelector("#temporary-status")?.textContent ?? "";
    return /(?:PUB открыт|Открыта редактируемая ревизия)/.test(status)
      && document.querySelectorAll("#temporary-pages svg.page").length > 0;
  }, null, { timeout: 90000 });
  await Promise.all(reads);
  const current = currentReplies.at(-1);
  const sceneReply = sceneReplies.at(-1);
  assert.equal(current?.source_hash, SHA);
  assert.equal(current?.revision_id, expected.revision_id);
  assert.equal(current?.revision_cursor, 1);
  assert.equal(sceneReply?.source_hash, SHA);
  assert.equal(sceneReply?.revision_id, expected.revision_id);
  assert.equal(sceneReply?.scene?.source_hash, SHA);
  assert.equal(sceneReply?.scene?.revision_id, expected.revision_id);
  assert.equal(sceneReply?.document_id, record.session_id);
  assert.equal(current?.expires_at_ms, record.expires_at_ms);
  assert.equal(sceneReply?.expires_at_ms, record.expires_at_ms);

  const actualScene = sceneReply.scene;
  for (const frame of expected.star_frames) {
    const sceneNodes = actualScene.nodes.filter(n => n.node_id === frame.node_id);
    assert.equal(sceneNodes.length, 1, "source-bound star node lost after browser resume");
    assert.equal(sceneNodes[0].resource_id, expected.star_resource_id);
    assert.equal(sceneNodes[0].page_id, frame.page_id);
    assert.deepEqual(sceneNodes[0].bounds, frame.bounds);
  }
  await page.evaluate(() => document.fonts.ready);
  const dom = await page.evaluate(async data => {
    const pageId = data.star_frames[0].page_id;
    const pageSvg = [...document.querySelectorAll("#temporary-pages svg.page")]
      .find(svg => svg.getAttribute("data-page-id") === pageId);
    if (!pageSvg) throw Error("the exact R1 page was not materialized in the editor");
    for (const frame of data.star_frames) {
      const group = [...pageSvg.querySelectorAll("g[data-node-id]")]
        .find(g => g.getAttribute("data-node-id") === frame.node_id);
      if (!group) throw Error("R1 star node not painted by editor");
      const images = [...group.querySelectorAll("image[data-resource-id]")]
        .filter(image => image.getAttribute("data-resource-id") === data.star_resource_id);
      if (images.length !== 1) throw Error("R1 source picture did not paint once per frame");
      const href = images[0].getAttribute("href");
      if (!/^data:image\/(?:jpeg|png);base64,/.test(href ?? "")) {
        throw Error("R1 picture painted without host-authorized SafePreview");
      }
      const decoded = new Image();
      decoded.src = href;
      await decoded.decode();
      const rect = images[0].getBoundingClientRect();
      if (!(decoded.naturalWidth > 0 && decoded.naturalHeight > 0 &&
        rect.width > 0 && rect.height > 0)) throw Error("R1 image not decoded or visible");
    }
    const logoImages = [...pageSvg.querySelectorAll("image[data-resource-id]")]
      .filter(image => image.getAttribute("data-resource-id") === data.logo_resource_id);
    if (logoImages.length < 1) throw Error("CMYK footer logo never reached editor DOM");
    for (const logo of logoImages) {
      const image = new Image();
      image.src = logo.getAttribute("href");
      await image.decode();
      if (image.naturalWidth < 1 || image.naturalHeight < 1 ||
        logo.getBoundingClientRect().width <= 0) throw Error("CMYK logo did not paint");
    }
    const vb = pageSvg.viewBox.baseVal;
    if (!(vb.width > 0 && vb.height > 0)) throw Error("R1 page viewport invalid");
    return { view_box: { x: vb.x, y: vb.y, width: vb.width, height: vb.height },
      star_elements: 4, logo_elements: logoImages.length };
  }, expected);

  // The REAL editor SVG is the renderer. The A/B control suppresses only the
  // four image elements after the R1 scene is already accepted. No sources,
  // fonts, geometry, CSS page scale or other resources change.
  const pageSelector = '#temporary-pages svg.page[data-page-id="' +
    expected.star_frames[0].page_id + '"]';
  const pageSvg = page.locator(pageSelector).first();
  await pageSvg.scrollIntoViewIfNeeded();
  const withStars = await pageSvg.screenshot();
  await page.evaluate(data => {
    for (const frame of data.star_frames) {
      const group = document.querySelector('#temporary-pages g[data-node-id="' +
        frame.node_id + '"]');
      const image = [...(group?.querySelectorAll("image[data-resource-id]") ?? [])]
        .find(el => el.getAttribute("data-resource-id") === data.star_resource_id);
      if (!image) throw Error("missing star for controlled R1 visibility toggle");
      image.setAttribute("visibility", "hidden");
    }
  }, expected);
  const withoutStars = await pageSvg.screenshot();

  const pixels = await page.evaluate(async data => {
    async function decode(base64) {
      const image = new Image();
      image.src = "data:image/png;base64," + base64;
      await image.decode();
      const canvas = document.createElement("canvas");
      canvas.width = image.naturalWidth;
      canvas.height = image.naturalHeight;
      const context = canvas.getContext("2d", { willReadFrequently: true });
      context.drawImage(image, 0, 0);
      return { width: canvas.width, height: canvas.height,
        pixels: context.getImageData(0, 0, canvas.width, canvas.height).data };
    }
    const yes = await decode(data.with_stars);
    const no = await decode(data.without_stars);
    if (yes.width !== no.width || yes.height !== no.height) {
      throw Error("R1 A/B screenshot size changed");
    }
    const r = value => Math.max(0, Math.min(value, 255));
    const red = (buf, i) => buf[i] > 180 && buf[i + 1] < 75 &&
      buf[i + 2] < 155 && buf[i] - buf[i + 1] > 140;
    const counts = [];
    for (const frame of data.star_frames) {
      const b = frame.bounds, vb = data.view_box;
      const left = Math.floor((b.x - vb.x) / vb.width * yes.width);
      const top = Math.floor((b.y - vb.y) / vb.height * yes.height);
      const right = Math.ceil((b.x + b.width - vb.x) / vb.width * yes.width);
      const bottom = Math.ceil((b.y + b.height - vb.y) / vb.height * yes.height);
      const x0 = Math.max(0, left), x1 = Math.min(yes.width, right);
      const y0 = Math.max(0, top), y1 = Math.min(yes.height, bottom);
      if (x1 - x0 < 4 || y1 - y0 < 4) {
        throw Error("R1 star paint rectangle collapsed or clipped");
      }
      let added = 0;
      for (let y = y0; y < y1; y++) {
        for (let x = x0; x < x1; x++) {
          const i = (y * yes.width + x) * 4;
          if (red(yes.pixels, i) && !red(no.pixels, i)) added++;
        }
      }
      if (added < 8) throw Error("R1 star image absent from actual browser pixels");
      counts.push(added);
    }
    return counts;
  }, { with_stars: withStars.toString("base64"),
    without_stars: withoutStars.toString("base64"),
    view_box: dom.view_box, star_frames: expected.star_frames });

  assert.equal(pixels.length, 4);
  assert.equal(dom.star_elements, 4);
  assert.ok(dom.logo_elements >= 1);
  assert.equal(errors.length, 0, "real R1 browser emitted JavaScript errors");
  assert.equal(external.length, 0, "real R1 browser fetched unapproved external resources");
  // Only source-neutral numbers can leave this process. Neither screenshot,
  // full scene, private bearer nor JPEG data is written to disk or logged.
  console.log("SBDC034_EDITOR_R1_BROWSER_PROOF " + JSON.stringify({
    schema: "chaptera.sbdc034-editor-r1-browser-pixels.v1",
    source_sha256: SHA,
    accepted_revision_matches_browser: true,
    actual_editor_svg_star_nodes: dom.star_elements,
    actual_editor_logo_images: dom.logo_elements,
    r1_star_additional_red_pixels: pixels,
    browser_page_errors: errors.length,
    outbound_network_requests: external.length,
    private_screenshots_uploaded: false,
  }));
} finally {
  await browser.close();
}
