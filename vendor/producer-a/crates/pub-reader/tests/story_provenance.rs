use pub_model::Sha256Digest;
use pub_reader::{
    build_mature_0x2c_source_graph, has_exact_mature_quill_story_identity_v1,
};
use std::io::Cursor;

fn decode_base64(input: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0_u32;
    let mut bits = 0_u8;

    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            byte if byte.is_ascii_whitespace() => continue,
            other => panic!("unexpected base64 byte: {other:#04x}"),
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= if bits == 0 { 0 } else { (1_u32 << bits) - 1 };
        }
    }

    output
}

#[test]
fn pinned_sample_fdpp_story_replacement_retains_exact_shared_provenance() {
    let bytes = decode_base64(include_str!("fixtures/Sample.pub.b64"));
    let source_hash = Sha256Digest::from_bytes([0x51; 32]);
    let built = build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash)
        .expect("build pinned Sample.pub mature source graph");

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
        "pinned Sample.pub must exercise the FDPP-bounded Story replacement ref shape"
    );
    assert!(
        fdpp_stories
            .iter()
            .all(|story| has_exact_mature_quill_story_identity_v1(&built.graph.source, story)),
        "every exact FDPP-bounded Story replacement must satisfy shared mature-Quill identity"
    );
}
