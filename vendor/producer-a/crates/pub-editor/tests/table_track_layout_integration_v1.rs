use std::collections::BTreeMap;

use pub_editor::{
    CreateTableRuntimeV1, EditorSession, LengthEmu, NodeId, PageId, RectEmu, StoryId, TableCellId,
    TableTrackTargetV1,
};
use pub_layout::{
    BoundedAuthoringSlice, BoundedNodeGeometryInput, EffectiveTableLayoutInputV1,
    bounded_table_input_from_effective_grid, project_bounded,
    resolve_bounded_effective_table_cells,
};
use pub_model::{
    Affine2D, Document, DocumentId, Page, ResolvedGraph, Sha256Digest, Size2D, SourceDescriptor,
    TableColumnId, TableRowId,
};
use pub_reader::PubResolvedGraph;

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn source_hash() -> Sha256Digest {
    "bcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbc"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn table_id() -> NodeId {
    canonical_id("01890f47-4000-7abc-8def-0123456789ab")
}

fn story_id() -> StoryId {
    canonical_id("01890f47-4001-7abc-8def-0123456789ab")
}

fn row_ids() -> Vec<TableRowId> {
    vec![
        canonical_id("01890f47-4010-7abc-8def-0123456789ab"),
        canonical_id("01890f47-4011-7abc-8def-0123456789ab"),
    ]
}

fn column_ids() -> Vec<TableColumnId> {
    vec![
        canonical_id("01890f47-4020-7abc-8def-0123456789ab"),
        canonical_id("01890f47-4021-7abc-8def-0123456789ab"),
    ]
}

fn cell_ids() -> Vec<TableCellId> {
    vec![
        canonical_id("01890f47-4030-7abc-8def-0123456789ab"),
        canonical_id("01890f47-4031-7abc-8def-0123456789ab"),
        canonical_id("01890f47-4032-7abc-8def-0123456789ab"),
        canonical_id("01890f47-4033-7abc-8def-0123456789ab"),
    ]
}

fn source_bounds() -> RectEmu {
    RectEmu::new(
        LengthEmu::new(100_000),
        LengthEmu::new(200_000),
        LengthEmu::new(600_000),
        LengthEmu::new(400_000),
    )
}

fn runtime() -> CreateTableRuntimeV1 {
    CreateTableRuntimeV1 {
        node_id: table_id(),
        story_id: story_id(),
        page_id: page_id(),
        bounds: source_bounds(),
        row_ids: row_ids(),
        column_ids: column_ids(),
        cell_ids: cell_ids(),
    }
}

fn graph() -> PubResolvedGraph {
    let source_hash = source_hash();
    let page_id = page_id();
    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "table-track-layout-integration-v1".into(),
        source: SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "pub-rs/test".into(),
            source_hash,
        },
        document: Document {
            id: canonical_id::<DocumentId>("33000000-0000-4000-8000-000000000001"),
            format_origin: "pub".into(),
            source_hash,
            pages: vec![page_id],
            resources: Vec::new(),
            styles: Vec::new(),
        },
        pages: BTreeMap::from([(
            page_id,
            Page {
                id: page_id,
                size: Size2D::new(LengthEmu::new(8_000_000), LengthEmu::new(10_000_000)),
                bleed: None,
                margins: None,
                children: Vec::new(),
                extensions: Vec::new(),
            },
        )]),
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
fn editor_track_history_drives_nonuniform_layout_without_source_metric_reparse() {
    let mut session = EditorSession::new(graph()).expect("session");
    session.create_table(runtime()).expect("CreateTable");
    session
        .set_table_track_extent_v1(
            table_id(),
            TableTrackTargetV1::Row(row_ids()[0]),
            LengthEmu::new(250_000),
        )
        .expect("resize first row");
    session
        .set_table_track_extent_v1(
            table_id(),
            TableTrackTargetV1::Column(column_ids()[1]),
            LengthEmu::new(350_000),
        )
        .expect("resize second column");

    let grid = session
        .current_table_grid_v1(table_id())
        .expect("current grid");
    let bounds = session
        .current_table_bounds_v1(table_id())
        .expect("current table bounds");

    assert_eq!(bounds.width, LengthEmu::new(650_000));
    assert_eq!(bounds.height, LengthEmu::new(450_000));
    assert_eq!(grid.rows[0].extent, Some(LengthEmu::new(250_000)));
    assert_eq!(grid.rows[1].extent, Some(LengthEmu::new(200_000)));
    assert_eq!(grid.columns[0].extent, Some(LengthEmu::new(300_000)));
    assert_eq!(grid.columns[1].extent, Some(LengthEmu::new(350_000)));

    let table = bounded_table_input_from_effective_grid(&grid).expect("table input");
    let projection = project_bounded(BoundedAuthoringSlice {
        pages: vec![Page {
            id: page_id(),
            size: Size2D::new(LengthEmu::new(8_000_000), LengthEmu::new(10_000_000)),
            bleed: None,
            margins: None,
            children: vec![table_id()],
            extensions: Vec::new(),
        }],
        node_geometry: vec![BoundedNodeGeometryInput {
            node_id: table_id(),
            parent_origin: page_id().into_canonical(),
            bounds: source_bounds(),
            transform: Affine2D::identity(),
        }],
        stories: Vec::new(),
        story_frames: Vec::new(),
        tables: vec![table],
        guides: Vec::new(),
        unknown_layout_state: Vec::new(),
    });

    let resolved = resolve_bounded_effective_table_cells(
        &projection,
        &[EffectiveTableLayoutInputV1 { grid, bounds }],
    )
    .expect("resolve current editor table state");

    assert_eq!(resolved.cells.len(), 4);
    assert_eq!(
        resolved.cells[0].bounds,
        RectEmu::new(
            LengthEmu::new(100_000),
            LengthEmu::new(200_000),
            LengthEmu::new(300_000),
            LengthEmu::new(250_000),
        )
    );
    assert_eq!(
        resolved.cells[1].bounds,
        RectEmu::new(
            LengthEmu::new(400_000),
            LengthEmu::new(200_000),
            LengthEmu::new(350_000),
            LengthEmu::new(250_000),
        )
    );
    assert_eq!(
        resolved.cells[2].bounds,
        RectEmu::new(
            LengthEmu::new(100_000),
            LengthEmu::new(450_000),
            LengthEmu::new(300_000),
            LengthEmu::new(200_000),
        )
    );
    assert_eq!(
        resolved.cells[3].bounds,
        RectEmu::new(
            LengthEmu::new(400_000),
            LengthEmu::new(450_000),
            LengthEmu::new(350_000),
            LengthEmu::new(200_000),
        )
    );

    assert_eq!(
        session.graph().nodes[&table_id()].header.bounds,
        source_bounds()
    );
    let source_metrics = session.graph().nodes[&table_id()]
        .payload
        .table
        .as_ref()
        .and_then(|table| table.layout_metrics.as_ref())
        .expect("source layout metrics remain present");
    assert_eq!(source_metrics.row_pitch, LengthEmu::new(200_000));
    assert_eq!(source_metrics.cell_width, LengthEmu::new(300_000));
}
