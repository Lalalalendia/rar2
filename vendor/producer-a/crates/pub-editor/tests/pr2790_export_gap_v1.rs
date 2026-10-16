//! CI-only, source-neutral diagnostic for PR #2790.
//!
//! Uses the ACTUAL pub-editor IDML/ODG adapters, unlike the synthetic XML
//! oracle self-test. It deliberately proves the known pre-fix overlay MoveNodes
//! rejection. GAP_CONFIRMED is not evidence that MoveNodes is implemented.

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
fn actual_chaptera_adapters_export_three_shapes_and_overlay_move_gap_is_confirmed() {
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

    // Isolate the current implementation gap. It is expected to fail until
    // the canonical authored_shapes MoveNodes consumer is implemented.
    let node_b: NodeId = canonical_id(B);
    let mut attempted = project;
    attempted.operations.push(EditOperation::MoveNodes {
        page_id: page_id(),
        entries: vec![MoveNodeBatchEntry {
            node_id: node_b,
            before: rectangle(300_000),
            after: rectangle(500_000),
        }],
    });
    let mut reopened = EditorSession::new(graph.clone()).expect("fresh EditorSession");
    let result = reopened.apply_project(&attempted);
    let exact_gap = match &result {
        Err(EditorProjectError::Operation {
            index: 3,
            error: EditorError::NodeMoveUnsupported { node_id },
        }) => *node_id == node_b,
        _ => false,
    };
    assert!(exact_gap,
        "expected pre-fix authored-overlays-not-in-graph refusal; got {result:?}");
    assert_eq!(reopened.graph(), &graph);
    assert!(reopened.operations().is_empty(), "failed project must not mutate history");
    fs::write(out.join("gap.json"),
              serde_json::to_vec_pretty(&json!({
                  "schema": "chaptera.pr2790-overlay-move-gap.v1",
                  "status": "GAP_CONFIRMED",
                  "source_graph_unchanged": true,
                  "created_shapes": 3,
                  "attempted_changed_nodes": 1,
                  "expected_error_code": "node_move_unsupported",
                  "real_adapter_baseline_export_only": true,
                  "author_created_move_nodes_implemented": false,
              })).expect("gap JSON"))
        .expect("write gap report");
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

    // The same authored-overlay MoveNodes gap must be reproduced when the
    // underlying source is a genuine PUB, not merely a synthetic graph.
    let mut attempt = project;
    let middle: NodeId = canonical_id(B);
    attempt.operations.push(EditOperation::MoveNodes {
        page_id: source_page,
        entries: vec![MoveNodeBatchEntry {
            node_id: middle, before: rectangle(300_000), after: rectangle(500_000),
        }],
    });
    let mut fresh = open_mature_0x2c_editor(&bytes, source_hash)
        .expect("fresh original source for fail-closed diagnosis");
    let result = fresh.apply_project(&attempt);
    assert!(matches!(result, Err(EditorProjectError::Operation {
        index: 3, error: EditorError::NodeMoveUnsupported { node_id },
    }) if node_id == middle), "current author-created MoveNodes gap must be fail-closed");
    assert!(fresh.operations().is_empty());
    assert_eq!(fresh.graph(), &source_graph);
    fs::write(output.join("gap-real.json"), serde_json::to_vec_pretty(&json!({
        "schema": "chaptera.pr2790-overlay-move-gap.v1",
        "source_sha256": PINNED,
        "source_type": "pinned_public_original_pub",
        "created_shapes": 3,
        "source_graph_unchanged": true,
        "canonical_project_replay": true,
        "create_undo_redo": true,
        "status": "GAP_CONFIRMED",
        "expected_error_code": "node_move_unsupported",
        "author_created_move_nodes_implemented": false,
    })).expect("gap diagnostic JSON")).expect("write source-free real PUB gap");
}
