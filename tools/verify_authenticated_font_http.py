#!/usr/bin/env python3
"""Verified authenticated real-PUB font revision exercise; no PDF claim."""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
API = "http://127.0.0.1:18766"
WEB = "http://127.0.0.1:18084"
PORT = 18766
SOURCE_SHA = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
FONT_SHA = "8809dcad25318225052f88333e208c5aad4adcb7b2c934c135735ec19aa410b4"
FONT_UUID = "f27a8036-8492-480f-8fa6-d2e775cc9f12"

def api(path: str, body: dict | None = None, principal: str = "synthetic-editor"):
    headers = {"x-chaptera-principal-id": principal}
    data = None
    if body is not None:
        headers["content-type"] = "application/json"
        data = json.dumps(body, separators=(",", ":")).encode("utf-8")
    try:
        with urllib.request.urlopen(urllib.request.Request(
            API + path, data=data, headers=headers,
            method="POST" if body is not None else "GET",
        ), timeout=30) as result:
            return result.status, json.load(result)
    except urllib.error.HTTPError as exc:
        return exc.code, json.loads(exc.read().decode("utf-8"))

def require(condition: bool, message: str):
    if not condition:
        raise AssertionError(message)

def wait_http(process: subprocess.Popen, url: str):
    for _ in range(200):
        if process.poll() is not None:
            raise RuntimeError("real Editor service ended during startup")
        try:
            with urllib.request.urlopen(url, timeout=1) as resp:
                if resp.status == 200:
                    return
        except (OSError, urllib.error.URLError):
            time.sleep(.2)
    raise RuntimeError("real Editor service did not become ready")

def check_denied(path, request, *, principal="synthetic-editor", code=403):
    status, response=api(path,request,principal)
    require(status==code, f"expected HTTP {code}, received {status}: {response}")
    return response

def history(scene: dict, kind: str, seq: int):
    status, result=api("/v1/commit",{
        "protocol_version":"chaptera.history-transition-intent.v1",
        "document_id":scene["document_id"],"source_hash":scene["source_hash"],
        "base_revision_id":scene["revision_id"],"client_operation_id":f"font-http-history-{kind}-{seq}",
        "command":{"kind":kind},
    })
    require(status==200 and result["protocol_version"]=="chaptera.history-transition-accepted.v1",
        f"font history {kind} not accepted: {status} {result}")
    status, fresh=api("/v1/scenes/current")
    require(status==200 and fresh["revision_id"]==result["revision_id"],
        "font history Scene mismatch")
    return fresh

def run(fixture: pathlib.Path, graph: pathlib.Path, viewer: pathlib.Path, *, browser: bool):
    source=fixture.resolve(strict=True)
    require(len(source.read_bytes())==291_840, "not exact Newsletter PUB length")
    require(hashlib.sha256(source.read_bytes()).hexdigest()==SOURCE_SHA,
        "not the pinned Newsletter source")
    tool=ROOT/"tools/native-pub-candidate-cli"
    subprocess.run(["cargo","build","--quiet","--manifest-path",
                    str(tool/"Cargo.toml"),"--bin","pinned_font_authoring_v1"],
                   cwd=ROOT,check=True,timeout=180)
    worker=tool/"target/debug/pinned_font_authoring_v1"
    proc=subprocess.run([str(worker),"init",str(source)],cwd=ROOT,
                        text=True,capture_output=True,check=True,timeout=30)
    baseline=json.loads(proc.stdout)
    require(baseline["source_hash"]==SOURCE_SHA and
        baseline["identity"]["document_id"],"missing real identity-bearing Project")
    work=ROOT/"target/font-http-acceptance"
    work.mkdir(parents=True,exist_ok=True)
    baseline_file=work/"source-project.json"
    baseline_file.write_text(json.dumps(baseline,sort_keys=True)+"\n",encoding="utf-8")
    service=subprocess.Popen([
        sys.executable,"services/editor-api/web_real_acceptance_service.py",
        "--interactive","--pinned-abel-demo","--fixture-profile","newsletter-font",
        "--baseline-project",str(baseline_file),"--font-worker",str(worker),
        "--port",str(PORT),"--fixture",str(source),
        "--resolved-graph",str(graph.resolve(strict=True)),
        "--viewer-receipt",str(viewer.resolve(strict=True)),
        "--exporter",str((ROOT/"vendor/producer-a/target/debug/chaptera-producer-a").resolve(strict=True)),
        "--work-dir",str(work/"service"),
    ],cwd=ROOT,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
    static=subprocess.Popen([
        sys.executable,"-m","http.server","18084","--bind","127.0.0.1",
        "--directory",str(ROOT),
    ],cwd=ROOT,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    try:
        wait_http(service,API+"/health")
        wait_http(static,WEB+"/apps/web/local-editor.html")
        status,scene=api("/v1/scenes/current")
        require(status==200 and scene["document_id"]==baseline["identity"]["document_id"]
                and scene["source_hash"]==SOURCE_SHA,"font service baseline mismatched Rust Project")
        status,scope=api("/v1/editor/font-format-scope")
        require(status==200 and scope["revision_id"]==scene["revision_id"] and
                scope["scene_snapshot_id"]==scene["snapshot_id"] and scope["stories"],
                "font range scope must come from independently re-admitted Rust Project")
        status,env=api("/v1/editor/font-environment")
        status2,admission=api("/v1/editor/font-authoring-admission")
        require(status==200 and status2==200 and len(env["fonts"])==1
                and len(admission["resources"])==1,"full font environment not active")
        descriptor=env["fonts"][0]
        grant=admission["resources"][0]
        require(descriptor["resource_id"]==FONT_UUID and
                descriptor["content_hash"]==FONT_SHA and
                grant["content_hash"]==FONT_SHA,"physical font delivery/admission mismatch")
        for target in ("/v1/editor/font-format-scope","/v1/editor/font-authoring-admission"):
            viewer_code,_=api(target,principal="synthetic-viewer")
            require(viewer_code==403,"viewer received font authoring capability")
        available_frames={x["story_id"] for x in scene["story_frames"]}
        chosen=next((s for s in scope["stories"] if s["story_id"] in available_frames
                     and s["story_scalar_len"]>0),None)
        require(chosen is not None,"no visible real Story with admitted Rust text-format overlay")
        cand={
            "protocol_version":"chaptera.font-replacement-candidate.v1",
            "document_id":scene["document_id"],
            "expected_revision_id":scene["revision_id"],
            "scene_snapshot_id":scene["snapshot_id"],
            "layout_environment_id":env["layout_environment_id"],
            "font_set_fingerprint":env["font_set_fingerprint"],
            "resource_id":FONT_UUID,
            "font_fingerprint":descriptor["font_fingerprint"],
            "content_hash":FONT_SHA,"face_index":0,
            "authority":"candidate_only_server_validation_required",
        }
        request={
            "protocol_version":"chaptera.font-resource-intent.v1",
            "document_id":scene["document_id"],"source_hash":SOURCE_SHA,
            "base_revision_id":scene["revision_id"],
            "client_operation_id":"real-font-commit-first",
            "command":{
                "kind":"set_admitted_font_resource","story_id":chosen["story_id"],
                "start_scalar":0,"end_scalar":1,
                "expected_state_hash":chosen["expected_state_hash"],
                "candidate":cand,
            },
        }
        check_denied("/v1/commit",request,principal="synthetic-viewer")
        forged=copy.deepcopy(request)
        forged["command"]["full_font_bytes"]="forged-client-authority"
        require(api("/v1/commit",forged)[0]==400,"client-provided font bytes not rejected")
        status,result=api("/v1/commit",request)
        require(status==200 and result.get("protocol_version")=="chaptera.commit-accepted.v1",
                "authorized Rust font commit rejected: "+str(result))
        op=result["canonical_operation"]
        require(op["kind"]=="set_text_format_property" and op["property"]=="font_resource"
                and op["value"]["resource_id"]==FONT_UUID and
                op["story_id"]==chosen["story_id"],"Rust canonical physical font differs")
        require(result["consequences"][0]["state"]=="partial",
                "unshaped font operation falsely reported full layout support")
        require(api("/v1/commit",request)[1]["revision_id"]==result["revision_id"],
                "font operation not idempotent")
        conflicting=copy.deepcopy(request)
        conflicting["command"]["end_scalar"]=2
        require(api("/v1/commit",conflicting)[1]["code"]=="idempotency_conflict",
                "changed same idempotency key incorrectly accepted")
        stale=copy.deepcopy(request)
        stale["client_operation_id"]="stale-revision-direct"
        require(api("/v1/commit",stale)[1]["code"]=="stale_revision",
                "stale original Scene not rejected")
        status,changed=api("/v1/scenes/current")
        require(status==200 and changed["revision_id"]==result["revision_id"]
                and changed["snapshot_id"]!=scene["snapshot_id"],
                "committed font operation not reflected in a new Scene revision")
        require(api("/v1/editor/font-resource/"+descriptor["fetch_handle"])[0]==409,
                "stale physical-resource fetch handle survived committed revision")
        status,post_scope=api("/v1/editor/font-format-scope")
        after_story=next(x for x in post_scope["stories"] if x["story_id"]==chosen["story_id"])
        require(status==200 and after_story["expected_state_hash"]!=chosen["expected_state_hash"],
                "fresh Rust authoring overlay still reports original resource")
        require(api("/v1/export/preview?target=idml")[0]==409,
                "unshaped font IDML export must fail closed")
        require(api("/v1/pub-save/download")[0]==409,
                "unapproved font-altered Publisher PUB downloadable")
        status,pub=api("/v1/pub-save/preview")
        require(status==200 and pub["can_serialize"] is False and
                pub["can_download"] is False and
                pub["blocker_code"]=="font_layout_unverified",
                "font-edited native Publisher writer not blocked")
        # Real service save/reopen verifies same revision and full OpenType again.
        status,reopened=api("/v1/scenes/current")
        require(reopened==changed,"source/shape unexpectedly changed after commit")
        undo=history(changed,"undo",1)
        status,undo_scope=api("/v1/editor/font-format-scope")
        undo_story=next(x for x in undo_scope["stories"] if x["story_id"]==chosen["story_id"])
        require(undo_story["expected_state_hash"]==chosen["expected_state_hash"],
                "Undo did not restore original native Rust font overlay")
        redo=history(undo,"redo",2)
        status,redo_scope=api("/v1/editor/font-format-scope")
        redo_story=next(x for x in redo_scope["stories"] if x["story_id"]==chosen["story_id"])
        require(redo_story["expected_state_hash"]==after_story["expected_state_hash"],
                "Redo did not restore admitted physical resource")
        before_browser=history(redo,"undo",3)
        require(hashlib.sha256(source.read_bytes()).hexdigest()==SOURCE_SHA,
                "HTTP font commits changed original Publisher file")
        chromium=None
        if browser:
            url=(WEB+"/apps/web/local-editor.html?api="+API+
                 "&emu_per_css_px=12700&pan_y_css_px=-2600")
            chromium=subprocess.run([
                "node","tools/verify_font_http_chromium.mjs",
                url,chosen["story_id"],
            ],cwd=ROOT,capture_output=True,text=True,timeout=100)
            if chromium.returncode:
                raise AssertionError("real Chromium selected font commit failed:\n"+
                                     chromium.stdout[-3500:]+"\n"+chromium.stderr[-3500:])
            require("REAL_FONT_HTTP_CHROMIUM_OK" in chromium.stdout,
                    "real browser click did not prove server font revision")
        receipt={
            "receipt_kind":"chaptera.real-pub-authenticated-font-commit.v1",
            "source_sha256":SOURCE_SHA,"font_sha256":FONT_SHA,
            "story_id":chosen["story_id"],"canonical_operation":op,
            "new_revision_id":result["revision_id"],
            "undo_redo_and_second_undo":True,
            "stale_viewer_forged_denials":True,
            "independent_rust_fresh_reopen":True,
            "fixed_output_eligible":False,
            "browser_selected_range":bool(chromium),
        }
        (work/"receipt.json").write_text(json.dumps(receipt,indent=2,sort_keys=True)+"\n",
                                         encoding="utf-8")
        print("REAL_PUB_HTTP_FONT_COMMIT_OK "+json.dumps(receipt,sort_keys=True))
    finally:
        for proc in (static,service):
            proc.terminate()
        for proc in (static,service):
            try:proc.wait(timeout=10)
            except subprocess.TimeoutExpired:proc.kill()

if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--fixture",type=pathlib.Path,required=True)
    parser.add_argument("--resolved-graph",type=pathlib.Path,required=True)
    parser.add_argument("--viewer",type=pathlib.Path,required=True)
    parser.add_argument("--browser",action="store_true")
    opts=parser.parse_args()
    run(opts.fixture,opts.resolved_graph,opts.viewer,browser=opts.browser)
