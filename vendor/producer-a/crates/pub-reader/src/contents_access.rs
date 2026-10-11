use super::*;

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
    if reference.chunk_offsets.len() != 1 {
        bail!(
            "Contents seq {} has {} chunk offsets, expected exactly one",
            reference.seq_num,
            reference.chunk_offsets.len()
        );
    }

    parse_confirmed_0x2c_chunk(stream, contents, reference.chunk_offsets[0].value)
        .with_context(|| format!("parse Contents chunk seq {}", reference.seq_num))
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
    match reference.raw_types.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
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
    raw_type == Some(RAW_TYPE_LOGO)
        && parent_raw_type == Some(RAW_TYPE_PAGE)
        && registered_page
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
