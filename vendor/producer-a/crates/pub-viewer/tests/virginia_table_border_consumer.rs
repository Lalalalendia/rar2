use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, env, fs, path::PathBuf};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const EXPECTED_PAGES: &[(u32, usize)] = &[
    (6, 288),
    (7, 177),
    (9, 177),
    (10, 177),
    (11, 36),
    (21, 35),
    (22, 63),
    (23, 25),
];

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_native_table_border_consumer() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_FIXTURE"),
    );
    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer");

    let mut total_source = 0usize;
    let mut total_viewer = 0usize;

    for &(viewer_page, expected) in EXPECTED_PAGES {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page - 1) as usize)
            .expect("selected page exists");
        let parent = page.id.into_canonical();

        let table_node_ids = bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
            .map(|node| node.header.id)
            .collect::<BTreeSet<_>>();

        let source_count = bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| table_node_ids.contains(&node.header.id))
            .map(|node| {
                node.payload
                    .table
                    .as_ref()
                    .expect("selected TABLE")
                    .border_segments
                    .len()
            })
            .sum::<usize>();

        let viewer_count = bundle
            .geometry
            .tables
            .iter()
            .filter(|table| table_node_ids.contains(&table.node_id))
            .map(|table| {
                assert!(
                    table
                        .borders
                        .iter()
                        .all(|border| border.width_emu > 0
                            && (border.x1_emu != border.x2_emu
                                || border.y1_emu != border.y2_emu)),
                    "Viewer TABLE borders must remain positive non-degenerate source-backed segments"
                );
                table.borders.len()
            })
            .sum::<usize>();

        assert_eq!(
            source_count, expected,
            "page {viewer_page} source border segment count"
        );
        assert_eq!(
            viewer_count, expected,
            "page {viewer_page} Viewer border segment count"
        );
        total_source += source_count;
        total_viewer += viewer_count;
    }

    assert_eq!(total_source, 978);
    assert_eq!(total_viewer, 978);
}
