use pub_model::Sha256Digest;
use pub_reader::{build_mature_0x2c_source_graph, has_exact_mature_quill_story_identity_v1};
use std::io::Cursor;

#[test]
#[ignore = "requires one pinned exact FDPP-positive PUB path"]
fn exact_fdpp_positive_story_replacement_retains_shared_provenance() {
    let path = std::env::var_os("CHAPTERA_FDPP_PROVENANCE_PUB")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_FDPP_PROVENANCE_PUB");
    let bytes = std::fs::read(path).expect("read exact FDPP-positive PUB");
    let source_hash = Sha256Digest::from_bytes([0x51; 32]);
    let built = build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash)
        .expect("build exact FDPP-positive mature source graph");

    let fdpp_stories = built
        .graph
        .stories
        .values()
        .filter(|story| {
            let paths = story
                .source_refs
                .iter()
                .filter_map(|reference| reference.path.as_deref())
                .collect::<Vec<_>>();
            paths.contains(&"Contents/0x65/textId")
                && paths.contains(&"FDPP/storyEnd")
                && paths.contains(&"TEXT")
        })
        .collect::<Vec<_>>();

    assert!(
        !fdpp_stories.is_empty(),
        "exact FDPP-positive witness must expose the current Reader replacement ref shape"
    );
    assert!(
        fdpp_stories
            .iter()
            .all(|story| has_exact_mature_quill_story_identity_v1(&built.graph.source, story)),
        "every exact FDPP-bounded Story replacement must satisfy shared mature-Quill identity"
    );
}
