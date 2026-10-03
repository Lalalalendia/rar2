use anyhow::{bail, Context, Result};
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference, parse_confirmed_mature_story_catalog,
    CONTENTS_RAW_TYPE_STORY_CATALOG,
};
use pub_core::StreamPath;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const CONTENTS_STREAM: &str = "/Contents";
const QUILL_STREAM: &str = "/Quill/QuillSub/CONTENTS";
const DESCRIPTOR_ROOT: u32 = 0x18;
const DESCRIPTOR_END: u32 = 0xffff_ffff;
const DESCRIPTOR_SIZE: usize = 24;
const DESCRIPTOR_PRESENT: u16 = 0x0018;

const EXPECTED_WITNESSES: [&str; 4] = [
    "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157",
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a",
    "9c03c6e897be6abb4538bbb12cee3041fe4eab3af9109ce1df5d64b46e4c0569",
    "ccfcbadc8951acece4d10cc27d71f28f318685845b94ae07fd46331c3571f3ff",
];

#[derive(Debug, Clone)]
struct Descriptor {
    name: [u8; 4],
    data_offset: u32,
    data_length: u32,
}

#[derive(Debug, Deserialize)]
struct OracleReceipt {
    schema: String,
    witness_count: usize,
    witnesses: Vec<OracleWitness>,
}

#[derive(Debug, Deserialize)]
struct OracleWitness {
    source_sha256: String,
    story_count: usize,
    story_utf16_sum: u64,
    source_unchanged: bool,
    stories: Vec<OracleStory>,
}

#[derive(Debug, Clone, Deserialize)]
struct OracleStory {
    com_ordinal: usize,
    utf16_code_units: u64,
    utf16le_sha256: String,
}

#[derive(Debug, Serialize)]
struct ResolutionRow {
    source_sha256: String,
    contents_serialization_revision: u16,
    grounded_story_count: u32,
    fdpp_boundary_count: usize,
    text_utf16_units: u64,
    oracle_story_count: usize,
    oracle_utf16_sum_matches_text: bool,
    length_partition_count: usize,
    hash_partition_count: usize,
    unique_hash_partition: bool,
    terminal_fdpp_ordinals_zero_based: Vec<usize>,
    source_order_story_utf16_lengths: Vec<u64>,
    com_order_matches_source_order: Option<bool>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn parse_descriptor_directory(bytes: &[u8]) -> Result<Vec<Descriptor>> {
    let mut current = DESCRIPTOR_ROOT;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    while current != DESCRIPTOR_END {
        if !seen.insert(current) {
            bail!("descriptor list cycle");
        }
        let start = usize::try_from(current).context("descriptor node offset too large")?;
        let count =
            usize::from(u16_at(bytes, start + 2).context("descriptor node header truncated")?);
        let next = u32_at(bytes, start + 4).context("descriptor node next truncated")?;
        let array_start = start.checked_add(8).context("descriptor array overflow")?;
        let array_end = array_start
            .checked_add(
                count
                    .checked_mul(DESCRIPTOR_SIZE)
                    .context("descriptor array size overflow")?,
            )
            .context("descriptor array end overflow")?;
        if array_end > bytes.len() {
            bail!("descriptor array outside Quill");
        }

        for index in 0..count {
            let offset = array_start + index * DESCRIPTOR_SIZE;
            if u16_at(bytes, offset) != Some(DESCRIPTOR_PRESENT) {
                bail!("unexpected descriptor presence marker");
            }
            let name_raw = bytes
                .get(offset + 2..offset + 6)
                .context("descriptor name truncated")?;
            let name = [name_raw[0], name_raw[1], name_raw[2], name_raw[3]];
            let data_offset =
                u32_at(bytes, offset + 16).context("descriptor data offset truncated")?;
            let data_length =
                u32_at(bytes, offset + 20).context("descriptor data length truncated")?;
            out.push(Descriptor {
                name,
                data_offset,
                data_length,
            });
        }

        current = next;
    }

    Ok(out)
}

fn unique_descriptor(descriptors: &[Descriptor], name: [u8; 4]) -> Result<&Descriptor> {
    let mut found = descriptors.iter().filter(|item| item.name == name);
    let first = found
        .next()
        .with_context(|| format!("missing Quill descriptor {name:?}"))?;
    if found.next().is_some() {
        bail!("duplicate Quill descriptor {name:?}");
    }
    Ok(first)
}

fn descriptor_range<'a>(bytes: &'a [u8], descriptor: &Descriptor) -> Result<&'a [u8]> {
    let start = usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
    let len = usize::try_from(descriptor.data_length).context("descriptor length too large")?;
    let end = start
        .checked_add(len)
        .context("descriptor range overflow")?;
    bytes
        .get(start..end)
        .with_context(|| format!("descriptor {:?} outside Quill", descriptor.name))
}

fn grounded_story_count(contents: &[u8]) -> Result<(u16, u32)> {
    let stream = StreamPath(CONTENTS_STREAM.into());
    let header =
        parse_0x2c_header(stream.clone(), contents).context("parse mature Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature Contents trailer")?;

    let mut story_refs = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse chunk reference {seq_num}"))?
        else {
            continue;
        };
        if reference.raw_types.len() == 1
            && reference.raw_types[0].value == CONTENTS_RAW_TYPE_STORY_CATALOG
            && reference.chunk_offsets.len() == 1
        {
            story_refs.push(reference);
        }
    }
    if story_refs.len() != 1 {
        bail!(
            "expected exactly one strict 0x65 Story catalog, found {}",
            story_refs.len()
        );
    }

    let chunk = parse_confirmed_0x2c_chunk(stream, contents, story_refs[0].chunk_offsets[0].value)
        .context("parse Story catalog chunk")?;
    let catalog = parse_confirmed_mature_story_catalog(contents, &chunk)
        .context("parse grounded Story catalog")?;

    Ok((
        header.preamble.serialization_revision,
        catalog.declared_count,
    ))
}

fn fdpp_boundaries_utf16(quill: &[u8], fdpp: &Descriptor, text: &Descriptor) -> Result<Vec<u64>> {
    let payload = descriptor_range(quill, fdpp)?;
    let count = usize::from(u16_at(payload, 0).context("FDPP stored count missing")?);
    let offsets_start = 8usize;
    let table_end = offsets_start
        .checked_add(count.checked_mul(4).context("FDPP table size overflow")?)
        .context("FDPP table end overflow")?;
    if table_end > payload.len() {
        bail!("FDPP boundary table outside payload");
    }

    let text_start = u64::from(text.data_offset);
    let text_end = text_start
        .checked_add(u64::from(text.data_length))
        .context("TEXT range overflow")?;
    let mut out = Vec::with_capacity(count);

    for index in 0..count {
        let absolute = u64::from(
            u32_at(payload, offsets_start + index * 4).context("FDPP boundary truncated")?,
        );
        if absolute < text_start || absolute > text_end {
            bail!("FDPP boundary outside TEXT");
        }
        let delta = absolute - text_start;
        if delta % 2 != 0 {
            bail!("FDPP boundary is not UTF-16 aligned");
        }
        out.push(delta / 2);
    }

    if !out.windows(2).all(|pair| pair[0] < pair[1]) {
        bail!("FDPP boundaries are not strictly increasing");
    }
    let text_units = u64::from(text.data_length) / 2;
    if out.last().copied() != Some(text_units) {
        bail!("terminal FDPP boundary does not close TEXT");
    }
    Ok(out)
}

fn search_partitions(
    text: &[u8],
    text_utf16_units: u64,
    fdpp_set: &BTreeSet<u64>,
    stories: &[OracleStory],
    check_hash: bool,
) -> BTreeSet<Vec<u64>> {
    struct Search<'a> {
        text: &'a [u8],
        text_utf16_units: u64,
        fdpp_set: &'a BTreeSet<u64>,
        stories: &'a [OracleStory],
        check_hash: bool,
        used: Vec<bool>,
        endpoints: Vec<u64>,
        solutions: BTreeSet<Vec<u64>>,
    }

    impl Search<'_> {
        fn walk(&mut self, start: u64) {
            if self.endpoints.len() == self.stories.len() {
                if start == self.text_utf16_units {
                    self.solutions.insert(self.endpoints.clone());
                }
                return;
            }

            for index in 0..self.stories.len() {
                if self.used[index] {
                    continue;
                }
                let story = &self.stories[index];
                let Some(end) = start.checked_add(story.utf16_code_units) else {
                    continue;
                };
                if end > self.text_utf16_units || !self.fdpp_set.contains(&end) {
                    continue;
                }

                if self.check_hash {
                    let Ok(byte_start) = usize::try_from(start.saturating_mul(2)) else {
                        continue;
                    };
                    let Ok(byte_end) = usize::try_from(end.saturating_mul(2)) else {
                        continue;
                    };
                    let Some(slice) = self.text.get(byte_start..byte_end) else {
                        continue;
                    };
                    if sha256_hex(slice) != story.utf16le_sha256.to_ascii_lowercase() {
                        continue;
                    }
                }

                self.used[index] = true;
                self.endpoints.push(end);
                self.walk(end);
                self.endpoints.pop();
                self.used[index] = false;
            }
        }
    }

    let mut search = Search {
        text,
        text_utf16_units,
        fdpp_set,
        stories,
        check_hash,
        used: vec![false; stories.len()],
        endpoints: Vec::with_capacity(stories.len()),
        solutions: BTreeSet::new(),
    };
    search.walk(0);
    search.solutions
}

fn com_order_matches(text: &[u8], endpoints: &[u64], stories: &[OracleStory]) -> bool {
    let mut ordered = stories.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|story| story.com_ordinal);
    if ordered.len() != endpoints.len() {
        return false;
    }

    let mut start = 0u64;
    for (story, end) in ordered.into_iter().zip(endpoints.iter().copied()) {
        if end < start || end - start != story.utf16_code_units {
            return false;
        }
        let Ok(byte_start) = usize::try_from(start.saturating_mul(2)) else {
            return false;
        };
        let Ok(byte_end) = usize::try_from(end.saturating_mul(2)) else {
            return false;
        };
        let Some(slice) = text.get(byte_start..byte_end) else {
            return false;
        };
        if sha256_hex(slice) != story.utf16le_sha256.to_ascii_lowercase() {
            return false;
        }
        start = end;
    }
    true
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = fs::read_dir(root)
        .with_context(|| format!("read witness dir {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    out.sort();
    Ok(out)
}

fn resolve_one(bytes: &[u8], oracle: &OracleWitness) -> Result<ResolutionRow> {
    let source_sha256 = sha256_hex(bytes);
    if source_sha256 != oracle.source_sha256.to_ascii_lowercase() {
        bail!("oracle/source SHA mismatch");
    }
    if !oracle.source_unchanged {
        bail!("oracle does not attest source immutability");
    }

    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS_STREAM)
        .context("read Contents stream")?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(bytes), QUILL_STREAM)
        .context("read Quill stream")?;

    let (revision, grounded_count) = grounded_story_count(&contents)?;
    let grounded_usize = usize::try_from(grounded_count).context("Story count too large")?;
    if grounded_usize != oracle.story_count || oracle.story_count != oracle.stories.len() {
        bail!("grounded Contents Story count does not match read-only COM oracle");
    }

    let descriptors = parse_descriptor_directory(&quill)?;
    let text = unique_descriptor(&descriptors, *b"TEXT")?;
    let fdpp = unique_descriptor(&descriptors, *b"FDPP")?;
    if text.data_length % 2 != 0 {
        bail!("TEXT length is not UTF-16 aligned");
    }
    let text_payload = descriptor_range(&quill, text)?;
    let text_utf16_units = u64::from(text.data_length) / 2;
    let fdpp_units = fdpp_boundaries_utf16(&quill, fdpp, text)?;
    let fdpp_set = fdpp_units.iter().copied().collect::<BTreeSet<_>>();

    let oracle_sum = oracle
        .stories
        .iter()
        .try_fold(0u64, |acc, story| acc.checked_add(story.utf16_code_units))
        .context("oracle Story length sum overflow")?;
    if oracle_sum != oracle.story_utf16_sum {
        bail!("oracle Story length sum is internally inconsistent");
    }

    let length_solutions = search_partitions(
        text_payload,
        text_utf16_units,
        &fdpp_set,
        &oracle.stories,
        false,
    );
    let hash_solutions = search_partitions(
        text_payload,
        text_utf16_units,
        &fdpp_set,
        &oracle.stories,
        true,
    );

    let unique = hash_solutions.len() == 1;
    let endpoints = hash_solutions.iter().next().cloned().unwrap_or_default();
    let terminal_fdpp_ordinals_zero_based = if unique {
        endpoints
            .iter()
            .map(|endpoint| {
                fdpp_units
                    .iter()
                    .position(|value| value == endpoint)
                    .context("resolved Story endpoint is not an FDPP boundary")
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let source_order_story_utf16_lengths = if unique {
        let mut start = 0u64;
        endpoints
            .iter()
            .map(|end| {
                let len = *end - start;
                start = *end;
                len
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(ResolutionRow {
        source_sha256,
        contents_serialization_revision: revision,
        grounded_story_count: grounded_count,
        fdpp_boundary_count: fdpp_units.len(),
        text_utf16_units,
        oracle_story_count: oracle.story_count,
        oracle_utf16_sum_matches_text: oracle_sum == text_utf16_units,
        length_partition_count: length_solutions.len(),
        hash_partition_count: hash_solutions.len(),
        unique_hash_partition: unique,
        terminal_fdpp_ordinals_zero_based,
        source_order_story_utf16_lengths,
        com_order_matches_source_order: unique
            .then(|| com_order_matches(text_payload, &endpoints, &oracle.stories)),
    })
}

fn main() -> Result<()> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        bail!("usage: quill-story-readonly-oracle-resolver WITNESS_DIR ORACLE.json OUTPUT.json");
    }
    let root = PathBuf::from(&args[0]);
    let oracle_path = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);

    let oracle: OracleReceipt =
        serde_json::from_slice(&fs::read(&oracle_path).context("read oracle receipt")?)
            .context("parse oracle receipt")?;
    if oracle.schema != "chaptera.quill-story-readonly-oracle.v1" {
        bail!("unsupported oracle schema: {}", oracle.schema);
    }
    if oracle.witness_count != 4 || oracle.witnesses.len() != 4 {
        bail!("oracle must contain exactly four witnesses");
    }

    let oracle_by_sha = oracle
        .witnesses
        .iter()
        .map(|row| (row.source_sha256.to_ascii_lowercase(), row))
        .collect::<BTreeMap<_, _>>();
    let expected = EXPECTED_WITNESSES
        .iter()
        .map(|sha| sha.to_string())
        .collect::<BTreeSet<_>>();
    let observed = oracle_by_sha.keys().cloned().collect::<BTreeSet<_>>();
    if observed != expected {
        bail!("oracle witness SHA set is not the exact unresolved #337 four");
    }

    let paths = pub_paths(&root)?;
    if paths.len() != 4 {
        bail!("expected exactly four PUB witnesses, found {}", paths.len());
    }

    let mut rows = Vec::with_capacity(4);
    let mut source_sha_set = BTreeSet::new();
    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let sha = sha256_hex(&bytes);
        if !expected.contains(&sha) {
            bail!("unexpected source PUB SHA {sha}");
        }
        if !source_sha_set.insert(sha.clone()) {
            bail!("duplicate source PUB SHA {sha}");
        }
        let oracle_row = oracle_by_sha
            .get(&sha)
            .with_context(|| format!("missing oracle row for {sha}"))?;
        rows.push(resolve_one(&bytes, oracle_row)?);
    }
    if source_sha_set != expected {
        bail!("source witness SHA set is not the exact unresolved #337 four");
    }
    rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));

    let uniquely_resolved = rows.iter().filter(|row| row.unique_hash_partition).count();
    let report = serde_json::json!({
        "schema": "chaptera.quill-story-fdpp-oracle-resolution.v1",
        "witness_count": rows.len(),
        "uniquely_resolved_count": uniquely_resolved,
        "rows": rows,
        "evidence_boundary": "Exact four #337 witnesses. Source parsing uses only Contents 0x65 cardinality, TEXT bytes and FDPP boundaries; oracle supplies only read-only COM Story UTF-16 lengths and text SHA-256. Output retains SHA, counts, booleans, Story lengths and zero-based FDPP ordinals; no document text, filenames, paths, raw bytes or absolute offsets."
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story(text: &str, ordinal: usize) -> OracleStory {
        let bytes = text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        OracleStory {
            com_ordinal: ordinal,
            utf16_code_units: u64::try_from(bytes.len() / 2).unwrap(),
            utf16le_sha256: sha256_hex(&bytes),
        }
    }

    #[test]
    fn hashes_disambiguate_equal_fdpp_superset_choices() {
        let text = ["aa\r", "bb\r", "cc\r"]
            .into_iter()
            .flat_map(|part| part.encode_utf16().flat_map(u16::to_le_bytes))
            .collect::<Vec<_>>();
        let stories = vec![story("aa\rbb\r", 0), story("cc\r", 1)];
        let fdpp = BTreeSet::from([3u64, 6, 9]);

        let lengths = search_partitions(&text, 9, &fdpp, &stories, false);
        let hashes = search_partitions(&text, 9, &fdpp, &stories, true);
        assert_eq!(lengths.len(), 1);
        assert_eq!(hashes, BTreeSet::from([vec![6u64, 9]]));
    }

    #[test]
    fn com_order_is_not_required() {
        let text = ["aa\r", "bbb\r", "c\r"]
            .into_iter()
            .flat_map(|part| part.encode_utf16().flat_map(u16::to_le_bytes))
            .collect::<Vec<_>>();
        let stories = vec![story("c\r", 0), story("aa\r", 1), story("bbb\r", 2)];
        let fdpp = BTreeSet::from([3u64, 7, 9]);

        let hashes = search_partitions(&text, 9, &fdpp, &stories, true);
        assert_eq!(hashes, BTreeSet::from([vec![3u64, 7, 9]]));
    }
}
