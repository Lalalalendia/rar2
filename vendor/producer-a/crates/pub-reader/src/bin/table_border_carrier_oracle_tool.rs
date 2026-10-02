use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

use anyhow::{Context, Result};
use pub_core::StreamPath;
use pub_escher::{inspect_sp_containers, PublisherFieldRecord, SpContainerObservation};
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FoptState {
    by_property: BTreeMap<u16, Vec<(u32, bool, bool)>>,
}

#[derive(Debug, Serialize)]
struct ProfileReceipt {
    schema: &'static str,
    source_sha256: String,
    border_carrier_count: usize,
    duplicate_anchor_group_count: usize,
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
    ambiguous_changed_group_count: usize,
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

fn read_carriers(path: &PathBuf) -> Result<(String, BTreeMap<CarrierKey, Vec<FoptState>>)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let escher = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_STREAM)
        .context("read EscherStm")?;
    let inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM.into()), &escher)
        .context("inspect SpContainers")?;

    let mut carriers = BTreeMap::<CarrierKey, Vec<FoptState>>::new();
    for shape in inventory
        .shapes
        .iter()
        .filter(|shape| is_border_carrier(shape))
    {
        let key =
            carrier_key(shape).context("border carrier has no stable ClientAnchor/FSP key")?;
        carriers.entry(key).or_default().push(fopt_state(shape));
    }
    for states in carriers.values_mut() {
        states.sort_unstable();
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
    let border_carrier_count = carriers.values().map(Vec::len).sum();
    let duplicate_anchor_group_count = carriers.values().filter(|states| states.len() > 1).count();
    for (key, states) in &carriers {
        for state in states {
            bump(
                &mut anchor_signature_histogram,
                anchor_signature_from_key(key),
            );
            bump(
                &mut fopt_property_id_signature_histogram,
                fopt_id_signature(state),
            );
        }
    }

    let receipt = ProfileReceipt {
        schema: "chaptera.table-border-carrier-profile.v1",
        source_sha256,
        border_carrier_count,
        duplicate_anchor_group_count,
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

fn subtract_exact_matches(
    before: &[FoptState],
    after: &[FoptState],
) -> (usize, Vec<FoptState>, Vec<FoptState>) {
    let mut after_remaining = after.to_vec();
    let mut before_remaining = Vec::new();
    let mut exact_matches = 0usize;

    for state in before {
        if let Some(index) = after_remaining.iter().position(|candidate| candidate == state) {
            after_remaining.remove(index);
            exact_matches += 1;
        } else {
            before_remaining.push(state.clone());
        }
    }

    (exact_matches, before_remaining, after_remaining)
}

fn diff(before: PathBuf, after: PathBuf, output: PathBuf) -> Result<()> {
    let (before_sha256, before_map) = read_carriers(&before)?;
    let (after_sha256, after_map) = read_carriers(&after)?;

    let keys = before_map
        .keys()
        .chain(after_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut matched_carrier_count = 0usize;
    let mut changed_carrier_count = 0usize;
    let mut removed_carrier_count = 0usize;
    let mut added_carrier_count = 0usize;
    let mut ambiguous_changed_group_count = 0usize;
    let mut changed_anchor_signature_histogram = BTreeMap::new();
    let mut changed_property_id_histogram = BTreeMap::new();
    let mut removed_anchor_signature_histogram = BTreeMap::new();
    let mut added_anchor_signature_histogram = BTreeMap::new();

    for key in keys {
        let before_states = before_map.get(&key).map(Vec::as_slice).unwrap_or_default();
        let after_states = after_map.get(&key).map(Vec::as_slice).unwrap_or_default();
        let (exact_matches, mut before_remaining, mut after_remaining) =
            subtract_exact_matches(before_states, after_states);
        matched_carrier_count += exact_matches;

        before_remaining.sort_unstable();
        after_remaining.sort_unstable();

        let changed_pairs = before_remaining.len().min(after_remaining.len());
        if changed_pairs > 1 {
            ambiguous_changed_group_count += 1;
        }

        for index in 0..changed_pairs {
            let before_state = &before_remaining[index];
            let after_state = &after_remaining[index];
            matched_carrier_count += 1;
            changed_carrier_count += 1;
            bump(
                &mut changed_anchor_signature_histogram,
                anchor_signature_from_key(&key),
            );
            for id in changed_property_ids(before_state, after_state) {
                bump(&mut changed_property_id_histogram, format!("0x{id:04x}"));
            }
        }

        for _ in changed_pairs..before_remaining.len() {
            removed_carrier_count += 1;
            bump(
                &mut removed_anchor_signature_histogram,
                anchor_signature_from_key(&key),
            );
        }
        for _ in changed_pairs..after_remaining.len() {
            added_carrier_count += 1;
            bump(
                &mut added_anchor_signature_histogram,
                anchor_signature_from_key(&key),
            );
        }
    }

    let receipt = DiffReceipt {
        schema: "chaptera.table-border-carrier-diff.v1",
        before_sha256,
        after_sha256,
        matched_carrier_count,
        changed_carrier_count,
        removed_carrier_count,
        added_carrier_count,
        ambiguous_changed_group_count,
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


#[cfg(test)]
mod tests {
    use super::*;

    fn state(property: u16, op: u32) -> FoptState {
        FoptState {
            by_property: BTreeMap::from([(property, vec![(op, false, false)])]),
        }
    }

    #[test]
    fn exact_match_subtraction_preserves_duplicate_anchor_multiplicity() {
        let before = vec![state(0x0181, 1), state(0x01c0, 2)];
        let after = vec![state(0x0181, 1), state(0x01c0, 3)];
        let (exact, before_remaining, after_remaining) =
            subtract_exact_matches(&before, &after);

        assert_eq!(exact, 1);
        assert_eq!(before_remaining, vec![state(0x01c0, 2)]);
        assert_eq!(after_remaining, vec![state(0x01c0, 3)]);
    }
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
