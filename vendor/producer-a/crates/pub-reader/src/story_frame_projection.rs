use super::*;

pub(super) fn build_story_frame(
    source_hash: Sha256Digest,
    seq_num: u32,
    chunk: &Contents0x2cChunk,
    story_by_syid: &BTreeMap<u32, StoryId>,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Result<Option<PubStoryFrameSource>> {
    let Some((text_id, _)) = unique_u32_field(chunk, FIELD_STORY_ID)? else {
        return Ok(None);
    };

    let story_id = story_by_syid.get(&text_id).copied();
    if story_id.is_none() {
        diagnostics.push(PubBridgeDiagnostic::MissingQuillStory { seq_num, text_id });
    }

    let explicit_ordinal = unique_u32_field(chunk, FIELD_FRAME_ORDINAL)?.map(|(value, _)| value);
    let previous_seq = unique_u32_field(chunk, FIELD_PREVIOUS_FRAME)?.map(|(value, _)| value);
    let next_seq = unique_u32_field(chunk, FIELD_NEXT_FRAME)?.map(|(value, _)| value);

    let previous_frame = previous_seq
        .map(|target| derive_pub_node_id(&source_hash, target))
        .transpose()?;
    let next_frame = next_seq
        .map(|target| derive_pub_node_id(&source_hash, target))
        .transpose()?;

    Ok(Some(PubStoryFrameSource {
        text_id,
        story_id,
        explicit_ordinal,
        previous_seq_num: previous_seq,
        previous_frame,
        next_seq_num: next_seq,
        next_frame,
        vertical_alignment: None,
    }))
}

pub(super) fn unique_story_id_scalar(chunk: &Contents0x2cChunk) -> Result<Option<u32>> {
    let mut matches = chunk
        .fields
        .iter()
        .filter(|field| field.id == FIELD_STORY_ID);
    let Some(field) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        bail!("duplicate Contents field 0x{FIELD_STORY_ID:02X} in one chunk");
    }

    match &field.body {
        RawContentsBlockBody::U16 { value, .. } => Ok(Some(u32::from(*value))),
        RawContentsBlockBody::U32 { value, .. } => Ok(Some(*value)),
        _ => bail!(
            "Contents Story field 0x{FIELD_STORY_ID:02X} at {} is not a confirmed u16/u32 scalar body",
            field.source.offset
        ),
    }
}

pub(super) fn add_missing_link_target_diagnostics(
    graph: &PubSourceGraph,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) {
    let node_ids = graph.nodes.keys().copied().collect::<BTreeSet<_>>();

    for node in graph.nodes.values() {
        let Some(frame) = node.payload.story_frame.as_ref() else {
            continue;
        };
        for (target_seq_num, target) in [
            (frame.previous_seq_num, frame.previous_frame),
            (frame.next_seq_num, frame.next_frame),
        ] {
            let (Some(target_seq_num), Some(target)) = (target_seq_num, target) else {
                continue;
            };
            if node_ids.contains(&target) {
                continue;
            }

            diagnostics.push(PubBridgeDiagnostic::LinkedFrameNotMaterialized {
                seq_num: node.payload.contents_seq_num,
                target_seq_num,
            });
        }
    }
}

pub(super) fn project_story_frame_layout_metadata(
    graph: &PubSourceGraph,
    story_frame: &mut Option<PubStoryFrameSource>,
    story_layout_keys: &BTreeMap<u32, (u32, RawSpan)>,
    mcld: Option<&pub_quill::QuillMcldChunk>,
) -> Option<PubTextFrameInsetSource> {
    let text_frame_inset = story_frame.as_ref().and_then(|frame| {
        let (layout_record_id, layout_key_source) = story_layout_keys.get(&frame.text_id)?;
        let mcld = mcld?;
        let inset = bounded_mcld_text_insets(mcld, *layout_record_id).ok()?;
        let object_key = quill_story_object_key(frame.text_id);
        let mut source_refs = vec![source_ref(
            &graph.source,
            layout_key_source,
            Some(object_key.clone()),
            Some("Contents/0x65/layoutKey".into()),
            SourceRole::Relation,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        )];
        source_refs.extend(inset.sources.iter().map(|source| {
            source_ref(
                &graph.source,
                source,
                Some(object_key.clone()),
                Some("MCLD/06..09/text-inset".into()),
                SourceRole::Semantic,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            )
        }));
        Some(PubTextFrameInsetSource {
            layout_record_id: *layout_record_id,
            top_emu: inset.top_emu,
            left_emu: inset.left_emu,
            bottom_emu: inset.bottom_emu,
            right_emu: inset.right_emu,
            source_refs,
        })
    });

    let text_frame_vertical_alignment = story_frame.as_ref().and_then(|frame| {
        let (layout_record_id, layout_key_source) = story_layout_keys.get(&frame.text_id)?;
        let mcld = mcld?;
        let vertical =
            bounded_mcld_text_frame_vertical_alignment(mcld, *layout_record_id).ok()?;
        let object_key = quill_story_object_key(frame.text_id);
        Some(PubTextFrameVerticalAlignmentSource {
            layout_record_id: *layout_record_id,
            alignment: match vertical.alignment {
                QuillMcldVerticalAlignment::Top => PubTextFrameVerticalAlignment::Top,
                QuillMcldVerticalAlignment::Center => PubTextFrameVerticalAlignment::Center,
                QuillMcldVerticalAlignment::Bottom => PubTextFrameVerticalAlignment::Bottom,
            },
            source_refs: vec![
                source_ref(
                    &graph.source,
                    layout_key_source,
                    Some(object_key.clone()),
                    Some("Contents/0x65/layoutKey".into()),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &vertical.source,
                    Some(object_key),
                    Some("MCLD/18/text-vertical-align".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            ],
        })
    });

    if let (Some(frame), Some(vertical_alignment)) =
        (story_frame.as_mut(), text_frame_vertical_alignment)
    {
        frame.vertical_alignment = Some(vertical_alignment);
    }

    text_frame_inset
}

