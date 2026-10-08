//! Image-specific EditorSession orchestration.
//!
//! This module owns image replacement/crop session state and its overlay
//! transitions. Project persistence and editable-export assembly remain in the
//! root facade because they consume multiple editor domains.

use super::*;
use pub_editor_image_core::{
    apply_image_crop_forward_v1, apply_image_crop_inverse_v1,
    apply_image_replacement_forward_v1, apply_image_replacement_inverse_v1,
    effective_image_crop_state_v1,
};

impl EditorSession {
    pub fn source_image_count(&self) -> usize {
        self.source_image_nodes
            .keys()
            .filter(|node_id| !self.image_replacements.contains_key(node_id))
            .count()
    }

    pub fn current_image_resources_v1(
        &self,
    ) -> Result<Vec<EditorCurrentImageResourceV1>, EditorCurrentImageResourceError> {
        current_image_resources_v1(
            &self.source_image_assets,
            &self.source_image_nodes,
            &self.replacement_assets,
            &self.image_replacements,
        )
    }

    pub(super) fn externally_projected_image_nodes(&self) -> BTreeSet<NodeId> {
        self.image_replacements
            .keys()
            .chain(self.source_image_nodes.keys())
            .copied()
            .collect()
    }

    pub fn replacement_assets(
        &self,
    ) -> impl ExactSizeIterator<Item = &EditorReplacementAsset> + DoubleEndedIterator {
        self.replacement_assets.values()
    }

    pub fn image_replacement_for(&self, node_id: NodeId) -> Option<Sha256Digest> {
        self.image_replacements.get(&node_id).copied()
    }

    pub fn image_crop_for(&self, node_id: NodeId) -> Option<ImageCropStateV1> {
        effective_image_crop_state(&self.graph, &self.image_crop_overrides, node_id)
    }

    pub fn import_replacement_asset(
        &mut self,
        mime: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<Sha256Digest, EditorAssetError> {
        let mime = mime.into();
        let asset = validated_editor_asset(mime, bytes)?;

        if let Some(existing) = self.replacement_assets.get(&asset.sha256) {
            if existing.mime != asset.mime {
                return Err(EditorAssetError::MimeConflict {
                    sha256: asset.sha256,
                    existing: existing.mime.clone(),
                    requested: asset.mime,
                });
            }
            return Ok(asset.sha256);
        }

        let sha256 = asset.sha256;
        self.replacement_assets.insert(sha256, asset);
        Ok(sha256)
    }

    pub fn can_replace_image(
        &self,
        node_id: NodeId,
        replacement_asset: Sha256Digest,
    ) -> Result<(), EditorError> {
        self.validate_source_identity()?;

        if !self.replacement_assets.contains_key(&replacement_asset) {
            return Err(EditorError::MissingReplacementAsset {
                sha256: replacement_asset,
            });
        }

        let node = self
            .graph
            .nodes
            .get(&node_id)
            .ok_or(EditorError::ImageReplaceUnsupported { node_id })?;
        if node.payload.image_slot.is_none() {
            return Err(EditorError::ImageReplaceUnsupported { node_id });
        }
        if let Some(crop) = node.payload.explicit_image_crop.as_ref() {
            let resource_id = self
                .source_image_nodes
                .get(&node_id)
                .ok_or(EditorError::ImageReplaceUnsupported { node_id })?;
            let source_asset = self
                .source_image_assets
                .get(resource_id)
                .ok_or(EditorError::ImageReplaceUnsupported { node_id })?;
            if crop.ambiguous
                || !matches!(source_asset.mime.as_str(), "image/png" | "image/jpeg")
                || node.header.transform != pub_model::Affine2D::identity()
                || !self
                    .graph
                    .document
                    .pages
                    .iter()
                    .any(|page_id| page_id.into_canonical() == node.header.parent_id)
            {
                return Err(EditorError::ImageReplaceUnsupported { node_id });
            }
        }
        if node.header.bounds.width.get() <= 0
            || node.header.bounds.height.get() <= 0
            || node.header.bounds.right().is_none()
            || node.header.bounds.bottom().is_none()
        {
            return Err(EditorError::ImageReplaceUnsupported { node_id });
        }
        if !self
            .graph
            .pages
            .keys()
            .any(|page_id| page_id.into_canonical() == node.header.parent_id)
        {
            return Err(EditorError::ImageReplaceUnsupported { node_id });
        }

        Ok(())
    }

    pub fn can_set_image_crop(&self, node_id: NodeId) -> Result<(), EditorError> {
        self.validate_source_identity()?;

        let node = self
            .graph
            .nodes
            .get(&node_id)
            .ok_or(EditorError::ImageCropUnsupported { node_id })?;
        let crop = node
            .payload
            .explicit_image_crop
            .as_ref()
            .ok_or(EditorError::ImageCropUnsupported { node_id })?;
        let resource_id = self
            .source_image_nodes
            .get(&node_id)
            .ok_or(EditorError::ImageCropUnsupported { node_id })?;
        let source_asset = self
            .source_image_assets
            .get(resource_id)
            .ok_or(EditorError::ImageCropUnsupported { node_id })?;

        if self.project_identity.is_none()
            || node.payload.image_slot.is_none()
            || crop.ambiguous
            || !matches!(source_asset.mime.as_str(), "image/png" | "image/jpeg")
            || node.header.transform != pub_model::Affine2D::identity()
            || node.header.bounds.width.get() <= 0
            || node.header.bounds.height.get() <= 0
            || node.header.bounds.right().is_none()
            || node.header.bounds.bottom().is_none()
            || !self
                .graph
                .document
                .pages
                .iter()
                .any(|page_id| page_id.into_canonical() == node.header.parent_id)
        {
            return Err(EditorError::ImageCropUnsupported { node_id });
        }

        Ok(())
    }

    pub fn set_image_crop(
        &mut self,
        node_id: NodeId,
        expected_before: ImageCropStateV1,
        after: ImageCropStateV1,
    ) -> Result<EditOperation, EditorError> {
        self.can_set_image_crop(node_id)?;
        let before = self
            .image_crop_for(node_id)
            .ok_or(EditorError::ImageCropUnsupported { node_id })?;
        if before != expected_before {
            return Err(EditorError::StaleImageCrop { node_id });
        }
        if before == after {
            return Err(EditorError::ImageCropNoChange { node_id });
        }

        let operation = EditOperation::SetImageCrop {
            node_id,
            before,
            after,
        };
        apply_crop_forward(&self.graph, &mut self.image_crop_overrides, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn replace_image(
        &mut self,
        node_id: NodeId,
        replacement_asset: Sha256Digest,
    ) -> Result<EditOperation, EditorError> {
        self.can_replace_image(node_id, replacement_asset)?;

        let before_asset = self.image_replacement_for(node_id);
        if before_asset == Some(replacement_asset) {
            return Err(EditorError::ImageReplacementNoChange {
                node_id,
                sha256: replacement_asset,
            });
        }

        let operation = EditOperation::ReplaceImage {
            node_id,
            before_asset,
            after_asset: replacement_asset,
        };
        apply_image_forward(&mut self.image_replacements, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }
}

pub(super) fn apply_image_forward(
    replacements: &mut BTreeMap<NodeId, Sha256Digest>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::ReplaceImage {
        node_id,
        before_asset,
        after_asset,
    } = operation
    else {
        unreachable!("only ReplaceImage reaches image overlay apply")
    };

    apply_image_replacement_forward_v1(replacements, *node_id, *before_asset, *after_asset)
        .map_err(|_| EditorError::StaleImageOperation { node_id: *node_id })
}

pub(super) fn apply_image_inverse(
    replacements: &mut BTreeMap<NodeId, Sha256Digest>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::ReplaceImage {
        node_id,
        before_asset,
        after_asset,
    } = operation
    else {
        unreachable!("only ReplaceImage reaches image overlay inverse")
    };

    apply_image_replacement_inverse_v1(replacements, *node_id, *before_asset, *after_asset)
        .map_err(|_| EditorError::StaleImageOperation { node_id: *node_id })
}

fn source_image_crop_state(graph: &PubResolvedGraph, node_id: NodeId) -> Option<ImageCropStateV1> {
    let crop = graph
        .nodes
        .get(&node_id)?
        .payload
        .explicit_image_crop
        .as_ref()?;
    if crop.ambiguous {
        return None;
    }
    Some(ImageCropStateV1 {
        top_raw: crop.top_raw,
        bottom_raw: crop.bottom_raw,
        left_raw: crop.left_raw,
        right_raw: crop.right_raw,
    })
}

fn effective_image_crop_state(
    graph: &PubResolvedGraph,
    overrides: &BTreeMap<NodeId, ImageCropStateV1>,
    node_id: NodeId,
) -> Option<ImageCropStateV1> {
    effective_image_crop_state_v1(source_image_crop_state(graph, node_id), overrides, node_id)
}

pub(super) fn apply_crop_forward(
    graph: &PubResolvedGraph,
    overrides: &mut BTreeMap<NodeId, ImageCropStateV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::SetImageCrop {
        node_id,
        before,
        after,
    } = operation
    else {
        unreachable!("only SetImageCrop reaches crop overlay apply")
    };

    apply_image_crop_forward_v1(
        source_image_crop_state(graph, *node_id),
        overrides,
        *node_id,
        *before,
        *after,
    )
    .map_err(|_| EditorError::StaleImageCrop { node_id: *node_id })
}

pub(super) fn apply_crop_inverse(
    graph: &PubResolvedGraph,
    overrides: &mut BTreeMap<NodeId, ImageCropStateV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::SetImageCrop {
        node_id,
        before,
        after,
    } = operation
    else {
        unreachable!("only SetImageCrop reaches crop overlay inverse")
    };

    apply_image_crop_inverse_v1(
        source_image_crop_state(graph, *node_id),
        overrides,
        *node_id,
        *before,
        *after,
    )
    .map_err(|_| EditorError::StaleImageCrop { node_id: *node_id })
}

#[cfg(test)]
mod image_crop_authoring_tests {
    use super::*;

    fn id<T: serde::de::DeserializeOwned>(value: &str) -> T {
        serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
    }

    fn source_hash() -> Sha256Digest {
        "1111111111111111111111111111111111111111111111111111111111111111"
            .parse()
            .expect("source hash")
    }

    fn crop_graph() -> (PubResolvedGraph, NodeId, ResourceId) {
        let page_id: PageId = id("10000000-0000-4000-8000-000000000001");
        let node_id: NodeId = id("20000000-0000-4000-8000-000000000001");
        let resource_id: ResourceId = id("40000000-0000-4000-8000-000000000001");
        let hash = source_hash();
        let bounds = RectEmu::new(
            LengthEmu::new(100_000),
            LengthEmu::new(200_000),
            LengthEmu::new(300_000),
            LengthEmu::new(400_000),
        );

        let mut pages = BTreeMap::new();
        pages.insert(
            page_id,
            pub_model::Page {
                id: page_id,
                size: pub_model::Size2D::new(LengthEmu::new(5_000_000), LengthEmu::new(5_000_000)),
                bleed: None,
                margins: None,
                children: vec![node_id],
                extensions: Vec::new(),
            },
        );

        let mut nodes = BTreeMap::new();
        nodes.insert(
            node_id,
            pub_model::Node {
                kind: pub_model::NodeKind::Shape,
                header: pub_model::NodeHeader {
                    id: node_id,
                    parent_id: page_id.into_canonical(),
                    bounds,
                    transform: pub_model::Affine2D::identity(),
                    source_refs: Vec::new(),
                    extensions: Vec::new(),
                },
                payload: PubResolvedNodePayload {
                    contents_seq_num: 1,
                    officeart_shape_type: Some(75),
                    officeart_spid: Some(1),
                    image_slot: Some(1),
                    legacy_ole: None,
                    explicit_image_crop: Some(pub_reader::PubExplicitImageCropSource {
                        top_raw: Some(10),
                        bottom_raw: Some(20),
                        left_raw: Some(30),
                        right_raw: Some(40),
                        ambiguous: false,
                    }),
                    explicit_image_cardinal_rotation_degrees: None,
                    explicit_paint: pub_reader::PubExplicitShapePaintSource::default(),
                    effective_paint: None,
                    story_frame: None,
                    text_frame_inset: None,
                    table_story: None,
                    table: None,
                },
            },
        );

        (
            pub_model::ResolvedGraph {
                cdm_version: "0.1".into(),
                resolver_version: "image-crop-authoring-test".into(),
                source: pub_model::SourceDescriptor {
                    format: "pub".into(),
                    format_version: Some("0x2c".into()),
                    adapter_version: "pub-rs/test".into(),
                    source_hash: hash,
                },
                document: pub_model::Document {
                    id: id::<pub_model::DocumentId>("30000000-0000-4000-8000-000000000001"),
                    format_origin: "pub".into(),
                    source_hash: hash,
                    pages: vec![page_id],
                    resources: Vec::new(),
                    styles: Vec::new(),
                },
                pages,
                nodes,
                stories: BTreeMap::new(),
                paragraphs: BTreeMap::new(),
                text_runs: BTreeMap::new(),
                resources: BTreeMap::new(),
                styles: BTreeMap::new(),
                extensions: BTreeMap::new(),
            },
            node_id,
            resource_id,
        )
    }

    fn install_source_png(session: &mut EditorSession, node_id: NodeId, resource_id: ResourceId) {
        session.source_image_nodes.insert(node_id, resource_id);
        session.source_image_assets.insert(
            resource_id,
            EditorSourceImageAsset {
                mime: "image/png".into(),
                bytes: b"source-image-authority".to_vec(),
            },
        );
    }

    #[test]
    fn crop_is_v0_19_overlay_with_exact_history_replay_and_independent_axes() {
        let (graph, node_id, resource_id) = crop_graph();
        let source_bounds = graph.nodes[&node_id].header.bounds;
        let mut session = EditorSession::new(graph).expect("session");
        install_source_png(&mut session, node_id, resource_id);

        let before = session.image_crop_for(node_id).expect("source crop");
        let after = ImageCropStateV1 {
            top_raw: Some(11),
            bottom_raw: Some(22),
            left_raw: Some(33),
            right_raw: Some(44),
        };

        let operation = session
            .set_image_crop(node_id, before, after)
            .expect("set crop");
        assert!(matches!(operation, EditOperation::SetImageCrop { .. }));
        assert_eq!(session.image_crop_for(node_id), Some(after));
        assert_eq!(session.graph.nodes[&node_id].header.bounds, source_bounds);
        assert_eq!(
            session.project().schema_version,
            EDITOR_PROJECT_VERSION_V0_19
        );

        assert!(matches!(
            session.set_image_crop(node_id, before, after),
            Err(EditorError::StaleImageCrop { .. })
        ));
        assert!(matches!(
            session.set_image_crop(node_id, after, after),
            Err(EditorError::ImageCropNoChange { .. })
        ));

        session.undo().expect("crop undo");
        assert_eq!(session.image_crop_for(node_id), Some(before));
        session.redo().expect("crop redo");
        assert_eq!(session.image_crop_for(node_id), Some(after));

        session
            .move_node_to(node_id, LengthEmu::new(120_000), LengthEmu::new(230_000))
            .expect("move preserves crop");
        assert_eq!(session.image_crop_for(node_id), Some(after));

        let resized = RectEmu::new(
            LengthEmu::new(120_000),
            LengthEmu::new(230_000),
            LengthEmu::new(350_000),
            LengthEmu::new(450_000),
        );
        session
            .resize_node_to(node_id, resized)
            .expect("resize preserves crop");
        assert_eq!(session.image_crop_for(node_id), Some(after));

        let project = session.project();
        assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_19);
        let (replay_graph, replay_node_id, replay_resource_id) = crop_graph();
        assert_eq!(replay_node_id, node_id);
        let mut replay = EditorSession::new(replay_graph).expect("replay session");
        install_source_png(&mut replay, replay_node_id, replay_resource_id);
        replay.apply_project(&project).expect("v0.19 crop replay");
        assert_eq!(replay.image_crop_for(node_id), Some(after));
        assert_eq!(replay.graph.nodes[&node_id].header.bounds, resized);
        assert_eq!(replay.project(), project);

        for target in [EditorEditableTarget::Idml, EditorEditableTarget::Odg] {
            let preview = session
                .preview_editable_export(target, "crop-test")
                .expect("crop export preview");
            assert!(
                !preview.report.can_serialize,
                "crop override must fail closed until {target} preserves content transform"
            );
        }

        let replacement = session
            .import_replacement_asset("image/png", b"\x89PNG\r\n\x1a\nreplacement".to_vec())
            .expect("replacement asset");
        session
            .replace_image(node_id, replacement)
            .expect("safe cropped replacement");
        assert_eq!(session.image_crop_for(node_id), Some(after));
        assert_eq!(session.graph.nodes[&node_id].header.bounds, resized);
    }

    #[test]
    fn cropped_picture_requires_exact_png_or_jpeg_source_authority() {
        let (graph, node_id, resource_id) = crop_graph();
        let mut session = EditorSession::new(graph).expect("session");
        let before = session.image_crop_for(node_id).expect("source crop");
        let after = ImageCropStateV1 {
            top_raw: Some(1),
            ..before
        };

        assert!(matches!(
            session.set_image_crop(node_id, before, after),
            Err(EditorError::ImageCropUnsupported { .. })
        ));

        session.source_image_nodes.insert(node_id, resource_id);
        session.source_image_assets.insert(
            resource_id,
            EditorSourceImageAsset {
                mime: "image/x-ms-bmp-dib".into(),
                bytes: b"bmp".to_vec(),
            },
        );
        assert!(matches!(
            session.set_image_crop(node_id, before, after),
            Err(EditorError::ImageCropUnsupported { .. })
        ));
    }
}
