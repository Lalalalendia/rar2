use super::{
    CONTENTS_STREAM_PATH, PubBridgeDiagnostic, PubEffectivePageProjection,
    PubEffectivePageProjectionAuthority, PubExplicitShapePaintSource, PubLegacyOleSource,
    PubNodePayload, PubSourceGraph, PubSourceGraphBuild, PubStoryFrameSource,
    PubTableCellCoordinates, PubTableCellSource, PubTableSource, PubTableStoryOwnershipSource,
    ROLE_DOCUMENT, ROLE_NODE, ROLE_PAGE, ROLE_STORY, bounded_wmf_metafile, derive_pub_id,
    source_ref,
};
use anyhow::{Context, Result, bail};
use pub_contents::{
    LEGACY_0X22_TABLE_CHUNK_TYPE, Legacy0x22Directory, Legacy0x22DirectoryEntry,
    Legacy0x22ResolvedTable, parse_legacy_0x22_directory, parse_legacy_0x22_formatting_descriptor,
    parse_legacy_0x22_resolved_tables, parse_legacy_0x22_text_info_map,
};
use pub_core::{RawSpan, StreamPath};
use pub_model::{
    Affine2D, AuthorityClass, CanonicalId, Document, DocumentId, LengthEmu, Node, NodeHeader,
    NodeId, NodeKind, Page, PageId, ReadConfidence, RectEmu, Sha256Digest, SimpleRectangularTable,
    SimpleTableCell, Size2D, SourceDescriptor, SourceRole, Story, StoryId, TableCellAddress,
    TableCellId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom};

const LEGACY_DOCUMENT_TYPE: u16 = 0x0015;
const LEGACY_PAGE_TYPE: u16 = 0x0014;
const LEGACY_TEXT_SHAPE_TYPE: u16 = 0x0000;
const LEGACY_IMAGE_TYPE: u16 = 0x0002;
const LEGACY_IMAGE_DATA_TYPE: u16 = 0x0021;
const LEGACY_IMAGE_DATA_PAYLOAD_OFFSET: usize = 0x08;
const LEGACY_OLE_TYPE: u16 = 0x0003;
const LEGACY_OLE_DATA_TYPE: u16 = 0x0022;
const LEGACY_OLE_DATA_LEN: usize = 18;
const LEGACY_SIMPLE_GEOMETRY_SHAPE_TYPES: [u16; 4] = [0x0004, 0x0005, 0x0006, 0x0007];
const LEGACY_GROUP_TYPE: u16 = 0x000f;
const LEGACY_GROUP_MAX_DEPTH: usize = 100;
const LEGACY_LIST_HEADER_SIZE: usize = 10;
const LEGACY_LIST_U16_RECORD_SIZE: u16 = 2;
const LEGACY_DOCUMENT_WIDTH_OFFSET: usize = 0x14;
const LEGACY_DOCUMENT_HEIGHT_OFFSET: usize = 0x18;
const LEGACY_SHAPE_XS_OFFSET: usize = 0x06;
const LEGACY_SHAPE_YS_OFFSET: usize = 0x0a;
const LEGACY_SHAPE_XE_OFFSET: usize = 0x0e;
const LEGACY_SHAPE_YE_OFFSET: usize = 0x12;

#[derive(Debug, Clone)]
struct LegacyIdList {
    ids: Vec<(u16, RawSpan)>,
}

#[derive(Debug, Clone)]
struct LegacyImageWmfProfile {
    source: RawSpan,
    normalized_bytes: Vec<u8>,
}

pub fn read_legacy_0x22_image_wmfs<R: Read + Seek>(
    mut reader: R,
    image_object_ids: &[u16],
) -> Result<BTreeMap<u16, Vec<u8>>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;
    let contents = match pub_cfb::read_stream_reader(
        Cursor::new(pub_bytes.as_slice()),
        CONTENTS_STREAM_PATH,
    ) {
        Ok(contents) => contents,
        Err(strict_error) => {
            let recovered = pub_cfb::recover_root_regular_stream_reader(
                Cursor::new(pub_bytes.as_slice()),
                CONTENTS_STREAM_PATH,
            )
            .with_context(|| {
                format!(
                    "strict CFB read failed ({strict_error}); bounded root Contents recovery failed"
                )
            })?;
            if recovered.root_entry_names.iter().any(|name| {
                name.eq_ignore_ascii_case("Quill") || name.eq_ignore_ascii_case("Escher")
            }) {
                bail!(
                    "bounded root Contents recovery is forbidden when Quill or Escher is present"
                );
            }
            recovered.bytes
        }
    };
    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let directory = parse_legacy_0x22_directory(stream, &contents)
        .context("parse legacy no-Quill 0x22 Contents directory")?;
    let mut images = BTreeMap::new();
    for image_object_id in image_object_ids {
        if images.contains_key(image_object_id) {
            continue;
        }
        let profile = legacy_image_wmf_profile(&contents, &directory, *image_object_id)
            .with_context(|| {
                format!(
                    "legacy IMAGE object {image_object_id} has no admitted direct native WMF payload"
                )
            })?;
        images.insert(*image_object_id, profile.normalized_bytes);
    }
    Ok(images)
}

pub fn read_legacy_0x22_image_wmf<R: Read + Seek>(
    reader: R,
    image_object_id: u16,
) -> Result<Vec<u8>> {
    let mut images = read_legacy_0x22_image_wmfs(reader, &[image_object_id])?;
    images
        .remove(&image_object_id)
        .context("legacy IMAGE batch result omitted requested object")
}

pub fn build_legacy_0x22_noquill_source_graph<R: Read + Seek>(
    mut reader: R,
    source_hash: Sha256Digest,
) -> Result<PubSourceGraphBuild> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;
    let contents = match pub_cfb::read_stream_reader(
        Cursor::new(pub_bytes.as_slice()),
        CONTENTS_STREAM_PATH,
    ) {
        Ok(contents) => contents,
        Err(strict_error) => {
            let recovered = pub_cfb::recover_root_regular_stream_reader(
                Cursor::new(pub_bytes.as_slice()),
                CONTENTS_STREAM_PATH,
            )
            .with_context(|| {
                format!(
                    "strict CFB read failed ({strict_error}); bounded root Contents recovery failed"
                )
            })?;
            if recovered.root_entry_names.iter().any(|name| {
                name.eq_ignore_ascii_case("Quill") || name.eq_ignore_ascii_case("Escher")
            }) {
                bail!(
                    "bounded root Contents recovery is forbidden when Quill or Escher is present"
                );
            }
            recovered.bytes
        }
    };
    build_legacy_0x22_noquill_from_contents(source_hash, &contents)
}

pub fn build_legacy_0x22_noquill_from_contents(
    source_hash: Sha256Digest,
    contents: &[u8],
) -> Result<PubSourceGraphBuild> {
    let source = SourceDescriptor {
        format: "pub".into(),
        format_version: Some("0x22-noquill".into()),
        adapter_version: format!("pub-rs/{}", env!("CARGO_PKG_VERSION")),
        source_hash,
    };
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let directory = parse_legacy_0x22_directory(contents_stream.clone(), contents)
        .context("parse legacy no-Quill 0x22 Contents directory")?;
    let descriptor = parse_legacy_0x22_formatting_descriptor(contents_stream.clone(), contents)
        .context("parse legacy no-Quill text descriptor")?;

    let document_entry = unique_entry_by_type(&directory, LEGACY_DOCUMENT_TYPE, "DOCUMENT 0x0015")?;
    let document_object_id = document_entry.object_id;
    let document_chunk = chunk_bytes(contents, document_entry)?;
    let page_width_emu = read_u32(document_chunk, LEGACY_DOCUMENT_WIDTH_OFFSET)
        .context("legacy DOCUMENT width is missing")?;
    let page_height_emu = read_u32(document_chunk, LEGACY_DOCUMENT_HEIGHT_OFFSET)
        .context("legacy DOCUMENT height is missing")?;
    if page_width_emu == 0 || page_height_emu == 0 {
        bail!(
            "legacy DOCUMENT has non-positive page extent {}x{}",
            page_width_emu,
            page_height_emu
        );
    }

    let document_page_list = parse_u16_id_list(contents, document_entry, "DOCUMENT PageList")?;
    if document_page_list.ids.is_empty() {
        bail!("legacy DOCUMENT PageList is empty");
    }

    let document_id = derive_legacy_document_id(&source_hash, document_object_id)?;
    let mut document_pages = Vec::with_capacity(document_page_list.ids.len());
    let mut pages = BTreeMap::new();
    let mut page_object_to_id = BTreeMap::new();
    let mut seen_pages = BTreeSet::new();

    for (page_object_id, _) in &document_page_list.ids {
        if !seen_pages.insert(*page_object_id) {
            bail!("legacy DOCUMENT PageList repeats object {page_object_id}");
        }
        let entry = directory
            .entry_by_object_id(*page_object_id)
            .with_context(|| {
                format!("legacy DOCUMENT PageList references missing object {page_object_id}")
            })?;
        if entry.chunk_type != LEGACY_PAGE_TYPE {
            bail!(
                "legacy DOCUMENT PageList object {} has type {:#06x}, expected PAGE 0x0014",
                page_object_id,
                entry.chunk_type
            );
        }
        let page_id = derive_legacy_page_id(&source_hash, *page_object_id)?;
        document_pages.push(page_id);
        page_object_to_id.insert(*page_object_id, page_id);
        pages.insert(
            page_id,
            Page {
                id: page_id,
                size: Size2D::new(
                    LengthEmu::new(i64::from(page_width_emu)),
                    LengthEmu::new(i64::from(page_height_emu)),
                ),
                bleed: None,
                margins: None,
                children: Vec::new(),
                extensions: Vec::new(),
            },
        );
    }

    let document = Document {
        id: document_id,
        format_origin: "pub".into(),
        source_hash,
        pages: document_pages.clone(),
        resources: Vec::new(),
        styles: Vec::new(),
    };
    let mut graph = PubSourceGraph::empty(source, document);
    graph.pages = pages;

    let text_start =
        usize::try_from(descriptor.text_start).context("legacy text start overflow")?;
    let text_end = usize::try_from(descriptor.text_end).context("legacy text end overflow")?;
    let text_bytes = contents
        .get(text_start..text_end)
        .context("legacy no-Quill text range exceeds Contents")?;
    let mut story_by_owner = BTreeMap::<u16, StoryId>::new();
    let mut diagnostics = Vec::new();
    let resolved_tables = if directory
        .entries
        .iter()
        .any(|entry| entry.chunk_type == LEGACY_0X22_TABLE_CHUNK_TYPE)
    {
        parse_legacy_0x22_resolved_tables(contents_stream.clone(), contents)
            .map(|catalog| {
                catalog
                    .tables
                    .into_iter()
                    .map(|table| (table.object.chunk_id, table))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };

    let text_info = parse_legacy_0x22_text_info_map(contents_stream.clone(), contents)
        .context("parse legacy no-Quill text owner map")?;
    if let Some(text_info) = text_info {
        let mut previous_end = 0usize;
        for owner in text_info.ends {
            let end = usize::try_from(owner.relative_end_offset)
                .context("legacy owner text end overflow")?
                .checked_add(1)
                .context("legacy owner text end overflow")?;
            if end < previous_end || end > text_bytes.len() {
                bail!(
                    "legacy owner {} text boundary {} is invalid for text length {}",
                    owner.owner_id,
                    end,
                    text_bytes.len()
                );
            }
            if end == previous_end {
                continue;
            }
            let absolute_start = text_start + previous_end;
            let owner_bytes = &text_bytes[previous_end..end];
            match decode_bounded_legacy_ascii(owner_bytes) {
                Ok(text) => {
                    let story_id = derive_legacy_story_id(&source_hash, owner.owner_id)?;
                    graph.stories.insert(
                        story_id,
                        Story {
                            id: story_id,
                            text,
                            paragraphs: Vec::new(),
                            runs: Vec::new(),
                            fields: Vec::new(),
                            hyperlinks: Vec::new(),
                            source_refs: vec![
                                source_ref(
                                    &graph.source,
                                    &RawSpan {
                                        stream: contents_stream.clone(),
                                        offset: absolute_start as u64,
                                        len: (end - previous_end) as u64,
                                    },
                                    Some(format!("contents/0x22/noquill-owner/{}", owner.owner_id)),
                                    Some("legacy_text".into()),
                                    SourceRole::Semantic,
                                    AuthorityClass::Authoritative,
                                    ReadConfidence::Exact,
                                ),
                                source_ref(
                                    &graph.source,
                                    &owner.owner_id_source,
                                    Some(format!("contents/0x22/noquill-owner/{}", owner.owner_id)),
                                    Some("owner_id".into()),
                                    SourceRole::Relation,
                                    AuthorityClass::Authoritative,
                                    ReadConfidence::Exact,
                                ),
                            ],
                        },
                    );
                    story_by_owner.insert(owner.owner_id, story_id);
                }
                Err(_) => diagnostics.push(PubBridgeDiagnostic::LegacyTextEncodingUnresolved {
                    owner_id: Some(u32::from(owner.owner_id)),
                    source: RawSpan {
                        stream: contents_stream.clone(),
                        offset: absolute_start as u64,
                        len: owner_bytes.len() as u64,
                    },
                    high_byte_count: owner_bytes.iter().filter(|byte| **byte >= 0x80).count(),
                }),
            }
            previous_end = end;
        }

        if previous_end < text_bytes.len() {
            materialize_unowned_text_story(
                &mut graph,
                &source_hash,
                &contents_stream,
                text_bytes,
                text_start,
                previous_end,
                &mut diagnostics,
            )?;
        }
    } else if !text_bytes.is_empty() {
        materialize_unowned_text_story(
            &mut graph,
            &source_hash,
            &contents_stream,
            text_bytes,
            text_start,
            0,
            &mut diagnostics,
        )?;
    }

    for (page_object_id, page_id) in &page_object_to_id {
        let page_entry = directory
            .entry_by_object_id(*page_object_id)
            .expect("PageList entry was verified above");
        let child_list = parse_u16_id_list(contents, page_entry, "PAGE child list")
            .with_context(|| format!("parse legacy PAGE {page_object_id} child list"))?;
        let page = graph
            .pages
            .get(page_id)
            .expect("verified page must be in SourceGraph")
            .clone();
        let mut group_stack = BTreeSet::new();

        for (child_object_id, _) in child_list.ids {
            materialize_legacy_noquill_child(
                &mut graph,
                &source_hash,
                contents,
                &contents_stream,
                &directory,
                &page,
                *page_object_id,
                page_id.into_canonical(),
                child_object_id,
                &story_by_owner,
                &resolved_tables,
                &mut diagnostics,
                &mut group_stack,
                0,
            )?;
        }
    }

    Ok(PubSourceGraphBuild {
        graph,
        effective_pages: PubEffectivePageProjection {
            authority: PubEffectivePageProjectionAuthority::RawDocumentPageList,
            page_ids: document_pages.clone(),
            raw_page_count: document_pages.len(),
            observed_scenario_page_ids: Vec::new(),
            scenario_evidence_list_count: 0,
        },
        source_page_paint_orders: Vec::new(),
        diagnostics,
        typography_runs: Vec::new(),
        typography_size_runs: Vec::new(),
        paragraph_alignments: Vec::new(),
        paragraph_line_spacings: Vec::new(),
        paragraph_flow_runs: Vec::new(),
        script_font_maps: Vec::new(),
    })
}

// Recursive legacy materialization carries the bounded source, page, identity, and
// diagnostic state explicitly so nested GROUP traversal cannot silently widen it.
#[allow(clippy::too_many_arguments)]
fn materialize_legacy_noquill_child(
    graph: &mut PubSourceGraph,
    source_hash: &Sha256Digest,
    contents: &[u8],
    contents_stream: &StreamPath,
    directory: &Legacy0x22Directory,
    page: &Page,
    parent_object_id: u16,
    parent_id: CanonicalId,
    child_object_id: u16,
    story_by_owner: &BTreeMap<u16, StoryId>,
    resolved_tables: &BTreeMap<u16, Legacy0x22ResolvedTable>,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
    group_stack: &mut BTreeSet<u16>,
    group_depth: usize,
) -> Result<()> {
    let Some(child_entry) = directory.entry_by_object_id(child_object_id) else {
        diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
            object_id: u32::from(child_object_id),
            raw_type: None,
            reason: "page_child_missing_from_directory".into(),
        });
        return Ok(());
    };
    if child_entry.parent_id != parent_object_id {
        diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
            object_id: u32::from(child_object_id),
            raw_type: Some(child_entry.chunk_type),
            reason: "page_child_parent_mismatch".into(),
        });
        return Ok(());
    }

    let is_text_shape = child_entry.chunk_type == LEGACY_TEXT_SHAPE_TYPE;
    let is_legacy_image = child_entry.chunk_type == LEGACY_IMAGE_TYPE;
    let is_group = child_entry.chunk_type == LEGACY_GROUP_TYPE;
    let is_legacy_ole = child_entry.chunk_type == LEGACY_OLE_TYPE;
    let is_table = child_entry.chunk_type == LEGACY_0X22_TABLE_CHUNK_TYPE;
    let legacy_image_source = if is_legacy_image {
        let Some(profile) = legacy_image_wmf_profile(contents, directory, child_object_id) else {
            diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
                object_id: u32::from(child_object_id),
                raw_type: Some(child_entry.chunk_type),
                reason: "legacy_image_native_wmf_not_admitted_v1".into(),
            });
            return Ok(());
        };
        Some(profile.source)
    } else {
        None
    };
    let (legacy_ole, legacy_ole_source) = if is_legacy_ole {
        let Some(profile) = legacy_ole_profile(contents, directory, child_object_id) else {
            diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
                object_id: u32::from(child_object_id),
                raw_type: Some(child_entry.chunk_type),
                reason: "legacy_ole_profile_not_admitted_v1".into(),
            });
            return Ok(());
        };
        (Some(profile.0), Some(profile.1))
    } else {
        (None, None)
    };
    if !is_text_shape
        && !is_legacy_image
        && !is_group
        && !is_legacy_ole
        && !is_table
        && !is_legacy_simple_geometry_shape_type(child_entry.chunk_type)
    {
        diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
            object_id: u32::from(child_object_id),
            raw_type: Some(child_entry.chunk_type),
            reason: "legacy_noquill_object_type_not_admitted_v1".into(),
        });
        return Ok(());
    }

    let resolved_table = if is_table {
        let Some(table) = resolved_tables.get(&child_object_id) else {
            diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
                object_id: u32::from(child_object_id),
                raw_type: Some(child_entry.chunk_type),
                reason: "legacy_table_not_resolved_v1".into(),
            });
            return Ok(());
        };
        if table.object.chunk_id != child_object_id || table.object.parent_id != parent_object_id {
            diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
                object_id: u32::from(child_object_id),
                raw_type: Some(child_entry.chunk_type),
                reason: "legacy_table_identity_mismatch_v1".into(),
            });
            return Ok(());
        }
        Some(table)
    } else {
        None
    };

    if is_group && (group_depth >= LEGACY_GROUP_MAX_DEPTH || !group_stack.insert(child_object_id)) {
        diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
            object_id: u32::from(child_object_id),
            raw_type: Some(child_entry.chunk_type),
            reason: "legacy_group_cycle_or_depth_limit".into(),
        });
        return Ok(());
    }

    let chunk = chunk_bytes(contents, child_entry)?;
    let Some(bounds) = legacy_shape_bounds(page, chunk, child_entry.chunk_type) else {
        diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
            object_id: u32::from(child_object_id),
            raw_type: Some(child_entry.chunk_type),
            reason: "invalid_or_incomplete_geometry".into(),
        });
        if is_group {
            group_stack.remove(&child_object_id);
        }
        return Ok(());
    };

    if let Some(table) = resolved_table {
        if !table.object.is_materialized_grid()
            || !table.object.columns_sum_to_declared_width()
            || !table.object.rows_sum_to_declared_height()
            || bounds.width.get() != i64::from(table.object.width_emu)
            || bounds.height.get() != i64::from(table.object.height_emu)
        {
            diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
                object_id: u32::from(child_object_id),
                raw_type: Some(child_entry.chunk_type),
                reason: "legacy_table_geometry_grid_mismatch_v1".into(),
            });
            return Ok(());
        }
    }

    let node_id = derive_legacy_node_id(source_hash, child_object_id)?;
    let story_id = if is_text_shape {
        story_by_owner.get(&child_object_id).copied()
    } else {
        None
    };
    let (table_story, table_source) = if let Some(table) = resolved_table {
        let Some((story, source)) =
            build_legacy_table_projection(graph, source_hash, table, bounds, diagnostics)?
        else {
            return Ok(());
        };
        (Some(story), Some(source))
    } else {
        (None, None)
    };
    let mut source_refs = vec![
        source_ref(
            &graph.source,
            &child_entry.entry_source,
            Some(format!("contents/0x22/object/{child_object_id}")),
            Some("directory_entry".into()),
            SourceRole::Relation,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ),
        source_ref(
            &graph.source,
            &RawSpan {
                stream: contents_stream.clone(),
                offset: child_entry.chunk_source.offset + LEGACY_SHAPE_XS_OFFSET as u64,
                len: 16,
            },
            Some(format!("contents/0x22/object/{child_object_id}")),
            Some("legacy_center_origin_geometry".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ),
    ];
    if let Some(source) = legacy_ole_source {
        source_refs.push(source_ref(
            &graph.source,
            &source,
            Some(format!("contents/0x22/object/{child_object_id}")),
            Some("legacy_ole_data".into()),
            SourceRole::Semantic,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }
    if let Some(source) = legacy_image_source {
        source_refs.push(source_ref(
            &graph.source,
            &source,
            Some(format!("contents/0x22/object/{child_object_id}")),
            Some("legacy_image_wmf".into()),
            SourceRole::Semantic,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }

    graph.nodes.insert(
        node_id,
        Node {
            kind: if is_group {
                NodeKind::Group
            } else if is_legacy_image {
                NodeKind::ImageFrame
            } else if is_legacy_ole {
                NodeKind::Unsupported
            } else if is_table {
                NodeKind::Table
            } else {
                NodeKind::Shape
            },
            header: NodeHeader {
                id: node_id,
                parent_id,
                bounds,
                transform: Affine2D::identity(),
                source_refs,
                extensions: Vec::new(),
            },
            payload: PubNodePayload {
                contents_seq_num: u32::from(child_object_id),
                officeart_shape_type: None,
                officeart_spid: None,
                image_slot: None,
                legacy_ole,
                explicit_image_crop: None,
                explicit_image_cardinal_rotation_degrees: None,
                explicit_image_recolor: None,
                explicit_paint: PubExplicitShapePaintSource::default(),
                effective_paint: None,
                story_frame: story_id.map(|story_id| PubStoryFrameSource {
                    text_id: u32::from(child_object_id),
                    story_id: Some(story_id),
                    explicit_ordinal: None,
                    previous_seq_num: None,
                    previous_frame: None,
                    next_seq_num: None,
                    next_frame: None,
                    vertical_alignment: None,
                }),
                text_frame_inset: None,
                table_story,
                table: table_source,
            },
        },
    );

    if is_group {
        let child_ids = directory
            .entries_by_parent_id(child_object_id)
            .map(|entry| entry.object_id)
            .collect::<Vec<_>>();
        for nested_object_id in child_ids {
            materialize_legacy_noquill_child(
                graph,
                source_hash,
                contents,
                contents_stream,
                directory,
                page,
                child_object_id,
                node_id.into_canonical(),
                nested_object_id,
                story_by_owner,
                resolved_tables,
                diagnostics,
                group_stack,
                group_depth + 1,
            )?;
        }
        group_stack.remove(&child_object_id);
    }

    Ok(())
}

fn build_legacy_table_projection(
    graph: &mut PubSourceGraph,
    source_hash: &Sha256Digest,
    table: &Legacy0x22ResolvedTable,
    table_bounds: RectEmu,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Result<Option<(PubTableStoryOwnershipSource, PubTableSource)>> {
    let rows = u32::from(table.object.row_count);
    let columns = u32::from(table.object.column_count);
    let chunk_id = table.object.chunk_id;
    let story_id = derive_legacy_table_story_id(source_hash, chunk_id, table.effective_text_id)?;

    let mut story_text = String::new();
    let mut utf16_cursor = 0_u32;
    let mut cells = Vec::with_capacity(table.text.cells.len());
    let mut simple_cells = Vec::with_capacity(table.text.cells.len());

    for (index, cell) in table.text.cells.iter().enumerate() {
        let Ok(text) = decode_bounded_legacy_ascii(&cell.content_bytes) else {
            diagnostics.push(PubBridgeDiagnostic::LegacyTextEncodingUnresolved {
                owner_id: Some(table.effective_text_id),
                source: cell.content_source.clone(),
                high_byte_count: cell
                    .content_bytes
                    .iter()
                    .filter(|byte| **byte >= 0x80)
                    .count(),
            });
            diagnostics.push(PubBridgeDiagnostic::LegacyObjectNotMaterialized {
                object_id: u32::from(chunk_id),
                raw_type: Some(LEGACY_0X22_TABLE_CHUNK_TYPE),
                reason: "legacy_table_text_encoding_unresolved_v1".into(),
            });
            return Ok(None);
        };

        let utf16_start = utf16_cursor;
        if index != 0 {
            story_text.push('\r');
            utf16_cursor = utf16_cursor
                .checked_add(1)
                .context("legacy table story offset overflow")?;
        }
        story_text.push_str(&text);
        let text_len = u32::try_from(text.encode_utf16().count())
            .context("legacy table cell text length overflow")?;
        utf16_cursor = utf16_cursor
            .checked_add(text_len)
            .context("legacy table story offset overflow")?;
        let utf16_end = utf16_cursor;

        let index_u32 = u32::try_from(index).context("legacy table cell index overflow")?;
        let address = TableCellAddress {
            row: index_u32 / columns,
            column: index_u32 % columns,
        };
        let id = derive_legacy_table_cell_id(source_hash, chunk_id, cell.cell_index)?;
        let column_index =
            usize::try_from(address.column).context("legacy table column index overflow")?;
        let row_index = usize::try_from(address.row).context("legacy table row index overflow")?;
        let column = table
            .object
            .columns
            .get(column_index)
            .context("legacy table column extent missing")?;
        let row = table
            .object
            .rows
            .get(row_index)
            .context("legacy table row extent missing")?;
        let column_start_emu = if column_index == 0 {
            0
        } else {
            table.object.columns[column_index - 1].cumulative_emu
        };
        let row_start_emu = if row_index == 0 {
            0
        } else {
            table.object.rows[row_index - 1].cumulative_emu
        };
        let cell_x = table_bounds
            .x
            .get()
            .checked_add(i64::from(column_start_emu))
            .context("legacy table cell x overflow")?;
        let cell_y = table_bounds
            .y
            .get()
            .checked_add(i64::from(row_start_emu))
            .context("legacy table cell y overflow")?;
        let cell_bounds = RectEmu::new(
            LengthEmu::new(cell_x),
            LengthEmu::new(cell_y),
            LengthEmu::new(i64::from(column.extent_emu)),
            LengthEmu::new(i64::from(row.extent_emu)),
        );
        let key = format!("contents/0x22/table/{chunk_id}/cell/{}", cell.cell_index);
        cells.push(PubTableCellSource {
            id,
            stored_record_index: cell.cell_index,
            coordinates: Some(PubTableCellCoordinates {
                start_row: address.row,
                end_row: address.row,
                start_column: address.column,
                end_column: address.column,
            }),
            utf16_start,
            utf16_end,
            bounds: Some(cell_bounds),
            paint: None,
            source_refs: vec![
                source_ref(
                    &graph.source,
                    &cell.content_source,
                    Some(key.clone()),
                    Some("legacy_table_cell_text".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &cell.separator_source,
                    Some(key.clone()),
                    Some("legacy_table_cell_boundary".into()),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &column.record_source,
                    Some(key.clone()),
                    Some("legacy_table_column_extent".into()),
                    SourceRole::Projection,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    &graph.source,
                    &row.record_source,
                    Some(key),
                    Some("legacy_table_row_extent".into()),
                    SourceRole::Projection,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            ],
        });
        simple_cells.push(SimpleTableCell { id, address });
    }

    let simple_table = if table.horizontal_merges.is_empty() {
        SimpleRectangularTable::new(rows, columns, simple_cells).ok()
    } else {
        None
    };

    graph.stories.insert(
        story_id,
        Story {
            id: story_id,
            text: story_text,
            paragraphs: Vec::new(),
            runs: Vec::new(),
            fields: Vec::new(),
            hyperlinks: Vec::new(),
            source_refs: vec![source_ref(
                &graph.source,
                &table.text.source,
                Some(format!("contents/0x22/table/{chunk_id}/text")),
                Some("legacy_table_text_projection".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            )],
        },
    );

    let mut ownership_refs = vec![source_ref(
        &graph.source,
        &table.object.local_text_index_source,
        Some(format!("contents/0x22/table/{chunk_id}")),
        Some("legacy_table_text_index".into()),
        SourceRole::Relation,
        AuthorityClass::Authoritative,
        ReadConfidence::Exact,
    )];
    if let Some(source) = &table.text.text_id_source {
        ownership_refs.push(source_ref(
            &graph.source,
            source,
            Some(format!("contents/0x22/table/{chunk_id}")),
            Some("legacy_table_text_owner".into()),
            SourceRole::Relation,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }

    let table_story = PubTableStoryOwnershipSource {
        text_id: table.effective_text_id,
        story_id: Some(story_id),
        source_refs: ownership_refs,
    };
    let table_source = PubTableSource {
        text_id: table.effective_text_id,
        story_id: Some(story_id),
        rows,
        columns,
        cells_seq_num: None,
        tcd_story_ordinal: None,
        cells,
        simple_table,
        layout_relation: None,
        layout_metrics: None,
        border_segments: Vec::new(),
        source_refs: vec![
            source_ref(
                &graph.source,
                &table.object.row_count_source,
                Some(format!("contents/0x22/table/{chunk_id}")),
                Some("legacy_table_rows".into()),
                SourceRole::Semantic,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                &graph.source,
                &table.object.column_count_source,
                Some(format!("contents/0x22/table/{chunk_id}")),
                Some("legacy_table_columns".into()),
                SourceRole::Semantic,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                &graph.source,
                &table.object.width_source,
                Some(format!("contents/0x22/table/{chunk_id}")),
                Some("legacy_table_width".into()),
                SourceRole::Semantic,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                &graph.source,
                &table.object.height_source,
                Some(format!("contents/0x22/table/{chunk_id}")),
                Some("legacy_table_height".into()),
                SourceRole::Semantic,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
        ],
    };

    Ok(Some((table_story, table_source)))
}

fn materialize_unowned_text_story(
    graph: &mut PubSourceGraph,
    source_hash: &Sha256Digest,
    stream: &StreamPath,
    text_bytes: &[u8],
    absolute_text_start: usize,
    relative_start: usize,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Result<()> {
    let remaining = &text_bytes[relative_start..];
    if remaining.is_empty() {
        return Ok(());
    }
    let absolute_start = absolute_text_start + relative_start;
    let Ok(text) = decode_bounded_legacy_ascii(remaining) else {
        diagnostics.push(PubBridgeDiagnostic::LegacyTextEncodingUnresolved {
            owner_id: None,
            source: RawSpan {
                stream: stream.clone(),
                offset: absolute_start as u64,
                len: remaining.len() as u64,
            },
            high_byte_count: remaining.iter().filter(|byte| **byte >= 0x80).count(),
        });
        return Ok(());
    };
    let story_id = derive_unowned_text_story_id(source_hash, absolute_start, remaining.len())?;
    graph.stories.insert(
        story_id,
        Story {
            id: story_id,
            text,
            paragraphs: Vec::new(),
            runs: Vec::new(),
            fields: Vec::new(),
            hyperlinks: Vec::new(),
            source_refs: vec![source_ref(
                &graph.source,
                &RawSpan {
                    stream: stream.clone(),
                    offset: absolute_start as u64,
                    len: remaining.len() as u64,
                },
                Some(format!(
                    "contents/0x22/noquill-text-range/{:#x}+{}",
                    absolute_start,
                    remaining.len()
                )),
                Some("legacy_text".into()),
                SourceRole::Semantic,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            )],
        },
    );
    Ok(())
}

fn decode_bounded_legacy_ascii(bytes: &[u8]) -> Result<String> {
    if bytes.iter().any(|byte| *byte >= 0x80) {
        bail!(
            "legacy no-Quill V1 text contains non-ASCII bytes; codepage authority is not admitted"
        );
    }
    String::from_utf8(bytes.to_vec()).context("legacy no-Quill ASCII text")
}

fn derive_legacy_document_id(source_hash: &Sha256Digest, object_id: u16) -> Result<DocumentId> {
    Ok(DocumentId::from_canonical(derive_pub_id(
        source_hash,
        &format!("contents/0x22/object/{object_id}"),
        ROLE_DOCUMENT,
    )?))
}

fn derive_legacy_page_id(source_hash: &Sha256Digest, object_id: u16) -> Result<PageId> {
    Ok(PageId::from_canonical(derive_pub_id(
        source_hash,
        &format!("contents/0x22/object/{object_id}"),
        ROLE_PAGE,
    )?))
}

fn derive_legacy_node_id(source_hash: &Sha256Digest, object_id: u16) -> Result<NodeId> {
    Ok(NodeId::from_canonical(derive_pub_id(
        source_hash,
        &format!("contents/0x22/object/{object_id}"),
        ROLE_NODE,
    )?))
}

fn derive_legacy_story_id(source_hash: &Sha256Digest, owner_id: u16) -> Result<StoryId> {
    Ok(StoryId::from_canonical(derive_pub_id(
        source_hash,
        &format!("contents/0x22/noquill-owner/{owner_id}"),
        ROLE_STORY,
    )?))
}

fn derive_legacy_table_story_id(
    source_hash: &Sha256Digest,
    chunk_id: u16,
    effective_text_id: u32,
) -> Result<StoryId> {
    Ok(StoryId::from_canonical(derive_pub_id(
        source_hash,
        &format!("contents/0x22/table/{chunk_id}/effective-text/{effective_text_id}"),
        ROLE_STORY,
    )?))
}

fn derive_legacy_table_cell_id(
    source_hash: &Sha256Digest,
    chunk_id: u16,
    cell_index: u32,
) -> Result<TableCellId> {
    Ok(TableCellId::from_canonical(derive_pub_id(
        source_hash,
        &format!("contents/0x22/table/{chunk_id}/cell/{cell_index}"),
        "cdm.table_cell",
    )?))
}

fn derive_unowned_text_story_id(
    source_hash: &Sha256Digest,
    absolute_start: usize,
    len: usize,
) -> Result<StoryId> {
    Ok(StoryId::from_canonical(derive_pub_id(
        source_hash,
        &format!("contents/0x22/noquill-text-range/{absolute_start:#x}+{len}"),
        ROLE_STORY,
    )?))
}

fn unique_entry_by_type<'a>(
    directory: &'a Legacy0x22Directory,
    chunk_type: u16,
    label: &str,
) -> Result<&'a Legacy0x22DirectoryEntry> {
    let mut matches = directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == chunk_type);
    let first = matches
        .next()
        .with_context(|| format!("missing legacy {label}"))?;
    if matches.next().is_some() {
        bail!("multiple legacy {label} objects");
    }
    Ok(first)
}

fn chunk_bytes<'a>(contents: &'a [u8], entry: &Legacy0x22DirectoryEntry) -> Result<&'a [u8]> {
    let start =
        usize::try_from(entry.chunk_source.offset).context("legacy chunk offset overflow")?;
    let len = usize::try_from(entry.chunk_source.len).context("legacy chunk length overflow")?;
    let end = start
        .checked_add(len)
        .filter(|end| *end <= contents.len())
        .context("legacy chunk span exceeds Contents")?;
    Ok(&contents[start..end])
}

fn parse_u16_id_list(
    contents: &[u8],
    entry: &Legacy0x22DirectoryEntry,
    label: &str,
) -> Result<LegacyIdList> {
    let chunk_start =
        usize::try_from(entry.chunk_source.offset).context("legacy chunk offset overflow")?;
    let chunk_len =
        usize::try_from(entry.chunk_source.len).context("legacy chunk length overflow")?;
    let chunk_end = chunk_start
        .checked_add(chunk_len)
        .filter(|end| *end <= contents.len())
        .context("legacy chunk span exceeds Contents")?;
    let data_delta = *contents
        .get(chunk_start + 3)
        .with_context(|| format!("{label}: missing dataRelativeOffset"))?;
    let list_start = chunk_start
        .checked_add(usize::from(data_delta))
        .context("legacy list offset overflow")?;
    let header_end = list_start
        .checked_add(LEGACY_LIST_HEADER_SIZE)
        .context("legacy list header overflow")?;
    if header_end > chunk_end {
        bail!("{label}: list header exceeds bounded chunk");
    }

    let count = read_u16(contents, list_start).expect("list header bounds checked");
    let max_count = read_u16(contents, list_start + 2).expect("list header bounds checked");
    let record_size = read_u16(contents, list_start + 4).expect("list header bounds checked");
    if max_count < count {
        bail!("{label}: max_count {max_count} is less than count {count}");
    }
    if record_size != LEGACY_LIST_U16_RECORD_SIZE {
        bail!("{label}: record size {record_size}, expected {LEGACY_LIST_U16_RECORD_SIZE}");
    }
    let payload_start = header_end;
    let payload_len = usize::from(count)
        .checked_mul(usize::from(record_size))
        .context("legacy list payload overflow")?;
    let payload_end = payload_start
        .checked_add(payload_len)
        .context("legacy list payload end overflow")?;
    if payload_end > chunk_end {
        bail!("{label}: list payload exceeds bounded chunk");
    }
    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    Ok(LegacyIdList {
        ids: (0..usize::from(count))
            .map(|index| {
                let offset = payload_start + index * usize::from(record_size);
                (
                    read_u16(contents, offset).expect("legacy list payload bounds checked"),
                    RawSpan {
                        stream: stream.clone(),
                        offset: offset as u64,
                        len: 2,
                    },
                )
            })
            .collect(),
    })
}

fn is_legacy_simple_geometry_shape_type(chunk_type: u16) -> bool {
    LEGACY_SIMPLE_GEOMETRY_SHAPE_TYPES.contains(&chunk_type)
}

fn legacy_shape_bounds(page: &Page, chunk: &[u8], chunk_type: u16) -> Option<RectEmu> {
    let xs = i64::from(read_i32(chunk, LEGACY_SHAPE_XS_OFFSET)?);
    let ys = i64::from(read_i32(chunk, LEGACY_SHAPE_YS_OFFSET)?);
    let xe = i64::from(read_i32(chunk, LEGACY_SHAPE_XE_OFFSET)?);
    let ye = i64::from(read_i32(chunk, LEGACY_SHAPE_YE_OFFSET)?);
    let delta_x = xe.checked_sub(xs)?;
    let delta_y = ye.checked_sub(ys)?;

    let (origin_x, origin_y, width, height) = if chunk_type == 0x0004 {
        if delta_x == 0 && delta_y == 0 {
            return None;
        }
        (
            xs.min(xe),
            ys.min(ye),
            delta_x.checked_abs()?,
            delta_y.checked_abs()?,
        )
    } else if chunk_type == LEGACY_GROUP_TYPE {
        (xs, ys, delta_x, delta_y)
    } else {
        if delta_x <= 0 || delta_y <= 0 {
            return None;
        }
        (xs, ys, delta_x, delta_y)
    };

    let x = page
        .size
        .width
        .get()
        .checked_div(2)?
        .checked_add(origin_x)?;
    let y = page
        .size
        .height
        .get()
        .checked_div(2)?
        .checked_add(origin_y)?;
    Some(RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    ))
}

fn parse_legacy_ole_data_chunk(chunk: &[u8]) -> Option<PubLegacyOleSource> {
    if chunk.len() != LEGACY_OLE_DATA_LEN
        || read_u16(chunk, 0x00)? != LEGACY_OLE_DATA_TYPE
        || read_u16(chunk, 0x02)? != 0x0800
        || read_u32(chunk, 0x04)? != 10
        || read_u16(chunk, 0x08)? != 0x4f4d
        || read_u16(chunk, 0x0c)? != 0
        || read_u16(chunk, 0x10)? != 0
    {
        return None;
    }

    Some(PubLegacyOleSource {
        storage_number: read_u16(chunk, 0x0a)?,
        raw_flag: read_u16(chunk, 0x0e)?,
    })
}

fn legacy_image_wmf_profile(
    contents: &[u8],
    directory: &Legacy0x22Directory,
    image_object_id: u16,
) -> Option<LegacyImageWmfProfile> {
    let mut image_data = directory
        .entries_by_parent_id(image_object_id)
        .filter(|entry| entry.chunk_type == LEGACY_IMAGE_DATA_TYPE);
    let child = image_data.next()?;
    if image_data.next().is_some() {
        return None;
    }

    let chunk = chunk_bytes(contents, child).ok()?;
    let declared_len = usize::try_from(read_u32(chunk, 0x04)?).ok()?;
    let payload_end = LEGACY_IMAGE_DATA_PAYLOAD_OFFSET.checked_add(declared_len)?;
    let payload = chunk.get(LEGACY_IMAGE_DATA_PAYLOAD_OFFSET..payload_end)?;
    let bounded = bounded_wmf_metafile(payload).ok()?;
    let source_len = u64::try_from(bounded.source_len).ok()?;
    let source_offset = child
        .chunk_source
        .offset
        .checked_add(LEGACY_IMAGE_DATA_PAYLOAD_OFFSET as u64)?;

    Some(LegacyImageWmfProfile {
        source: RawSpan {
            stream: child.chunk_source.stream.clone(),
            offset: source_offset,
            len: source_len,
        },
        normalized_bytes: bounded.normalized_bytes,
    })
}

fn legacy_ole_profile(
    contents: &[u8],
    directory: &Legacy0x22Directory,
    parent_object_id: u16,
) -> Option<(PubLegacyOleSource, RawSpan)> {
    let children = directory
        .entries_by_parent_id(parent_object_id)
        .collect::<Vec<_>>();
    if children.len() != 1 || children[0].chunk_type != LEGACY_OLE_DATA_TYPE {
        return None;
    }
    let child = children[0];
    let chunk = chunk_bytes(contents, child).ok()?;
    let profile = parse_legacy_ole_data_chunk(chunk)?;
    Some((profile, child.chunk_source.clone()))
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_decoder_fails_closed_on_unproven_codepage() {
        assert_eq!(
            decode_bounded_legacy_ascii(b"OPEN HOUSE").unwrap(),
            "OPEN HOUSE"
        );
        assert!(decode_bounded_legacy_ascii(&[0x80]).is_err());
    }

    #[test]
    fn unresolved_non_ascii_text_can_be_recorded_without_guessing_unicode() {
        let mut graph = PubSourceGraph::empty(
            SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x22-noquill".into()),
                adapter_version: "test".into(),
                source_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                    .parse()
                    .unwrap(),
            },
            Document {
                id: DocumentId::from_canonical(
                    derive_pub_id(
                        &"0000000000000000000000000000000000000000000000000000000000000000"
                            .parse()
                            .unwrap(),
                        "test/document",
                        ROLE_DOCUMENT,
                    )
                    .unwrap(),
                ),
                format_origin: "pub".into(),
                source_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                    .parse()
                    .unwrap(),
                pages: Vec::new(),
                resources: Vec::new(),
                styles: Vec::new(),
            },
        );
        let mut diagnostics = Vec::new();
        materialize_unowned_text_story(
            &mut graph,
            &"0000000000000000000000000000000000000000000000000000000000000000"
                .parse()
                .unwrap(),
            &StreamPath(CONTENTS_STREAM_PATH.into()),
            b"A\x92B",
            0x120,
            0,
            &mut diagnostics,
        )
        .unwrap();

        assert!(graph.stories.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert!(matches!(
            &diagnostics[0],
            PubBridgeDiagnostic::LegacyTextEncodingUnresolved {
                owner_id: None,
                source,
                high_byte_count: 1,
            } if source.offset == 0x120 && source.len == 3
        ));
    }

    #[test]
    fn legacy_line_bounds_admit_axis_aligned_and_reversed_endpoints() {
        let page_id = PageId::from_canonical(pub_model::CanonicalId::from_bytes([7; 16]));
        let page = Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(10_000), LengthEmu::new(8_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        };

        let mut horizontal = vec![0_u8; 0x20];
        horizontal[LEGACY_SHAPE_XS_OFFSET..LEGACY_SHAPE_XS_OFFSET + 4]
            .copy_from_slice(&(-1_000_i32).to_le_bytes());
        horizontal[LEGACY_SHAPE_YS_OFFSET..LEGACY_SHAPE_YS_OFFSET + 4]
            .copy_from_slice(&(500_i32).to_le_bytes());
        horizontal[LEGACY_SHAPE_XE_OFFSET..LEGACY_SHAPE_XE_OFFSET + 4]
            .copy_from_slice(&(2_000_i32).to_le_bytes());
        horizontal[LEGACY_SHAPE_YE_OFFSET..LEGACY_SHAPE_YE_OFFSET + 4]
            .copy_from_slice(&(500_i32).to_le_bytes());

        let bounds = legacy_shape_bounds(&page, &horizontal, 0x0004).expect("horizontal line");
        assert_eq!(bounds.x.get(), 4_000);
        assert_eq!(bounds.y.get(), 4_500);
        assert_eq!(bounds.width.get(), 3_000);
        assert_eq!(bounds.height.get(), 0);

        let mut reversed = horizontal.clone();
        reversed[LEGACY_SHAPE_XS_OFFSET..LEGACY_SHAPE_XS_OFFSET + 4]
            .copy_from_slice(&(2_000_i32).to_le_bytes());
        reversed[LEGACY_SHAPE_YS_OFFSET..LEGACY_SHAPE_YS_OFFSET + 4]
            .copy_from_slice(&(1_500_i32).to_le_bytes());
        reversed[LEGACY_SHAPE_XE_OFFSET..LEGACY_SHAPE_XE_OFFSET + 4]
            .copy_from_slice(&(-1_000_i32).to_le_bytes());
        reversed[LEGACY_SHAPE_YE_OFFSET..LEGACY_SHAPE_YE_OFFSET + 4]
            .copy_from_slice(&(500_i32).to_le_bytes());

        let bounds = legacy_shape_bounds(&page, &reversed, 0x0004).expect("reversed line");
        assert_eq!(bounds.x.get(), 4_000);
        assert_eq!(bounds.y.get(), 4_500);
        assert_eq!(bounds.width.get(), 3_000);
        assert_eq!(bounds.height.get(), 1_000);
    }

    #[test]
    fn legacy_line_bounds_reject_point_and_other_shapes_keep_rectangle_gate() {
        let page_id = PageId::from_canonical(pub_model::CanonicalId::from_bytes([8; 16]));
        let page = Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(10_000), LengthEmu::new(8_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        };

        let mut chunk = vec![0_u8; 0x20];
        for offset in [
            LEGACY_SHAPE_XS_OFFSET,
            LEGACY_SHAPE_YS_OFFSET,
            LEGACY_SHAPE_XE_OFFSET,
            LEGACY_SHAPE_YE_OFFSET,
        ] {
            chunk[offset..offset + 4].copy_from_slice(&(500_i32).to_le_bytes());
        }
        assert!(legacy_shape_bounds(&page, &chunk, 0x0004).is_none());

        chunk[LEGACY_SHAPE_XE_OFFSET..LEGACY_SHAPE_XE_OFFSET + 4]
            .copy_from_slice(&(1_500_i32).to_le_bytes());
        assert!(legacy_shape_bounds(&page, &chunk, 0x0005).is_none());
    }

    #[test]
    fn legacy_group_bounds_preserve_point_and_signed_extents() {
        let page_id = PageId::from_canonical(pub_model::CanonicalId::from_bytes([9; 16]));
        let page = Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(10_000), LengthEmu::new(8_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        };

        let mut point = vec![0_u8; 0x20];
        for offset in [
            LEGACY_SHAPE_XS_OFFSET,
            LEGACY_SHAPE_YS_OFFSET,
            LEGACY_SHAPE_XE_OFFSET,
            LEGACY_SHAPE_YE_OFFSET,
        ] {
            point[offset..offset + 4].copy_from_slice(&(500_i32).to_le_bytes());
        }
        let bounds =
            legacy_shape_bounds(&page, &point, LEGACY_GROUP_TYPE).expect("point group bounds");
        assert_eq!(bounds.x.get(), 5_500);
        assert_eq!(bounds.y.get(), 4_500);
        assert_eq!(bounds.width.get(), 0);
        assert_eq!(bounds.height.get(), 0);

        let mut reversed = point;
        reversed[LEGACY_SHAPE_XS_OFFSET..LEGACY_SHAPE_XS_OFFSET + 4]
            .copy_from_slice(&(2_000_i32).to_le_bytes());
        reversed[LEGACY_SHAPE_YS_OFFSET..LEGACY_SHAPE_YS_OFFSET + 4]
            .copy_from_slice(&(1_500_i32).to_le_bytes());
        reversed[LEGACY_SHAPE_XE_OFFSET..LEGACY_SHAPE_XE_OFFSET + 4]
            .copy_from_slice(&(-1_000_i32).to_le_bytes());
        reversed[LEGACY_SHAPE_YE_OFFSET..LEGACY_SHAPE_YE_OFFSET + 4]
            .copy_from_slice(&(500_i32).to_le_bytes());

        let bounds =
            legacy_shape_bounds(&page, &reversed, LEGACY_GROUP_TYPE).expect("signed group bounds");
        assert_eq!(bounds.x.get(), 7_000);
        assert_eq!(bounds.y.get(), 5_500);
        assert_eq!(bounds.width.get(), -3_000);
        assert_eq!(bounds.height.get(), -1_000);
    }

    #[test]
    fn legacy_image_profile_uses_direct_image_data_and_bounded_first_eof() {
        let stream = StreamPath(CONTENTS_STREAM_PATH.into());
        let span = |offset: u64, len: u64| RawSpan {
            stream: stream.clone(),
            offset,
            len,
        };
        let directory = Legacy0x22Directory {
            trailer_offset: 96,
            trailer_offset_source: span(96, 4),
            entry_count: 2,
            entry_count_source: span(96, 2),
            entries: vec![
                Legacy0x22DirectoryEntry {
                    directory_index: 0,
                    entry_source: span(98, 10),
                    service_word: 0,
                    service_word_source: span(98, 2),
                    object_id: 10,
                    object_id_source: span(100, 2),
                    parent_id: 1,
                    parent_id_source: span(102, 2),
                    chunk_offset: 0,
                    chunk_offset_source: span(104, 4),
                    chunk_type: LEGACY_IMAGE_TYPE,
                    chunk_type_source: span(0, 2),
                    chunk_source: span(0, 32),
                },
                Legacy0x22DirectoryEntry {
                    directory_index: 1,
                    entry_source: span(108, 10),
                    service_word: 0,
                    service_word_source: span(108, 2),
                    object_id: 11,
                    object_id_source: span(110, 2),
                    parent_id: 10,
                    parent_id_source: span(112, 2),
                    chunk_offset: 32,
                    chunk_offset_source: span(114, 4),
                    chunk_type: LEGACY_IMAGE_DATA_TYPE,
                    chunk_type_source: span(32, 2),
                    chunk_source: span(32, 34),
                },
            ],
        };

        let mut wmf = Vec::new();
        wmf.extend_from_slice(&1u16.to_le_bytes());
        wmf.extend_from_slice(&9u16.to_le_bytes());
        wmf.extend_from_slice(&0x0300u16.to_le_bytes());
        wmf.extend_from_slice(&11u32.to_le_bytes()); // stale: actual first-EOF size is 12 words
        wmf.extend_from_slice(&0u16.to_le_bytes());
        wmf.extend_from_slice(&3u32.to_le_bytes());
        wmf.extend_from_slice(&0u16.to_le_bytes());
        wmf.extend_from_slice(&3u32.to_le_bytes());
        wmf.extend_from_slice(&0u16.to_le_bytes());

        let mut contents = vec![0u8; 66];
        contents[0..2].copy_from_slice(&LEGACY_IMAGE_TYPE.to_le_bytes());
        contents[32..34].copy_from_slice(&LEGACY_IMAGE_DATA_TYPE.to_le_bytes());
        contents[36..40].copy_from_slice(&(wmf.len() as u32).to_le_bytes());
        contents[40..64].copy_from_slice(&wmf);
        contents[64..66].copy_from_slice(&[0xaa, 0xbb]);

        let profile = legacy_image_wmf_profile(&contents, &directory, 10).expect("image WMF");
        assert_eq!(profile.source.offset, 40);
        assert_eq!(profile.source.len, 24);
        assert_eq!(read_u32(&profile.normalized_bytes, 6), Some(12));
        assert!(crate::wmf::validate_wmf_metafile(&profile.normalized_bytes).is_ok());
        assert!(legacy_image_wmf_profile(&contents, &directory, 11).is_none());
    }

    #[test]
    fn legacy_ole_data_profile_is_exact_and_preserves_opaque_flag() {
        let mut chunk = vec![0_u8; LEGACY_OLE_DATA_LEN];
        chunk[0x00..0x02].copy_from_slice(&LEGACY_OLE_DATA_TYPE.to_le_bytes());
        chunk[0x02..0x04].copy_from_slice(&0x0800_u16.to_le_bytes());
        chunk[0x04..0x08].copy_from_slice(&10_u32.to_le_bytes());
        chunk[0x08..0x0a].copy_from_slice(&0x4f4d_u16.to_le_bytes());
        chunk[0x0a..0x0c].copy_from_slice(&73_u16.to_le_bytes());
        chunk[0x0c..0x0e].copy_from_slice(&0_u16.to_le_bytes());
        chunk[0x0e..0x10].copy_from_slice(&0x8000_u16.to_le_bytes());
        chunk[0x10..0x12].copy_from_slice(&0_u16.to_le_bytes());

        assert_eq!(
            parse_legacy_ole_data_chunk(&chunk),
            Some(PubLegacyOleSource {
                storage_number: 73,
                raw_flag: 0x8000,
            })
        );

        chunk[0x08] ^= 0x01;
        assert!(parse_legacy_ole_data_chunk(&chunk).is_none());
    }

    #[test]
    fn simple_geometry_shape_admission_is_bounded() {
        for chunk_type in LEGACY_SIMPLE_GEOMETRY_SHAPE_TYPES {
            assert!(is_legacy_simple_geometry_shape_type(chunk_type));
        }

        for chunk_type in [
            LEGACY_TEXT_SHAPE_TYPE,
            LEGACY_OLE_TYPE, // OLE is admitted only through its bounded OleData profile
            LEGACY_0X22_TABLE_CHUNK_TYPE, // low/no-Quill table, admitted separately
            LEGACY_IMAGE_TYPE, // image is admitted only through direct bounded IMAGE_2K_DATA
            0x0008,          // Quill-era text shape
            0x000a,          // table
            LEGACY_GROUP_TYPE, // group is admitted separately from simple geometry
            LEGACY_PAGE_TYPE,
            LEGACY_DOCUMENT_TYPE,
        ] {
            assert!(!is_legacy_simple_geometry_shape_type(chunk_type));
        }
    }

    #[test]
    fn group_admission_is_separate_and_bounded() {
        assert_eq!(LEGACY_GROUP_TYPE, 0x000f);
        assert!(!is_legacy_simple_geometry_shape_type(LEGACY_GROUP_TYPE));
        assert_eq!(LEGACY_GROUP_MAX_DEPTH, 100);
    }

    #[test]
    fn group_materialization_preserves_parent_hierarchy() {
        let source_hash: Sha256Digest =
            "0000000000000000000000000000000000000000000000000000000000000000"
                .parse()
                .unwrap();
        let document_id = DocumentId::from_canonical(
            derive_pub_id(&source_hash, "test/document", ROLE_DOCUMENT).unwrap(),
        );
        let page_id = derive_legacy_page_id(&source_hash, 1).unwrap();
        let page = Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(1000), LengthEmu::new(1000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        };
        let source = SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x22-noquill".into()),
            adapter_version: "test".into(),
            source_hash,
        };
        let mut graph = PubSourceGraph::empty(
            source,
            Document {
                id: document_id,
                format_origin: "pub".into(),
                source_hash,
                pages: vec![page_id],
                resources: Vec::new(),
                styles: Vec::new(),
            },
        );
        graph.pages.insert(page_id, page.clone());

        let stream = StreamPath(CONTENTS_STREAM_PATH.into());
        let span = |offset: u64, len: u64| RawSpan {
            stream: stream.clone(),
            offset,
            len,
        };
        let directory = Legacy0x22Directory {
            trailer_offset: 96,
            trailer_offset_source: span(96, 4),
            entry_count: 2,
            entry_count_source: span(96, 2),
            entries: vec![
                Legacy0x22DirectoryEntry {
                    directory_index: 0,
                    entry_source: span(98, 10),
                    service_word: 0,
                    service_word_source: span(98, 2),
                    object_id: 10,
                    object_id_source: span(100, 2),
                    parent_id: 1,
                    parent_id_source: span(102, 2),
                    chunk_offset: 0,
                    chunk_offset_source: span(104, 4),
                    chunk_type: LEGACY_GROUP_TYPE,
                    chunk_type_source: span(0, 2),
                    chunk_source: span(0, 32),
                },
                Legacy0x22DirectoryEntry {
                    directory_index: 1,
                    entry_source: span(108, 10),
                    service_word: 0,
                    service_word_source: span(108, 2),
                    object_id: 11,
                    object_id_source: span(110, 2),
                    parent_id: 10,
                    parent_id_source: span(112, 2),
                    chunk_offset: 32,
                    chunk_offset_source: span(114, 4),
                    chunk_type: 0x0005,
                    chunk_type_source: span(32, 2),
                    chunk_source: span(32, 32),
                },
            ],
        };
        let mut contents = vec![0_u8; 64];
        contents[0..2].copy_from_slice(&LEGACY_GROUP_TYPE.to_le_bytes());
        contents[32..34].copy_from_slice(&0x0005_u16.to_le_bytes());
        for base in [0_usize, 32] {
            contents[base + LEGACY_SHAPE_XS_OFFSET..base + LEGACY_SHAPE_XS_OFFSET + 4]
                .copy_from_slice(&(-100_i32).to_le_bytes());
            contents[base + LEGACY_SHAPE_YS_OFFSET..base + LEGACY_SHAPE_YS_OFFSET + 4]
                .copy_from_slice(&(-100_i32).to_le_bytes());
            contents[base + LEGACY_SHAPE_XE_OFFSET..base + LEGACY_SHAPE_XE_OFFSET + 4]
                .copy_from_slice(&(100_i32).to_le_bytes());
            contents[base + LEGACY_SHAPE_YE_OFFSET..base + LEGACY_SHAPE_YE_OFFSET + 4]
                .copy_from_slice(&(100_i32).to_le_bytes());
        }

        let mut diagnostics = Vec::new();
        let mut group_stack = BTreeSet::new();
        materialize_legacy_noquill_child(
            &mut graph,
            &source_hash,
            &contents,
            &stream,
            &directory,
            &page,
            1,
            page_id.into_canonical(),
            10,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut diagnostics,
            &mut group_stack,
            0,
        )
        .unwrap();

        assert!(diagnostics.is_empty());
        let group_id = derive_legacy_node_id(&source_hash, 10).unwrap();
        let child_id = derive_legacy_node_id(&source_hash, 11).unwrap();
        assert_eq!(graph.nodes[&group_id].kind, NodeKind::Group);
        assert_eq!(
            graph.nodes[&group_id].header.parent_id,
            page_id.into_canonical()
        );
        assert_eq!(graph.nodes[&child_id].kind, NodeKind::Shape);
        assert_eq!(
            graph.nodes[&child_id].header.parent_id,
            group_id.into_canonical()
        );
    }
}
