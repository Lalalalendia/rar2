//! CI-only bounded positive consumer test for PR #2790.
//!
//! Exercises the actual canonical MoveNodes authored Rectangle runtime,
//! persisted replay, Undo/Redo, and Chaptera IDML/ODG adapters before and
//! after movement. Real public PUB bytes never leave the Actions runner.

use std::{collections::BTreeMap, env, fs, path::PathBuf};

use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredShapePaintV1, AuthoredSolidFillV1,
    AuthoredSolidStrokeV1, EditorEditableTarget, EditorError, EditorProjectError,
    EditorProject, EditorSession, EditOperation, LengthEmu, MoveNodeBatchEntry, RectEmu, Srgb8V1,
    open_mature_0x2c_editor,
};
use pub_model::{
    Document, DocumentId, NodeId, Page, PageId, ResolvedGraph, Sha256Digest,
    Size2D, SourceDescriptor,
};
use pub_reader::PubResolvedGraph;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const A: &str = "019a2d80-1112-7001-8001-aabbccddee01";
const B: &str = "019a2d80-1112-7001-8001-aabbccddee02";
const C: &str = "019a2d80-1112-7001-8001-aabbccddee03";
const PAGE: &str = "019a2d80-1112-7001-8001-aabbccddee00";
const SOURCE_HASH: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn canonical_id<T: serde::de::DeserializeOwned>(s: &str) -> T {
    serde_json::from_str(&format!("\"{s}\"")).expect("canonical typed ID")
}

fn page_id() -> PageId {
    canonical_id(PAGE)
}

fn rectangle(x: i64) -> RectEmu {
    RectEmu::new(
        LengthEmu::new(x), LengthEmu::new(100_000),
        LengthEmu::new(500_000), LengthEmu::new(250_000),
    )
}

fn paint() -> AuthoredShapePaintV1 {
    AuthoredShapePaintV1 {
        fill: AuthoredSolidFillV1 {
            visible: true,
            color: Srgb8V1 {r: 18, g: 52, b: 86},
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 {r: 170, g: 187, b: 204},
            width_emu: 12_700,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    }
}

fn baseline_graph() -> PubResolvedGraph {
    let page = page_id();
    let hash: Sha256Digest = SOURCE_HASH.parse().expect("canonical SHA-256");
    let mut pages = BTreeMap::new();
    pages.insert(page, Page {
        id: page,
        size: Size2D::new(LengthEmu::new(3_000_000), LengthEmu::new(2_000_000)),
        bleed: None,
        margins: None,
        children: Vec::new(),
        extensions: Vec::new(),
    });
    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "pr2790-ci-source-neutral".into(),
        source: SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "pub-rs/test".into(),
            source_hash: hash,
        },
        document: Document {
            id: canonical_id::<DocumentId>("33000000-0000-4000-8000-000000000001"),
            format_origin: "pub".into(),
            source_hash: hash,
            pages: vec![page],
            resources: Vec::new(),
            styles: Vec::new(),
        },
        pages,
        nodes: BTreeMap::new(),
        stories: BTreeMap::new(),
        paragraphs: BTreeMap::new(),
        text_runs: BTreeMap::new(),
        resources: BTreeMap::new(),
        styles: BTreeMap::new(),
        extensions: BTreeMap::new(),
    }
}

#[test]
fn actual_chaptera_adapters_move_authored_middle_and_reopen() {
    let graph = baseline_graph();
    let mut session = EditorSession::new(graph.clone()).expect("source-neutral EditorSession");
    let positions = [(A, 100_000), (B, 300_000), (C, 900_000)];
    let mut expected_bounds = BTreeMap::<String, Value>::new();
    let mut selected = Vec::<String>::new();
    for (key, x) in positions {
        let node: NodeId = canonical_id(key);
        session.create_shape(node, page_id(), rectangle(x), paint()).expect("create rectangle");
        assert_eq!(session.authored_shape(node).unwrap().bounds, rectangle(x));
        assert!(!session.graph().nodes.contains_key(&node));
        selected.push(key.to_string());
        expected_bounds.insert(key.to_string(), json!({
            "x": x, "y": 100_000, "width": 500_000, "height": 250_000
        }));
    }
    assert_eq!(session.graph(), &graph, "CreateShape must preserve source graph");
    let project = session.project();
    assert_eq!(project.operations.len(), 3);

    // Non-authoritative CI artifacts contain no source PUB or credentials.
    let out = env::var("PR2790_GEOMETRY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::temp_dir().join("pr2790-geometry-probe"));
    fs::create_dir_all(&out).expect("CI artifact directory");

    let witness = json!({
        "schema": "chaptera.temporary-editor-move-nodes-export-witness.v1",
        "source_sha256": SOURCE_HASH,
        "revision_id": format!("sha256:{}", "0".repeat(64)),
        "page_id": PAGE,
        "page_size": {"width": 3_000_000, "height": 2_000_000},
        "selected_node_ids": selected,
        "operations": project.operations.clone(),
        "expected_bounds": expected_bounds,
        "expected_moved_node_ids": [],
    });
    fs::write(out.join("witness-baseline.json"),
              serde_json::to_vec_pretty(&witness).expect("witness JSON"))
        .expect("write source-neutral witness");

    for (target, extension) in [
        (EditorEditableTarget::Idml, "idml"),
        (EditorEditableTarget::Odg, "odg"),
    ] {
        let export = session.export_editable(target, "pr2790-three-authored-rectangles")
            .expect("real Chaptera adapter export");
        assert!(export.report.can_serialize, "no blocking export losses");
        fs::write(out.join(format!("actual.{extension}")), &export.bytes)
            .expect("write real Chaptera editable artifact");
    }

    let middle: NodeId = canonical_id(B);
    let mut moved_project = project;
    moved_project.operations.push(EditOperation::MoveNodes {
        page_id: page_id(),
        entries: vec![MoveNodeBatchEntry {
            node_id: middle, before: rectangle(300_000), after: rectangle(500_000),
        }],
    });
    let mut reopened = EditorSession::new(graph.clone()).expect("fresh EditorSession");
    reopened.apply_project(&moved_project).expect("author-created MoveNodes replay");
    assert_eq!(reopened.project(), moved_project, "canonical history must replay exactly");
    assert_eq!(reopened.operations().len(), 4, "one atomic MoveNodes history unit");
    assert_eq!(reopened.authored_shape(middle).unwrap().bounds, rectangle(500_000));
    for (id, x) in [(A, 100_000), (C, 900_000)] {
        assert_eq!(reopened.authored_shape(canonical_id(id)).unwrap().bounds, rectangle(x));
    }
    assert_eq!(reopened.graph(), &graph, "never rewrite imported source graph");
    reopened.undo().expect("one MoveNodes undo");
    assert_eq!(reopened.operations().len(), 3);
    assert_eq!(reopened.authored_shape(middle).unwrap().bounds, rectangle(300_000));
    reopened.redo().expect("one MoveNodes redo");
    assert_eq!(reopened.project(), moved_project);
    assert_eq!(reopened.authored_shape(middle).unwrap().bounds, rectangle(500_000));
    let mut separate = EditorSession::new(graph.clone()).expect("another fresh source");
    separate.apply_project(&moved_project).expect("persisted project source-bound reopen");
    assert_eq!(separate.project(), moved_project);
    assert_eq!(separate.authored_shape(middle).unwrap().bounds, rectangle(500_000));
    assert_eq!(separate.graph(), &graph);

    let mut moved_expected = expected_bounds;
    moved_expected.insert(B.to_string(), json!({
        "x": 500_000, "y": 100_000, "width": 500_000, "height": 250_000
    }));
    let moved_witness = json!({
        "schema": "chaptera.temporary-editor-move-nodes-export-witness.v1",
        "source_sha256": SOURCE_HASH,
        "revision_id": format!("sha256:{}", "1".repeat(64)),
        "page_id": PAGE,
        "page_size": {"width": 3_000_000, "height": 2_000_000},
        "selected_node_ids": selected,
        "operations": moved_project.operations.clone(),
        "expected_bounds": moved_expected,
        "expected_moved_node_ids": [B],
    });
    fs::write(out.join("witness-moved.json"),
        serde_json::to_vec_pretty(&moved_witness).expect("moved witness"))
        .expect("persist moved geometry witness");
    for (target, extension) in [
        (EditorEditableTarget::Idml, "idml"),
        (EditorEditableTarget::Odg, "odg"),
    ] {
        let export = reopened.export_editable(target, "pr2790-after-MoveNodes")
            .expect("real Chaptera moved-geometry export");
        assert!(export.report.can_serialize);
        assert_eq!(export.report.counts.blocking, 0);
        fs::write(out.join(format!("actual-moved.{extension}")), &export.bytes)
            .expect("write moved geometry editable export");
    }

    // Stale or foreign entries fail before any mutation and never partially
    // append one batch to the baseline history.
    let mut stale = moved_project.clone();
    let EditOperation::MoveNodes { entries, .. } = stale.operations.last_mut().unwrap()
        else { unreachable!("canonical last operation must be MoveNodes") };
    entries[0].before = rectangle(300_001);
    let mut denied = EditorSession::new(graph.clone()).expect("stale source");
    assert!(matches!(
        denied.apply_project(&stale),
        Err(EditorProjectError::Operation {
            index: 3, error: EditorError::StaleNodeMove { node_id },
        }) if node_id == middle
    ));
    assert!(denied.operations().is_empty());
    assert_eq!(denied.graph(), &graph);
    let mut foreign = moved_project.clone();
    let EditOperation::MoveNodes { entries, .. } = foreign.operations.last_mut().unwrap()
        else { unreachable!("canonical last operation must be MoveNodes") };
    entries.push(MoveNodeBatchEntry {
        node_id: canonical_id("019a2d80-1112-7001-8001-aabbccddee04"),
        before: rectangle(400_000),
        after: rectangle(600_000),
    });
    let mut denied = EditorSession::new(graph.clone()).expect("foreign source");
    assert!(denied.apply_project(&foreign).is_err());
    assert!(denied.operations().is_empty());
    assert_eq!(denied.graph(), &graph);

    fs::write(out.join("move-positive.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": "chaptera.pr2790-authored-move-proof.v1",
            "status": "PASS",
            "source_graph_unchanged": true,
            "created_shapes": 3,
            "attempted_changed_nodes": 1,
            "changed_node_ids": [B],
            "canonical_project_replay": true,
            "one_atomic_history_operation": true,
            "move_undo_redo": true,
            "stale_and_foreign_fail_closed": true,
            "author_created_move_nodes_implemented": true,
        })).expect("positive proof JSON"))
        .expect("write source-neutral positive result");
}

/// Source bytes come from the immutable, SHA-pinned public Apache POI fixture.
/// Only canonical author-created operations and source-free receipts leave CI.
#[test]
fn pinned_public_real_pub_three_shapes_replay_and_export() {
    const PINNED: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";
    let input = env::var("PR2790_PUBLIC_REAL_PUB")
        .expect("CI must provide PR2790_PUBLIC_REAL_PUB from the pinned Apache POI fixture");
    let bytes = fs::read(&input).expect("read public pinned PUB bytes");
    assert_eq!(bytes.len(), 291_840, "unexpected public fixture size");

    let digest = Sha256::digest(&bytes);
    let mut raw_hash = [0_u8; 32];
    raw_hash.copy_from_slice(&digest);
    let source_hash = Sha256Digest::from_bytes(raw_hash);
    assert_eq!(source_hash.to_string(), PINNED, "original public PUB SHA-256 changed");

    let mut authoring = open_mature_0x2c_editor(&bytes, source_hash)
        .expect("original public PUB must be parsed by actual Publisher reader");
    let source_graph = authoring.graph().clone();
    let source_page = *source_graph.document.pages.first().expect("PUB must have a first page");
    let page = source_graph.pages.get(&source_page).expect("page graph entry");
    assert!(page.size.width.get() >= 1_500_000 && page.size.height.get() >= 400_000,
        "public page cannot safely contain three fixture rectangles");

    let positions = [(A, 100_000_i64), (B, 300_000_i64), (C, 900_000_i64)];
    let mut selected = Vec::<String>::new();
    let mut expected_bounds = BTreeMap::<String, Value>::new();
    for (id, x) in positions {
        let node_id: NodeId = canonical_id(id);
        authoring.create_shape(node_id, source_page, rectangle(x), paint())
            .expect("create 3 rectangles on genuine PUB");
        assert_eq!(authoring.authored_shape(node_id).unwrap().bounds, rectangle(x));
        assert!(!authoring.graph().nodes.contains_key(&node_id));
        selected.push(id.to_string());
        expected_bounds.insert(id.to_string(), json!({
            "x": x, "y": 100_000, "width": 500_000, "height": 250_000
        }));
    }
    assert_eq!(authoring.graph(), &source_graph, "source PUB graph must remain immutable");

    let project_json = serde_json::to_vec(&authoring.project()).expect("serialize canonical project");
    let project: EditorProject =
        serde_json::from_slice(&project_json).expect("reload canonical project JSON");
    assert_eq!(project.operations.len(), 3);
    let mut replayed = open_mature_0x2c_editor(&bytes, source_hash)
        .expect("fresh original source for project replay");
    replayed.apply_project(&project).expect("CreateShape x3 canonical replay");
    assert_eq!(replayed.project(), project);
    assert_eq!(replayed.graph(), &source_graph);
    for (id, x) in positions {
        let node_id: NodeId = canonical_id(id);
        assert_eq!(replayed.authored_shape(node_id).expect("replayed shape").bounds, rectangle(x));
    }
    for _ in 0..3 {
        replayed.undo().expect("undo one CreateShape");
    }
    for (id, _) in positions {
        let node_id: NodeId = canonical_id(id);
        assert!(replayed.authored_shape(node_id).is_none(), "undo removed all authored shapes");
    }
    for _ in 0..3 {
        replayed.redo().expect("redo one CreateShape");
    }
    assert_eq!(replayed.project(), project, "redo must restore exact canonical project");
    assert_eq!(replayed.graph(), &source_graph);

    let output = env::var("PR2790_GEOMETRY_DIR")
        .map(PathBuf::from)
        .expect("CI source-free receipt output path required");
    fs::create_dir_all(&output).expect("create CI receipt directory");
    let witness = json!({
        "schema": "chaptera.temporary-editor-move-nodes-export-witness.v1",
        "source_sha256": PINNED,
        "revision_id": format!("sha256:{}", "0".repeat(64)),
        "page_id": source_page.as_canonical().to_string(),
        "page_size": {
            "width": page.size.width.get(), "height": page.size.height.get()
        },
        "selected_node_ids": selected,
        "operations": project.operations.clone(),
        "expected_bounds": expected_bounds,
        "expected_moved_node_ids": [],
    });
    fs::write(output.join("witness-real-baseline.json"),
        serde_json::to_vec_pretty(&witness).expect("bounded canonical witness"))
        .expect("write source-free real PUB witness");
    for (target, suffix) in [
        (EditorEditableTarget::Idml, "idml"),
        (EditorEditableTarget::Odg, "odg"),
    ] {
        let export = replayed.export_editable(target, "public-pinned-sample")
            .expect("real source -> authentic Chaptera editable exporter");
        assert!(export.report.can_serialize, "export report must admit serialization");
        assert_eq!(export.report.counts.blocking, 0);
        fs::write(output.join(format!("actual-real.{suffix}")), &export.bytes)
            .expect("ephemeral real-derived editable artifact");
    }

    // Replay ONE exact MoveNodes operation against original Publisher bytes.
    let mut moved = project;
    let middle: NodeId = canonical_id(B);
    moved.operations.push(EditOperation::MoveNodes {
        page_id: source_page,
        entries: vec![MoveNodeBatchEntry {
            node_id: middle, before: rectangle(300_000), after: rectangle(500_000),
        }],
    });
    let mut fresh = open_mature_0x2c_editor(&bytes, source_hash)
        .expect("fresh original public PUB");
    fresh.apply_project(&moved).expect("real PUB CreateShape*3 -> MoveNodes success");
    assert_eq!(fresh.project(), moved);
    assert_eq!(fresh.graph(), &source_graph);
    assert_eq!(fresh.operations().len(), 4);
    assert_eq!(fresh.authored_shape(middle).unwrap().bounds, rectangle(500_000));
    fresh.undo().expect("real PUB MoveNodes Undo");
    assert_eq!(fresh.authored_shape(middle).unwrap().bounds, rectangle(300_000));
    assert_eq!(fresh.operations().len(), 3);
    fresh.redo().expect("real PUB MoveNodes Redo");
    assert_eq!(fresh.project(), moved);
    assert_eq!(fresh.authored_shape(middle).unwrap().bounds, rectangle(500_000));
    for (id, x) in [(A, 100_000), (C, 900_000)] {
        assert_eq!(fresh.authored_shape(canonical_id(id)).unwrap().bounds, rectangle(x));
    }
    let mut independently_reopened = open_mature_0x2c_editor(&bytes, source_hash)
        .expect("independent source reopen");
    independently_reopened.apply_project(&moved).expect("persisted changed project replay");
    assert_eq!(independently_reopened.project(), moved);
    assert_eq!(independently_reopened.graph(), &source_graph);
    assert_eq!(independently_reopened.authored_shape(middle).unwrap().bounds, rectangle(500_000));

    let mut moved_expected = expected_bounds;
    moved_expected.insert(B.to_string(), json!({
        "x": 500_000, "y": 100_000, "width": 500_000, "height": 250_000
    }));
    let witness = json!({
        "schema": "chaptera.temporary-editor-move-nodes-export-witness.v1",
        "source_sha256": PINNED,
        "revision_id": format!("sha256:{}", "1".repeat(64)),
        "page_id": source_page.as_canonical().to_string(),
        "page_size": {"width": page.size.width.get(), "height": page.size.height.get()},
        "selected_node_ids": selected,
        "operations": moved.operations.clone(),
        "expected_bounds": moved_expected,
        "expected_moved_node_ids": [B],
    });
    fs::write(output.join("witness-real-moved.json"),
        serde_json::to_vec_pretty(&witness).expect("moved real witness"))
        .expect("write source-free moved witness");
    for (target, suffix) in [
        (EditorEditableTarget::Idml, "idml"),
        (EditorEditableTarget::Odg, "odg"),
    ] {
        let export = fresh.export_editable(target, "public-pinned-sample-after-move")
            .expect("real PUB post MoveNodes exporter");
        assert!(export.report.can_serialize);
        assert_eq!(export.report.counts.blocking, 0);
        fs::write(output.join(format!("actual-real-moved.{suffix}")), &export.bytes)
            .expect("write ephemeral changed editable export");
    }
    fs::write(output.join("move-real.json"), serde_json::to_vec_pretty(&json!({
        "schema": "chaptera.pr2790-authored-move-proof.v1",
        "source_sha256": PINNED,
        "source_type": "pinned_public_original_pub",
        "created_shapes": 3,
        "source_graph_unchanged": true,
        "canonical_project_replay": true,
        "create_undo_redo": true,
        "move_undo_redo": true,
        "one_atomic_history_operation": true,
        "changed_node_ids": [B],
        "status": "PASS",
        "author_created_move_nodes_implemented": true,
    })).expect("real positive JSON")).expect("write source-free real move proof");
}
