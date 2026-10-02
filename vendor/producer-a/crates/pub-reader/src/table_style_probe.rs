use super::*;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PubTableMcldStyleSignatureClass {
    LayoutKeyMissing,
    McldUnavailable,
    RecordMissing,
    StyleRangeAbsent,
    StyleRangeUniform,
    StyleRangeVariesByChild,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubTableMcldStyleObservation {
    pub contents_seq_num: u32,
    pub table_cell_count: usize,
    pub layout_key_present: bool,
    pub mcld_record_present: bool,
    pub mcld_child_count: usize,
    pub child_count_matches_cell_count: bool,
    pub signature_class: PubTableMcldStyleSignatureClass,
    pub style_field_child_presence: BTreeMap<String, usize>,
    pub control_field_child_presence: BTreeMap<String, usize>,
}

fn field_key(id: u8, wire_type: u8) -> String {
    format!("0x{id:02x}/wire_0x{wire_type:02x}")
}

/// Research-only TABLE -> MCLD field-presence observation.
///
/// This deliberately emits no MCLD field values and assigns no table-style,
/// fill, border-side, color, or width semantics to the open 0x1D..0x2C range.
/// It exists only to select the next bounded discriminator.
pub fn analyze_mature_0x2c_table_mcld_style_fields<R: Read + Seek>(
    mut reader: R,
    source_hash: Sha256Digest,
) -> Result<Vec<PubTableMcldStyleObservation>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let build = build_mature_0x2c_source_graph(Cursor::new(pub_bytes.as_slice()), source_hash)
        .context("build mature source graph for TABLE MCLD style observation")?;

    let quill = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), QUILL_STREAM_PATH)
        .with_context(|| {
            format!("read {QUILL_STREAM_PATH} for TABLE MCLD style observation")
        })?;
    let quill_stream = StreamPath(QUILL_STREAM_PATH.into());
    let catalog = parse_confirmed_story_catalog(quill_stream.clone(), &quill)
        .context("parse Quill story catalog for TABLE MCLD style observation")?;
    let mcld = match parse_bounded_mcld(quill_stream, &quill, &catalog.descriptor_nodes) {
        Ok(mcld) => Some(mcld),
        Err(QuillMcldReadError::MissingMcldDescriptor) => None,
        Err(error) => return Err(error).context("parse bounded MCLD for TABLE style observation"),
    };

    let mut diagnostic_layout_key_by_seq = BTreeMap::<u32, Option<u32>>::new();
    for diagnostic in &build.diagnostics {
        if let PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
            seq_num,
            layout_key,
            ..
        } = diagnostic
        {
            diagnostic_layout_key_by_seq.insert(*seq_num, *layout_key);
        }
    }

    let mut observations = Vec::new();
    for node in build.graph.nodes.values() {
        let Some(table) = node.payload.table.as_ref() else {
            continue;
        };
        let seq_num = node.payload.contents_seq_num;
        let layout_key = table
            .layout_metrics
            .as_ref()
            .map(|metrics| metrics.story_layout_key)
            .or_else(|| {
                diagnostic_layout_key_by_seq
                    .get(&seq_num)
                    .copied()
                    .flatten()
            });

        let Some(layout_key) = layout_key else {
            observations.push(PubTableMcldStyleObservation {
                contents_seq_num: seq_num,
                table_cell_count: table.cells.len(),
                layout_key_present: false,
                mcld_record_present: false,
                mcld_child_count: 0,
                child_count_matches_cell_count: false,
                signature_class: PubTableMcldStyleSignatureClass::LayoutKeyMissing,
                style_field_child_presence: BTreeMap::new(),
                control_field_child_presence: BTreeMap::new(),
            });
            continue;
        };

        let Some(mcld) = mcld.as_ref() else {
            observations.push(PubTableMcldStyleObservation {
                contents_seq_num: seq_num,
                table_cell_count: table.cells.len(),
                layout_key_present: true,
                mcld_record_present: false,
                mcld_child_count: 0,
                child_count_matches_cell_count: false,
                signature_class: PubTableMcldStyleSignatureClass::McldUnavailable,
                style_field_child_presence: BTreeMap::new(),
                control_field_child_presence: BTreeMap::new(),
            });
            continue;
        };

        let Some(record) = mcld
            .records
            .iter()
            .find(|record| record.record_id == layout_key)
        else {
            observations.push(PubTableMcldStyleObservation {
                contents_seq_num: seq_num,
                table_cell_count: table.cells.len(),
                layout_key_present: true,
                mcld_record_present: false,
                mcld_child_count: 0,
                child_count_matches_cell_count: false,
                signature_class: PubTableMcldStyleSignatureClass::RecordMissing,
                style_field_child_presence: BTreeMap::new(),
                control_field_child_presence: BTreeMap::new(),
            });
            continue;
        };

        let mut style_field_child_presence = BTreeMap::<String, usize>::new();
        let mut control_field_child_presence = BTreeMap::<String, usize>::new();
        let mut child_signatures = BTreeSet::<Vec<String>>::new();
        let mut any_style_field = false;

        for child in &record.children {
            let style_signature = child
                .fields
                .iter()
                .filter(|field| (0x1d..=0x2c).contains(&field.id))
                .map(|field| field_key(field.id, field.wire_type))
                .collect::<BTreeSet<_>>();
            any_style_field |= !style_signature.is_empty();
            for key in &style_signature {
                *style_field_child_presence.entry(key.clone()).or_default() += 1;
            }
            child_signatures.insert(style_signature.into_iter().collect());

            let control_signature = child
                .fields
                .iter()
                .filter(|field| (0x04..=0x09).contains(&field.id))
                .map(|field| field_key(field.id, field.wire_type))
                .collect::<BTreeSet<_>>();
            for key in control_signature {
                *control_field_child_presence.entry(key).or_default() += 1;
            }
        }

        let signature_class = if !any_style_field {
            PubTableMcldStyleSignatureClass::StyleRangeAbsent
        } else if child_signatures.len() == 1 {
            PubTableMcldStyleSignatureClass::StyleRangeUniform
        } else {
            PubTableMcldStyleSignatureClass::StyleRangeVariesByChild
        };

        observations.push(PubTableMcldStyleObservation {
            contents_seq_num: seq_num,
            table_cell_count: table.cells.len(),
            layout_key_present: true,
            mcld_record_present: true,
            mcld_child_count: record.children.len(),
            child_count_matches_cell_count: record.children.len() == table.cells.len(),
            signature_class,
            style_field_child_presence,
            control_field_child_presence,
        });
    }

    observations.sort_by_key(|observation| observation.contents_seq_num);
    Ok(observations)
}
