use super::*;

pub(super) struct MatureNodeMaterializationContext<'a> {
    pub(super) source_hash: &'a Sha256Digest,
    pub(super) contents_stream: &'a StreamPath,
    pub(super) contents: &'a [u8],
    pub(super) escher: &'a [u8],
    pub(super) references: &'a BTreeMap<u32, Contents0x2cChunkReference>,
    pub(super) page_seq_to_id: &'a BTreeMap<u32, PageId>,
    pub(super) story_by_syid: &'a BTreeMap<u32, StoryId>,
    pub(super) story_layout_keys: &'a BTreeMap<u32, (u32, RawSpan)>,
    pub(super) quill_catalog: Option<&'a pub_quill::QuillStoryCatalog>,
    pub(super) mcld: Option<&'a pub_quill::QuillMcldChunk>,
    pub(super) color_scheme: Option<&'a super::publication_document::PubPublicationColorScheme>,
}

pub(super) fn materialize_mature_nodes(
    context: MatureNodeMaterializationContext<'_>,
    graph: &mut PubSourceGraph,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Result<Vec<PubSourcePagePaintOrderV1>> {
    let MatureNodeMaterializationContext {
        source_hash,
        contents_stream,
        contents,
        escher,
        references,
        page_seq_to_id,
        story_by_syid,
        story_layout_keys,
        quill_catalog,
        mcld,
        color_scheme,
    } = context;
    let contents_stream = contents_stream.to_owned();

    let escher_inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.into()), escher)
        .context("parse OfficeArt SpContainers")?;
    let escher_by_contents_seq = index_escher_by_contents_seq(&escher_inventory);
    let dgg_default_inventory =
        inspect_dgg_default_options(StreamPath(ESCHER_STREAM_PATH.into()), escher)
            .context("parse OfficeArt DGG default options")?;
    let dgg_defaults_unambiguous = dgg_default_inventory.drawing_groups.len() <= 1;
    if !dgg_defaults_unambiguous {
        diagnostics.push(PubBridgeDiagnostic::AmbiguousOfficeArtDggDefaults {
            count: dgg_default_inventory.drawing_groups.len(),
        });
    }
    let dgg_defaults = dgg_default_inventory.drawing_groups.first();

    for reference in references.values() {
        let raw_type = single_raw_type(reference);
        if raw_type != Some(RAW_TYPE_SHAPE) && raw_type != Some(RAW_TYPE_TABLE) {
            continue;
        }

        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };

        let seq_num = seq_u32(reference.seq_num)?;
        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        if let Some(tail) = &chunk.unsupported_tail {
            diagnostics.push(PubBridgeDiagnostic::OpaqueContentsTail {
                seq_num,
                byte_range: ByteRange::new(tail.offset, tail.len),
            });
        }

        let matches = escher_by_contents_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let shape = match matches {
            [] => {
                diagnostics.push(PubBridgeDiagnostic::MissingEscherGeometry { seq_num });
                continue;
            }
            [index] => &escher_inventory.shapes[*index],
            many => {
                diagnostics.push(PubBridgeDiagnostic::AmbiguousEscherGeometry {
                    seq_num,
                    matches: many.len(),
                });
                continue;
            }
        };

        let direct_page = page_seq_to_id.get(&parent_seq).copied();
        let exact_story_identity = match raw_type {
            Some(RAW_TYPE_SHAPE) => unique_u32_field(&chunk, FIELD_STORY_ID)?
                .map(|(value, _)| value)
                .filter(|value| story_by_syid.contains_key(value)),
            Some(RAW_TYPE_TABLE) => {
                unique_story_id_scalar(&chunk)?.filter(|value| story_by_syid.contains_key(value))
            }
            _ => None,
        };
        let image_slot = exact_image_slot(shape, seq_num, diagnostics);
        let exact_grouped_image_identity = raw_type == Some(RAW_TYPE_SHAPE) && image_slot.is_some();
        let exact_grouped_primitive_shape_type = if raw_type == Some(RAW_TYPE_SHAPE)
            && exact_story_identity.is_none()
            && image_slot.is_none()
            && (has_default_ellipse_geometry(shape)
                || super::paint_projection::has_bounded_grouped_solid_rectangle(shape))
        {
            shape.fsp.as_ref().map(|fsp| fsp.shape_type)
        } else {
            None
        };
        let grouped_projection = if direct_page.is_none()
            && references.get(&parent_seq).and_then(single_raw_type) == Some(RAW_TYPE_GROUP)
            && (exact_story_identity.is_some()
                || exact_grouped_image_identity
                || exact_grouped_primitive_shape_type.is_some())
        {
            match project_grouped_object_shape(
                parent_seq,
                shape,
                &GroupedProjectionContext::new(
                    references,
                    page_seq_to_id,
                    &graph.pages,
                    &escher_inventory,
                    &escher_by_contents_seq,
                ),
                exact_grouped_image_identity && exact_story_identity.is_none(),
            ) {
                Ok(Some(projection)) => {
                    diagnostics.push(if raw_type == Some(RAW_TYPE_TABLE) {
                        PubBridgeDiagnostic::GroupedTableProjected {
                            seq_num,
                            depth: projection.depth,
                        }
                    } else if exact_story_identity.is_some() {
                        PubBridgeDiagnostic::GroupedStoryProjected {
                            seq_num,
                            depth: projection.depth,
                        }
                    } else if let Some(shape_type) = exact_grouped_primitive_shape_type {
                        PubBridgeDiagnostic::GroupedPrimitiveProjected {
                            seq_num,
                            shape_type,
                            depth: projection.depth,
                        }
                    } else {
                        PubBridgeDiagnostic::GroupedImageProjected {
                            seq_num,
                            depth: projection.depth,
                        }
                    });
                    Some(projection)
                }
                Ok(None) => None,
                Err(error) => {
                    diagnostics.push(if raw_type == Some(RAW_TYPE_TABLE) {
                        PubBridgeDiagnostic::GroupedTableProjectionUnavailable {
                            seq_num,
                            reason: error.to_string(),
                        }
                    } else if exact_story_identity.is_some() {
                        PubBridgeDiagnostic::GroupedStoryProjectionUnavailable {
                            seq_num,
                            reason: error.to_string(),
                        }
                    } else if let Some(shape_type) = exact_grouped_primitive_shape_type {
                        PubBridgeDiagnostic::GroupedPrimitiveProjectionUnavailable {
                            seq_num,
                            shape_type,
                            reason: error.to_string(),
                        }
                    } else {
                        PubBridgeDiagnostic::GroupedImageProjectionUnavailable {
                            seq_num,
                            reason: error.to_string(),
                        }
                    });
                    None
                }
            }
        } else {
            None
        };

        let (
            page_id,
            bounds,
            grouped_sources,
            grouped_image_transform,
            direct_image_anchor_recovered_from_contents_extent,
        ) = if let Some(page_id) = direct_page {
            let Some(anchor) = shape.client_anchor.as_ref() else {
                diagnostics.push(PubBridgeDiagnostic::IncompleteEscherAnchor { seq_num });
                continue;
            };
            let page = graph
                .pages
                .get(&page_id)
                .expect("page id came from graph registry");
            let (bounds, recovered_from_contents_extent) =
                if let Some(bounds) = page_relative_bounds(page, anchor) {
                    (bounds, false)
                } else {
                    let recovered = if raw_type == Some(RAW_TYPE_SHAPE)
                        && exact_story_identity.is_none()
                        && image_slot.is_some()
                    {
                        match (
                            unique_u32_field(&chunk, FIELD_SHAPE_WIDTH)?,
                            unique_u32_field(&chunk, FIELD_SHAPE_HEIGHT)?,
                        ) {
                            (Some((width, _)), Some((height, _))) => {
                                page_relative_bounds_from_contents_missing_xe(
                                    page, anchor, width, height,
                                )
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    let Some(bounds) = recovered else {
                        let complete = anchor_has_unique_geometry_fields(anchor);
                        diagnostics.push(if complete {
                            PubBridgeDiagnostic::InvalidEscherAnchor { seq_num }
                        } else {
                            PubBridgeDiagnostic::IncompleteEscherAnchor { seq_num }
                        });
                        continue;
                    };
                    (bounds, true)
                };
            (
                page_id,
                bounds,
                Vec::new(),
                None,
                recovered_from_contents_extent,
            )
        } else if let Some(projection) = grouped_projection {
            (
                projection.page_id,
                projection.bounds,
                projection.group_sources,
                projection.image_transform,
                false,
            )
        } else {
            continue;
        };

        let node_id = derive_pub_node_id(source_hash, seq_num)?;
        let explicit_paint =
            explicit_officeart_paint(shape, color_scheme.map(|scheme| &scheme.scheme));
        let effective_paint = dgg_defaults_unambiguous
            .then(|| {
                resolve_bounded_effective_officeart_paint(
                    shape,
                    dgg_defaults,
                    color_scheme.map(|scheme| &scheme.scheme),
                    admits_normative_2d_paint_defaults(shape),
                )
            })
            .flatten();
        let explicit_image_crop = image_slot
            .is_some()
            .then(|| bounded_officeart_image_crop(shape))
            .flatten();
        let explicit_image_recolor = image_slot
            .is_some()
            .then(|| {
                bounded_officeart_image_recolor(shape, color_scheme.map(|scheme| &scheme.scheme))
            })
            .flatten();
        let mut story_frame = if raw_type == Some(RAW_TYPE_SHAPE) {
            build_story_frame(*source_hash, seq_num, &chunk, story_by_syid, diagnostics)?
        } else {
            None
        };
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
        let (table_story, table) = if raw_type == Some(RAW_TYPE_TABLE) {
            if let Some(quill_catalog) = quill_catalog {
                let context = table_bridge::TableBridgeContext {
                    source: &graph.source,
                    contents_stream: &contents_stream,
                    contents,
                    references,
                    quill_catalog,
                    story_by_syid,
                    story_layout_keys,
                    mcld,
                    table_bounds: &bounds,
                    officeart_owner_shape: shape,
                    officeart_inventory: &escher_inventory,
                    color_scheme: color_scheme.map(|value| &value.scheme),
                    dgg_defaults: if dgg_defaults_unambiguous {
                        dgg_defaults
                    } else {
                        None
                    },
                };
                (
                    table_bridge::build_table_story_ownership_source(&context, seq_num, &chunk)?,
                    table_bridge::build_table_source(&context, seq_num, &chunk, diagnostics)?,
                )
            } else {
                // The only no-catalog admission is the already-fenced physical-empty
                // Story65 variant with no live SHAPE/TABLE Story identity. Do not
                // fabricate Quill/TCD-backed table semantics in that geometry-only path.
                (None, None)
            }
        } else {
            (None, None)
        };

        let direct_image_candidate = raw_type == Some(RAW_TYPE_SHAPE)
            && exact_story_identity.is_none()
            && image_slot.is_some()
            && grouped_sources.is_empty();
        let direct_story_candidate =
            raw_type == Some(RAW_TYPE_SHAPE) && story_frame.is_some() && grouped_sources.is_empty();
        let node_transform_projection = bounded_node_transform_projection(
            shape,
            bounds,
            direct_image_candidate,
            grouped_image_transform,
            direct_story_candidate,
            explicit_image_crop.is_some(),
        );

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
                if let Some((_, value_source)) = unique_u32_field(&chunk, field_id)? {
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
        if has_default_roundrect_geometry(shape) {
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
        if has_default_ellipse_geometry(shape) {
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
        if has_default_line_geometry(shape) {
            source_refs.push(source_ref(
                &graph.source,
                &shape.source,
                Some(format!("escher/client-data-shape-id/{seq_num}")),
                Some("SpContainer/FSP/default-line".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
            if has_shape_local_dash_gel(shape) {
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
        if paint_context_uses_officeart_scheme_color(shape, dgg_defaults) {
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
        if effective_paint
            .as_ref()
            .is_some_and(effective_paint_has_dgg_authority)
        {
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
        if let Some(text_frame_inset) = &text_frame_inset {
            source_refs.extend(text_frame_inset.source_refs.clone());
        }
        if let Some(vertical_alignment) = story_frame
            .as_ref()
            .and_then(|frame| frame.vertical_alignment.as_ref())
        {
            source_refs.extend(vertical_alignment.source_refs.clone());
        }
        if let Some(table) = &table {
            source_refs.extend(table.source_refs.clone());
        } else if let Some(table_story) = &table_story {
            source_refs.extend(table_story.source_refs.clone());
        }

        graph.nodes.insert(
            node_id,
            Node {
                // Grouped Story/image shapes and TABLEs are projected to page-relative
                // geometry while exact group ancestry remains in provenance.
                // The current resolver does not yet compose Group transforms.
                kind: if raw_type == Some(RAW_TYPE_TABLE) {
                    NodeKind::Table
                } else {
                    NodeKind::Shape
                },
                header: NodeHeader {
                    id: node_id,
                    parent_id: page_id.into_canonical(),
                    bounds,
                    transform: node_transform_projection.transform,
                    source_refs,
                    extensions: Vec::new(),
                },
                payload: PubNodePayload {
                    contents_seq_num: seq_num,
                    officeart_shape_type: shape.fsp.as_ref().map(|fsp| fsp.shape_type),
                    officeart_spid: shape.fsp.as_ref().map(|fsp| fsp.spid),
                    image_slot,
                    legacy_ole: None,
                    explicit_image_crop,
                    explicit_image_cardinal_rotation_degrees: node_transform_projection
                        .image_cardinal_rotation_degrees,
                    explicit_image_recolor,
                    explicit_paint,
                    effective_paint,
                    story_frame,
                    text_frame_inset,
                    table_story,
                    table,
                },
            },
        );
    }

    add_missing_link_target_diagnostics(graph, diagnostics);

    let source_page_paint_orders = source_page_paint_orders_v1(
        *source_hash,
        graph,
        references,
        page_seq_to_id,
        &escher_inventory,
    );
    Ok(source_page_paint_orders)
}

fn exact_image_slot(
    shape: &pub_escher::SpContainerObservation,
    seq_num: u32,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Option<u32> {
    let mut slots = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| {
            property.property_id() == OFFICE_ART_PROPERTY_PIB && property.op_is_blip_id()
        })
        .map(|property| property.op)
        .collect::<BTreeSet<_>>();

    match slots.len() {
        0 => None,
        1 => slots.pop_first(),
        _ => {
            diagnostics.push(PubBridgeDiagnostic::AmbiguousImageSlot {
                seq_num,
                slots: slots.into_iter().collect(),
            });
            None
        }
    }
}
