use anyhow::{Context, Result, bail};
use pub_contents::{
    BLOCK_TYPE_REFERENCE_U32, BLOCK_TYPE_U32, ContentsCursor, RawContentsBlockBody,
    parse_confirmed_block,
};
use pub_core::StreamPath;
use pub_model::{PageId, RectEmu};
use pub_reader::{
    CONTENTS_STREAM_PATH, PubStructuralBaseCandidate, build_mature_0x2c_structural_base_manifest,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const SCHEMA: &str = "chaptera.auth-wrap-oracle-tool.v1";
const FIELD_WRAP_COUNT: u16 = 0x46;
const FIELD_WRAP_REFS: u16 = 0x47;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WrapRefObservation {
    target_seq_num: u32,
    value_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FoptScalar {
    property_id: u16,
    opid: u16,
    op: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ShapeObservation {
    page_id: PageId,
    seq_num: u32,
    bounds_emu: RectEmu,
    officeart_spid: u32,
    officeart_shape_type: u16,
    is_text_frame: bool,
    fopt: Vec<FoptScalar>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WrapFrameObservation {
    page_id: PageId,
    frame_seq_num: u32,
    bounds_emu: RectEmu,
    count: u32,
    refs: Vec<WrapRefObservation>,
    replacement_candidates: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct InspectReceipt {
    schema: &'static str,
    source_sha256: String,
    source_len: u64,
    shapes: Vec<ShapeObservation>,
    wrap_frames: Vec<WrapFrameObservation>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn fopt_for(candidate: &PubStructuralBaseCandidate) -> Vec<FoptScalar> {
    let mut out = candidate
        .escher_shape
        .fopts
        .iter()
        .flat_map(|fopt| fopt.properties.iter())
        .filter(|property| !property.f_complex())
        .map(|property| FoptScalar {
            property_id: property.property_id(),
            opid: property.opid,
            op: property.op,
        })
        .collect::<Vec<_>>();
    out.sort_by_key(|property| (property.property_id, property.opid, property.op));
    out
}

fn unique_u32_field(candidate: &PubStructuralBaseCandidate, id: u16) -> Option<u32> {
    let matches = candidate
        .contents_chunk
        .fields
        .iter()
        .filter(|field| field.id == id && field.block_type == BLOCK_TYPE_U32)
        .collect::<Vec<_>>();
    let [field] = matches.as_slice() else {
        return None;
    };
    match &field.body {
        RawContentsBlockBody::U32 { value, .. } => Some(*value),
        _ => None,
    }
}

fn wrap_refs_for(
    candidate: &PubStructuralBaseCandidate,
    contents: &[u8],
) -> Result<Option<Vec<WrapRefObservation>>> {
    let matches = candidate
        .contents_chunk
        .fields
        .iter()
        .filter(|field| field.id == FIELD_WRAP_REFS)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Ok(None);
    }
    let [field] = matches.as_slice() else {
        bail!(
            "shape seq {} has non-unique 0x47 field count {}",
            candidate.contents_seq_num,
            matches.len()
        );
    };
    let RawContentsBlockBody::Container { content_source, .. } = &field.body else {
        bail!(
            "shape seq {} field 0x47 is not a confirmed container",
            candidate.contents_seq_num
        );
    };

    let start = usize::try_from(content_source.offset).context("0x47 offset does not fit usize")?;
    let len = usize::try_from(content_source.len).context("0x47 length does not fit usize")?;
    let mut cursor = ContentsCursor::bounded(
        StreamPath(CONTENTS_STREAM_PATH.into()),
        contents,
        start,
        len,
    )
    .context("bound 0x47 container")?;
    let mut refs = Vec::new();
    while cursor.remaining() > 0 {
        let block = parse_confirmed_block(&mut cursor)
            .with_context(|| format!("parse seq {} 0x47 member", candidate.contents_seq_num))?;
        if block.id != 0 || block.block_type != BLOCK_TYPE_REFERENCE_U32 {
            bail!(
                "shape seq {} 0x47 contains unexpected member id=0x{:X} type=0x{:02X}",
                candidate.contents_seq_num,
                block.id,
                block.block_type
            );
        }
        let RawContentsBlockBody::U32 {
            value,
            value_source,
        } = block.body
        else {
            bail!("0x47 reference member is not U32");
        };
        refs.push(WrapRefObservation {
            target_seq_num: value,
            value_offset: value_source.offset,
        });
    }
    Ok(Some(refs))
}

fn inspect_bytes(bytes: &[u8]) -> Result<InspectReceipt> {
    let manifest = build_mature_0x2c_structural_base_manifest(bytes)
        .context("build mature structural manifest")?;
    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS_STREAM_PATH)
        .context("read /Contents")?;

    let shapes = manifest
        .candidates
        .iter()
        .map(|candidate| ShapeObservation {
            page_id: candidate.page_id,
            seq_num: candidate.contents_seq_num,
            bounds_emu: candidate.bounds_emu,
            officeart_spid: candidate.officeart_spid,
            officeart_shape_type: candidate.officeart_shape_type,
            is_text_frame: candidate.escher_shape.client_textbox.is_some(),
            fopt: fopt_for(candidate),
        })
        .collect::<Vec<_>>();

    let by_page = manifest.candidates.iter().fold(
        BTreeMap::<PageId, Vec<&PubStructuralBaseCandidate>>::new(),
        |mut map, candidate| {
            map.entry(candidate.page_id).or_default().push(candidate);
            map
        },
    );

    let mut wrap_frames = Vec::new();
    for candidate in &manifest.candidates {
        if candidate.escher_shape.client_textbox.is_none() {
            continue;
        }
        let Some(count) = unique_u32_field(candidate, FIELD_WRAP_COUNT) else {
            continue;
        };
        let Some(refs) = wrap_refs_for(candidate, &contents)? else {
            continue;
        };
        if usize::try_from(count).ok() != Some(refs.len()) {
            bail!(
                "shape seq {} 0x46 count {} != 0x47 refs {}",
                candidate.contents_seq_num,
                count,
                refs.len()
            );
        }
        if refs.is_empty() {
            continue;
        }
        let ref_set = refs
            .iter()
            .map(|reference| reference.target_seq_num)
            .collect::<BTreeSet<_>>();
        let mut replacement_candidates = by_page
            .get(&candidate.page_id)
            .into_iter()
            .flatten()
            .filter(|shape| {
                shape.contents_seq_num != candidate.contents_seq_num
                    && shape.escher_shape.client_textbox.is_none()
                    && !ref_set.contains(&shape.contents_seq_num)
            })
            .map(|shape| shape.contents_seq_num)
            .collect::<Vec<_>>();
        replacement_candidates.sort_unstable();

        wrap_frames.push(WrapFrameObservation {
            page_id: candidate.page_id,
            frame_seq_num: candidate.contents_seq_num,
            bounds_emu: candidate.bounds_emu,
            count,
            refs,
            replacement_candidates,
        });
    }
    wrap_frames.sort_by_key(|frame| frame.frame_seq_num);

    Ok(InspectReceipt {
        schema: SCHEMA,
        source_sha256: sha256_hex(bytes),
        source_len: u64::try_from(bytes.len()).context("source length does not fit u64")?,
        shapes,
        wrap_frames,
    })
}

fn write_inspect(input: &Path, output: &Path) -> Result<()> {
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let receipt = inspect_bytes(&bytes)?;
    fs::write(output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}

fn patch_ref(
    input: &Path,
    output: &Path,
    frame_seq: u32,
    ref_index: usize,
    replacement_seq: u32,
) -> Result<()> {
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let before = inspect_bytes(&bytes)?;
    let frame = before
        .wrap_frames
        .iter()
        .find(|frame| frame.frame_seq_num == frame_seq)
        .with_context(|| format!("wrap frame seq {frame_seq} not found"))?;
    let reference = frame
        .refs
        .get(ref_index)
        .with_context(|| format!("ref index {ref_index} out of range"))?;
    if reference.target_seq_num == replacement_seq {
        bail!("replacement target equals original target");
    }
    if !frame.replacement_candidates.contains(&replacement_seq) {
        bail!(
            "replacement seq {replacement_seq} is not an admitted same-page non-text candidate"
        );
    }

    let mut contents = pub_cfb::read_stream_reader(Cursor::new(&bytes), CONTENTS_STREAM_PATH)
        .context("read /Contents before patch")?;
    let offset = usize::try_from(reference.value_offset).context("reference offset too large")?;
    let target = contents
        .get_mut(offset..offset + 4)
        .context("reference value lies outside /Contents")?;
    let actual = u32::from_le_bytes(target.try_into().expect("four-byte slice"));
    if actual != reference.target_seq_num {
        bail!(
            "reference value drift at offset {offset}: expected {}, got {actual}",
            reference.target_seq_num
        );
    }
    target.copy_from_slice(&replacement_seq.to_le_bytes());

    let rewritten = pub_cfb::replace_stream_reader(
        Cursor::new(bytes.as_slice()),
        CONTENTS_STREAM_PATH,
        &contents,
    )
    .context("replace /Contents in-copy")?;
    let after = inspect_bytes(&rewritten)?;
    let after_frame = after
        .wrap_frames
        .iter()
        .find(|frame| frame.frame_seq_num == frame_seq)
        .context("patched wrap frame disappeared")?;
    if after_frame.count != frame.count || after_frame.refs.len() != frame.refs.len() {
        bail!("0x46/0x47 cardinality changed during one-ref patch");
    }
    for (index, (old, new)) in frame.refs.iter().zip(&after_frame.refs).enumerate() {
        let expected = if index == ref_index {
            replacement_seq
        } else {
            old.target_seq_num
        };
        if new.target_seq_num != expected {
            bail!(
                "unexpected 0x47 ref drift at index {index}: expected {expected}, got {}",
                new.target_seq_num
            );
        }
    }

    fs::write(output, rewritten).with_context(|| format!("write {}", output.display()))?;
    Ok(())
}

fn usage() -> ! {
    eprintln!(
        "usage:\n  auth_wrap_oracle_tool inspect <input.pub> <output.json>\n  auth_wrap_oracle_tool patch-ref <input.pub> <output.pub> <frame-seq> <ref-index> <replacement-seq>"
    );
    std::process::exit(2);
}

fn main() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [command, input, output] if command == "inspect" => {
            write_inspect(Path::new(input), Path::new(output))
        }
        [command, input, output, frame_seq, ref_index, replacement_seq]
            if command == "patch-ref" =>
        {
            patch_ref(
                Path::new(input),
                Path::new(output),
                frame_seq.parse().context("parse frame seq")?,
                ref_index.parse().context("parse ref index")?,
                replacement_seq.parse().context("parse replacement seq")?,
            )
        }
        _ => usage(),
    }
}
