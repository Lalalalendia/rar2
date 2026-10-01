import test from "node:test";
import assert from "node:assert/strict";
import { orderedPages, searchStories, guestRequestPath, contributionEligible, extractableImages, assertSalvageObservation, classificationMessage, errorMessage } from "./reader-model.mjs";

const scene = (extra = {}) => ({
  protocol_version: "chaptera.reader-scene.v1",
  pages: [{ page_id: "page-2", order: 1, width_emu: 952500, height_emu: 1905000 },
    { page_id: "page-1", order: 0, width_emu: 952500, height_emu: 1905000 }],
  nodes: [], stories: [], ...extra
});

test("page navigation uses canonical order without changing source data", () => {
  const input = scene();
  const before = JSON.stringify(input);
  assert.deepEqual(orderedPages(input).map((page) => page.page_id), ["page-1", "page-2"]);
  assert.equal(JSON.stringify(input), before);
});

test("malformed scene IDs, dimensions and private source fields fail closed", () => {
  for (const input of [scene({ protocol_version: "other" }), scene({ pages: [null] }),
    scene({ pages: [{ page_id: "p", order: 0, width_emu: 0, height_emu: 1 }] }),
    scene({ pages: [{ page_id: "p", order: 0, width_emu: Number.MAX_SAFE_INTEGER + 1, height_emu: 1 }] }),
    scene({ nodes: [null] }), scene({ fonts: {} }), scene({ raw_pub_bytes: "private" })]) {
    assert.throws(() => orderedPages(input));
  }
  const repeated = scene();
  repeated.pages[1].order = repeated.pages[0].order;
  assert.throws(() => orderedPages(repeated), /scene_protocol_mismatch/);
  repeated.pages[1].order = 0;
  repeated.pages[1].page_id = repeated.pages[0].page_id;
  assert.throws(() => orderedPages(repeated), /scene_protocol_mismatch/);
});

test("searchable stories require unique IDs and recovered strings", () => {
  assert.throws(() => orderedPages(scene({ stories: [{ story_id: "s", text: "a" }, { story_id: "s", text: "b" }] })));
  assert.throws(() => orderedPages(scene({ stories: [{ story_id: "s", text: null }] })));
});

test("case-insensitive Cyrillic search keeps exact scalar and UTF-16 selections", () => {
  const input = scene({ stories: [{ story_id: "s", text: "😀 Привет 🌍 ПРИВЕТ" }] });
  const result = searchStories(input, "привет");
  assert.deepEqual(result.matches.map((match) => [match.scalar_start, match.scalar_end, match.utf16_start, match.utf16_end]),
    [[2, 8, 3, 9], [11, 17, 13, 19]]);
  for (const match of result.matches) {
    assert.equal(input.stories[0].text.slice(match.utf16_start, match.utf16_end).toLowerCase(), "привет");
  }
});

test("literal queries cannot become regular expressions and emoji stay whole", () => {
  const input = scene({ stories: [{ story_id: "s", text: "[a]+ .* 😀😀" }] });
  assert.equal(searchStories(input, "[a]+").matches.length, 1);
  assert.equal(searchStories(input, ".*").matches.length, 1);
  assert.deepEqual(searchStories(input, "😀").matches.map((match) => [match.scalar_start, match.scalar_end]), [[8, 9], [9, 10]]);
  assert.deepEqual(searchStories(input, ""), { matches: [], truncated: false });
  assert.equal(searchStories(input, "a".repeat(201)).matches.length, 0);
});

test("search bounds results and distinguishes a full list from truncation", () => {
  assert.equal(searchStories(scene({ stories: [{ story_id: "s", text: "a ".repeat(200) }] }), "a").truncated, false);
  const result = searchStories(scene({ stories: [{ story_id: "s", text: "a ".repeat(201) }] }), "a");
  assert.equal(result.matches.length, 200);
  assert.equal(result.truncated, true);
});

test("research contribution is fail-closed to server-owned eligible classes", () => {
  const base = { protocol_version: "chaptera.failure-classifier.v1", confidence: "high", reason_flags: ["bounded"] };
  assert.equal(contributionEligible({ ...base, class: "PUB_HIGH_VALUE" }), true);
  assert.equal(contributionEligible({ ...base, class: "PUB_DAMAGED" }), true);
  for (const value of [
    { ...base, class: "PUB_POSSIBLE" },
    { ...base, class: "ARCHIVE_WITH_PUB" },
    { ...base, class: "NOT_PUB" },
    { ...base, class: "SUSPICIOUS/POLYGLOT" },
    { ...base, protocol_version: "other", class: "PUB_DAMAGED" },
    null
  ]) {
    assert.equal(contributionEligible(value), false);
  }
});

test("guest paths admit the actual colon-bearing session ID only on the same origin", () => {
  const origin = "https://reader.example";
  assert.equal(guestRequestPath("/v1/reader/guest-sessions/guest:abc/content", "content", origin), "/v1/reader/guest-sessions/guest:abc/content");
  for (const path of ["https://foreign.example/v1/reader/guest-sessions/x/content", "//foreign.example/content",
    "/v1/reader/guest-sessions/x/open", "/v1/reader/guest-sessions/x/content?token=secret",
    "/v1/reader/guest-sessions/x/content#secret", "/v1/reader/guest-sessions/x%2Fy/content", "/v1/projects/x"]) {
    assert.throws(() => guestRequestPath(path, "content", origin), /guest_path_invalid/);
  }
});

test("extraction admits only bounded inline images, never external or active content", () => {
  const image = { resource_id: "image", mime: "image/png", availability: "inline_data_url", inline_data_url: "data:image/png;base64,YQ==" };
  const resources = [image, { ...image, mime: "image/jpeg" }, { ...image, inline_data_url: "https://foreign.example/a.png" },
    { ...image, inline_data_url: "data:image/svg+xml;base64,YQ==" }, { ...image, availability: "descriptor_only" },
    { ...image, inline_data_url: "data:image/png;base64,YQ=" }, { ...image, inline_data_url: "data:image/png;base64," }];
  assert.deepEqual(extractableImages(scene({ resources })), [image]);
  const full = { ...image, inline_data_url: "data:image/png;base64," + Buffer.alloc(4 * 1024 * 1024).toString("base64") };
  const tooBig = { ...image, inline_data_url: "data:image/png;base64," + Buffer.alloc(4 * 1024 * 1024 + 1).toString("base64") };
  assert.equal(extractableImages(scene({ resources: [full, full, full, tooBig] })).length, 2);
});

test("salvage observation is source-neutral and distinct from Reader scene", () => {
  const salvage = {
    schema_version: "chaptera.reader-partial-source-graph.v1",
    source_sha256: "a".repeat(64),
    contents_family: "0x2c",
    subsystems: {
      contents: "readable", quill: "readable", escher: "absent", escher_delay: "absent"
    },
    facts: [{
      kind: "text_range", story_key: "quill-syid:00000001",
      utf16_start: 0, utf16_end: 4, text: "test"
    }],
    gaps: ["image_facts_unavailable", "geometry_facts_unavailable"]
  };
  assert.equal(assertSalvageObservation(salvage), salvage);
  assert.throws(
    () => assertSalvageObservation({ ...salvage, raw_pub_bytes: "private" }),
    /forbidden source field/
  );
  assert.throws(
    () => assertSalvageObservation({ ...salvage, schema_version: "chaptera.reader-scene.v1" }),
    /salvage_protocol_mismatch/
  );
});

test("server-owned failure class refines only unsupported terminal guidance", () => {
  assert.match(classificationMessage("unsupported", "PUB_DAMAGED"), /appears damaged/);
  assert.match(classificationMessage("unsupported", "NOT_PUB"), /not a Publisher document/);
  assert.match(classificationMessage("unsupported", "PUB_HIGH_VALUE"), /improve compatibility/);
  assert.match(classificationMessage("unsupported", "PUB_POSSIBLE"), /cannot verify/);
  assert.match(classificationMessage("unsupported", "ARCHIVE_WITH_PUB"), /archive contains Publisher/);
  assert.equal(classificationMessage("supported", "PUB_DAMAGED"), classificationMessage("supported"));
  assert.equal(classificationMessage("unsupported", "SUSPICIOUS/POLYGLOT"), classificationMessage("unsupported"));
});

test("typed terminal states and HTTP errors provide recovery without exposing internals", () => {
  const states = ["supported", "partial", "salvage", "unsupported", "damaged", "not_pub", "security_rejected"];
  assert.equal(new Set(states.map(classificationMessage)).size, states.length);
  for (const status of [401, 403, 404, 410, 413, 429, 500, 503]) {
    assert.ok(errorMessage({ status, message: "private database path" }).length > 25);
    assert.ok(!errorMessage({ status, message: "private database path" }).includes("private database"));
  }
  assert.match(errorMessage({ name: "AbortError" }), /cancelled/);
  assert.match(errorMessage(new Error("guest_protocol_mismatch")), /incompatible/);
});
