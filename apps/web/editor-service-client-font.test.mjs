import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";

import { HttpEditorServiceV1 } from "./editor-service-client-v1.mjs";

const RAW = readFileSync(new URL("../../assets/fonts/ofl/abel/Abel-Regular.ttf", import.meta.url));
const HASH = "8809dcad25318225052f88333e208c5aad4adcb7b2c934c135735ec19aa410b4";
const RESOURCE = "f27a8036-8492-480f-8fa6-d2e775cc9f12";
const REV = "sha256:" + "1".repeat(64);
const SNAP = "sha256:" + "2".repeat(64);
const HANDLE = "abel_0123456789abcdef0123456789abcdef01234567";

function descriptor() {
  return {
    resource_id: RESOURCE, content_hash: HASH,
    delivery: "deliver_exact", fetch_handle: HANDLE, face_index: 0,
    font_fingerprint: "sha256:" + HASH,
  };
}

function withFetch(stub, fn) {
  const original = globalThis.fetch;
  globalThis.fetch = stub;
  return Promise.resolve().then(fn).finally(() => { globalThis.fetch = original; });
}

test("same-origin exact font fetch rehashes actual licensed OpenType bytes", async () => {
  assert.equal(createHash("sha256").update(RAW).digest("hex"), HASH);
  let requests = 0;
  await withFetch(async (url, options) => {
    requests++;
    assert.equal(new URL(url).origin, "http://127.0.0.1:18765");
    assert.equal(new URL(url).pathname, "/v1/editor/font-resource/" + HANDLE);
    assert.equal(options.headers["x-chaptera-principal-id"], "synthetic-editor");
    assert.equal(options.credentials, "omit");
    return new Response(RAW, {
      status: 200,
      headers: {
        "content-type": "font/ttf",
        "x-chaptera-font-content-sha256": HASH,
      },
    });
  }, async () => {
    const service = new HttpEditorServiceV1("http://127.0.0.1:18765");
    const bytes = await service.exactFontResourceBytes(descriptor());
    assert.equal(Buffer.compare(Buffer.from(bytes), RAW), 0);
  });
  assert.equal(requests, 1);
});

test("browser HTTP client fetches independent current-revision font catalogs", async () => {
  const paths = [];
  await withFetch(async (url, options) => {
    const path = new URL(url).pathname;
    paths.push(path);
    assert.equal(options.headers["x-chaptera-principal-id"], "synthetic-editor");
    return new Response(JSON.stringify({protocol_version: "test.v1", path}), {
      headers: {"content-type": "application/json"},
    });
  }, async () => {
    const service = new HttpEditorServiceV1("http://127.0.0.1:18765");
    assert.equal((await service.fontEnvironment()).path, "/v1/editor/font-environment");
    assert.equal((await service.fontAuthoringAdmission()).path,
      "/v1/editor/font-authoring-admission");
  });
  assert.deepEqual(paths, [
    "/v1/editor/font-environment", "/v1/editor/font-authoring-admission",
  ]);
});

test("current physical glyph scope requires an exact Story and current Scene identity", async () => {
  const storyId = "15613e56-726e-5ae7-8c54-ec876c9bcfda";
  const seen = [];
  await withFetch(async (url, options) => {
    const parsed = new URL(url);
    seen.push(parsed.pathname);
    assert.equal(parsed.pathname, "/v1/editor/font-glyph-spans");
    assert.equal(parsed.searchParams.get("story_id"), storyId);
    assert.equal(parsed.searchParams.get("revision_id"), REV);
    assert.equal(parsed.searchParams.get("snapshot_id"), SNAP);
    assert.equal(parsed.searchParams.size, 3);
    assert.equal(options.headers["x-chaptera-principal-id"], "synthetic-editor");
    return new Response(JSON.stringify({
      protocol_version:"chaptera.current-physical-glyph-spans.v1",
      story_id:storyId, revision_id:REV, scene_snapshot_id:SNAP,
      admitted_scalar_count:1, source_unresolved_scalar_count:3,
      shaped_glyph_count:1, authoritative_line_breaks:false,
      fixed_pdf_allowed:false,
    }),{headers:{"content-type":"application/json"}});
  }, async () => {
    const service = new HttpEditorServiceV1("http://127.0.0.1:18765");
    const received=await service.currentPhysicalFontSpans({
      story_id:storyId,revision_id:REV,snapshot_id:SNAP,
    });
    assert.equal(received.admitted_scalar_count,1);
    assert.equal(received.authoritative_line_breaks,false);
    assert.equal(received.fixed_pdf_allowed,false);
  });
  assert.deepEqual(seen,["/v1/editor/font-glyph-spans"]);
  const service=new HttpEditorServiceV1("http://127.0.0.1:18765");
  await withFetch(()=>{throw new Error("invalid query must never fetch");},async ()=>{
    for(const props of [
      {story_id:"Abel",revision_id:REV,snapshot_id:SNAP},
      {story_id:storyId,revision_id:"main",snapshot_id:SNAP},
      {story_id:storyId,revision_id:REV,snapshot_id:"sha256:short"},
      {story_id:storyId+"?source=evil",revision_id:REV,snapshot_id:SNAP},
    ]){
      await assert.rejects(service.currentPhysicalFontSpans(props),TypeError);
    }
  });
});

test("source-bound physical line-fit HTTP client accepts only exact Story/Scene scope",async()=>{
  const story="15613e56-726e-5ae7-8c54-ec876c9bcfda";
  const service=new HttpEditorServiceV1("http://127.0.0.1:18765");
  const old=globalThis.fetch;
  let calls=0;
  try{
    globalThis.fetch=async(url)=>{
      const parsed=new URL(url);
      assert.equal(parsed.pathname,"/v1/editor/font-line-fit");
      assert.equal(parsed.searchParams.get("story_id"),story);
      assert.equal(parsed.searchParams.get("revision_id"),REV);
      assert.equal(parsed.searchParams.get("snapshot_id"),SNAP);
      calls++;
      return new Response(JSON.stringify({state:"source_font_unresolved"}),
        {headers:{"content-type":"application/json"}});
    };
    const response=await service.currentPhysicalFontLineFit({
      story_id:story,revision_id:REV,snapshot_id:SNAP,
    });
    assert.equal(response.state,"source_font_unresolved");
    await assert.rejects(service.currentPhysicalFontLineFit({
      story_id:"unknown",revision_id:REV,snapshot_id:SNAP,
    }),TypeError);
    assert.equal(calls,1);
  }finally{globalThis.fetch=old;}
});

test("name, relative unscoped route, host substitution and invalid candidate cannot deliver", async () => {
  const service = new HttpEditorServiceV1("http://127.0.0.1:18765");
  const alternatives = [
    {...descriptor(), resource_id: "Abel"},
    {...descriptor(), delivery: "deliver_subset"},
    {...descriptor(), fetch_handle: "/v1/editor/font-resource/Abel"},
    {...descriptor(), fetch_handle: "https://example.com/fonts/Abel.ttf"},
    {...descriptor(), fetch_handle: "//evil.example/fonts/Abel.ttf"},
    {...descriptor(), fetch_handle: "/v1/editor/font-resource/" + RESOURCE},
    {...descriptor(), fetch_handle: HANDLE + "&other=1"},
    {...descriptor(), content_hash: "bad"},
  ];
  await withFetch(() => { throw new Error("must not fetch"); }, async () => {
    for (const bad of alternatives) {
      await assert.rejects(service.exactFontResourceBytes(bad));
    }
  });
});

test("fake digest header, altered bytes and denied HTTP never become preview fonts", async () => {
  const service = new HttpEditorServiceV1("http://127.0.0.1:18765");
  for (const response of [
    new Response(Buffer.concat([RAW, Buffer.from("tampered")]), {
      headers: {
        "content-type": "font/ttf",
        "x-chaptera-font-content-sha256": HASH,
      },
    }),
    new Response(RAW, {
      headers: {
        "content-type": "font/ttf",
        "x-chaptera-font-content-sha256": "0".repeat(64),
      },
    }),
    new Response(JSON.stringify({error: "font_resource_not_available_or_stale"}), {
      status: 409, headers: {"content-type": "application/json"},
    }),
  ]) {
    await withFetch(async () => response, async () => {
      await assert.rejects(service.exactFontResourceBytes(descriptor()));
    });
  }
});
