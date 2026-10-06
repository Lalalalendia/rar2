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

    let inset_rejections = build
        .diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            pub_reader::PubBridgeDiagnostic::TableLayoutMetricsUnavailable { reason, .. }
                if reason.starts_with("uniform_cell_text_inset:") =>
            {
                Some(reason.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        inset_rejections.len(),
        1,
        "exact Carlton TABLE must expose one source-safe uniform-cell-inset rejection"
    );

    println!(
        "TABLE_MERGED_AUTOFORMAT_CONSUMER tables={} cells={} spanning={} painted={} borders={} inset_rejection={}",
        tables.len(),
        table.cells.len(),
        spanning,
        painted,
        table.border_segments.len(),
        inset_rejections[0],
    );
}
