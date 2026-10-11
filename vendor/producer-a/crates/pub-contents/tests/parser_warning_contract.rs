use pub_contents::{
    BlockReadError, ChunkReadError, ChunkReferenceReadError, Contents0x2cChunkReference,
    ContentsCursor, DirectoryReadError, encode_packed_field_tag, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_directory, parse_confirmed_block, parse_confirmed_chunk_reference,
};
use pub_core::{RawSpan, StreamPath};

fn span(len: u64) -> RawSpan {
    RawSpan {
        stream: StreamPath("/Synthetic".into()),
        offset: 0,
        len,
    }
}

fn reference_fields(fields: &[(u16, u8, &[u8])]) -> (Vec<u8>, Contents0x2cChunkReference) {
    // One empty slot before the occupied slot proves positional seqNum and
    // nonzero provenance. Payload values are synthetic, not private corpus bytes.
    let mut payload = Vec::new();
    for &(id, wire, value) in fields {
        payload.extend(encode_packed_field_tag(id, wire).unwrap());
        payload.extend(value);
    }
    let mut bytes = vec![0x00, 0x78, 0x00, 0x88];
    bytes.extend(((payload.len() + 4) as u32).to_le_bytes());
    bytes.extend(payload);
    let directory = parse_confirmed_0x2c_directory(&bytes, span(bytes.len() as u64)).unwrap();
    let reference = parse_confirmed_chunk_reference(&bytes, &directory, 1)
        .unwrap()
        .unwrap();
    (bytes, reference)
}

#[test]
fn missing_directory_fields_18_byte_slot_preserves_raw_fields_and_parent() {
    let (bytes, reference) = reference_fields(&[
        (0x01, 0x10, &7u16.to_le_bytes()),
        (0x05, 0x68, &23u32.to_le_bytes()),
        (0x06, 0x08, &[]),
    ]);
    assert_eq!(
        reference.source,
        RawSpan {
            offset: 2,
            len: 18,
            ..span(18)
        }
    );
    assert_eq!(&bytes[2..], &bytes[reference.source.offset as usize..]);
    assert_eq!(reference.fields.len(), 3);
    assert_eq!(
        reference
            .fields
            .iter()
            .map(|field| field.source.len)
            .sum::<u64>(),
        12
    );
    assert_eq!(reference.fields[0].source.offset, 8);
    assert_eq!(
        reference.fields[2].source.offset + reference.fields[2].source.len,
        20
    );
    assert_eq!(reference.parent_seq_nums.len(), 1);
    assert_eq!(reference.parent_seq_nums[0].value, 23);
    assert_eq!(reference.parent_seq_nums[0].source.offset, 14);
    assert!(matches!(reference.required_raw_type(),
        Err(ChunkReferenceReadError::MissingRawType { seq_num: 1, source })
            if source == reference.source));
    assert!(matches!(reference.required_chunk_offset(),
        Err(ChunkReferenceReadError::MissingChunkOffset { seq_num: 1, source })
            if source == reference.source));
    // The source span still selects every original byte; no normalized slot is written.
    let directory = parse_confirmed_0x2c_directory(&bytes, span(bytes.len() as u64)).unwrap();
    assert_eq!(
        parse_confirmed_chunk_reference(&bytes, &directory, 1).unwrap(),
        Some(reference)
    );
}

#[test]
fn missing_directory_fields_are_independent_and_optional_parent_is_not_synthesized() {
    let (_, type_only) = reference_fields(&[(0x02, 0x18, &0x43u16.to_le_bytes())]);
    assert_eq!(type_only.required_raw_type().unwrap().value, 0x43);
    assert!(matches!(
        type_only.required_chunk_offset(),
        Err(ChunkReferenceReadError::MissingChunkOffset { .. })
    ));
    let (_, offset_only) = reference_fields(&[(0x04, 0xB8, &0u32.to_le_bytes())]);
    assert_eq!(offset_only.required_chunk_offset().unwrap().value, 0);
    assert!(matches!(
        offset_only.required_raw_type(),
        Err(ChunkReferenceReadError::MissingRawType { .. })
    ));
    assert!(type_only.parent_seq_nums.is_empty());
    assert!(offset_only.parent_seq_nums.is_empty());
}

#[test]
fn noncanonical_wire_and_duplicate_directory_fields_are_not_fabricated_absence_values() {
    let (_, wrong_wire) = reference_fields(&[
        (0x02, 0x10, &0x43u16.to_le_bytes()),
        (0x04, 0x68, &0u32.to_le_bytes()),
    ]);
    assert_eq!(wrong_wire.fields.len(), 2);
    assert!(matches!(
        wrong_wire.required_raw_type(),
        Err(ChunkReferenceReadError::MissingRawType { .. })
    ));
    assert!(matches!(
        wrong_wire.required_chunk_offset(),
        Err(ChunkReferenceReadError::MissingChunkOffset { .. })
    ));
    let (_, duplicates) = reference_fields(&[
        (0x02, 0x18, &0x43u16.to_le_bytes()),
        (0x02, 0x18, &0x43u16.to_le_bytes()),
        (0x04, 0xB8, &0u32.to_le_bytes()),
        (0x04, 0xB8, &0u32.to_le_bytes()),
    ]);
    assert_eq!(duplicates.fields.len(), 4);
    assert!(matches!(
        duplicates.required_raw_type(),
        Err(ChunkReferenceReadError::AmbiguousRawType { count: 2, .. })
    ));
    assert!(matches!(
        duplicates.required_chunk_offset(),
        Err(ChunkReferenceReadError::AmbiguousChunkOffset { count: 2, .. })
    ));
}

#[test]
fn present_directory_offset_zero_decodes_and_explicit_invalid_offset_still_has_range_error() {
    let chunk = 4u32.to_le_bytes();
    for offset in [0u32, 4, u32::MAX] {
        let (_, reference) = reference_fields(&[
            (0x02, 0x18, &0x43u16.to_le_bytes()),
            (0x04, 0xB8, &offset.to_le_bytes()),
        ]);
        assert_eq!(reference.required_raw_type().unwrap().value, 0x43);
        let observed = reference.required_chunk_offset().unwrap();
        assert_eq!(observed.value, offset);
        let result =
            parse_confirmed_0x2c_chunk(StreamPath("/Synthetic".into()), &chunk, observed.value);
        if offset == 0 {
            assert!(result.unwrap().is_fully_decoded());
        } else {
            assert!(
                matches!(result, Err(ChunkReadError::DeclaredRangeOutOfBounds { offset: actual, stream_len: 4, .. }) if actual == offset)
            );
        }
    }
}

#[test]
fn unsupported_block_wire_has_dedicated_error_and_preserves_cursor() {
    let bytes = [0x00, 0xC0];
    let mut cursor = ContentsCursor::new(StreamPath("/Synthetic".into()), &bytes);
    let error = parse_confirmed_block(&mut cursor).expect_err("unsupported wire must fail");

    assert!(matches!(
        error,
        BlockReadError::UnsupportedType {
            block_type: 0xC0,
            offset: 0
        }
    ));
    assert_eq!(cursor.position(), 0);
}

#[test]
fn truncated_u32_is_not_reported_as_unsupported_wire() {
    let bytes = [0x00, 0x20, 0x11, 0x22];
    let mut cursor = ContentsCursor::new(StreamPath("/Synthetic".into()), &bytes);
    let error = parse_confirmed_block(&mut cursor).expect_err("truncated u32 must fail");

    assert!(matches!(error, BlockReadError::Contents(_)));
    assert_eq!(cursor.position(), 0);
}

#[test]
fn invalid_container_length_has_dedicated_error() {
    let bytes = [0x00, 0x88, 0x03, 0x00, 0x00, 0x00];
    let mut cursor = ContentsCursor::new(StreamPath("/Synthetic".into()), &bytes);
    let error = parse_confirmed_block(&mut cursor).expect_err("declared length < 4 must fail");

    assert!(matches!(
        error,
        BlockReadError::InvalidDeclaredLength {
            block_type: 0x88,
            offset: 0,
            declared_length: 3
        }
    ));
    assert_eq!(cursor.position(), 0);
}

#[test]
fn chunk_declared_range_oob_is_distinct_from_block_error() {
    let bytes = [0x20, 0x00, 0x00, 0x00, 0, 0, 0, 0];
    let error = parse_confirmed_0x2c_chunk(StreamPath("/Synthetic".into()), &bytes, 0)
        .expect_err("chunk range must fit stream");

    assert!(matches!(
        error,
        ChunkReadError::DeclaredRangeOutOfBounds {
            offset: 0,
            declared_length: 32,
            stream_len: 8
        }
    ));
}

#[test]
fn directory_wrong_slot_id_is_distinct_from_wire_parse_failure() {
    let bytes = [0x01, 0x78];
    let error = parse_confirmed_0x2c_directory(&bytes, span(bytes.len() as u64))
        .expect_err("directory slot id must be zero");

    assert!(matches!(
        error,
        DirectoryReadError::UnexpectedSlotId { offset: 0, id: 1 }
    ));
}

#[test]
fn directory_wrong_supported_wire_is_context_error() {
    let bytes = [0x00, 0x20, 0x01, 0x00, 0x00, 0x00];
    let error = parse_confirmed_0x2c_directory(&bytes, span(bytes.len() as u64))
        .expect_err("generic u32 block is not a directory slot");

    assert!(matches!(
        error,
        DirectoryReadError::UnexpectedSlotType {
            offset: 0,
            block_type: 0x20
        }
    ));
}

#[test]
fn reference_context_reports_unsupported_reference_wire_separately() {
    let bytes = [0x00, 0x88, 0x06, 0x00, 0x00, 0x00, 0x02, 0xC0];
    let directory = parse_confirmed_0x2c_directory(&bytes, span(bytes.len() as u64))
        .expect("directory envelope must parse");
    let error = parse_confirmed_chunk_reference(&bytes, &directory, 0)
        .expect_err("reference-only wire taxonomy must fail locally");

    assert!(matches!(
        error,
        ChunkReferenceReadError::UnsupportedReferenceWireType {
            block_type: 0xC0,
            offset: 6
        }
    ));
}

#[test]
fn reference_out_of_range_is_not_collapsed_into_generic_parse_error() {
    let bytes = [0x00, 0x78];
    let directory = parse_confirmed_0x2c_directory(&bytes, span(bytes.len() as u64))
        .expect("single empty directory slot must parse");
    let error = parse_confirmed_chunk_reference(&bytes, &directory, 1)
        .expect_err("out of range seqNum must be explicit");

    assert!(matches!(
        error,
        ChunkReferenceReadError::SlotOutOfRange {
            seq_num: 1,
            slot_count: 1
        }
    ));
}
