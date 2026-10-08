use pub_viewer::{
    READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1, ReaderPartialSourceFact, ReaderPartialSourceGap,
    ReaderPartialSourceGraph, ReaderSalvageStreamState, ReaderSalvageSubsystemProbe,
    ViewerProductOpenOutcome,
};

#[test]
fn salvage_product_outcome_carries_partial_source_graph_not_probe() {
    let graph = ReaderPartialSourceGraph {
        schema_version: READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1.to_owned(),
        source_sha256: "a".repeat(64),
        contents_family: None,
        subsystems: ReaderSalvageSubsystemProbe {
            contents: ReaderSalvageStreamState::Readable,
            quill: ReaderSalvageStreamState::Absent,
            escher: ReaderSalvageStreamState::Absent,
            escher_delay: ReaderSalvageStreamState::Absent,
        },
        facts: Vec::<ReaderPartialSourceFact>::new(),
        recovered_resources: Vec::new(),
        gaps: vec![
            ReaderPartialSourceGap::TextUnavailable,
            ReaderPartialSourceGap::ImageFactsUnavailable,
            ReaderPartialSourceGap::GeometryFactsUnavailable,
        ],
    };
    let outcome = ViewerProductOpenOutcome::Salvage(graph.clone());
    assert_eq!(outcome, ViewerProductOpenOutcome::Salvage(graph));
}
