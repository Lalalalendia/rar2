// Public-safe UI evidence only: the service below supplies synthetic DTOs.
// It deliberately cannot establish real-PUB fidelity or live-service safety.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { chromium } from "playwright";

const root = dirname(fileURLToPath(import.meta.url));
const output = resolve(process.env.READER_UI_OUTPUT ?? join(root, "../../target/cloud-reader-ui"));
const png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==";
const emu = (px) => px * 9525;
const original = Buffer.from("public synthetic reader UI fixture");
const text = "😀 Привет 🌍 ПРИВЕТ\nLiteral [a]+ stays literal.\nRecovered text <script>never executes</script>.";
function fixture(label = text) {
  return {
    protocol_version: "chaptera.reader-scene.v1", document_id: "synthetic", revision_id: "synthetic-ui-only",
    fidelity: { state: "partial", reasons: ["stacking_order_unavailable", "text_layout_partial"] },
    pages: [{ page_id: "p2", order: 1, width_emu: emu(600), height_emu: emu(760) },
      { page_id: "p1", order: 0, width_emu: emu(600), height_emu: emu(760) }],
    nodes: [
      { node_id: "n1", page_id: "p1", kind: "shape", bounds: { x: emu(40), y: emu(40), width: emu(520), height: emu(90) }, paint: { fill_rgb: [30, 79, 137] } },
      { node_id: "n2", page_id: "p1", kind: "shape", bounds: { x: emu(40), y: emu(155), width: emu(240), height: emu(440) }, paint: { fill_rgb: [228, 236, 247] } },
      { node_id: "n3", page_id: "p1", kind: "shape", bounds: { x: emu(310), y: emu(155), width: emu(250), height: emu(440) }, paint: { fill_rgb: [230, 244, 237] } },
      { node_id: "n4", page_id: "p2", kind: "shape", bounds: { x: emu(70), y: emu(60), width: emu(460), height: emu(600) }, paint: { fill_rgb: [244, 225, 204] } }
    ],
    stories: [{ story_id: "s1", text: label, text_fidelity: "partial" }, { story_id: "s2", text: "Second section.", text_fidelity: "partial" }],
    resources: [{ resource_id: "r1", mime: "image/png", availability: "inline_data_url", inline_data_url: png },
      { resource_id: "r2", mime: "image/svg+xml", availability: "descriptor_only" }],
    diagnostics: [{ message: "Synthetic UI fixture; this is not real-PUB fidelity evidence." }]
  };
}

const requests = [];
const sessions = new Map();
let nextScenario = {};
let nextSession = 0;
const staticFiles = new Set(["index.html", "reader.css", "reader-app.mjs", "reader-model.mjs", "render-v1.mjs", "observability-v1.mjs"]);
const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://127.0.0.1");
    if (url.pathname.startsWith("/v1/")) {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const body = Buffer.concat(chunks);
      requests.push({ path: req.url, method: req.method, headers: req.headers, body });
      let payload = {};
      let status = 200;
      let redirect;
      if (url.pathname === "/v1/reader/guest-sessions") {
        const session_id = "guest:" + (++nextSession).toString().padStart(32, "0");
        const scenario = nextScenario;
        scenario.expiresAt = Date.now() + (scenario.sessionTtl ?? 60_000);
        sessions.set(session_id, scenario);
        payload = {
          protocol_version: "chaptera.reader-guest-session.v1", session_id, access_token: "synthetic-token-" + nextSession,
          upload_path: "/v1/reader/guest-sessions/" + session_id + "/content",
          open_path: "/v1/reader/guest-sessions/" + session_id + "/open",
          expires_at_ms: scenario.expiresAt, ...scenario.issue
        };
        status = scenario.issueStatus ?? 200;
        redirect = scenario.issueRedirect;
      } else if (url.pathname.startsWith("/v1/reader/guest-sessions/")) {
        const [, session_id, action] = url.pathname.match(
          /guest-sessions\/([^/]+)\/(content|open|contribution-capability|contribute)$/
        ) ?? [];
        const scenario = sessions.get(session_id) ?? {};
        const submission_id = "submission:" + session_id.split(":").at(-1);
        payload = { protocol_version: "chaptera.reader-guest-session.v1", session_id, expires_at_ms: scenario.expiresAt };
        if (action === "content") {
          Object.assign(payload, { state: "uploaded" }, scenario.upload);
          redirect = scenario.uploadRedirect;
        } else if (action === "open") {
          if (scenario.wait) await scenario.wait;
          status = scenario.openStatus ?? 200;
          Object.assign(payload, { classification: "partial", scene: fixture() }, scenario.open);
          redirect = scenario.openRedirect;
        } else if (action === "contribution-capability") {
          if (scenario.capabilityWait) await scenario.capabilityWait;
          payload = {
            protocol_version: "chaptera.reader-contribution-capability.v1",
            submission_id,
            capability_token: "a".repeat(64),
            expires_at_ms: scenario.expiresAt,
            retention_policy: "chaptera-intake-retention-v1",
            ...scenario.capability
          };
          status = scenario.capabilityStatus ?? 200;
        } else if (action === "contribute") {
          if (scenario.contributionWait) await scenario.contributionWait;
          payload = {
            protocol_version: "chaptera.intake-receipt.v1",
            submission_id,
            server_sha256: "b".repeat(64),
            exact_byte_disposition: "new_exact_bytes",
            cluster_disposition: "deferred",
            retention_policy: "chaptera-intake-retention-v1",
            ...scenario.contribution
          };
          status = scenario.contributionStatus ?? 200;
        } else {
          status = 404;
        }
      } else if (url.pathname.startsWith("/v1/reader/documents/")) payload = nextScenario.saved ?? fixture("Saved document.");
      else status = 404;
      res.writeHead(redirect ? 307 : status, { "content-type": "application/json", "cache-control": "no-store", ...(redirect ? { location: redirect } : {}) });
      res.end(JSON.stringify(payload));
      return;
    }
    const name = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
    if (!staticFiles.has(name)) { res.writeHead(404); res.end(); return; }
    const type = name.endsWith(".html") ? "text/html" : name.endsWith(".css") ? "text/css" : "text/javascript";
    const source = name === "observability-v1.mjs" ? join(root, "../web/observability-v1.mjs") : join(root, name);
    const bytes = await readFile(source);
    res.writeHead(200, { "content-type": type + "; charset=utf-8" });
    res.end(bytes);
  } catch (error) { res.writeHead(500); res.end("synthetic_service_failure"); console.error(error); }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = "http://127.0.0.1:" + server.address().port;
let browser;
const passed = [];
const browserErrors = [];
const unexpectedRequests = [];
async function open(page, scenario = {}, name = "private-name.pub", bytes = original) {
  nextScenario = scenario;
  await page.locator("#pub-file").setInputFiles({ name, mimeType: "application/octet-stream", buffer: bytes });
  await page.locator("#open-file").click();
}
async function status(page, pattern) {
  await page.waitForFunction((source) => new RegExp(source).test(document.querySelector("#status").textContent), pattern.source);
}
async function check(name, run) { await run(); passed.push(name); console.log("ok - " + name); }

try {
  await mkdir(output, { recursive: true });
  browser = await chromium.launch({ headless: true, ...(process.env.READER_UI_BROWSER ? { executablePath: process.env.READER_UI_BROWSER } : {}) });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  await context.addCookies([{ name: "account", value: "synthetic-cookie", url: origin }]);
  await context.addInitScript(() => {
    const nativeFetch = window.fetch;
    window.__fetchOptions = [];
    window.fetch = (url, options = {}) => {
      window.__fetchOptions.push({ url, credentials: options.credentials, redirect: options.redirect, method: options.method ?? "GET" });
      return nativeFetch(url, options);
    };
    window.__clipboard = [];
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async (value) => { window.__clipboard.push(value); } } });
  });
  context.on("request", (request) => { if (!request.url().startsWith(origin + "/")) unexpectedRequests.push(request.url()); });
  const page = await context.newPage();
  page.on("pageerror", (error) => browserErrors.push(error.message));
  await page.goto(origin);

  await check("private raw upload and atomic scene publication", async () => {
    await open(page);
    await status(page, /Opened with display limitations/);
    assert.equal(await page.locator("#reader").isVisible(), true);
    assert.equal(await page.locator("#pages svg").count(), 2);
    assert.equal(await page.locator("#pages svg").first().getAttribute("data-page-id"), "p1");
    const guest = requests.filter((request) => request.path.startsWith("/v1/reader/guest-sessions"));
    assert.deepEqual(guest.map((request) => request.method), ["POST", "PUT", "POST"]);
    assert.deepEqual(JSON.parse(guest[0].body), { expected_byte_len: original.length });
    assert.deepEqual(guest[1].body, original);
    for (const request of guest) {
      assert.equal(request.headers.cookie, undefined);
      assert.ok(!request.path.includes("synthetic-token"));
      assert.ok(!request.body.includes(Buffer.from("private-name")));
      assert.ok(request.headers["x-csrf-token"]);
    }
    assert.equal(guest[1].headers["x-chaptera-reader-session"], "synthetic-token-1");
    assert.equal(guest[2].headers["x-chaptera-reader-session"], "synthetic-token-1");
    const options = await page.evaluate(() => window.__fetchOptions);
    assert.ok(options.every((option) => option.credentials === "omit"));
    assert.ok(options.every((option) => option.redirect === "error"));
    assert.deepEqual(await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage } })), { local: {}, session: {} });
  });

  await check("ordered navigation, keyboard access and unchanged zoom geometry", async () => {
    assert.equal(await page.locator("#previous-page").isDisabled(), true);
    await page.locator("#next-page").click();
    assert.equal(await page.locator("#page-select").inputValue(), "1");
    assert.equal(await page.locator("#next-page").isDisabled(), true);
    await page.locator("#viewer").focus();
    await page.keyboard.press("PageUp");
    assert.equal(await page.locator("#page-select").inputValue(), "0");
    const viewBox = await page.locator("#pages svg").first().getAttribute("viewBox");
    await page.locator("#zoom-select").selectOption("1.5");
    assert.equal(Number(await page.locator("#pages svg").first().getAttribute("width")), 900);
    assert.equal(Number(await page.locator("#pages svg").first().getAttribute("height")), 1140);
    assert.equal(await page.locator("#pages svg").first().getAttribute("viewBox"), viewBox);
    await page.locator("#zoom-select").selectOption("fit");
    await page.locator("#page-select").selectOption("0");
  });

  await check("Unicode search selects exact recovered text and copy preserves bytes", async () => {
    await page.keyboard.press("Control+f");
    assert.equal(await page.locator("#search-query").evaluate((element) => element === document.activeElement), true);
    await page.locator("#search-query").fill("привет");
    await page.locator("#search-form button").click();
    assert.equal(await page.locator("#search-results button").count(), 2);
    await page.locator("#search-results button").nth(1).click();
    assert.deepEqual(await page.locator("#story-text").evaluate((element) => [element.selectionStart, element.selectionEnd, element.value.slice(element.selectionStart, element.selectionEnd)]), [13, 19, "ПРИВЕТ"]);
    await page.locator("#copy-text").click();
    assert.deepEqual(await page.evaluate(() => window.__clipboard), [text]);
    await page.evaluate(() => { navigator.clipboard.writeText = async () => { throw new Error("denied"); }; });
    await page.locator("#copy-text").click();
    assert.match(await page.locator("#copy-status").textContent(), /Text selected/);
    assert.equal(await page.locator("#story-text").evaluate((element) => element.selectionEnd - element.selectionStart), text.length);
    assert.equal(await page.locator("script:not([src])").count(), 0);
  });

  await check("only admitted inline images are downloadable and limitations are readable", async () => {
    await page.locator("#assets-summary").click();
    assert.equal(await page.locator("#assets a").count(), 1);
    const downloadPromise = page.waitForEvent("download");
    await page.locator("#assets a").click();
    const download = await downloadPromise;
    assert.equal(download.suggestedFilename(), "chaptera-image-1.png");
    await download.saveAs(join(output, "synthetic-image.png"));
    assert.deepEqual(await readFile(join(output, "synthetic-image.png")), Buffer.from(png.split(",")[1], "base64"));
    await page.locator("#diagnostics").click();
    assert.match(await page.locator("#limitations").textContent(), /stacking order/);
    assert.match(await page.locator("#limitations").textContent(), /Synthetic UI fixture/);
    await page.locator("#text-panel > summary").click();
    await page.locator("#reader").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(output, "desktop.png"), fullPage: true });
  });

  await check("mobile chrome fits and portrait pages keep their aspect ratio", async () => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.waitForFunction(() => {
      const viewer = document.querySelector("#viewer");
      const svg = document.querySelector("#pages svg");
      return Number(svg.getAttribute("width")) <= viewer.clientWidth - 24;
    });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    const dimensions = await page.locator("#pages svg").first().evaluate((svg) => [Number(svg.getAttribute("width")), Number(svg.getAttribute("height"))]);
    assert.ok(Math.abs(dimensions[1] / dimensions[0] - 760 / 600) < 0.001);
    await page.screenshot({ path: join(output, "mobile.png"), fullPage: true });
    await page.setViewportSize({ width: 1280, height: 900 });
  });

  await check("terminal classifications clear the previous document and explain recovery", async () => {
    for (const classification of ["unsupported", "damaged", "not_pub", "security_rejected"]) {
      await open(page, { open: { classification, scene: undefined } });
      await status(page, /Choose another|Choose a \.PUB|Keep the original/);
      assert.equal(await page.locator("#reader").isVisible(), false);
      assert.equal(await page.locator("#pages svg").count(), 0);
    }
    for (const [code, expected] of [[413, /too large/], [429, /busy/], [403, /Access/], [410, /no longer available/], [503, /temporarily unavailable/]]) {
      await open(page, { issueStatus: code });
      await status(page, expected);
      assert.equal(await page.locator("#open-file").isDisabled(), false);
    }
    const before = requests.length;
    await open(page, {}, "empty.pub", Buffer.alloc(0));
    await status(page, /file is empty/);
    assert.equal(requests.length, before);
  });

  await check("eligible unsupported PUB requires separate explicit contribution consent", async () => {
    const before = requests.length;
    const source = Buffer.from("eligible synthetic publisher research bytes");
    await open(page, {
      open: {
        classification: "unsupported",
        scene: undefined,
        failure_classification: {
          protocol_version: "chaptera.failure-classifier.v1",
          class: "PUB_DAMAGED",
          confidence: "high",
          reason_flags: ["cfb_parse_failed"]
        }
      }
    }, "private-research-name.pub", source);
    await status(page, /appears damaged/);
    assert.equal(await page.locator("#contribution-panel").isVisible(), true);
    assert.equal(
      requests.slice(before).filter((request) => request.path.includes("contribut")).length,
      0
    );

    await page.locator("#open-contribution").click();
    assert.equal(await page.locator("#contribution-dialog").evaluate((dialog) => dialog.open), true);
    assert.equal(await page.locator("#contribution-filename").textContent(), "private-research-name.pub");
    assert.equal(
      requests.slice(before).filter((request) => request.path.includes("contribut")).length,
      0
    );

    await page.locator("#send-contribution").click();
    await status(page, /Contribution received/);
    const contribution = requests
      .slice(before)
      .filter((request) => request.path.includes("contribut"));
    assert.equal(contribution.length, 2);
    assert.match(contribution[0].path, /\/contribution-capability$/);
    assert.match(contribution[1].path, /\/contribute$/);
    assert.deepEqual(JSON.parse(contribution[0].body), {
      protocol_version: "chaptera.intake-capability-request.v1",
      consent_version: "chaptera-intake-consent-v1"
    });
    assert.equal(contribution[1].body.length, 0);
    for (const request of contribution) {
      assert.equal(request.method, "POST");
      assert.equal(request.headers.cookie, undefined);
      assert.ok(!request.path.includes("private-research-name"));
      assert.ok(!request.body.includes(Buffer.from("private-research-name")));
      assert.ok(!request.body.includes(source));
      assert.match(request.headers["x-chaptera-reader-session"], /^synthetic-token-/);
    }
    assert.equal(contribution[0].headers["x-chaptera-reader-contribution"], undefined);
    assert.equal(contribution[1].headers["x-chaptera-reader-contribution"], "a".repeat(64));
    assert.ok(!contribution[1].path.includes("a".repeat(16)));
    assert.deepEqual(
      await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage } })),
      { local: {}, session: {} }
    );

    for (const className of ["PUB_POSSIBLE", "ARCHIVE_WITH_PUB", "NOT_PUB", "SUSPICIOUS/POLYGLOT"]) {
      await open(page, {
        open: {
          classification: "unsupported",
          scene: undefined,
          failure_classification: {
            protocol_version: "chaptera.failure-classifier.v1",
            class: className,
            confidence: "high",
            reason_flags: ["bounded"]
          }
        }
      });
      await status(page, /not supported yet|Publisher-related|not a Publisher|archive contains/);
      assert.equal(await page.locator("#contribution-panel").isVisible(), false);
    }
  });

  const eligible = {
    classification: "unsupported", scene: undefined,
    failure_classification: {
      protocol_version: "chaptera.failure-classifier.v1", class: "PUB_HIGH_VALUE",
      confidence: "high", reason_flags: ["bounded"]
    }
  };

  await check("cancel before consent sends nothing and clears contribution access", async () => {
    await open(page, { open: eligible });
    await status(page, /not supported yet/);
    const before = requests.length;
    await page.locator("#open-contribution").click();
    await page.locator("#cancel-contribution").click();
    await status(page, /Contribution cancelled/);
    assert.equal(await page.locator("#contribution-panel").isVisible(), false);
    assert.equal(await page.locator("#contribution-filename").textContent(), "");
    assert.equal(requests.length, before);
  });

  await check("expiry clears eligibility and blocks retention after an expired capability", async () => {
    await open(page, { open: eligible, sessionTtl: 500 });
    await status(page, /not supported yet/);
    await page.locator("#open-contribution").click();
    await status(page, /viewing session expired/);
    assert.equal(await page.locator("#contribution-panel").isVisible(), false);
    assert.equal(await page.locator("#contribution-dialog").evaluate((dialog) => dialog.open), false);
    assert.equal(await page.locator("#contribution-filename").textContent(), "");

    await open(page, { open: eligible, capability: { expires_at_ms: Date.now() - 1 } });
    await status(page, /not supported yet/);
    const before = requests.length;
    await page.locator("#open-contribution").click();
    await page.locator("#send-contribution").click();
    await page.waitForFunction(() => document.querySelector("#contribution-status").textContent.includes("could not start"));
    assert.deepEqual(requests.slice(before).map((request) => request.path.split("/").at(-1)), ["contribution-capability"]);
    await page.locator("#cancel-contribution").click();
  });

  await check("replacing a file during consent cannot retain it or overwrite the new document", async () => {
    let release;
    const capabilityWait = new Promise((resolve) => { release = resolve; });
    await open(page, { open: eligible, capabilityWait });
    await status(page, /not supported yet/);
    await page.locator("#open-contribution").click();
    const started = page.waitForRequest((request) => request.url().endsWith("/contribution-capability"));
    await page.locator("#send-contribution").click();
    await started;
    nextScenario = { open: { scene: fixture("New document after consent") } };
    await page.locator("#drop").evaluate((element) => {
      const transfer = new DataTransfer();
      transfer.items.add(new File(["replacement"], "replacement.pub"));
      element.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, bubbles: true }));
    });
    await status(page, /Opened with/);
    const before = requests.length;
    release();
    await page.waitForTimeout(100);
    assert.equal(requests.length, before);
    assert.equal(await page.locator("#story-text").inputValue(), "New document after consent");
    assert.match(await page.locator("#status").textContent(), /Opened with/);
  });

  await check("a late contribution receipt cannot clear a replacement contribution dialog", async () => {
    let release;
    const contributionWait = new Promise((resolve) => { release = resolve; });
    await open(page, { open: eligible, contributionWait });
    await status(page, /not supported yet/);
    await page.locator("#open-contribution").click();
    const started = page.waitForRequest((request) => request.url().endsWith("/contribute"));
    await page.locator("#send-contribution").click();
    await started;
    nextScenario = { open: eligible };
    await page.locator("#drop").evaluate((element) => {
      const transfer = new DataTransfer();
      transfer.items.add(new File(["second"], "second.pub"));
      element.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, bubbles: true }));
    });
    await status(page, /not supported yet/);
    await page.locator("#open-contribution").click();
    release();
    await page.waitForTimeout(100);
    assert.equal(await page.locator("#contribution-dialog").evaluate((dialog) => dialog.open), true);
    assert.equal(await page.locator("#contribution-filename").textContent(), "second.pub");
    assert.match(await page.locator("#status").textContent(), /not supported yet/);
    await page.locator("#cancel-contribution").click();
  });

  await check("an uncertain retention response never claims that nothing was authorized", async () => {
    await open(page, { open: eligible, contributionStatus: 503 });
    await status(page, /not supported yet/);
    await page.locator("#open-contribution").click();
    await page.locator("#send-contribution").click();
    await page.waitForFunction(() => document.querySelector("#contribution-status").textContent.includes("may already have been received"));
    assert.equal(await page.locator("#send-contribution").isDisabled(), false);
    await page.locator("#cancel-contribution").click();
  });

  await check("incompatible protocols and off-origin paths never receive a capability", async () => {
    for (const scenario of [
      { issue: { upload_path: "https://foreign.example/v1/reader/guest-sessions/x/content" } },
      { issue: { open_path: "/v1/reader/guest-sessions/other/open" } },
      { upload: { session_id: "other" } },
      { open: { protocol_version: "other" } },
      { open: { scene: undefined } },
      { open: { scene: fixture(), session_id: "other" } },
      { open: { scene: { ...fixture(), stories: [{ story_id: "s", text: null }] } } }
    ]) {
      await open(page, scenario);
      await status(page, /incompatible/);
      assert.equal(await page.locator("#reader").isVisible(), false);
    }
  });

  await check("guest redirects cannot forward raw bytes or capability headers", async () => {
    for (const key of ["issueRedirect", "uploadRedirect", "openRedirect"]) {
      await open(page, { [key]: "https://foreign.example/receive" });
      await status(page, /Opening failed/);
      assert.equal(await page.locator("#reader").isVisible(), false);
    }
    assert.deepEqual(unexpectedRequests, []);
  });

  await check("cancelled open cannot replace a later reading session", async () => {
    let release;
    const wait = new Promise((resolve) => { release = resolve; });
    await open(page, { wait, open: { scene: fixture("Old cancelled document") } });
    await status(page, /Scanning and opening/);
    await page.locator("#cancel-open").click();
    await status(page, /cancelled/);
    await open(page, { open: { scene: fixture("Current document") } });
    await status(page, /Opened with/);
    release();
    await page.waitForFunction(() => document.querySelector("#story-text").value === "Current document");
    await page.waitForTimeout(100);
    assert.equal(await page.locator("#story-text").inputValue(), "Current document");
  });

  await check("deferred font activation from a replaced open cannot publish stale pages", async () => {
    await page.evaluate(() => {
      const NativeFontFace = window.FontFace;
      window.FontFace = class extends NativeFontFace {
        load() { return new Promise((resolve) => { window.__releaseFont = () => resolve(this); }); }
      };
    });
    const old = fixture("Slow font document");
    old.fonts = [{ resource_id: "font", availability: "inline_data_url", expected_sha256: "synthetic", inline_data_url: "data:font/ttf;base64,Zm9v" }];
    await open(page, { open: { scene: old } });
    await page.waitForFunction(() => typeof window.__releaseFont === "function");
    nextScenario = { open: { scene: fixture("Replacement document") } };
    await page.locator("#drop").evaluate((element) => {
      const transfer = new DataTransfer();
      transfer.items.add(new File(["replacement"], "replacement.pub", { type: "application/octet-stream" }));
      element.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, bubbles: true }));
    });
    await status(page, /Opened with/);
    await page.evaluate(() => window.__releaseFont());
    await page.waitForTimeout(100);
    assert.equal(await page.locator("#story-text").inputValue(), "Replacement document");
    assert.equal(await page.locator("#cancel-open").isVisible(), false);
  });

  await check("saved documents use account credentials and remain read-only", async () => {
    nextScenario = {};
    await page.locator(".saved > summary").click();
    await page.locator("#document-id").fill("saved/document");
    await page.locator("#document-id").press("Enter");
    await status(page, /Opened saved document/);
    assert.equal(await page.locator("#story-text").inputValue(), "Saved document.");
    const saved = requests.at(-1);
    assert.equal(saved.path, "/v1/reader/documents/saved%2Fdocument/scene");
    assert.match(saved.headers.cookie, /account=synthetic-cookie/);
    assert.equal(saved.headers["x-chaptera-reader-session"], undefined);
    assert.equal(saved.headers["x-chaptera-reader-contribution"], undefined);
    assert.ok(!requests.some((request) => /\/commit|\/v1\/projects|\/v1\/uploads/.test(request.path)));
  });
  assert.deepEqual(unexpectedRequests, []);
  assert.deepEqual(browserErrors, []);
  const receipt = {
    protocol: "chaptera.cloud-reader-ui-acceptance.v1",
    scope: "synthetic UI only; excludes real-PUB visual fidelity and live service/deployment acceptance",
    repository_commit_sha: process.env.REPOSITORY_COMMIT_SHA ?? "local-uncommitted",
    browser: await browser.version(), passed, unexpected_requests: unexpectedRequests, browser_errors: browserErrors
  };
  await writeFile(join(output, "receipt.json"), JSON.stringify(receipt, null, 2) + "\n");
  console.log(JSON.stringify({ passed: passed.length, receipt: join(output, "receipt.json") }));
} finally {
  if (browser) await browser.close();
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}
