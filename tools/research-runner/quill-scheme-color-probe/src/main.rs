use anyhow::{Context, Result, bail, ensure};
use pub_core::StreamPath;
use pub_quill::{
    QuillResearchColorReferenceKind, QuillResearchColorReferenceSource,
    inspect_effective_text_color_references, parse_confirmed_story_catalog,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, io::Cursor, path::Path};

const STREAM: &str = "/Quill/QuillSub/CONTENTS";
const SCHEMA: &str = "chaptera.quill-scheme-color-carrier-map.v1";

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn source_name(source: QuillResearchColorReferenceSource) -> &'static str {
    match source {
        QuillResearchColorReferenceSource::ExplicitFdpc => "explicit_fdpc",
        QuillResearchColorReferenceSource::InheritedStsh1 => "inherited_stsh1",
        QuillResearchColorReferenceSource::None => "none",
    }
}

fn inspect(path: &Path) -> Result<(Vec<u8>, pub_quill::QuillStoryCatalog, Vec<pub_quill::QuillResearchEffectiveColorReferenceRun>)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), STREAM)
        .with_context(|| format!("read Quill stream from {}", path.display()))?;
    let stories = parse_confirmed_story_catalog(StreamPath(STREAM.into()), &quill)
        .with_context(|| format!("parse Story catalog from {}", path.display()))?;
    let runs = inspect_effective_text_color_references(&quill, &stories)
        .with_context(|| format!("inspect effective color references from {}", path.display()))?;
    Ok((bytes, stories, runs))
}

fn synthetic_map(path: &Path) -> Result<Value> {
    let (bytes, stories, runs) = inspect(path)?;
    ensure!(stories.stories.len() == 1, "synthetic fixture must contain exactly one Story");
    let story_index = stories.stories[0].index;

    let mut positions = Vec::new();
    for zero_based in 0_u32..9 {
        let matches = runs
            .iter()
            .filter(|run| {
                run.story_index == story_index
                    && run.story_start_utf16 <= zero_based
                    && zero_based < run.story_end_utf16
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "synthetic position {} has {} effective color owners",
            zero_based + 1,
            matches.len()
        );
        let run = matches[0];
        let source = source_name(run.source);
        let position = zero_based + 1;
        let value = match &run.kind {
            QuillResearchColorReferenceKind::PublisherEightSlotScheme { slot } => {
                ensure!(position <= 8, "direct RGB control unexpectedly uses a scheme reference");
                json!({
                    "position": position,
                    "expected_com_scheme_role": position,
                    "source": source,
                    "pl_index": run.pl_index,
                    "kind": "publisher_eight_slot_scheme",
                    "scheme_slot": slot,
                })
            }
            QuillResearchColorReferenceKind::DirectRgb => {
                ensure!(position == 9, "scheme-bound position {position} unexpectedly became direct RGB");
                json!({
                    "position": position,
                    "source": source,
                    "pl_index": run.pl_index,
                    "kind": "direct_rgb_control",
                })
            }
            other => bail!("synthetic position {position} has unexpected color reference kind {other:?}"),
        };
        positions.push(value);
    }

    Ok(json!({
        "source_sha256": sha256(&bytes),
        "source_bytes": bytes.len(),
        "story_index": story_index,
        "positions": positions,
    }))
}

fn scheme_slots(map: &Value) -> Result<Vec<u8>> {
    let positions = map["positions"]
        .as_array()
        .context("synthetic positions missing")?;
    ensure!(positions.len() == 9, "synthetic position count drift");
    let mut slots = Vec::new();
    for (index, value) in positions.iter().take(8).enumerate() {
        let slot = value["scheme_slot"]
            .as_u64()
            .with_context(|| format!("scheme slot missing at position {}", index + 1))?;
        slots.push(u8::try_from(slot).context("scheme slot exceeds u8")?);
    }
    Ok(slots)
}

fn carlton_summary(path: &Path) -> Result<Value> {
    let (bytes, _stories, runs) = inspect(path)?;
    let mut slot_counts = BTreeMap::<u8, usize>::new();
    let mut source_counts = BTreeMap::<String, usize>::new();
    let mut scheme_run_count = 0_usize;
    let mut direct_run_count = 0_usize;
    for run in runs {
        match run.kind {
            QuillResearchColorReferenceKind::PublisherEightSlotScheme { slot } => {
                scheme_run_count += 1;
                *slot_counts.entry(slot).or_default() += 1;
                *source_counts.entry(source_name(run.source).to_owned()).or_default() += 1;
            }
            QuillResearchColorReferenceKind::DirectRgb => {
                direct_run_count += 1;
            }
            _ => {}
        }
    }
    Ok(json!({
        "source_sha256": sha256(&bytes),
        "source_bytes": bytes.len(),
        "publisher_eight_slot_scheme_run_count": scheme_run_count,
        "direct_rgb_run_count": direct_run_count,
        "scheme_slot_counts": slot_counts,
        "scheme_source_counts": source_counts,
        "raw_customer_text_emitted": false,
        "raw_rgb_emitted": false,
        "raw_source_offsets_emitted": false,
    }))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let initial = args.next().context(
        "usage: quill-scheme-color-probe <initial.pub> <switched.pub> <carlton.pub> <output.json>",
    )?;
    let switched = args.next().context("missing switched.pub")?;
    let carlton = args.next().context("missing carlton.pub")?;
    let output = args.next().context("missing output.json")?;
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }

    let initial_map = synthetic_map(Path::new(&initial))?;
    let switched_map = synthetic_map(Path::new(&switched))?;
    let initial_slots = scheme_slots(&initial_map)?;
    let switched_slots = scheme_slots(&switched_map)?;
    ensure!(
        initial_slots == switched_slots,
        "persisted scheme-slot mapping changed across document ColorScheme switch: {initial_slots:?} -> {switched_slots:?}"
    );

    let mut role_to_scheme_slot = Vec::new();
    for (index, slot) in initial_slots.iter().copied().enumerate() {
        role_to_scheme_slot.push(json!({
            "com_scheme_role": index + 1,
            "persisted_scheme_slot": slot,
        }));
    }

    let out = json!({
        "schema": SCHEMA,
        "synthetic_initial": initial_map,
        "synthetic_switched": switched_map,
        "role_to_persisted_scheme_slot": role_to_scheme_slot,
        "scheme_slot_mapping_stable_across_switch": true,
        "carlton": carlton_summary(Path::new(&carlton))?,
        "authority": "research carrier observation; COM semantics are joined by the enclosing native receipt",
    });

    fs::write(&output, serde_json::to_string_pretty(&out)? + "\n")
        .with_context(|| format!("write {}", Path::new(&output).display()))?;
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}
