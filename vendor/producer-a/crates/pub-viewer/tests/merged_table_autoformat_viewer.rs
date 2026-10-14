use std::fs;
use std::path::PathBuf;

#[test]
#[ignore = "requires exact Carlton March PUB via CHAPTERA_TABLE_CELL_PAINT_FIXTURE"]
fn exact_carlton_merged_table_reaches_viewer_surface() {
    let fixture = PathBuf::from(
        std::env::var_os("CHAPTERA_TABLE_CELL_PAINT_FIXTURE")
            .expect("CHAPTERA_TABLE_CELL_PAINT_FIXTURE"),
    );
    let bytes = fs::read(&fixture).expect("read exact Carlton PUB");
    let geometry = pub_viewer::open_mature_0x2c_geometry(
        &bytes,
        pub_viewer::viewer_geometry_environment_v0_1(),
    )
    .expect("open exact Carlton through Viewer geometry boundary");

    assert_eq!(
        geometry.tables.len(),
        1,
        "exact Carlton must expose one semantic TABLE at the Viewer boundary",
    );
    let table = &geometry.tables[0];
    assert_eq!((table.rows, table.columns), (9, 2));
    assert_eq!(table.cells.len(), 17);

    let spanning = table
        .cells
        .iter()
        .filter(|cell| cell.row_span > 1 || cell.column_span > 1)
        .count();
    assert_eq!(spanning, 1);

    let painted = table
        .cells
        .iter()
        .filter(|cell| cell.fill_rgb.is_some() && cell.fill_visible.is_some())
        .count();
    assert_eq!(
        painted, 17,
        "all exact native AutoFormat cell carriers must reach Viewer paint state",
    );
    assert_eq!(
        table.borders.len(),
        20,
        "all bounded native border/decor carriers must reach Viewer geometry",
    );

    let ranged = table
        .cells
        .iter()
        .filter(|cell| cell.story_scalar_start.is_some() && cell.story_scalar_end.is_some())
        .count();
    let nonempty = table
        .cells
        .iter()
        .filter(|cell| !cell.text.is_empty())
        .count();
    let nonempty_ranged = table
        .cells
        .iter()
        .filter(|cell| !cell.text.is_empty())
        .filter(|cell| {
            matches!(
                (cell.story_scalar_start, cell.story_scalar_end),
                (Some(start), Some(end)) if start < end
            )
        })
        .count();
    assert_eq!(ranged, 17);
    assert_eq!(nonempty, 14);
    assert_eq!(nonempty_ranged, 14);

    println!(
        "VIEWER_MERGED_TABLE_AUTOFORMAT tables={} cells={} spanning={} painted={} borders={} ranged={} nonempty={} nonempty_ranged={}",
        geometry.tables.len(),
        table.cells.len(),
        spanning,
        painted,
        table.borders.len(),
        ranged,
        nonempty,
        nonempty_ranged,
    );
}
