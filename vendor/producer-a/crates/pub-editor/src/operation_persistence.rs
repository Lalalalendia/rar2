//! Durable operation asset reachability and persistence requirements.
//!
//! Kept out of the pub-editor facade so new history operations do not grow the
//! central session monolith. The matches remain exhaustive over EditOperation.

use super::*;

impl EditOperation {
    /// Exact editor-owned asset identities required to retain this canonical
    /// operation in a durable EditorProject.
    ///
    /// Keep this match exhaustive: every future asset-bearing operation must
    /// make an explicit reachability decision here.
    pub fn durable_editor_asset_refs_v1(&self) -> Vec<Sha256Digest> {
        match self {
            Self::ReplaceImage {
                before_asset,
                after_asset,
                ..
            } => {
                let mut refs = Vec::with_capacity(2);
                if let Some(before_asset) = before_asset {
                    refs.push(*before_asset);
                }
                refs.push(*after_asset);
                refs.sort_unstable();
                refs.dedup();
                refs
            }
            Self::ReplaceStoryRange { .. }
            | Self::ReplaceStoryText { .. }
            | Self::BreakTextFrameForwardLink { .. }
            | Self::LinkTextFrameTail { .. }
            | Self::ReplaceTableCellText { .. }
            | Self::SetImageCrop { .. }
            | Self::MoveNode { .. }
            | Self::MoveNodes { .. }
            | Self::ResizeNode { .. }
            | Self::ResizeNodes { .. }
            | Self::CreateTextBox { .. }
            | Self::CreateShape { .. }
            | Self::CreateLine { .. }
            | Self::CreateTable { .. }
            | Self::SetTableTrackExtent { .. }
            | Self::InsertTableRow { .. }
            | Self::DeleteTableRow { .. }
            | Self::InsertTableColumn { .. }
            | Self::DeleteTableColumn { .. }
            | Self::DeleteNode { .. }
            | Self::ReorderAuthoredStack { .. }
            | Self::ReorderPagesV1 { .. }
            | Self::RegisterAuthoredPageIdentityV1 { .. }
            | Self::AppendBlankPageV1 { .. }
            | Self::DeleteBlankAuthoredPageV1 { .. }
            | Self::SetTextFormatProperty { .. }
            | Self::ClearTextFormatPropertyOverride { .. }
            | Self::SetTextFormatPropertyScopedV1 { .. }
            | Self::ClearTextFormatPropertyOverrideScopedV1 { .. }
            | Self::SetParagraphAlignmentOverride { .. }
            | Self::ClearParagraphAlignmentOverride { .. } => Vec::new(),
        }
    }
}

impl PersistenceRequirements for EditOperation {
    fn persistence_requirements(&self) -> Vec<PersistenceRequirement> {
        match self {
            Self::ReplaceStoryRange { story_id, .. } | Self::ReplaceStoryText { story_id, .. } => {
                vec![PersistenceRequirement {
                    feature: "story.text".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.text".into()),
                }]
            }
            Self::BreakTextFrameForwardLink {
                story_id,
                new_story_id,
                ..
            } => vec![
                PersistenceRequirement {
                    feature: "story.linked_frames".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.frames".into()),
                },
                PersistenceRequirement {
                    feature: "story.created_identity".into(),
                    origin: Some(new_story_id.into_canonical()),
                    property_path: Some("story".into()),
                },
            ],
            Self::LinkTextFrameTail { transition } => vec![
                PersistenceRequirement {
                    feature: "story.linked_frames".into(),
                    origin: Some(transition.story_id.into_canonical()),
                    property_path: Some("story.frames".into()),
                },
                PersistenceRequirement {
                    feature: "story.created_identity".into(),
                    origin: Some(transition.target_empty_story.id.into_canonical()),
                    property_path: Some("story.inverse_empty_target".into()),
                },
            ],
            Self::ReplaceTableCellText {
                story_id, cell_id, ..
            } => vec![
                PersistenceRequirement {
                    feature: "story.text".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.text".into()),
                },
                PersistenceRequirement {
                    feature: "table.cell_text".into(),
                    origin: Some(cell_id.into_canonical()),
                    property_path: Some("table.cell.text".into()),
                },
            ],
            Self::ReplaceImage { node_id, .. } => vec![PersistenceRequirement {
                feature: "image.replacement".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.image.resource".into()),
            }],
            Self::SetImageCrop { node_id, .. } => vec![PersistenceRequirement {
                feature: IMAGE_CONTENT_TRANSFORM_FEATURE.into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.image.crop".into()),
            }],
            Self::MoveNode { node_id, .. } => vec![PersistenceRequirement {
                feature: "node.geometry.position".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.bounds.position".into()),
            }],
            Self::MoveNodes { entries, .. } => entries
                .iter()
                .map(|entry| PersistenceRequirement {
                    feature: "node.geometry.position".into(),
                    origin: Some(entry.node_id.into_canonical()),
                    property_path: Some("node.bounds.position".into()),
                })
                .collect(),
            Self::ResizeNode { node_id, .. } => vec![PersistenceRequirement {
                feature: "node.geometry.bounds".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node.bounds".into()),
            }],
            Self::ResizeNodes { entries, .. } => entries
                .iter()
                .map(|entry| PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(entry.node_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                })
                .collect(),
            Self::CreateTextBox {
                node_id, story_id, ..
            } => vec![
                PersistenceRequirement {
                    feature: "node.created_identity".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node".into()),
                },
                PersistenceRequirement {
                    feature: "story.created_identity".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
                PersistenceRequirement {
                    feature: "story.text".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.text".into()),
                },
            ],
            Self::CreateShape { node_id, .. } => vec![
                PersistenceRequirement {
                    feature: "node.created_identity".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
                PersistenceRequirement {
                    feature: "shape.paint".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.paint".into()),
                },
            ],
            Self::CreateLine { node_id, .. } => vec![
                PersistenceRequirement {
                    feature: "node.created_identity".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node".into()),
                },
                PersistenceRequirement {
                    feature: "line.geometry.endpoints".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.line.geometry".into()),
                },
                PersistenceRequirement {
                    feature: "line.stroke".into(),
                    origin: Some(node_id.into_canonical()),
                    property_path: Some("node.line.stroke".into()),
                },
            ],
            Self::CreateTable { table } => vec![
                PersistenceRequirement {
                    feature: "node.created_identity".into(),
                    origin: Some(table.node_id.into_canonical()),
                    property_path: Some("node".into()),
                },
                PersistenceRequirement {
                    feature: "story.created_identity".into(),
                    origin: Some(table.story_id.into_canonical()),
                    property_path: Some("story".into()),
                },
                PersistenceRequirement {
                    feature: "table.grid".into(),
                    origin: Some(table.node_id.into_canonical()),
                    property_path: Some("table.grid".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(table.node_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
            ],
            Self::SetTableTrackExtent { history } => vec![
                PersistenceRequirement {
                    feature: "table.track_extent".into(),
                    origin: Some(history.table_id.into_canonical()),
                    property_path: Some("table.grid.track.extent".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(history.table_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
            ],
            Self::InsertTableRow { history }
            | Self::DeleteTableRow { history }
            | Self::InsertTableColumn { history }
            | Self::DeleteTableColumn { history } => vec![
                PersistenceRequirement {
                    feature: "table.structure".into(),
                    origin: Some(history.table_id.into_canonical()),
                    property_path: Some("table.grid".into()),
                },
                PersistenceRequirement {
                    feature: "story.text".into(),
                    origin: Some(history.story_id.into_canonical()),
                    property_path: Some("story.text".into()),
                },
                PersistenceRequirement {
                    feature: "node.geometry.bounds".into(),
                    origin: Some(history.table_id.into_canonical()),
                    property_path: Some("node.bounds".into()),
                },
            ],
            Self::DeleteNode { node_id, .. } => vec![PersistenceRequirement {
                feature: "node.deleted_identity".into(),
                origin: Some(node_id.into_canonical()),
                property_path: Some("node".into()),
            }],
            Self::ReorderAuthoredStack { transition } => vec![PersistenceRequirement {
                feature: "node.authored_stack_order".into(),
                origin: Some(transition.node_id.into_canonical()),
                property_path: Some("page.authored_stack".into()),
            }],
            Self::ReorderPagesV1 { transition } => vec![PersistenceRequirement {
                feature: "document.page_order".into(),
                origin: Some(transition.document_id.into_canonical()),
                property_path: Some("document.pages".into()),
            }],
            Self::RegisterAuthoredPageIdentityV1 { identity } => vec![PersistenceRequirement {
                feature: "page.created_identity".into(),
                origin: Some(identity.page_id.into_canonical()),
                property_path: Some("page.identity".into()),
            }],
            Self::AppendBlankPageV1 { transition } => {
                append_blank_page_persistence_requirements_v1(transition)
            }
            Self::DeleteBlankAuthoredPageV1 { transition } => vec![
                PersistenceRequirement {
                    feature: "page.created_identity".into(),
                    origin: Some(transition.identity.page_id.into_canonical()),
                    property_path: Some("page.identity".into()),
                },
                PersistenceRequirement {
                    feature: "document.page_membership".into(),
                    origin: Some(transition.document_id.into_canonical()),
                    property_path: Some("document.pages".into()),
                },
            ],
            Self::SetTextFormatProperty { story_id, .. }
            | Self::ClearTextFormatPropertyOverride { story_id, .. }
            | Self::SetTextFormatPropertyScopedV1 { story_id, .. }
            | Self::ClearTextFormatPropertyOverrideScopedV1 { story_id, .. } => {
                vec![PersistenceRequirement {
                    feature: "story.character_format_overlay".into(),
                    origin: Some(story_id.into_canonical()),
                    property_path: Some("story.character_format".into()),
                }]
            }
            Self::SetParagraphAlignmentOverride { paragraph_ids, .. }
            | Self::ClearParagraphAlignmentOverride { paragraph_ids, .. } => paragraph_ids
                .iter()
                .map(|paragraph_id| PersistenceRequirement {
                    feature: "story.paragraph_alignment".into(),
                    origin: Some(paragraph_id.into_canonical()),
                    property_path: Some("paragraph.alignment".into()),
                })
                .collect(),
        }
    }
}
