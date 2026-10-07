use super::*;

// CI routing control: no semantic change.
pub(super) fn materialize_story_catalogs(
    source_hash: &Sha256Digest,
    quill_catalog: Option<&pub_quill::QuillStoryCatalog>,
    fdpp_story_catalog: Option<&pub_quill::QuillFdppStoryCatalog>,
    graph: &mut PubSourceGraph,
) -> Result<BTreeMap<u32, StoryId>> {
    let mut story_by_syid = BTreeMap::new();

    if let Some(quill_catalog) = quill_catalog {
        for story_slice in &quill_catalog.stories {
            let syid = story_slice.syid.0;
            let story_id = derive_pub_story_id(source_hash, syid)?;
            let object_key = quill_story_object_key(syid);
            let text = decode_utf16le_strict(&story_slice.utf16le)
                .with_context(|| format!("decode Quill story SYID {syid} as strict UTF-16LE"))?;

            let source_refs = vec![
                source_ref(
                    &graph.source,
                    &story_slice.syid_source,
                    Some(object_key.clone()),
                    Some("SYID".into()),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &story_slice.text_source,
                    Some(object_key),
                    Some("TEXT".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            ];

            graph.stories.insert(
                story_id,
                Story {
                    id: story_id,
                    text,
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs,
                },
            );
            story_by_syid.insert(syid, story_id);
        }
    }

    if let Some(fdpp_catalog) = fdpp_story_catalog {
        for story_slice in &fdpp_catalog.stories {
            let syid = story_slice.syid.0;
            let story_id = derive_pub_story_id(source_hash, syid)?;
            let object_key = quill_story_object_key(syid);
            let text = decode_utf16le_strict(&story_slice.utf16le)
                .with_context(|| format!("decode FDPP-bounded Story {syid} as strict UTF-16LE"))?;

            let source_refs = vec![
                source_ref(
                    &graph.source,
                    &story_slice.identity_source,
                    Some(object_key.clone()),
                    Some("Contents/0x65/textId".into()),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &story_slice.boundary_source,
                    Some(object_key.clone()),
                    Some("FDPP/storyEnd".into()),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &story_slice.text_source,
                    Some(object_key),
                    Some("TEXT".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            ];

            graph.stories.insert(
                story_id,
                Story {
                    id: story_id,
                    text,
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs,
                },
            );
            story_by_syid.insert(syid, story_id);
        }
    }

    Ok(story_by_syid)
}
