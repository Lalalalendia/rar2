use super::*;

pub(super) struct PublicationDocumentBootstrap {
    pub(super) graph: PubSourceGraph,
    pub(super) effective_pages: PubEffectivePageProjection,
    pub(super) page_seq_to_id: BTreeMap<u32, PageId>,
    pub(super) color_scheme: Option<PubPublicationColorScheme>,
    pub(super) diagnostics: Vec<PubBridgeDiagnostic>,
}

#[derive(Debug, Clone)]
pub(super) struct PubPublicationColorScheme {
    pub(super) seq_num: u32,
    pub(super) scheme: MatureColorScheme,
}

pub(super) fn materialize_publication_document(
    source_hash: Sha256Digest,
    source: SourceDescriptor,
    contents_stream: StreamPath,
    contents: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
) -> Result<PublicationDocumentBootstrap> {
    let document_reference =
        unique_reference_by_raw_type(references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_seq = seq_u32(document_reference.seq_num)?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), contents, document_reference)?;
    let page_list_block = unique_block(&document_chunk, DOCUMENT_PAGE_LIST_ID)?.clone();
    let page_list = parse_confirmed_document_page_list(contents, page_list_block)
        .context("parse DOCUMENT PageList")?;

    let mut diagnostics = Vec::new();
    let color_scheme =
        match current_publication_color_scheme(contents_stream.clone(), contents, references) {
            Ok(scheme) => scheme,
            Err(error) => {
                diagnostics.push(PubBridgeDiagnostic::ColorSchemeProjectionUnavailable {
                    reason: error.to_string(),
                });
                None
            }
        };
    let (page_width_emu, page_height_emu, margins_count) =
        consensus_publication_page_extent(contents_stream.clone(), contents, references)?;
    if margins_count > 1 {
        diagnostics.push(PubBridgeDiagnostic::EquivalentMarginsPageExtents {
            count: margins_count,
            width_emu: page_width_emu,
            height_emu: page_height_emu,
        });
    }

    let document_id = derive_pub_document_id(&source_hash, document_seq)?;
    let mut document_pages = Vec::new();
    let mut pages = BTreeMap::new();
    let mut page_seq_to_id = BTreeMap::new();
    let mut seen_page_handles = BTreeSet::new();

    for entry in &page_list.entries {
        let Some(reference) = references.get(&entry.handle) else {
            diagnostics.push(PubBridgeDiagnostic::PageListUnknownEntry {
                handle: entry.handle,
                raw_type: None,
            });
            continue;
        };

        match single_raw_type(reference) {
            Some(RAW_TYPE_PAGE) => {
                if !seen_page_handles.insert(entry.handle) {
                    bail!("DOCUMENT PageList repeats PAGE handle {}", entry.handle);
                }
                let page_id = derive_pub_page_id(&source_hash, entry.handle)?;
                document_pages.push(page_id);
                page_seq_to_id.insert(entry.handle, page_id);
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
            Some(RAW_TYPE_PAGE_LIST_SPECIAL) => {
                diagnostics.push(PubBridgeDiagnostic::PageListSpecialEntry {
                    handle: entry.handle,
                    raw_type: RAW_TYPE_PAGE_LIST_SPECIAL,
                });
            }
            other => {
                diagnostics.push(PubBridgeDiagnostic::PageListUnknownEntry {
                    handle: entry.handle,
                    raw_type: other,
                });
            }
        }
    }

    if pages.is_empty() {
        bail!("DOCUMENT PageList exposes no confirmed PAGE 0x43 entries");
    }

    let (effective_pages, page_projection_diagnostics) = derive_effective_page_projection(
        contents_stream,
        contents,
        references,
        &page_seq_to_id,
        &document_pages,
    );
    diagnostics.extend(page_projection_diagnostics);

    let document = Document {
        id: document_id,
        format_origin: "pub".into(),
        source_hash,
        pages: document_pages,
        resources: Vec::new(),
        styles: Vec::new(),
    };
    let mut graph = PubSourceGraph::empty(source, document);
    graph.pages = pages;

    Ok(PublicationDocumentBootstrap {
        graph,
        effective_pages,
        page_seq_to_id,
        color_scheme,
        diagnostics,
    })
}

fn current_publication_color_scheme(
    stream: StreamPath,
    contents: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
) -> Result<Option<PubPublicationColorScheme>> {
    let matches = references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(RAW_TYPE_COLOR_SCHEME))
        .collect::<Vec<_>>();

    let reference = match matches.as_slice() {
        [] => return Ok(None),
        [reference] => *reference,
        many => bail!(
            "multiple OplSccm/current ColorScheme raw type 0x{RAW_TYPE_COLOR_SCHEME:02X} objects: {}",
            many.len()
        ),
    };

    let seq_num = seq_u32(reference.seq_num)?;
    let chunk = chunk_for_reference(stream, contents, reference)?;
    let scheme = parse_confirmed_mature_color_scheme(contents, &chunk)
        .with_context(|| format!("parse OplSccm current ColorScheme seq {seq_num}"))?;
    Ok(Some(PubPublicationColorScheme { seq_num, scheme }))
}

fn consensus_publication_page_extent(
    stream: StreamPath,
    contents: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
) -> Result<(u32, u32, usize)> {
    let margins = references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(RAW_TYPE_MARGINS))
        .collect::<Vec<_>>();

    if margins.is_empty() {
        bail!("missing Margins/OplMg raw type 0x{RAW_TYPE_MARGINS:02X}");
    }

    let mut dimensions = Vec::with_capacity(margins.len());
    for reference in margins {
        let chunk = chunk_for_reference(stream.clone(), contents, reference)?;
        let extent = parse_confirmed_margins_page_extent(contents, &chunk).with_context(|| {
            format!("parse Margins/OplMg page extent seq {}", reference.seq_num)
        })?;
        dimensions.push((extent.width_emu, extent.height_emu));
    }

    let (width_emu, height_emu) = require_consensus_page_extent(&dimensions)?;
    Ok((width_emu, height_emu, dimensions.len()))
}

fn require_consensus_page_extent(extents: &[(u32, u32)]) -> Result<(u32, u32)> {
    let first = extents
        .first()
        .copied()
        .context("publication has no confirmed Margins/OplMg page extent")?;
    if first.0 == 0 || first.1 == 0 {
        bail!("publication page extent must be positive");
    }

    for &(width_emu, height_emu) in &extents[1..] {
        if width_emu == 0 || height_emu == 0 {
            bail!("publication page extent must be positive");
        }
        if (width_emu, height_emu) != first {
            bail!(
                "conflicting Margins/OplMg page extents: expected {}x{} EMU, found {}x{} EMU",
                first.0,
                first.1,
                width_emu,
                height_emu
            );
        }
    }

    Ok(first)
}
