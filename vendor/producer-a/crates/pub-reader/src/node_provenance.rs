//! Canonical provenance assembly for already-admitted mature Reader nodes.
//!
//! Product geometry, paint, Story/TABLE semantics and transform admission are
//! resolved by their owning modules before this helper is called. This module
//! only attaches exact source references to the resulting canonical node.

use super::*;

pub(super) struct NodeProvenanceContext<'a> {
    pub(super) graph: &'a PubSourceGraph,
    pub(super) chunk: &'a Contents0x2cChunk,
    pub(super) shape: &'a pub_escher::SpContainerObservation,
    pub(super) seq_num: u32,
    pub(super) grouped_sources: &'a [RawSpan],
    pub(super) direct_image_anchor_recovered_from_contents_extent: bool,
    pub(super) node_transform_projection:
        &'a super::direct_transform::BoundedNodeTransformProjection,
    pub(super) text_frame_inset: &'a Option<PubTextFrameInsetSource>,
    pub(super) story_frame: &'a Option<PubStoryFrameSource>,
    pub(super) table: &'a Option<PubTableSource>,
    pub(super) table_story: &'a Option<PubTableStoryOwnershipSource>,
    pub(super) color_scheme:
        Option<&'a super::publication_document::PubPublicationColorScheme>,
    pub(super) dgg_defaults: Option<&'a pub_escher::DggDefaultOptionsObservation>,
    pub(super) effective_paint: &'a Option<PubEffectiveShapePaintSource>,
    pub(super) shape_has_default_roundrect: bool,
    pub(super) shape_has_default_ellipse: bool,
    pub(super) shape_has_default_line: bool,
    pub(super) shape_has_dash_gel: bool,
    pub(super) uses_officeart_scheme_color: bool,
    pub(super) effective_paint_has_dgg_authority: bool,
}

pub(super) fn build_node_source_refs(
    context: NodeProvenanceContext<'_>,
) -> Result<Vec<SourceRef>> {
    let NodeProvenanceContext {
        graph,
        chunk,
        shape,
        seq_num,
        grouped_sources,
        direct_image_anchor_recovered_from_contents_extent,
        node_transform_projection,
        text_frame_inset,
        story_frame,
        table,
        table_story,
        color_scheme,
        dgg_defaults,
        effective_paint,
        shape_has_default_roundrect,
        shape_has_default_ellipse,
        shape_has_default_line,
        shape_has_dash_gel,
        uses_officeart_scheme_color,
        effective_paint_has_dgg_authority,
    } = context;
    let object_key = contents_object_key(seq_num);
    let mut source_refs = vec![source_ref(
        &graph.source,
        &chunk.source,
        Some(object_key.clone()),
        Some("chunk".into()),
        SourceRole::Semantic,
        AuthorityClass::Authoritative,
        ReadConfidence::Exact,
    )];
    source_refs.push(source_ref(
        &graph.source,
        &shape.source,
        Some(format!("escher/client-data-shape-id/{seq_num}")),
        Some(if grouped_sources.is_empty() {
            "SpContainer/ClientAnchor".into()
        } else {
            "SpContainer/ChildAnchor".into()
        }),
        SourceRole::Projection,
        AuthorityClass::Authoritative,
        ReadConfidence::Exact,
    ));
    if direct_image_anchor_recovered_from_contents_extent {
        for (field_id, path) in [
            (FIELD_SHAPE_WIDTH, "Contents/0x01/shape-width"),
            (FIELD_SHAPE_HEIGHT, "Contents/0x01/shape-height"),
        ] {
            if let Some((_, value_source)) = unique_u32_field(chunk, field_id)? {
                source_refs.push(source_ref(
                    &graph.source,
                    &value_source,
                    Some(object_key.clone()),
                    Some(path.into()),
                    SourceRole::Projection,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ));
            }
        }
    }
    if shape_has_default_roundrect {
        source_refs.push(source_ref(
            &graph.source,
            &shape.source,
            Some(format!("escher/client-data-shape-id/{seq_num}")),
            Some("SpContainer/FSP/default-roundrect".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }
    if shape_has_default_ellipse {
        source_refs.push(source_ref(
            &graph.source,
            &shape.source,
            Some(format!("escher/client-data-shape-id/{seq_num}")),
            Some("SpContainer/FSP/default-ellipse".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }
    if shape_has_default_line {
        source_refs.push(source_ref(
            &graph.source,
            &shape.source,
            Some(format!("escher/client-data-shape-id/{seq_num}")),
            Some("SpContainer/FSP/default-line".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
        if shape_has_dash_gel {
            source_refs.push(source_ref(
                &graph.source,
                &shape.source,
                Some(format!("escher/client-data-shape-id/{seq_num}")),
                Some("SpContainer/FOPT/line-dashing-dash-gel".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }
    }
    if node_transform_projection.image_rotation_applied
        || node_transform_projection
            .image_cardinal_rotation_degrees
            .is_some()
    {
        source_refs.push(source_ref(
            &graph.source,
            &shape.source,
            Some(format!("escher/client-data-shape-id/{seq_num}")),
            Some("SpContainer/FOPT/rotation".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }
    if has_explicit_officeart_paint_observation(shape) {
        source_refs.push(source_ref(
            &graph.source,
            &shape.source,
            Some(format!("escher/client-data-shape-id/{seq_num}")),
            Some("SpContainer/FOPT".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }
    if uses_officeart_scheme_color {
        if let Some(color_scheme) = color_scheme {
            source_refs.push(source_ref(
                &graph.source,
                &color_scheme.scheme.source,
                Some(contents_object_key(color_scheme.seq_num)),
                Some("OplSccm/current-color-scheme".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }
    }
    if effective_paint_has_dgg_authority {
        if let Some(dgg_defaults) = dgg_defaults {
            source_refs.push(source_ref(
                &graph.source,
                &dgg_defaults.source,
                Some("escher/dgg/default-options".into()),
                Some("DggContainer/FOPT-defaults".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }
    }
    for (depth, span) in grouped_sources.iter().enumerate() {
        source_refs.push(source_ref(
            &graph.source,
            span,
            Some(format!("escher/group-ancestor/{seq_num}/{depth}")),
            Some("SpgrContainer/SpContainer".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }
    if let Some(text_frame_inset) = text_frame_inset.as_ref() {
        source_refs.extend(text_frame_inset.source_refs.clone());
    }
    if let Some(vertical_alignment) = story_frame
        .as_ref()
        .and_then(|frame| frame.vertical_alignment.as_ref())
    {
        source_refs.extend(vertical_alignment.source_refs.clone());
    }
    if let Some(table) = table.as_ref() {
        source_refs.extend(table.source_refs.clone());
    } else if let Some(table_story) = table_story.as_ref() {
        source_refs.extend(table_story.source_refs.clone());
    }

    Ok(source_refs)
}
