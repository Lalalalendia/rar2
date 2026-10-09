use super::*;

#[test]
fn source_typography_marks_table_story_ranges_as_unremapped() {
    let source_hash: Sha256Digest =
        "abababababababababababababababababababababababababababababababab"
            .parse()
            .expect("source hash");
    let story_id: StoryId =
        serde_json::from_str("\"55000000-0000-4000-8000-000000000001\"").expect("story id");
    let graph: PubResolvedGraph = pub_model::ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "rowcol-metadata-test".into(),
        source: pub_model::SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "pub-rs/test".into(),
            source_hash,
        },
        document: pub_model::Document {
            id: serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
                .expect("document id"),
            format_origin: "pub".into(),
            source_hash,
            pages: Vec::new(),
            resources: Vec::new(),
            styles: Vec::new(),
        },
        pages: BTreeMap::new(),
        nodes: BTreeMap::new(),
        stories: BTreeMap::from([(
            story_id,
            Story {
                id: story_id,
                text: "x".into(),
                paragraphs: Vec::new(),
                runs: Vec::new(),
                fields: Vec::new(),
                hyperlinks: Vec::new(),
                source_refs: Vec::new(),
            },
        )]),
        paragraphs: BTreeMap::new(),
        text_runs: BTreeMap::new(),
        resources: BTreeMap::new(),
        styles: BTreeMap::new(),
        extensions: BTreeMap::new(),
    };
    let mut session = EditorSession::new(graph).expect("session");
    assert!(!session.table_story_has_unremapped_range_metadata_v1(story_id));

    session.source_typography_runs.push(PubTypographyRun {
        story_id,
        story_utf16_start: 0,
        story_utf16_end: 1,
        story_scalar_start: 0,
        story_scalar_end: 1,
        source_font_index: 0,
        source_font_name: "Arial".into(),
        text_size_emu: 152_400,
        font_inherited: false,
        size_inherited: false,
        color_rgb: Some([0, 0, 0]),
        color_scheme_slot: None,
        color_inherited: false,
        bold: None,
        italic: None,
    });

    assert!(session.table_story_has_unremapped_range_metadata_v1(story_id));
}
