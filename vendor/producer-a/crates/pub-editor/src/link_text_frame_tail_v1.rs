//! One reversible tail-to-empty-frame transaction. Frame ownership and Story
//! identity stay in the resolved graph; no second linked-text state is kept.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFrameLinkTransitionV1 {
    pub story_id: StoryId,
    pub source_frame_id: NodeId,
    pub target_frame_id: NodeId,
    pub target_empty_story: Story,
    pub source_story_state_id: String,
    pub before_frames: Vec<StoryFrame<StoryId, NodeId>>,
    pub after_frames: Vec<StoryFrame<StoryId, NodeId>>,
}

fn denied(source_frame_id: NodeId, target_frame_id: NodeId, reason: &str) -> EditorError {
    EditorError::LinkTextFrameUnsupported {
        source_frame_id,
        target_frame_id,
        reason: reason.to_owned(),
    }
}

fn placed_frame(graph: &PubResolvedGraph, frame_id: NodeId) -> bool {
    graph.nodes.get(&frame_id).is_some_and(|node| {
        node.kind == NodeKind::TextFrame
            && node.header.id == frame_id
            && node.header.transform == Affine2D::identity()
            && node.header.bounds.width.get() > 0
            && node.header.bounds.height.get() > 0
            && node.header.bounds.right().is_some()
            && node.header.bounds.bottom().is_some()
            && node.payload.table.is_none()
            && node.payload.table_story.is_none()
            && graph.pages.iter().any(|(page_id, page)| {
                graph
                    .document
                    .pages
                    .iter()
                    .filter(|id| *id == page_id)
                    .count()
                    == 1
                    && page.id == *page_id
                    && node.header.parent_id == page_id.into_canonical()
                    && page
                        .children
                        .iter()
                        .filter(|child| **child == frame_id)
                        .count()
                        == 1
            })
    })
}

/// Order follows reciprocal edges, never ordinal sorting or visual proximity.
pub(super) fn ordered_chain(
    graph: &PubResolvedGraph,
    story_id: StoryId,
) -> Option<Vec<StoryFrame<StoryId, NodeId>>> {
    let frames = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            let frame = frame_from_payload(*id, &node.payload)?;
            (frame.story_id == story_id).then_some(frame)
        })
        .collect::<Vec<_>>();
    if frames.is_empty()
        || !validate_story_frames(&frames).is_empty()
        || frames
            .iter()
            .any(|frame| !placed_frame(graph, frame.frame_id))
    {
        return None;
    }
    let heads = frames
        .iter()
        .filter(|frame| frame.previous.is_none())
        .collect::<Vec<_>>();
    if heads.len() != 1 {
        return None;
    }
    let by_id = frames
        .iter()
        .map(|frame| (frame.frame_id, frame))
        .collect::<BTreeMap<_, _>>();
    let mut ordered = Vec::with_capacity(frames.len());
    let mut seen = BTreeSet::new();
    let mut cursor = Some(heads[0].frame_id);
    while let Some(id) = cursor {
        if !seen.insert(id) {
            return None;
        }
        let frame = *by_id.get(&id)?;
        ordered.push(frame.clone());
        cursor = frame.next;
    }
    (ordered.len() == frames.len()).then_some(ordered)
}

impl EditorSession {
    pub fn can_link_text_frame_source_v1(
        &self,
        source_frame_id: NodeId,
    ) -> Result<StoryId, EditorError> {
        self.validate_source_identity()?;
        let fail = |reason| denied(source_frame_id, source_frame_id, reason);
        let source = frame_snapshot(&self.graph, source_frame_id)
            .ok_or_else(|| fail("Select a placed ordinary text frame."))?;
        if source.next.is_some() {
            return Err(fail("Only the last frame of a text chain can continue it."));
        }
        self.can_replace_story_text(source.story_id)?;
        let chain = ordered_chain(&self.graph, source.story_id)
            .ok_or_else(|| fail("The Story has no single proven explicit flow chain."))?;
        if chain.last().map(|frame| frame.frame_id) != Some(source_frame_id) {
            return Err(fail("The selected frame is not the chain tail."));
        }
        if chain.len() == 1
            && self
                .prove_author_created_story_v1(source.story_id)
                .map_err(|_| fail("The standalone imported frame has no proven linkability."))?
                .is_none()
        {
            return Err(fail(
                "Only an existing explicit chain or a Chaptera-created text box can be continued.",
            ));
        }
        if source.ordinal.checked_add(1).is_none() {
            return Err(fail("The text chain ordinal cannot be extended."));
        }
        Ok(source.story_id)
    }

    pub fn can_link_text_frame_tail(
        &self,
        source_frame_id: NodeId,
        target_frame_id: NodeId,
    ) -> Result<(), EditorError> {
        self.plan_text_frame_link_v1(source_frame_id, target_frame_id)
            .map(|_| ())
    }

    fn plan_text_frame_link_v1(
        &self,
        source_frame_id: NodeId,
        target_frame_id: NodeId,
    ) -> Result<TextFrameLinkTransitionV1, EditorError> {
        let fail = |reason| denied(source_frame_id, target_frame_id, reason);
        if source_frame_id == target_frame_id {
            return Err(fail("A text box cannot link to itself."));
        }
        let story_id = self.can_link_text_frame_source_v1(source_frame_id)?;
        let target = frame_snapshot(&self.graph, target_frame_id)
            .ok_or_else(|| fail("The target is not a text box in this document."))?;
        if target.story_id == story_id || target.previous.is_some() || target.next.is_some() {
            return Err(fail("The target already belongs to a text chain."));
        }
        let proof = self
            .prove_author_created_story_v1(target.story_id)
            .map_err(|_| {
                fail("The target must be an ordinary standalone Chaptera-created text box.")
            })?
            .ok_or_else(|| fail("Imported text boxes are not admitted as link targets."))?;
        if proof.frame_id != target_frame_id || !placed_frame(&self.graph, target_frame_id) {
            return Err(fail("The target has no unique direct page placement."));
        }
        let target_empty_story = self.graph.stories[&target.story_id].clone();
        if target_empty_story != empty_editor_story(target.story_id) {
            return Err(fail("The target text box must be empty."));
        }
        let mut before_frames = ordered_chain(&self.graph, story_id)
            .ok_or_else(|| fail("The source chain changed."))?;
        let mut after_frames = before_frames.clone();
        let source = after_frames.last_mut().expect("source chain is nonempty");
        source.next = Some(target_frame_id);
        let ordinal = source
            .ordinal
            .checked_add(1)
            .ok_or_else(|| fail("The chain is too long."))?;
        after_frames.push(StoryFrame {
            story_id,
            frame_id: target_frame_id,
            ordinal,
            previous: Some(source_frame_id),
            next: None,
        });
        before_frames.push(target);
        if !validate_story_frames(&after_frames).is_empty() {
            return Err(fail("The resulting text chain is inconsistent."));
        }
        Ok(TextFrameLinkTransitionV1 {
            story_id,
            source_frame_id,
            target_frame_id,
            target_empty_story,
            source_story_state_id: story_state_id_v1(story_id, &self.graph.stories[&story_id].text),
            before_frames,
            after_frames,
        })
    }

    pub fn link_text_frame_tail(
        &mut self,
        source_frame_id: NodeId,
        target_frame_id: NodeId,
    ) -> Result<EditOperation, EditorError> {
        let transition = self.plan_text_frame_link_v1(source_frame_id, target_frame_id)?;
        let operation = EditOperation::LinkTextFrameTail { transition };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        Ok(operation)
    }

    // Extends the existing created-Story provenance proof only for a chain
    // composed of its original CreateTextBox and applied canonical tail links.
    pub(super) fn prove_author_created_linked_story_v1(
        &self,
        story_id: StoryId,
        head_id: NodeId,
        page_id: PageId,
        preset: &AuthoringTextPresetV1,
    ) -> Result<Option<ProvenAuthorCreatedStoryV1>, AuthorCreatedStoryProofError> {
        let fail = || {
            AuthorCreatedStoryProofError::unproven(
                "current created Story chain does not match its applied CreateTextBox/LinkTextFrameTail history",
            )
        };
        let chain = ordered_chain(&self.graph, story_id).ok_or_else(fail)?;
        let mut expected = vec![head_id];
        for operation in &self.undo {
            match operation {
                EditOperation::LinkTextFrameTail { transition }
                    if transition.story_id == story_id =>
                {
                    if expected.last() != Some(&transition.source_frame_id)
                        || expected.contains(&transition.target_frame_id)
                        || self
                            .graph
                            .stories
                            .contains_key(&transition.target_empty_story.id)
                    {
                        return Err(fail());
                    }
                    expected.push(transition.target_frame_id);
                }
                EditOperation::BreakTextFrameForwardLink {
                    story_id: changed, ..
                } if *changed == story_id => return Err(fail()),
                _ => {}
            }
        }
        if chain.iter().map(|frame| frame.frame_id).collect::<Vec<_>>() != expected {
            return Err(fail());
        }
        for frame in &chain {
            let claims = self
                .undo
                .iter()
                .filter_map(|operation| match operation {
                    EditOperation::CreateTextBox {
                        node_id, page_id, ..
                    } if *node_id == frame.frame_id => Some(*page_id),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let node = &self.graph.nodes[&frame.frame_id];
            if claims.len() != 1
                || node.header.parent_id != claims[0].into_canonical()
                || !node.header.source_refs.is_empty()
                || node.payload.contents_seq_num != 0
            {
                return Err(fail());
            }
        }
        Ok(Some(ProvenAuthorCreatedStoryV1 {
            story_id,
            frame_id: head_id,
            page_id,
            text_preset: preset.clone(),
        }))
    }
}

fn exact_owner_frames(
    graph: &PubResolvedGraph,
    story_id: StoryId,
    snapshots: &[StoryFrame<StoryId, NodeId>],
) -> bool {
    let current = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            let frame = frame_from_payload(*id, &node.payload)?;
            (frame.story_id == story_id).then_some(frame.frame_id)
        })
        .collect::<BTreeSet<_>>();
    let expected = snapshots
        .iter()
        .filter(|frame| frame.story_id == story_id)
        .map(|frame| frame.frame_id)
        .collect::<BTreeSet<_>>();
    current == expected
}

pub(super) fn apply_transition(
    graph: &mut PubResolvedGraph,
    transition: &TextFrameLinkTransitionV1,
    forward: bool,
) -> Result<(), EditorError> {
    let stale = || EditorError::StaleFrameTopology {
        story_id: transition.story_id,
    };
    let before = if forward {
        &transition.before_frames
    } else {
        &transition.after_frames
    };
    let after = if forward {
        &transition.after_frames
    } else {
        &transition.before_frames
    };
    let target_story_id = transition.target_empty_story.id;
    if graph.stories.get(&transition.story_id).is_none_or(|story| {
        story_state_id_v1(transition.story_id, &story.text) != transition.source_story_state_id
    }) || !frames_match_snapshots(graph, before)
        || !exact_owner_frames(graph, transition.story_id, before)
        || !exact_owner_frames(graph, target_story_id, before)
        || (forward && graph.stories.get(&target_story_id) != Some(&transition.target_empty_story))
        || (!forward && graph.stories.contains_key(&target_story_id))
        || !validate_story_frames(after).is_empty()
    {
        return Err(stale());
    }
    // All fallible checks precede the first graph write.
    if forward {
        graph.stories.remove(&target_story_id);
    } else {
        graph
            .stories
            .insert(target_story_id, transition.target_empty_story.clone());
    }
    for frame in after {
        set_story_frame_snapshot(graph, frame)
            .expect("all target frames were checked before mutation");
    }
    Ok(())
}
