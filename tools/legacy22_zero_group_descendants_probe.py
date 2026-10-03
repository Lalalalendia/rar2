"""Source-safe descendant census for zero-size legacy Group Scene nodes."""
from __future__ import annotations
import argparse, hashlib, json, re, shutil, subprocess
from pathlib import Path

FIXTURES = [
    ("001","0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade",72704,"control"),
    ("018","48384326430f61ecd6b924d0010631eb72838b0ad09dc33e0e53f3998ff642fe",343552,"target"),
    ("072","7860acc670667c456fb29048a4cfa1840e4a7d5b57b7b2a2975c8d76929063dc",314880,"target"),
]
PREFIX = "CHAPTERA_ZERO_GROUP_DESCENDANTS "
MARKER = "            match from_viewer_geometry_with_fonts("
RUST_CALL = "            research_zero_group_descendants(&bundle);\n"
RUST_HELPER = r'''
fn research_zero_group_identity<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}
fn research_zero_group_kind<T: serde::Serialize>(value: &T) -> &'static str {
    match serde_json::to_value(value).ok().as_ref().and_then(serde_json::Value::as_str) {
        Some("shape") => "shape", Some("text_frame") => "text_frame",
        Some("image_frame") => "image_frame", Some("vector_path") => "vector_path",
        Some("group") => "group", Some("connector") => "connector",
        Some("table") => "table", Some("placed_artifact") => "placed_artifact",
        Some("unsupported") => "unsupported", _ => "unclassified",
    }
}
fn research_zero_group_extent<T: serde::Serialize>(bounds: &T) -> &'static str {
    let Ok(v) = serde_json::to_value(bounds) else { return "unclassified"; };
    let (Some(w),Some(h))=(v.get("width").and_then(serde_json::Value::as_i64),v.get("height").and_then(serde_json::Value::as_i64)) else { return "unclassified"; };
    if w < 0 || h < 0 { "negative" } else if w == 0 && h == 0 { "both_zero" }
    else if w == 0 { "zero_width" } else if h == 0 { "zero_height" } else { "positive" }
}
fn research_zero_group_descendants(bundle: &pub_viewer::ViewerOpenBundle) {
    use std::collections::{BTreeMap,BTreeSet,VecDeque};
    let graph=&bundle.resolved_graph;
    let graph_nodes=graph.nodes.values().map(|n|(research_zero_group_identity(&n.header.id),n)).collect::<BTreeMap<_,_>>();
    let scene_nodes=bundle.geometry.scene.nodes.iter().map(|n|(research_zero_group_identity(&n.origin),n)).collect::<BTreeMap<_,_>>();
    let mut graph_children=BTreeMap::<String,Vec<String>>::new();
    for (id,n) in &graph_nodes { graph_children.entry(research_zero_group_identity(&n.header.parent_id)).or_default().push(id.clone()); }
    let mut scene_children=BTreeMap::<String,Vec<String>>::new();
    for (id,n) in &scene_nodes { scene_children.entry(research_zero_group_identity(&n.parent_origin)).or_default().push(id.clone()); }

    let mut histogram=BTreeMap::<String,(serde_json::Value,u64)>::new();
    let mut group_count=0u64;
    for (id,scene) in &scene_nodes {
        if research_zero_group_extent(&scene.bounds)!="both_zero" { continue; }
        let Some(source)=graph_nodes.get(id) else { continue; };
        if research_zero_group_kind(&source.kind)!="group" { continue; }
        group_count += 1;

        let direct_graph=graph_children.get(id).map_or(0,Vec::len);
        let direct_scene=scene_children.get(id).map_or(0,Vec::len);

        let mut graph_seen=BTreeSet::new(); let mut q=VecDeque::new();
        if let Some(children)=graph_children.get(id) { for child in children { q.push_back(child.clone()); } }
        while let Some(child)=q.pop_front() {
            if !graph_seen.insert(child.clone()) { continue; }
            if let Some(children)=graph_children.get(&child) { for nested in children { q.push_back(nested.clone()); } }
        }
        let mut scene_seen=BTreeSet::new(); let mut qs=VecDeque::new();
        if let Some(children)=scene_children.get(id) { for child in children { qs.push_back(child.clone()); } }
        while let Some(child)=qs.pop_front() {
            if !scene_seen.insert(child.clone()) { continue; }
            if let Some(children)=scene_children.get(&child) { for nested in children { qs.push_back(nested.clone()); } }
        }
        let graph_positive=graph_seen.iter().filter(|child| graph_nodes.get(*child).is_some_and(|n| research_zero_group_extent(&n.header.bounds)=="positive")).count();
        let scene_positive=scene_seen.iter().filter(|child| scene_nodes.get(*child).is_some_and(|n| research_zero_group_extent(&n.bounds)=="positive")).count();
        let mut kinds=BTreeMap::<&'static str,u64>::new();
        for child in &graph_seen {
            if let Some(n)=graph_nodes.get(child) { *kinds.entry(research_zero_group_kind(&n.kind)).or_default() += 1; }
        }
        let value=serde_json::json!({
            "direct_graph_children":direct_graph,"recursive_graph_descendants":graph_seen.len(),
            "positive_graph_descendants":graph_positive,"direct_scene_children":direct_scene,
            "recursive_scene_descendants":scene_seen.len(),"positive_scene_descendants":scene_positive,
            "descendant_kinds":kinds
        });
        let key=serde_json::to_string(&value).unwrap();
        histogram.entry(key).and_modify(|e|e.1+=1).or_insert((value,1));
    }
    let rows=histogram.into_values().map(|(mut v,count)|{v["group_count"]=serde_json::json!(count);v}).collect::<Vec<_>>();
    eprintln!("CHAPTERA_ZERO_GROUP_DESCENDANTS {}",serde_json::json!({"zero_group_count":group_count,"profiles":rows}));
}
'''

def sha(data: bytes) -> str: return hashlib.sha256(data).hexdigest()

def instrument(root: Path) -> None:
    p=root/"apps/chaptera-server/src/guest_reader_worker.rs"
    s=p.read_text()
    if s.count(MARKER)!=1: raise SystemExit("projection marker mismatch")
    s=s.replace(MARKER,RUST_CALL+MARKER)+RUST_HELPER
    p.write_text(s)

def valid(payload):
    if not isinstance(payload,dict) or set(payload)!={"zero_group_count","profiles"}: return False
    if type(payload["zero_group_count"]) is not int or not 0<=payload["zero_group_count"]<=10000: return False
    total=0
    kinds={"shape","text_frame","image_frame","vector_path","group","connector","table","placed_artifact","unsupported","unclassified"}
    for row in payload["profiles"]:
        if set(row)!={"direct_graph_children","recursive_graph_descendants","positive_graph_descendants","direct_scene_children","recursive_scene_descendants","positive_scene_descendants","descendant_kinds","group_count"}: return False
        if any(type(row[k]) is not int or not 0<=row[k]<=1000000 for k in row if k!="descendant_kinds"): return False
        if not isinstance(row["descendant_kinds"],dict) or any(k not in kinds or type(v) is not int or v<0 for k,v in row["descendant_kinds"].items()): return False
        total += row["group_count"]
    return total==payload["zero_group_count"]

def observe(root: Path, corpus: Path, out: Path):
    worker=root/"target/zero-group-descendants/debug/chaptera"
    harness=root/"tools/migration_pdf_worker_isolation.py"
    rows=[]
    for idx,(fid,digest,size,role) in enumerate(FIXTURES,1):
        source=corpus/f"{digest}.pub"; data=source.read_bytes()
        assert len(data)==size and sha(data)==digest
        worker_out=out.parent/f"private-{fid}"
        cp=subprocess.run([
            "python3",str(harness),"run","--output-dir",str(worker_out),"--input",str(source),
            "--timeout","120","--cpu-seconds","90","--address-space-mb","768","--open-files","64","--output-file-mb","32",
            "--clear-environment","--",str(worker),"guest-reader-scene","--session-id","guest:"+str(idx).zfill(32),
            "--expected-sha256",digest,"--expected-byte-len",str(size)
        ],cwd=root,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=135,check=False)
        isolation=json.loads(cp.stdout)
        payloads=[]
        for line in str(isolation.get("stderr_tail") or "").splitlines():
            if line.startswith(PREFIX):
                try:
                    p=json.loads(line[len(PREFIX):])
                    if valid(p): payloads.append(p)
                except Exception: pass
        receipt=None
        rp=worker_out/"result.json"
        if rp.is_file(): receipt=json.loads(rp.read_text())
        row={"fixture_id":fid,"source_sha256":digest,"source_byte_len":size,"role":role,
             "isolation_status":isolation.get("status"),"exit_code":isolation.get("exit_code"),
             "reader_classification":receipt.get("classification") if receipt else None,
             "reader_terminal_code":receipt.get("terminal_code") if receipt else None,
             "descendants":payloads[0] if len(payloads)==1 else None}
        rows.append(row); print(json.dumps(row,sort_keys=True))
        shutil.rmtree(worker_out,ignore_errors=True)
    assert rows[0]["descendants"] and rows[0]["descendants"]["zero_group_count"]==0, rows[0]
    assert all(r["descendants"] and r["descendants"]["zero_group_count"]>0 for r in rows[1:]), rows
    result={"schema":"chaptera.legacy22-zero-group-descendants.v1","raw_source_bytes_emitted":False,
            "raw_source_text_emitted":False,"raw_ids_emitted":False,"fixtures":rows}
    out.parent.mkdir(parents=True,exist_ok=True); out.write_text(json.dumps(result,indent=2,sort_keys=True)+"\n")

def main():
    ap=argparse.ArgumentParser(); ap.add_argument("action",choices=["instrument","observe"])
    ap.add_argument("--source-root",type=Path,required=True); ap.add_argument("--corpus",type=Path); ap.add_argument("--output",type=Path,required=True)
    a=ap.parse_args(); root=a.source_root.resolve()
    if a.action=="instrument": instrument(root); a.output.parent.mkdir(parents=True,exist_ok=True); a.output.write_text('{"instrumented":true}\n')
    else: observe(root,a.corpus.resolve(),a.output.resolve())
if __name__=="__main__": main()
