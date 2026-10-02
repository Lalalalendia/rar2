use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

use anyhow::{Context, Result};
use pub_core::StreamPath;
use pub_escher::{PublisherFieldRecord, SpContainerObservation, inspect_sp_containers};
use serde::Serialize;
use sha2::{Digest, Sha256};

const ESCHER_STREAM: &str = "/Escher/EscherStm";
const TABLE_OWNER_REF: u16 = 0x6802;
const CELL_ORDINAL: u16 = 0x2003;
const BORDER_FIELDS: [u16; 5] = [0x2001, 0x2004, 0x2005, 0x2006, 0x2007];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CarrierKey {
    shape_type: u16,
    anchor: Vec<(u16, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FoptState {
    by_property: BTreeMap<u16, Vec<(u32, bool, bool)>>,
}

#[derive(Debug, Serialize)]
struct ProfileReceipt {
    schema: &'static str,
    source_sha256: String,
    border_carrier_count: usize,
    anchor_signature_histogram: BTreeMap<String, usize>,
    fopt_property_id_signature_histogram: BTreeMap<String, usize>,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct DiffReceipt {
    schema: &'static str,
    before_sha256: String,
    after_sha256: String,
    matched_carrier_count: usize,
    changed_carrier_count: usize,
    removed_carrier_count: usize,
    added_carrier_count: usize,
    changed_anchor_signature_histogram: BTreeMap<String, usize>,
    changed_property_id_histogram: BTreeMap<String, usize>,
    removed_anchor_signature_histogram: BTreeMap<String, usize>,
    added_anchor_signature_histogram: BTreeMap<String, usize>,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    raw_anchor_values_emitted: bool,
    raw_property_values_emitted: bool,
    object_ids_emitted: bool,
    coordinates_emitted: bool,
    colors_emitted: bool,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let value = matches.next()?.value;
    matches.next().is_none().then_some(value)
}

fn is_border_carrier(shape: &SpContainerObservation) -> bool {
    let Some(anchor) = shape.client_anchor.as_ref() else {
        return false;
    };
    if unique_field(anchor, TABLE_OWNER_REF).is_none() {
        return false;
    }
    if anchor.fields.iter().any(|field| field.id == CELL_ORDINAL) {
        return false;
    }
    anchor
        .fields
        .iter()
        .any(|field| BORDER_FIELDS.contains(&field.id))
}

fn anchor_signature_from_key(key: &CarrierKey) -> String {
    let mut ids = key.anchor.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .map(|id| format!("0x{id:04x}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn fopt_id_signature(state: &FoptState) -> String {
    state
        .by_property
        .keys()
        .map(|id| format!("0x{id:04x}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn carrier_key(shape: &SpContainerObservation) -> Option<CarrierKey> {
    let anchor = shape.client_anchor.as_ref()?;
    let shape_type = shape.fsp.as_ref()?.shape_type;
    let mut fields = anchor
        .fields
        .iter()
        .map(|field| (field.id, field.value))
        .collect::<Vec<_>>();
    fields.sort_unstable();
    Some(CarrierKey {
        shape_type,
        anchor: fields,
    })
}

fn fopt_state(shape: &SpContainerObservation) -> FoptState {
    let mut by_property = BTreeMap::<u16, Vec<(u32, bool, bool)>>::new();
    for property in shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
    {
        by_property
            .entry(property.property_id())
            .or_default()
            .push((property.op, property.f_bid(), property.f_complex()));
    }
    for values in by_property.values_mut() {
        values.sort_unstable();
    }
    FoptState { by_property }
}

fn read_carriers(path: &PathBuf) -> Result<(String, BTreeMap<CarrierKey, FoptState>)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let escher = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_STREAM)
        .context("read EscherStm")?;
    let inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM.into()), &escher)
        .context("inspect SpContainers")?;

    let mut carriers = BTreeMap::new();
    for shape in inventory.shapes.iter().filter(|shape| is_border_carrier(shape)) {
        let key = carrier_key(shape).context("border carrier has no stable ClientAnchor/FSP key")?;
        anyhow::ensure!(
            carriers.insert(key, fopt_state(shape)).is_none(),
            "duplicate border-carrier anchor key"
        );
    }

    Ok((sha256_hex(&bytes), carriers))
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn profile(input: PathBuf, output: PathBuf) -> Result<()> {
    let (source_sha256, carriers) = read_carriers(&input)?;
    let mut anchor_signature_histogram = BTreeMap::new();
    let mut fopt_property_id_signature_histogram = BTreeMap::new();
    for (key, state) in &carriers {
        bump(
            &mut anchor_signature_histogram,
            anchor_signature_from_key(key),
        );
        bump(
            &mut fopt_property_id_signature_histogram,
            fopt_id_signature(state),
        );
    }

    let receipt = ProfileReceipt {
        schema: "chaptera.table-border-carrier-profile.v1",
        source_sha256,
        border_carrier_count: carriers.len(),
        anchor_signature_histogram,
        fopt_property_id_signature_histogram,
        claims: Claims {
            raw_anchor_values_emitted: false,
            raw_property_values_emitted: false,
            object_ids_emitted: false,
            coordinates_emitted: false,
            colors_emitted: false,
        },
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

fn changed_property_ids(before: &FoptState, after: &FoptState) -> BTreeSet<u16> {
    let ids = before
        .by_property
        .keys()
        .chain(after.by_property.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    ids.into_iter()
        .filter(|id| before.by_property.get(id) != after.by_property.get(id))
        .collect()
}

fn diff(before: PathBuf, after: PathBuf, output: PathBuf) -> Result<()> {
    let (before_sha256, before_map) = read_carriers(&before)?;
    let (after_sha256, after_map) = read_carriers(&after)?;

    let mut matched_carrier_count = 0usize;
    let mut changed_carrier_count = 0usize;
    let mut changed_anchor_signature_histogram = BTreeMap::new();
    let mut changed_property_id_histogram = BTreeMap::new();

    for (key, before_state) in &before_map {
        let Some(after_state) = after_map.get(key) else {
            continue;
        };
        matched_carrier_count += 1;
        if before_state == after_state {
            continue;
        }
        changed_carrier_count += 1;
        bump(
            &mut changed_anchor_signature_histogram,
            anchor_signature_from_key(key),
        );
        for id in changed_property_ids(before_state, after_state) {
            bump(&mut changed_property_id_histogram, format!("0x{id:04x}"));
        }
    }

    let mut removed_anchor_signature_histogram = BTreeMap::new();
    for key in before_map.keys().filter(|key| !after_map.contains_key(*key)) {
        bump(
            &mut removed_anchor_signature_histogram,
            anchor_signature_from_key(key),
        );
    }
    let mut added_anchor_signature_histogram = BTreeMap::new();
    for key in after_map.keys().filter(|key| !before_map.contains_key(*key)) {
        bump(
            &mut added_anchor_signature_histogram,
            anchor_signature_from_key(key),
        );
    }

    let receipt = DiffReceipt {
        schema: "chaptera.table-border-carrier-diff.v1",
        before_sha256,
        after_sha256,
        matched_carrier_count,
        changed_carrier_count,
        removed_carrier_count: before_map.len() - matched_carrier_count,
        added_carrier_count: after_map.len() - matched_carrier_count,
        changed_anchor_signature_histogram,
        changed_property_id_histogram,
        removed_anchor_signature_histogram,
        added_anchor_signature_histogram,
        claims: Claims {
            raw_anchor_values_emitted: false,
            raw_property_values_emitted: false,
            object_ids_emitted: false,
            coordinates_emitted: false,
            colors_emitted: false,
        },
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

fn main() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    match args.as_slice() {
        [_, command, input, output] if command == "profile" => {
            profile(PathBuf::from(input), PathBuf::from(output))
        }
        [_, command, before, after, output] if command == "diff" => diff(
            PathBuf::from(before),
            PathBuf::from(after),
            PathBuf::from(output),
        ),
        _ => anyhow::bail!(
            "usage: table_border_carrier_oracle_tool profile INPUT.pub OUT.json | diff BEFORE.pub AFTER.pub OUT.json"
        ),
    }
}
