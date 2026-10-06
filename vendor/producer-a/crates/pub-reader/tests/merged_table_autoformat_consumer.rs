use pub_model::Sha256Digest;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

#[test]
fn exact_carlton_merged_table_autoformat_consumer() {
    let fixture = PathBuf::from(
        std::env::var_os("CHAPTERA_TABLE_CELL_PAINT_FIXTURE")
            .expect("CHAPTERA_TABLE_CELL_PAINT_FIXTURE"),
    );
    let bytes = fs::read(&fixture).expect("read exact TABLE fixture");
    let build = pub_reader::build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash(&bytes),
    )
    .expect("mature SourceGraph");

    let tables = build
        .graph
        .nodes
        .values()
        .filter_map(|node| node.payload.table.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        tables.len(),
        1,
        "exact Carlton fixture must expose one TABLE"
    );

    let table = tables[0];
    let layout_relation = table
        .layout_relation
        .as_ref()
        .expect("exact Carlton TABLE must retain its source-backed layout relation");
    assert!(
        !layout_relation.source_refs.is_empty(),
        "TABLE layout relation must retain exact source provenance"
    );
    assert!(
        table.layout_metrics.is_none(),
        "non-uniform Carlton MCLD metrics must remain fail-closed"
    );
    let uniform_text_inset = table
        .uniform_text_inset
        .as_ref()
        .expect("exact Carlton TABLE must retain unanimous source-backed text inset");
    assert_eq!(
        uniform_text_inset.story_layout_key,
        layout_relation.story_layout_key,
        "TABLE text inset must be bound to the exact retained layout relation"
    );
    assert!(
        uniform_text_inset.inset_emu.get() >= 0,
        "TABLE text inset must remain a bounded non-negative physical length"
    );
    assert!(
        !uniform_text_inset.source_refs.is_empty(),
        "TABLE text inset must retain exact MCLD provenance"
    );
    assert_eq!((table.rows, table.columns), (9, 2));
    assert_eq!(table.cells.len(), 17);
    assert!(
        table.simple_table.is_none(),
        "Carlton merged TABLE must remain outside the simple-table contract"
    );

    let spanning = table
        .cells
        .iter()
        .filter(|cell| {
            cell.coordinates.is_some_and(|coordinates| {
                coordinates.start_row != coordinates.end_row
                    || coordinates.start_column != coordinates.end_column
            })
        })
        .count();
    assert_eq!(spanning, 1);

    let painted = table
        .cells
        .iter()
        .filter(|cell| cell.paint.is_some())
        .count();
    assert_eq!(painted, 17);
    assert_eq!(table.border_segments.len(), 20);

    let story_id = table.story_id.expect("exact Carlton TABLE Story");
    let story = build
        .graph
        .stories
        .get(&story_id)
        .expect("exact Carlton TABLE Story payload");
    let materialized = pub_reader::materialize_bounded_table_cells(table, story)
        .expect("materialize Carlton cells");
    assert_eq!(materialized.len(), 17);
    let nonempty = materialized
        .iter()
        .filter(|cell| !cell.text.is_empty())
        .count();
    let nonempty_ranged = materialized
        .iter()
        .filter(|cell| !cell.text.is_empty())
        .filter(|cell| cell.story_scalar_start < cell.story_scalar_end)
        .count();
    let empty_collapsed = materialized
        .iter()
        .filter(|cell| cell.text.is_empty())
        .filter(|cell| cell.story_scalar_start == cell.story_scalar_end)
        .count();
    assert_eq!(nonempty, 14);
    assert_eq!(nonempty_ranged, 14);
    assert_eq!(empty_collapsed, 3);

    println!(
        "TABLE_MERGED_AUTOFORMAT_CONSUMER tables={} cells={} spanning={} painted={} borders={} nonempty={} nonempty_ranged={} empty_collapsed={}",
        tables.len(),
        table.cells.len(),
        spanning,
        painted,
        table.border_segments.len(),
        nonempty,
        nonempty_ranged,
        empty_collapsed,
    );
}
