use super::{
    ByteRangeDeltaV1, ExperimentManifestV1, PubReReceiptV1, analyze_manifest, load_input,
    stream_bytes, validate_manifest,
};
use anyhow::{Context, Result, bail};
use pub_core::{RawSpan, StreamPath};
use pub_escher::{OfficeArtBody, OfficeArtRecord, parse_officeart_stream};
use serde::Serialize;
use std::{fs, path::Path};

pub const OFFICEART_ATTRIBUTION_SCHEMA_V1: &str =
    "chaptera.pub-re-officeart-attribution.v1";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OfficeArtAttributionReceiptV1 {
    pub schema: &'static str,
    pub analyzer_version: &'static str,
    pub experiment_id: String,
    pub question: String,
    pub stream: String,
    pub changed_ranges_truncated: bool,
    pub before_parse: OfficeArtParseStatusV1,
    pub after_parse: OfficeArtParseStatusV1,
    pub ranges: Vec<OfficeArtRangeAttributionV1>,
    pub invariants: OfficeArtAttributionInvariantsV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OfficeArtParseStatusV1 {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OfficeArtRangeAttributionV1 {
    pub start: u64,
    pub before_len: u64,
    pub after_len: u64,
    pub before_candidates: Vec<OfficeArtCandidateV1>,
    pub after_candidates: Vec<OfficeArtCandidateV1>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OfficeArtCandidateV1 {
    pub kind: &'static str,
    pub span_start: u64,
    pub span_len: u64,
    pub record_depth: u32,
    pub record_type: u16,
    pub record_instance: u16,
    pub record_version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_complex: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher_field_id: Option<u16>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OfficeArtAttributionInvariantsV1 {
    pub decoder_selected_explicitly: bool,
    pub nearest_offset_guessing_used: bool,
    pub raw_byte_values_emitted: bool,
    pub ambiguous_overlaps_preserved: bool,
}

pub fn attribute_officeart_manifest_file(
    manifest_path: &Path,
    stream_path: &str,
) -> Result<OfficeArtAttributionReceiptV1> {
    if !stream_path.starts_with('/') {
        bail!("OfficeArt stream path must be an absolute logical CFB path");
    }

    let source = fs::read(manifest_path)
        .with_context(|| format!("read manifest {}", manifest_path.display()))?;
    let manifest: ExperimentManifestV1 = serde_json::from_slice(&source)
        .with_context(|| format!("parse manifest {}", manifest_path.display()))?;
    validate_manifest(&manifest)?;
    let base_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));

    let core_receipt = analyze_manifest(&manifest, base_dir)?;
    attribute_officeart(&manifest, base_dir, &core_receipt, stream_path)
}

fn attribute_officeart(
    manifest: &ExperimentManifestV1,
    base_dir: &Path,
    core_receipt: &PubReReceiptV1,
    stream_path: &str,
) -> Result<OfficeArtAttributionReceiptV1> {
    let changed = core_receipt
        .cfb
        .changed_streams
        .iter()
        .find(|stream| stream.path == stream_path)
        .with_context(|| format!("requested stream {stream_path} has no logical payload delta"))?;

    let before = load_input(&manifest.before, base_dir, "before", &manifest.policy)?;
    let after = load_input(&manifest.after, base_dir, "after", &manifest.policy)?;
    let before_bytes = stream_bytes(&before, stream_path)
        .with_context(|| format!("read before OfficeArt stream {stream_path}"))?;
    let after_bytes = stream_bytes(&after, stream_path)
        .with_context(|| format!("read after OfficeArt stream {stream_path}"))?;

    let before_parsed =
        parse_officeart_stream(StreamPath(stream_path.to_owned()), &before_bytes);
    let after_parsed = parse_officeart_stream(StreamPath(stream_path.to_owned()), &after_bytes);

    let before_parse = parse_status(&before_parsed);
    let after_parse = parse_status(&after_parsed);

    let ranges = changed
        .changed_ranges
        .iter()
        .map(|range| OfficeArtRangeAttributionV1 {
            start: range.start,
            before_len: range.before_len,
            after_len: range.after_len,
            before_candidates: before_parsed
                .as_ref()
                .map(|parsed| candidates_for_range(&parsed.records, range.start, range.before_len))
                .unwrap_or_default(),
            after_candidates: after_parsed
                .as_ref()
                .map(|parsed| candidates_for_range(&parsed.records, range.start, range.after_len))
                .unwrap_or_default(),
        })
        .collect();

    Ok(OfficeArtAttributionReceiptV1 {
        schema: OFFICEART_ATTRIBUTION_SCHEMA_V1,
        analyzer_version: env!("CARGO_PKG_VERSION"),
        experiment_id: manifest.experiment_id.clone(),
        question: manifest.question.clone(),
        stream: stream_path.to_owned(),
        changed_ranges_truncated: changed.changed_ranges_truncated,
        before_parse,
        after_parse,
        ranges,
        invariants: OfficeArtAttributionInvariantsV1 {
            decoder_selected_explicitly: true,
            nearest_offset_guessing_used: false,
            raw_byte_values_emitted: false,
            ambiguous_overlaps_preserved: true,
        },
    })
}

fn parse_status<T, E: std::fmt::Display>(result: &Result<T, E>) -> OfficeArtParseStatusV1 {
    match result {
        Ok(_) => OfficeArtParseStatusV1 {
            status: "ok",
            error: None,
        },
        Err(error) => OfficeArtParseStatusV1 {
            status: "error",
            error: Some(error.to_string()),
        },
    }
}

fn candidates_for_range(
    records: &[OfficeArtRecord],
    start: u64,
    len: u64,
) -> Vec<OfficeArtCandidateV1> {
    let mut output = Vec::new();
    collect_candidates(records, 0, start, len, &mut output);
    output.sort_by(|left, right| {
        (
            left.span_len,
            left.span_start,
            left.kind,
            left.property_id,
            left.publisher_field_id,
        )
            .cmp(&(
                right.span_len,
                right.span_start,
                right.kind,
                right.property_id,
                right.publisher_field_id,
            ))
    });
    output
}

fn collect_candidates(
    records: &[OfficeArtRecord],
    depth: u32,
    start: u64,
    len: u64,
    output: &mut Vec<OfficeArtCandidateV1>,
) {
    for record in records {
        push_span_candidate(
            output,
            "record_header",
            &record.header.source,
            depth,
            record,
            start,
            len,
            None,
            None,
            None,
        );
        push_span_candidate(
            output,
            "record_payload",
            &record.payload_source,
            depth,
            record,
            start,
            len,
            None,
            None,
            None,
        );
        if let Some(span) = record.sibling_tail_source.as_ref() {
            push_span_candidate(
                output,
                "record_sibling_tail",
                span,
                depth,
                record,
                start,
                len,
                None,
                None,
                None,
            );
        }

        match &record.body {
            OfficeArtBody::Container { children } => {
                collect_candidates(children, depth + 1, start, len, output);
            }
            OfficeArtBody::Fopt(fopt) => {
                for property in &fopt.properties {
                    push_span_candidate(
                        output,
                        "fopt_property",
                        &property.source,
                        depth,
                        record,
                        start,
                        len,
                        Some(property.property_id()),
                        Some(property.f_complex()),
                        None,
                    );
                    if let Some(span) = property.complex_source.as_ref() {
                        push_span_candidate(
                            output,
                            "fopt_complex_data",
                            span,
                            depth,
                            record,
                            start,
                            len,
                            Some(property.property_id()),
                            Some(true),
                            None,
                        );
                    }
                }
                if let Some(span) = fopt.trailing_source.as_ref() {
                    push_span_candidate(
                        output,
                        "fopt_trailing",
                        span,
                        depth,
                        record,
                        start,
                        len,
                        None,
                        None,
                        None,
                    );
                }
            }
            OfficeArtBody::PublisherFields(fields) => {
                push_span_candidate(
                    output,
                    "publisher_duplicated_length",
                    &fields.duplicated_length_source,
                    depth,
                    record,
                    start,
                    len,
                    None,
                    None,
                    None,
                );
                for field in &fields.fields {
                    push_span_candidate(
                        output,
                        "publisher_field",
                        &field.source,
                        depth,
                        record,
                        start,
                        len,
                        None,
                        None,
                        Some(field.id),
                    );
                }
                if let Some(span) = fields.trailing_source.as_ref() {
                    push_span_candidate(
                        output,
                        "publisher_field_trailing",
                        span,
                        depth,
                        record,
                        start,
                        len,
                        None,
                        None,
                        None,
                    );
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_span_candidate(
    output: &mut Vec<OfficeArtCandidateV1>,
    kind: &'static str,
    span: &RawSpan,
    depth: u32,
    record: &OfficeArtRecord,
    changed_start: u64,
    changed_len: u64,
    property_id: Option<u16>,
    property_complex: Option<bool>,
    publisher_field_id: Option<u16>,
) {
    if !span_intersects(span, changed_start, changed_len) {
        return;
    }
    output.push(OfficeArtCandidateV1 {
        kind,
        span_start: span.offset,
        span_len: span.len,
        record_depth: depth,
        record_type: record.header.rec_type,
        record_instance: record.header.rec_instance,
        record_version: record.header.rec_ver,
        property_id,
        property_complex,
        publisher_field_id,
    });
}

fn span_intersects(span: &RawSpan, changed_start: u64, changed_len: u64) -> bool {
    let Some(span_end) = span.end() else {
        return false;
    };
    if changed_len == 0 {
        return span.offset <= changed_start && changed_start <= span_end;
    }
    let Some(changed_end) = changed_start.checked_add(changed_len) else {
        return false;
    };
    span.offset < changed_end && changed_start < span_end
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_escher::OFFICE_ART_FOPT;

    fn one_property_fopt(opid: u16, op: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let initial = (1u16 << 4) | 0x3;
        bytes.extend_from_slice(&initial.to_le_bytes());
        bytes.extend_from_slice(&OFFICE_ART_FOPT.to_le_bytes());
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.extend_from_slice(&opid.to_le_bytes());
        bytes.extend_from_slice(&op.to_le_bytes());
        bytes
    }

    #[test]
    fn exact_fopt_delta_maps_to_property_without_byte_values() {
        let bytes = one_property_fopt(0x01bf, 0x1122_3344);
        let parsed = parse_officeart_stream(StreamPath("/Escher".to_owned()), &bytes)
            .expect("synthetic FOPT should parse");

        let candidates = candidates_for_range(&parsed.records, 10, 1);
        assert!(candidates.iter().any(|candidate| {
            candidate.kind == "fopt_property"
                && candidate.property_id == Some(0x01bf)
                && candidate.span_start == 8
                && candidate.span_len == 6
        }));

        let json = serde_json::to_string(&candidates).expect("serialize candidates");
        assert!(!json.contains("11223344"));
        assert!(!json.contains("44"));
    }

    #[test]
    fn zero_length_insertion_point_can_still_bind_to_enclosing_span() {
        let bytes = one_property_fopt(0x01bf, 1);
        let parsed = parse_officeart_stream(StreamPath("/Escher".to_owned()), &bytes)
            .expect("synthetic FOPT should parse");

        let candidates = candidates_for_range(&parsed.records, 9, 0);
        assert!(candidates
            .iter()
            .any(|candidate| candidate.kind == "fopt_property"));
    }
}
