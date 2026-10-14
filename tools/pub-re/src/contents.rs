//! Bounded relocation-aware attribution of mature 0x2C Contents chunks.
//! A stable result means only directory-addressed chunk bytes are unchanged.
//! Root state and other CFB streams are not included in this contract.
use crate::{ExperimentManifestV1, LoadedInput, load_input, stream_bytes, validate_manifest};
use anyhow::{Context, Result, bail};
use pub_contents::{
    CHUNK_REFERENCE_OFFSET_ID, CHUNK_REFERENCE_WIRE_OFFSET, Contents0x2cDirectorySlot,
    RawContentsBlockBody, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::{RawSpan, StreamPath};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub const CONTENTS_DIFF_SCHEMA: &str = "chaptera.pub-re-contents-diff.v1";
const CONTENTS: &str = "/Contents";
const MAX_SLOTS: usize = 40_000;
const MAX_REFS: usize = 40_000;
const MAX_ORDINALS: usize = 128;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ContentsDiffReceipt {
    pub schema: &'static str,
    pub experiment_id: String,
    pub status: &'static str,
    pub before_sha256: String,
    pub after_sha256: String,
    pub before_contents_len: usize,
    pub after_contents_len: usize,
    pub trailer_offset_delta: i64,
    pub before_slot_count: usize,
    pub after_slot_count: usize,
    pub compared_chunks: usize,
    pub unchanged_chunks: usize,
    pub changed_chunks: usize,
    pub unmatched_chunks: usize,
    pub unresolved_slots: usize,
    pub reference_metadata_unchanged: bool,
    pub offset_delta_histogram: BTreeMap<i64, usize>,
    pub changed_chunk_ordinals: Vec<usize>,
    pub changed_reference_ordinals: Vec<usize>,
    pub ordinals_truncated: bool,
    pub raw_bytes_emitted: bool,
    pub absolute_paths_emitted: bool,
    pub whole_pub_semantic_equality_claimed: bool,
    pub limitations: Vec<&'static str>,
}

#[derive(Debug, Clone)]
struct Slot {
    metadata: Vec<Vec<u8>>,
    offsets: Vec<usize>,
    chunks: Vec<Vec<u8>>,
}
#[derive(Debug, Clone)]
struct Snapshot {
    contents_len: usize,
    trailer_offset: u32,
    slots: Vec<Option<Slot>>,
    unresolved_slots: usize,
}

fn bytes_at<'a>(bytes: &'a [u8], span: &RawSpan) -> Result<&'a [u8]> {
    let start = usize::try_from(span.offset)?;
    let len = usize::try_from(span.len)?;
    bytes
        .get(start..start.checked_add(len).context("span overflow")?)
        .context("span out of range")
}

fn parse_snapshot(input: &LoadedInput) -> Result<Snapshot> {
    let bytes = stream_bytes(input, CONTENTS)?;
    let head =
        parse_0x2c_header(StreamPath(CONTENTS.into()), &bytes).context("require mature 0x2C")?;
    let root =
        parse_confirmed_0x2c_trailer_root(&bytes, &head).context("require confirmed trailer")?;
    if root.directory.slots.len() > MAX_SLOTS
        || !root.observed_slot_count_matches_directory()
        || !root.observed_max_ordinal_matches_directory()
        || !root.roots_fill_declared_trailer()
    {
        bail!("invalid 0x2C trailer / directory bounds");
    }
    let mut slots = Vec::new();
    let mut ranges = Vec::<(usize, usize)>::new();
    let mut starts = BTreeSet::new();
    let mut reference_count = 0usize;
    let mut unresolved_slots = 0usize;
    for (ordinal, entry) in root.directory.slots.iter().enumerate() {
        if matches!(entry, Contents0x2cDirectorySlot::Empty { .. }) {
            slots.push(None);
            continue;
        }
        let reference = parse_confirmed_chunk_reference(&bytes, &root.directory, ordinal)?
            .context("occupied slot has no reference")?;
        let mut metadata = Vec::new();
        for field in &reference.fields {
            let mut raw = bytes_at(&bytes, &field.source)?.to_vec();
            if field.id == CHUNK_REFERENCE_OFFSET_ID
                && field.block_type == CHUNK_REFERENCE_WIRE_OFFSET
            {
                if raw.len() != 6 || !matches!(field.body, RawContentsBlockBody::U32 { .. }) {
                    bail!("invalid confirmed 0x04/B8 pointer shape");
                }
                raw[2..6].fill(0);
            }
            metadata.push(raw);
        }
        if reference.chunk_offsets.is_empty() {
            unresolved_slots += 1;
        }
        reference_count += reference.chunk_offsets.len();
        if reference_count > MAX_REFS {
            bail!("too many Contents chunk references");
        }
        let mut offsets = Vec::new();
        let mut chunks = Vec::new();
        for field in &reference.chunk_offsets {
            let start = usize::try_from(field.value)?;
            if start < 0x1E || start >= usize::try_from(head.trailer_offset)? {
                bail!("chunk starts outside admitted pre-trailer region");
            }
            if !starts.insert(start) {
                bail!("duplicate chunk source offset");
            }
            let parsed =
                parse_confirmed_0x2c_chunk(StreamPath(CONTENTS.into()), &bytes, field.value)
                    .context("invalid confirmed chunk")?;
            let raw = bytes_at(&bytes, &parsed.source)?;
            let end = start.checked_add(raw.len()).context("chunk end overflow")?;
            if end > usize::try_from(head.trailer_offset)? {
                bail!("chunk extends beyond pre-trailer range");
            }
            ranges.push((start, end));
            offsets.push(start);
            chunks.push(raw.to_vec());
        }
        slots.push(Some(Slot {
            metadata,
            offsets,
            chunks,
        }));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        bail!("overlapping directory-addressed chunks");
    }
    Ok(Snapshot {
        contents_len: bytes.len(),
        trailer_offset: head.trailer_offset,
        slots,
        unresolved_slots,
    })
}

pub fn attribute_contents_manifest_file(path: &Path) -> Result<ContentsDiffReceipt> {
    let raw = fs::read(path)?;
    let manifest: ExperimentManifestV1 =
        serde_json::from_slice(raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw))?;
    attribute_contents_manifest(&manifest, path.parent().unwrap_or(Path::new(".")))
}

pub fn attribute_contents_manifest(
    manifest: &ExperimentManifestV1,
    directory: &Path,
) -> Result<ContentsDiffReceipt> {
    validate_manifest(manifest)?;
    let before = load_input(&manifest.before, directory, "before", &manifest.policy)?;
    let after = load_input(&manifest.after, directory, "after", &manifest.policy)?;
    let a = parse_snapshot(&before).context("before not admissible")?;
    let b = parse_snapshot(&after).context("after not admissible")?;

    let mut histogram = BTreeMap::new();
    let mut same_metadata = a.slots.len() == b.slots.len();
    let mut compared = 0;
    let mut unchanged = 0;
    let mut changed = 0;
    let mut unmatched = 0;
    let mut chunk_ordinals = Vec::new();
    let mut ref_ordinals = Vec::new();
    let mut truncated = false;

    for index in 0..a.slots.len().max(b.slots.len()) {
        let left = a.slots.get(index).and_then(Option::as_ref);
        let right = b.slots.get(index).and_then(Option::as_ref);
        match (left, right) {
            (None, None) => {}
            (Some(x), Some(y)) => {
                if x.metadata != y.metadata || x.chunks.len() != y.chunks.len() {
                    same_metadata = false;
                    if ref_ordinals.len() < MAX_ORDINALS {
                        ref_ordinals.push(index);
                    } else {
                        truncated = true;
                    }
                }
                unmatched += x.chunks.len().abs_diff(y.chunks.len());
                for ((bx, by), (ox, oy)) in x
                    .chunks
                    .iter()
                    .zip(&y.chunks)
                    .zip(x.offsets.iter().zip(&y.offsets))
                {
                    *histogram.entry(*oy as i64 - *ox as i64).or_insert(0) += 1;
                    compared += 1;
                    if bx == by {
                        unchanged += 1;
                    } else {
                        changed += 1;
                        if chunk_ordinals.last().copied() != Some(index) {
                            if chunk_ordinals.len() < MAX_ORDINALS {
                                chunk_ordinals.push(index);
                            } else {
                                truncated = true;
                            }
                        }
                    }
                }
            }
            (Some(x), None) => {
                same_metadata = false;
                unmatched += x.chunks.len();
                if ref_ordinals.len() < MAX_ORDINALS {
                    ref_ordinals.push(index);
                } else {
                    truncated = true;
                }
            }
            (None, Some(y)) => {
                same_metadata = false;
                unmatched += y.chunks.len();
                if ref_ordinals.len() < MAX_ORDINALS {
                    ref_ordinals.push(index);
                } else {
                    truncated = true;
                }
            }
        }
    }
    let unresolved = a.unresolved_slots + b.unresolved_slots;
    let status = if unresolved > 0 || compared == 0 {
        "not_evaluable"
    } else if !same_metadata || unmatched > 0 {
        "reference_or_slot_changed"
    } else if changed > 0 {
        "referenced_chunk_payload_changed"
    } else {
        "referenced_chunks_unchanged"
    };
    Ok(ContentsDiffReceipt {
        schema: CONTENTS_DIFF_SCHEMA,
        experiment_id: manifest.experiment_id.clone(),
        status,
        before_sha256: before.sha256,
        after_sha256: after.sha256,
        before_contents_len: a.contents_len,
        after_contents_len: b.contents_len,
        trailer_offset_delta: i64::from(b.trailer_offset) - i64::from(a.trailer_offset),
        before_slot_count: a.slots.len(),
        after_slot_count: b.slots.len(),
        compared_chunks: compared,
        unchanged_chunks: unchanged,
        changed_chunks: changed,
        unmatched_chunks: unmatched,
        unresolved_slots: unresolved,
        reference_metadata_unchanged: same_metadata,
        offset_delta_histogram: histogram,
        changed_chunk_ordinals: chunk_ordinals,
        changed_reference_ordinals: ref_ordinals,
        ordinals_truncated: truncated,
        raw_bytes_emitted: false,
        absolute_paths_emitted: false,
        whole_pub_semantic_equality_claimed: false,
        limitations: vec![
            "Scope is confirmed mature 0x2C directory-addressed chunk bytes only.",
            "Root path/service fields and unreferenced Contents regions remain unclassified.",
            "Quill, Escher, other streams and full authoring semantics are not compared.",
        ],
    })
}
