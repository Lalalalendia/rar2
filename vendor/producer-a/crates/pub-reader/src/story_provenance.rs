use crate::{CONTENTS_STREAM_PATH, QUILL_STREAM_PATH};
use pub_model::{
    AuthorityClass, ReadConfidence, SourceDescriptor, SourceRole, Story,
};
use std::collections::BTreeSet;

fn exact_story_keys_v1(
    source: &SourceDescriptor,
    story: &Story,
    carrier: &str,
    role: SourceRole,
    path: &str,
) -> BTreeSet<String> {
    story
        .source_refs
        .iter()
        .filter(|reference| {
            reference.validate_primary_source(source).is_ok()
                && reference.carrier == carrier
                && reference.path.as_deref() == Some(path)
                && reference.role == role
                && reference.authority == AuthorityClass::Authoritative
                && reference.confidence == Some(ReadConfidence::Exact)
                && reference
                    .object_key
                    .as_deref()
                    .is_some_and(|key| key.starts_with("quill/syid/"))
        })
        .filter_map(|reference| reference.object_key.clone())
        .collect()
}

/// Returns true only when one current mature-0x2C Story retains exact persisted
/// Quill identity + text authority on the same canonical Story object key.
///
/// Current Reader has two admitted source-ref shapes:
/// - direct Quill catalog: SYID + TEXT;
/// - FDPP-bounded Story replacement: Contents/0x65/textId + FDPP/storyEnd + TEXT.
///
/// The FDPP form deliberately requires the exact terminal Story boundary in
/// addition to the Contents identity key. This predicate never infers identity
/// from Story text content or a trailing U+000D.
pub fn has_exact_mature_quill_story_identity_v1(
    source: &SourceDescriptor,
    story: &Story,
) -> bool {
    if source.format != "pub"
        || source.format_version.as_deref() != Some("0x2c")
        || !source.adapter_version.starts_with("pub-rs/")
    {
        return false;
    }

    let text = exact_story_keys_v1(
        source,
        story,
        QUILL_STREAM_PATH,
        SourceRole::Semantic,
        "TEXT",
    );

    let syid = exact_story_keys_v1(
        source,
        story,
        QUILL_STREAM_PATH,
        SourceRole::Relation,
        "SYID",
    );
    if syid.iter().any(|key| text.contains(key)) {
        return true;
    }

    let contents_text_id = exact_story_keys_v1(
        source,
        story,
        CONTENTS_STREAM_PATH,
        SourceRole::Relation,
        "Contents/0x65/textId",
    );
    let fdpp_story_end = exact_story_keys_v1(
        source,
        story,
        QUILL_STREAM_PATH,
        SourceRole::Relation,
        "FDPP/storyEnd",
    );

    contents_text_id
        .iter()
        .any(|key| text.contains(key) && fdpp_story_end.contains(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::{CanonicalId, Sha256Digest, SourceRef, StoryId};

    fn source_hash() -> Sha256Digest {
        Sha256Digest::from_bytes([0x31; 32])
    }

    fn source() -> SourceDescriptor {
        SourceDescriptor {
            format: "pub".to_owned(),
            format_version: Some("0x2c".to_owned()),
            adapter_version: "pub-rs/test".to_owned(),
            source_hash: source_hash(),
        }
    }

    fn source_ref(
        carrier: &str,
        role: SourceRole,
        path: &str,
        object_key: &str,
    ) -> SourceRef {
        SourceRef {
            format: "pub".to_owned(),
            adapter_version: "pub-rs/test".to_owned(),
            source_hash: source_hash(),
            carrier: carrier.to_owned(),
            object_key: Some(object_key.to_owned()),
            path: Some(path.to_owned()),
            byte_range: None,
            role,
            authority: AuthorityClass::Authoritative,
            confidence: Some(ReadConfidence::Exact),
        }
    }

    fn story(source_refs: Vec<SourceRef>) -> Story {
        Story {
            id: StoryId::from_canonical(CanonicalId::from_bytes([0x42; 16])),
            text: "alpha\r".to_owned(),
            paragraphs: Vec::new(),
            runs: Vec::new(),
            fields: Vec::new(),
            hyperlinks: Vec::new(),
            source_refs,
        }
    }

    #[test]
    fn direct_quill_syid_and_text_on_one_key_are_exact_identity() {
        let proven_story = story(vec![
            source_ref(
                QUILL_STREAM_PATH,
                SourceRole::Relation,
                "SYID",
                "quill/syid/7",
            ),
            source_ref(
                QUILL_STREAM_PATH,
                SourceRole::Semantic,
                "TEXT",
                "quill/syid/7",
            ),
        ]);
        assert!(has_exact_mature_quill_story_identity_v1(
            &source(),
            &story
        ));
    }

    #[test]
    fn fdpp_bounded_story_requires_identity_boundary_and_text_on_one_key() {
        let story = story(vec![
            source_ref(
                CONTENTS_STREAM_PATH,
                SourceRole::Relation,
                "Contents/0x65/textId",
                "quill/syid/7",
            ),
            source_ref(
                QUILL_STREAM_PATH,
                SourceRole::Relation,
                "FDPP/storyEnd",
                "quill/syid/7",
            ),
            source_ref(
                QUILL_STREAM_PATH,
                SourceRole::Semantic,
                "TEXT",
                "quill/syid/7",
            ),
        ]);
        assert!(has_exact_mature_quill_story_identity_v1(
            &source(),
            &proven_story
        ));

        let missing_boundary = story(vec![
            source_ref(
                CONTENTS_STREAM_PATH,
                SourceRole::Relation,
                "Contents/0x65/textId",
                "quill/syid/7",
            ),
            source_ref(
                QUILL_STREAM_PATH,
                SourceRole::Semantic,
                "TEXT",
                "quill/syid/7",
            ),
        ]);
        assert!(!has_exact_mature_quill_story_identity_v1(
            &source(),
            &missing_boundary
        ));
    }

    #[test]
    fn mismatched_story_keys_fail_closed() {
        let story = story(vec![
            source_ref(
                CONTENTS_STREAM_PATH,
                SourceRole::Relation,
                "Contents/0x65/textId",
                "quill/syid/7",
            ),
            source_ref(
                QUILL_STREAM_PATH,
                SourceRole::Relation,
                "FDPP/storyEnd",
                "quill/syid/7",
            ),
            source_ref(
                QUILL_STREAM_PATH,
                SourceRole::Semantic,
                "TEXT",
                "quill/syid/8",
            ),
        ]);
        assert!(!has_exact_mature_quill_story_identity_v1(
            &source(),
            &story
        ));
    }
}
