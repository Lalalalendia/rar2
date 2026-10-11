use super::*;

const RAW_TYPE_LOGO: u16 = 0x31;

pub(super) fn build_reference_index(
    contents: &[u8],
    directory: &pub_contents::Contents0x2cDirectory,
) -> Result<BTreeMap<u32, Contents0x2cChunkReference>> {
    let mut references = BTreeMap::new();

    for seq_num in 0..directory.slots.len() {
        let Some(reference) = parse_confirmed_chunk_reference(contents, directory, seq_num)
            .with_context(|| format!("parse Contents directory reference seq {seq_num}"))?
        else {
            continue;
        };
        let key = seq_u32(reference.seq_num)?;
        if references.insert(key, reference).is_some() {
            bail!("duplicate Contents directory seq {key}");
        }
    }

    Ok(references)
}

pub(super) fn unique_reference_by_raw_type<'a>(
    references: &'a BTreeMap<u32, Contents0x2cChunkReference>,
    raw_type: u16,
    label: &str,
) -> Result<&'a Contents0x2cChunkReference> {
    let mut matches = references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(raw_type));
    let first = matches
        .next()
        .with_context(|| format!("missing {label} raw type 0x{raw_type:02X}"))?;
    if matches.next().is_some() {
        bail!("multiple {label} raw type 0x{raw_type:02X} objects");
    }
    Ok(first)
}

pub(super) fn chunk_for_reference(
    stream: StreamPath,
    contents: &[u8],
    reference: &Contents0x2cChunkReference,
) -> Result<Contents0x2cChunk> {
    let offset = reference.required_chunk_offset()?;
    parse_confirmed_0x2c_chunk(stream, contents, offset.value)
        .with_context(|| format!("parse Contents chunk seq {}", reference.seq_num))
}

/// Missing directory evidence is observable even for unclassified occupied slots.
/// These diagnostics make no claim about slot semantics or document health.
pub(super) fn reference_diagnostics(
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
) -> Vec<PubBridgeDiagnostic> {
    let mut diagnostics = Vec::new();
    for (&seq_num, reference) in references {
        if reference.raw_types.is_empty() {
            diagnostics.push(PubBridgeDiagnostic::MissingRawType {
                seq_num,
                source: reference.source.clone(),
                parent_seq_num: single_parent_seq(reference),
            });
        }
        if reference.chunk_offsets.is_empty() {
            diagnostics.push(PubBridgeDiagnostic::MissingChunkOffset {
                seq_num,
                source: reference.source.clone(),
                parent_seq_num: single_parent_seq(reference),
            });
        }
    }
    diagnostics
}

pub(super) fn unique_block(chunk: &Contents0x2cChunk, id: u16) -> Result<&RawContentsBlock> {
    let mut matches = chunk.fields.iter().filter(|field| field.id == id);
    let first = matches
        .next()
        .with_context(|| format!("missing Contents field 0x{id:02X}"))?;
    if matches.next().is_some() {
        bail!("duplicate Contents field 0x{id:02X}");
    }
    Ok(first)
}

pub(super) fn single_raw_type(reference: &Contents0x2cChunkReference) -> Option<u16> {
    reference.required_raw_type().ok().map(|field| field.value)
}

pub(super) fn single_parent_seq(reference: &Contents0x2cChunkReference) -> Option<u32> {
    match reference.parent_seq_nums.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn bounded_page_logo_context(
    raw_type: Option<u16>,
    parent_raw_type: Option<u16>,
    registered_page: bool,
) -> bool {
    raw_type == Some(RAW_TYPE_LOGO) && parent_raw_type == Some(RAW_TYPE_PAGE) && registered_page
}

/// LOGO (0x31) is context-overloaded. Only mature PAGE-owned carriers whose
/// parent is an actual registered 0x43 PAGE may join the bounded grouped
/// projection; never promote 0x31 solely by its raw type.
pub(super) fn is_page_owned_logo_group(
    group_seq: u32,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &BTreeMap<u32, PageId>,
) -> bool {
    let Some(reference) = references.get(&group_seq) else {
        return false;
    };
    let Some(parent_seq) = single_parent_seq(reference) else {
        return false;
    };
    bounded_page_logo_context(
        single_raw_type(reference),
        references.get(&parent_seq).and_then(single_raw_type),
        page_seq_to_id.contains_key(&parent_seq),
    )
}

pub(super) fn seq_u32(seq_num: usize) -> Result<u32> {
    u32::try_from(seq_num).map_err(|_| anyhow!("Contents seqNum does not fit u32: {seq_num}"))
}

pub(super) fn unique_u32_field(
    chunk: &Contents0x2cChunk,
    id: u16,
) -> Result<Option<(u32, RawSpan)>> {
    let mut matches = chunk.fields.iter().filter(|field| field.id == id);
    let Some(field) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        bail!("duplicate Contents field 0x{id:02X} in one chunk");
    }

    match &field.body {
        RawContentsBlockBody::U32 {
            value,
            value_source,
        } => Ok(Some((*value, value_source.clone()))),
        _ => bail!(
            "Contents field 0x{id:02X} at {} is not a confirmed u32/reference body",
            field.source.offset
        ),
    }
}

#[cfg(test)]
mod missing_directory_fields_contract {
    use super::*;
    use pub_contents::{ChunkReadError, ChunkReferenceReadError, encode_packed_field_tag};

    fn synthetic_slot(fields: &[(u16, u8, &[u8])]) -> Vec<u8> {
        let mut payload = Vec::new();
        for &(id, wire, value) in fields {
            payload.extend(encode_packed_field_tag(id, wire).unwrap());
            payload.extend(value);
        }
        let mut bytes = vec![0x00, 0x88];
        bytes.extend(((payload.len() + 4) as u32).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn synthetic_reference(fields: &[(u16, u8, &[u8])]) -> Contents0x2cChunkReference {
        let mut bytes = vec![0x00, 0x78];
        bytes.extend(synthetic_slot(fields));
        let source = RawSpan {
            stream: StreamPath("/Synthetic".into()),
            offset: 0,
            len: bytes.len() as u64,
        };
        let directory = pub_contents::parse_confirmed_0x2c_directory(&bytes, source).unwrap();
        build_reference_index(&bytes, &directory)
            .unwrap()
            .remove(&1)
            .unwrap()
    }

    #[test]
    fn publication_bridge_reports_unknown_slot_absence_without_dropping_other_warnings() {
        // Synthetic DOCUMENT PageList -> PAGE and Margins extent, using the
        // existing confirmed wire grammar. No private document bytes are inputs.
        let mut bytes = vec![
            16, 0, 0, 0, 0x02, 0xA0, 10, 0, 0, 0, 0x00, 0x70, 1, 0, 0, 0, 22, 0, 0, 0, 0x03, 0x88,
            16, 0, 0, 0, 0x01, 0x20, 0x40, 0x5B, 0x73, 0, 0x02, 0x20, 0xA0, 0x25, 0xA3, 0,
        ];
        let directory_start = bytes.len();
        for (raw_type, offset) in [
            (RAW_TYPE_DOCUMENT, 0u32),
            (RAW_TYPE_PAGE, 0),
            (RAW_TYPE_MARGINS, 16),
        ] {
            bytes.extend(synthetic_slot(&[
                (0x02, 0x18, &raw_type.to_le_bytes()),
                (0x04, 0xB8, &offset.to_le_bytes()),
            ]));
        }
        bytes.extend(synthetic_slot(&[
            (0x01, 0x10, &7u16.to_le_bytes()),
            (0x05, 0x68, &1u32.to_le_bytes()),
            (0x06, 0x08, &[]),
        ]));
        let stream = StreamPath("/Synthetic".into());
        let directory = pub_contents::parse_confirmed_0x2c_directory(
            &bytes,
            RawSpan {
                stream: stream.clone(),
                offset: directory_start as u64,
                len: (bytes.len() - directory_start) as u64,
            },
        )
        .unwrap();
        let mut references = build_reference_index(&bytes, &directory).unwrap();
        let hash = Sha256Digest::from_bytes([0x24; 32]);
        let source = SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "synthetic".into(),
            source_hash: hash,
        };
        let built = materialize_publication_document(
            hash,
            source.clone(),
            stream.clone(),
            &bytes,
            &references,
        )
        .unwrap();
        assert_eq!(built.graph.pages.len(), 1);
        assert!(built.diagnostics.iter().any(|diagnostic| matches!(diagnostic,
            PubBridgeDiagnostic::MissingRawType { seq_num: 3, parent_seq_num: Some(1), source } if source.len == 18)));
        assert!(built.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            PubBridgeDiagnostic::MissingChunkOffset { seq_num: 3, .. }
        )));
        references.remove(&3);
        let baseline =
            materialize_publication_document(hash, source, stream, &bytes, &references).unwrap();
        assert_eq!(built.graph.document, baseline.graph.document);
        let unrelated = built
            .diagnostics
            .into_iter()
            .filter(|diagnostic| {
                !matches!(
                    diagnostic,
                    PubBridgeDiagnostic::MissingRawType { .. }
                        | PubBridgeDiagnostic::MissingChunkOffset { .. }
                )
            })
            .collect::<Vec<_>>();
        assert!(!baseline.diagnostics.is_empty());
        assert_eq!(unrelated, baseline.diagnostics);
    }

    #[test]
    fn missing_fields_runtime_receipt_distinguishes_absence_from_physical_range_error() {
        let absent = synthetic_reference(&[
            (0x01, 0x10, &7u16.to_le_bytes()),
            (0x05, 0x68, &23u32.to_le_bytes()),
            (0x06, 0x08, &[]),
        ]);
        assert_eq!(absent.source.len, 18);
        let diagnostics = reference_diagnostics(&BTreeMap::from([(1, absent.clone())]));
        assert_eq!(diagnostics.len(), 2);
        let serialized = serde_json::to_value(&diagnostics).unwrap();
        assert_eq!(serialized[0]["code"], "missing_raw_type");
        assert_eq!(serialized[1]["code"], "missing_chunk_offset");
        for diagnostic in serialized.as_array().unwrap() {
            assert_eq!(diagnostic["seq_num"], 1);
            assert_eq!(diagnostic["source"]["offset"], 2);
            assert_eq!(diagnostic["source"]["len"], 18);
            assert_eq!(diagnostic["parent_seq_num"], 23);
            assert!(diagnostic.get("chunk_offset").is_none());
            assert!(diagnostic.get("health").is_none());
        }
        let roundtrip: Vec<PubBridgeDiagnostic> =
            serde_json::from_value(serialized.clone()).unwrap();
        assert_eq!(roundtrip, diagnostics);
        let missing =
            chunk_for_reference(StreamPath("/Synthetic".into()), &[], &absent).unwrap_err();
        assert!(matches!(missing.downcast_ref::<ChunkReferenceReadError>(),
            Some(ChunkReferenceReadError::MissingChunkOffset { seq_num: 1, source })
                if source == &absent.source));
        assert!(missing.downcast_ref::<ChunkReadError>().is_none());

        let chunk = 4u32.to_le_bytes();
        let present = synthetic_reference(&[
            (0x02, 0x18, &0x43u16.to_le_bytes()),
            (0x04, 0xB8, &0u32.to_le_bytes()),
        ]);
        let present_diagnostics = reference_diagnostics(&BTreeMap::from([(1, present.clone())]));
        assert!(present_diagnostics.is_empty());
        let decoded =
            chunk_for_reference(StreamPath("/Synthetic".into()), &chunk, &present).unwrap();
        assert!(decoded.is_fully_decoded());

        let invalid = synthetic_reference(&[
            (0x02, 0x18, &0x43u16.to_le_bytes()),
            (0x04, 0xB8, &u32::MAX.to_le_bytes()),
        ]);
        assert!(reference_diagnostics(&BTreeMap::from([(1, invalid.clone())])).is_empty());
        let range =
            chunk_for_reference(StreamPath("/Synthetic".into()), &chunk, &invalid).unwrap_err();
        let Some(ChunkReadError::DeclaredRangeOutOfBounds {
            offset,
            declared_length,
            stream_len,
        }) = range.downcast_ref::<ChunkReadError>()
        else {
            panic!("explicit invalid offset lost physical range error: {range}")
        };
        assert_eq!(*offset, u32::MAX);
        assert_eq!(*stream_len, 4);
        assert!(range.downcast_ref::<ChunkReferenceReadError>().is_none());

        // The existing warning receipt consumes actual serialized Reader output.
        // No runtime file is produced unless the acceptance workflow requests it.
        if let Some(path) = std::env::var_os("CHAPTERA_PARSER_WARNING_RUNTIME_RECEIPT") {
            let receipt = serde_json::json!({
                "schema": "chaptera.contents-missing-directory-fields.runtime.v1",
                "diagnostics": serialized,
                "present_valid": {"diagnostic_count": present_diagnostics.len(), "chunk_offset": decoded.source.offset, "fully_decoded": decoded.is_fully_decoded()},
                "present_invalid": {"code": "declared_range_out_of_bounds", "offset": offset, "declared_length": declared_length, "stream_len": stream_len},
                "source_free": true,
            });
            std::fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        }
    }

    #[test]
    fn independent_absence_and_duplicates_keep_existing_reader_disposition() {
        for (fields, expected_code) in [
            (
                vec![(0x02, 0x18, 0x43u16.to_le_bytes().to_vec())],
                "missing_chunk_offset",
            ),
            (
                vec![(0x04, 0xB8, 0u32.to_le_bytes().to_vec())],
                "missing_raw_type",
            ),
        ] {
            let fields = fields
                .iter()
                .map(|(id, wire, bytes)| (*id, *wire, bytes.as_slice()))
                .collect::<Vec<_>>();
            let reference = synthetic_reference(&fields);
            let diagnostics = reference_diagnostics(&BTreeMap::from([(1, reference)]));
            assert_eq!(diagnostics.len(), 1);
            let value = serde_json::to_value(&diagnostics[0]).unwrap();
            assert_eq!(value["code"], expected_code);
            assert!(value.get("parent_seq_num").is_none());
        }
        let duplicates = synthetic_reference(&[
            (0x02, 0x18, &0x43u16.to_le_bytes()),
            (0x02, 0x18, &0x43u16.to_le_bytes()),
            (0x04, 0xB8, &0u32.to_le_bytes()),
            (0x04, 0xB8, &0u32.to_le_bytes()),
        ]);
        assert_eq!(single_raw_type(&duplicates), None);
        assert!(reference_diagnostics(&BTreeMap::from([(1, duplicates.clone())])).is_empty());
        let error = chunk_for_reference(
            StreamPath("/Synthetic".into()),
            &4u32.to_le_bytes(),
            &duplicates,
        )
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ChunkReferenceReadError>(),
            Some(ChunkReferenceReadError::AmbiguousChunkOffset { count: 2, .. })
        ));
    }
}

#[cfg(test)]
mod page_logo_admission_tests {
    use super::*;

    #[test]
    fn only_registered_page_owned_logo_context_is_eligible() {
        assert!(bounded_page_logo_context(
            Some(RAW_TYPE_LOGO),
            Some(RAW_TYPE_PAGE),
            true
        ));
        for (raw_type, parent_type, is_page) in [
            (Some(RAW_TYPE_LOGO), Some(RAW_TYPE_PAGE), false),
            (Some(RAW_TYPE_LOGO), Some(RAW_TYPE_GROUP), true),
            (Some(RAW_TYPE_LOGO), None, true),
            (Some(RAW_TYPE_GROUP), Some(RAW_TYPE_PAGE), true),
            (Some(RAW_TYPE_SHAPE), Some(RAW_TYPE_PAGE), true),
            (None, Some(RAW_TYPE_PAGE), true),
        ] {
            assert!(!bounded_page_logo_context(raw_type, parent_type, is_page));
        }
    }
}
