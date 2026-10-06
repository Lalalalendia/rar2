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
#[ignore = "requires exact public Carlton March PUB path"]
fn exact_carlton_table_story_layout_key_profile_is_source_safe() {
    let fixture = PathBuf::from(
        std::env::var_os("CHAPTERA_GOLDEN_CARLTON_MARCH")
            .expect("CHAPTERA_GOLDEN_CARLTON_MARCH"),
    );
    let bytes = fs::read(fixture).expect("read exact Carlton PUB");
    let build = pub_reader::build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash(&bytes),
    )
    .expect("build exact Carlton graph");

    let tables = build
        .graph
        .nodes
        .values()
        .filter_map(|node| node.payload.table.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(tables.len(), 1, "exact Carlton must expose one TABLE");

    let table = tables[0];
    let layout_key = table
        .layout_metrics
        .as_ref()
        .map(|metrics| metrics.story_layout_key);

    eprintln!(
        "CARLTON_TABLE_MCLD_JOIN text_id={} cells={} layout_key={:?}",
        table.text_id,
        table.cells.len(),
        layout_key,
    );

    assert_eq!(table.cells.len(), 17);
    assert!(layout_key.is_some(), "Carlton TABLE must retain its source-backed Story layout key");
}
