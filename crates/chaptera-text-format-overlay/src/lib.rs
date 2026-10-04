//! Canonical Chaptera-owned character-format overlay.
//!
//! This is the Rust product-authority port of the proven
//! `chaptera.text-format-overlay.v1` semantic law. Source/base formatting is
//! immutable input; Chaptera owns only explicit property overrides over
//! canonical Unicode-scalar ranges.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;

pub const OVERLAY_PROTOCOL_V1: &str = "chaptera.text-format-overlay.v1";
pub const OPERATION_PROTOCOL_V1: &str = "chaptera.text-format-operation.v1";
pub const RECEIPT_PROTOCOL_V1: &str = "chaptera.text-format-operation-receipt.v1";
pub const EXPORT_POLICY_V1: &str = "chaptera_override_or_explicit_loss";
const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFormatOverlayError {
    message: String,
}

impl TextFormatOverlayError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TextFormatOverlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TextFormatOverlayError {}

type Result<T> = std::result::Result<T, TextFormatOverlayError>;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FormatPropertyV1 {
    Bold,
    FontSizeEmu,
    Italic,
    TextColorRgb,
}

impl FormatPropertyV1 {
    pub const ALL: [Self; 4] = [
        Self::Bold,
        Self::FontSizeEmu,
        Self::Italic,
        Self::TextColorRgb,
    ];
}

impl TryFrom<&str> for FormatPropertyV1 {
    type Error = TextFormatOverlayError;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "bold" => Ok(Self::Bold),
            "font_size_emu" => Ok(Self::FontSizeEmu),
            "italic" => Ok(Self::Italic),
            "text_color_rgb" => Ok(Self::TextColorRgb),
            _ => Err(TextFormatOverlayError::new(
                "unsupported character-format property",
            )),
        }
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(untagged)]
pub enum FormatValueV1 {
    Bool(bool),
    Integer(u64),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseCharacterFormatV1 {
    pub font_resource_id: String,
    pub font_size_emu: u64,
    pub bold: bool,
    pub italic: bool,
    pub text_color_rgb: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseFormatRunV1 {
    pub start_scalar: u32,
    pub end_scalar: u32,
    pub format: BaseCharacterFormatV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatOverrideRunV1 {
    pub start_scalar: u32,
    pub end_scalar: u32,
    pub property: FormatPropertyV1,
    pub value: FormatValueV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatOverlayStateV1 {
    pub protocol_version: String,
    pub story_id: String,
    pub base_revision_id: String,
    pub story_scalar_len: u32,
    pub base_runs: Vec<BaseFormatRunV1>,
    pub overrides: Vec<TextFormatOverrideRunV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectivePropertySourceV1 {
    Base,
    ChapteraOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectivePropertySegmentV1 {
    pub start_scalar: u32,
    pub end_scalar: u32,
    pub property: FormatPropertyV1,
    pub value: FormatValueV1,
    pub source: EffectivePropertySourceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFormatOperationKindV1 {
    SetTextFormatProperty,
    ClearTextFormatPropertyOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatCommandV1 {
    pub protocol_version: String,
    pub kind: TextFormatOperationKindV1,
    pub story_id: String,
    pub start_scalar: u32,
    pub end_scalar: u32,
    pub property: FormatPropertyV1,
    pub value: Option<FormatValueV1>,
    pub expected_state_hash: String,
    pub before_state_hash: String,
    pub after_state_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatOperationReceiptV1 {
    pub protocol_version: String,
    pub command: TextFormatCommandV1,
    pub before_state: TextFormatOverlayStateV1,
    pub after_state: TextFormatOverlayStateV1,
    pub before_effective: Vec<EffectivePropertySegmentV1>,
    pub after_effective: Vec<EffectivePropertySegmentV1>,
    pub requires_authoritative_relayout: bool,
    pub export_policy: String,
}

fn validate_rgb(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    if bytes.len() != 7
        || bytes.first() != Some(&b'#')
        || !bytes[1..].iter().all(u8::is_ascii_hexdigit)
    {
        return Err(TextFormatOverlayError::new(
            "text_color_rgb must be #RRGGBB",
        ));
    }
    Ok(value.to_ascii_uppercase())
}

fn validate_property_value(
    property: FormatPropertyV1,
    value: &FormatValueV1,
) -> Result<FormatValueV1> {
    match (property, value) {
        (FormatPropertyV1::FontSizeEmu, FormatValueV1::Integer(size))
            if *size > 0 && *size <= MAX_JS_SAFE_INTEGER =>
        {
            Ok(FormatValueV1::Integer(*size))
        }
        (FormatPropertyV1::FontSizeEmu, _) => Err(TextFormatOverlayError::new(
            "font_size_emu must be a positive JavaScript-safe integer",
        )),
        (FormatPropertyV1::Bold | FormatPropertyV1::Italic, FormatValueV1::Bool(value)) => {
            Ok(FormatValueV1::Bool(*value))
        }
        (FormatPropertyV1::Bold, _) => {
            Err(TextFormatOverlayError::new("bold must be boolean"))
        }
        (FormatPropertyV1::Italic, _) => {
            Err(TextFormatOverlayError::new("italic must be boolean"))
        }
        (FormatPropertyV1::TextColorRgb, FormatValueV1::String(value)) => {
            Ok(FormatValueV1::String(validate_rgb(value)?))
        }
        (FormatPropertyV1::TextColorRgb, _) => Err(TextFormatOverlayError::new(
            "text_color_rgb must be #RRGGBB",
        )),
    }
}

fn validate_base_format(format: &BaseCharacterFormatV1) -> Result<BaseCharacterFormatV1> {
    if format.font_resource_id.is_empty() {
        return Err(TextFormatOverlayError::new(
            "base formatting requires resolved font_resource_id",
        ));
    }
    if format.font_size_emu == 0 || format.font_size_emu > MAX_JS_SAFE_INTEGER {
        return Err(TextFormatOverlayError::new(
            "font_size_emu must be a positive JavaScript-safe integer",
        ));
    }
    Ok(BaseCharacterFormatV1 {
        font_resource_id: format.font_resource_id.clone(),
        font_size_emu: format.font_size_emu,
        bold: format.bold,
        italic: format.italic,
        text_color_rgb: validate_rgb(&format.text_color_rgb)?,
    })
}

fn validate_range(start: u32, end: u32, story_len: u32) -> Result<()> {
    if end <= start || end > story_len {
        return Err(TextFormatOverlayError::new(
            "format range must be non-empty and within Story scalar extent",
        ));
    }
    Ok(())
}

fn validate_base_runs(
    story_len: u32,
    base_runs: &[BaseFormatRunV1],
) -> Result<Vec<BaseFormatRunV1>> {
    if story_len == 0 {
        if !base_runs.is_empty() {
            return Err(TextFormatOverlayError::new(
                "empty Story must not carry base character-format runs",
            ));
        }
        return Ok(Vec::new());
    }
    if base_runs.is_empty() {
        return Err(TextFormatOverlayError::new(
            "non-empty Story requires explicit base formatting coverage",
        ));
    }

    let mut out = Vec::with_capacity(base_runs.len());
    let mut cursor = 0;
    for run in base_runs {
        if run.start_scalar != cursor {
            return Err(TextFormatOverlayError::new(
                "base formatting must cover Story contiguously without gaps",
            ));
        }
        if run.end_scalar <= run.start_scalar || run.end_scalar > story_len {
            return Err(TextFormatOverlayError::new(
                "base formatting run range is invalid",
            ));
        }
        out.push(BaseFormatRunV1 {
            start_scalar: run.start_scalar,
            end_scalar: run.end_scalar,
            format: validate_base_format(&run.format)?,
        });
        cursor = run.end_scalar;
    }
    if cursor != story_len {
        return Err(TextFormatOverlayError::new(
            "base formatting must cover full Story scalar extent",
        ));
    }
    Ok(out)
}

fn base_value_at(
    base_runs: &[BaseFormatRunV1],
    property: FormatPropertyV1,
    scalar: u32,
) -> Result<FormatValueV1> {
    let run = base_runs
        .iter()
        .find(|run| run.start_scalar <= scalar && scalar < run.end_scalar)
        .ok_or_else(|| {
            TextFormatOverlayError::new("base formatting does not cover requested scalar")
        })?;
    Ok(match property {
        FormatPropertyV1::Bold => FormatValueV1::Bool(run.format.bold),
        FormatPropertyV1::FontSizeEmu => {
            FormatValueV1::Integer(run.format.font_size_emu)
        }
        FormatPropertyV1::Italic => FormatValueV1::Bool(run.format.italic),
        FormatPropertyV1::TextColorRgb => {
            FormatValueV1::String(run.format.text_color_rgb.clone())
        }
    })
}

fn override_value_at(
    overrides: &[TextFormatOverrideRunV1],
    property: FormatPropertyV1,
    scalar: u32,
) -> Result<Option<FormatValueV1>> {
    let mut found = None;
    for run in overrides {
        if run.property != property || !(run.start_scalar <= scalar && scalar < run.end_scalar) {
            continue;
        }
        if found.is_some() {
            return Err(TextFormatOverlayError::new(
                "canonical override state contains overlap",
            ));
        }
        found = Some(run.value.clone());
    }
    Ok(found)
}

fn normalize_overrides(
    story_len: u32,
    base_runs: &[BaseFormatRunV1],
    overrides: &[TextFormatOverrideRunV1],
) -> Result<Vec<TextFormatOverrideRunV1>> {
    if story_len == 0 {
        if !overrides.is_empty() {
            return Err(TextFormatOverlayError::new(
                "empty Story cannot carry durable formatting overrides",
            ));
        }
        return Ok(Vec::new());
    }

    let mut validated = Vec::with_capacity(overrides.len());
    for run in overrides {
        validate_range(run.start_scalar, run.end_scalar, story_len)?;
        validated.push(TextFormatOverrideRunV1 {
            start_scalar: run.start_scalar,
            end_scalar: run.end_scalar,
            property: run.property,
            value: validate_property_value(run.property, &run.value)?,
        });
    }

    let mut normalized: Vec<TextFormatOverrideRunV1> = Vec::new();
    for property in FormatPropertyV1::ALL {
        let property_runs = validated
            .iter()
            .filter(|run| run.property == property)
            .collect::<Vec<_>>();
        let mut boundaries = BTreeSet::from([0, story_len]);
        for run in base_runs {
            boundaries.insert(run.start_scalar);
            boundaries.insert(run.end_scalar);
        }
        for run in &property_runs {
            boundaries.insert(run.start_scalar);
            boundaries.insert(run.end_scalar);
        }
        let points = boundaries.into_iter().collect::<Vec<_>>();

        for pair in points.windows(2) {
            let [start, end] = [pair[0], pair[1]];
            if start == end {
                continue;
            }
            let covering = property_runs
                .iter()
                .filter(|run| run.start_scalar <= start && end <= run.end_scalar)
                .collect::<Vec<_>>();
            if covering.len() > 1 {
                return Err(TextFormatOverlayError::new(
                    "format override runs for one property must not overlap",
                ));
            }
            let Some(run) = covering.first() else {
                continue;
            };
            let explicit = run.value.clone();
            if explicit == base_value_at(base_runs, property, start)? {
                continue;
            }

            if let Some(previous) = normalized.last_mut()
                && previous.property == property
                && previous.value == explicit
                && previous.end_scalar == start
            {
                previous.end_scalar = end;
            } else {
                normalized.push(TextFormatOverrideRunV1 {
                    start_scalar: start,
                    end_scalar: end,
                    property,
                    value: explicit,
                });
            }
        }
    }
    Ok(normalized)
}

pub fn build_text_format_overlay_state_v1(
    story_id: impl Into<String>,
    base_revision_id: impl Into<String>,
    story_scalar_len: u32,
    base_runs: Vec<BaseFormatRunV1>,
    overrides: Vec<TextFormatOverrideRunV1>,
) -> Result<TextFormatOverlayStateV1> {
    let story_id = story_id.into();
    let base_revision_id = base_revision_id.into();
    if story_id.is_empty() {
        return Err(TextFormatOverlayError::new("story_id is required"));
    }
    if base_revision_id.is_empty() {
        return Err(TextFormatOverlayError::new("base_revision_id is required"));
    }
    let base_runs = validate_base_runs(story_scalar_len, &base_runs)?;
    let overrides = normalize_overrides(story_scalar_len, &base_runs, &overrides)?;
    Ok(TextFormatOverlayStateV1 {
        protocol_version: OVERLAY_PROTOCOL_V1.to_owned(),
        story_id,
        base_revision_id,
        story_scalar_len,
        base_runs,
        overrides,
    })
}

pub fn effective_property_segments_v1(
    state: &TextFormatOverlayStateV1,
    property: FormatPropertyV1,
    start_scalar: u32,
    end_scalar: u32,
) -> Result<Vec<EffectivePropertySegmentV1>> {
    validate_range(start_scalar, end_scalar, state.story_scalar_len)?;
    let mut boundaries = BTreeSet::from([start_scalar, end_scalar]);
    for run in &state.base_runs {
        if start_scalar < run.end_scalar && run.start_scalar < end_scalar {
            boundaries.insert(start_scalar.max(run.start_scalar));
            boundaries.insert(end_scalar.min(run.end_scalar));
        }
    }
    for run in &state.overrides {
        if run.property == property
            && start_scalar < run.end_scalar
            && run.start_scalar < end_scalar
        {
            boundaries.insert(start_scalar.max(run.start_scalar));
            boundaries.insert(end_scalar.min(run.end_scalar));
        }
    }
    let points = boundaries.into_iter().collect::<Vec<_>>();
    let mut segments: Vec<EffectivePropertySegmentV1> = Vec::new();
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        let (value, source) = match override_value_at(&state.overrides, property, start)? {
            Some(value) => (value, EffectivePropertySourceV1::ChapteraOverride),
            None => (
                base_value_at(&state.base_runs, property, start)?,
                EffectivePropertySourceV1::Base,
            ),
        };
        if let Some(previous) = segments.last_mut()
            && previous.end_scalar == start
            && previous.value == value
            && previous.source == source
        {
            previous.end_scalar = end;
        } else {
            segments.push(EffectivePropertySegmentV1 {
                start_scalar: start,
                end_scalar: end,
                property,
                value,
                source,
            });
        }
    }
    Ok(segments)
}

fn trim_property_runs(
    overrides: &[TextFormatOverrideRunV1],
    property: FormatPropertyV1,
    start: u32,
    end: u32,
) -> Vec<TextFormatOverrideRunV1> {
    let mut out = Vec::new();
    for run in overrides {
        if run.property != property || run.end_scalar <= start || end <= run.start_scalar {
            out.push(run.clone());
            continue;
        }
        if run.start_scalar < start {
            out.push(TextFormatOverrideRunV1 {
                start_scalar: run.start_scalar,
                end_scalar: start,
                property: run.property,
                value: run.value.clone(),
            });
        }
        if end < run.end_scalar {
            out.push(TextFormatOverrideRunV1 {
                start_scalar: end,
                end_scalar: run.end_scalar,
                property: run.property,
                value: run.value.clone(),
            });
        }
    }
    out
}

fn canonical_json(value: &Value, output: &mut String) -> Result<()> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&value.to_string()),
        Value::String(value) => {
            let encoded = serde_json::to_string(value).map_err(|error| {
                TextFormatOverlayError::new(format!("could not encode canonical JSON: {error}"))
            })?;
            output.push_str(&encoded);
        }
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                canonical_json(value, output)?;
            }
            output.push(']');
        }
        Value::Object(map) => {
            output.push('{');
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                let encoded_key = serde_json::to_string(key).map_err(|error| {
                    TextFormatOverlayError::new(format!(
                        "could not encode canonical JSON key: {error}"
                    ))
                })?;
                output.push_str(&encoded_key);
                output.push(':');
                canonical_json(&map[key], output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

pub fn state_hash_v1(state: &TextFormatOverlayStateV1) -> Result<String> {
    let value = serde_json::to_value(state).map_err(|error| {
        TextFormatOverlayError::new(format!("could not serialize overlay state: {error}"))
    })?;
    let mut canonical = String::new();
    canonical_json(&value, &mut canonical)?;
    let digest = Sha256::digest(canonical.as_bytes());
    Ok(format!("{digest:x}"))
}

fn apply_format_operation_v1(
    state: &TextFormatOverlayStateV1,
    kind: TextFormatOperationKindV1,
    start_scalar: u32,
    end_scalar: u32,
    property: FormatPropertyV1,
    value: Option<FormatValueV1>,
    expected_state_hash: &str,
) -> Result<TextFormatOperationReceiptV1> {
    let before_state_hash = state_hash_v1(state)?;
    if expected_state_hash != before_state_hash {
        return Err(TextFormatOverlayError::new("stale format-overlay state"));
    }
    validate_range(start_scalar, end_scalar, state.story_scalar_len)?;
    let before_effective =
        effective_property_segments_v1(state, property, start_scalar, end_scalar)?;

    let mut provisional =
        trim_property_runs(&state.overrides, property, start_scalar, end_scalar);
    let normalized_value = match kind {
        TextFormatOperationKindV1::SetTextFormatProperty => {
            let value = value.ok_or_else(|| {
                TextFormatOverlayError::new("set_text_format_property requires a value")
            })?;
            let value = validate_property_value(property, &value)?;
            provisional.push(TextFormatOverrideRunV1 {
                start_scalar,
                end_scalar,
                property,
                value: value.clone(),
            });
            Some(value)
        }
        TextFormatOperationKindV1::ClearTextFormatPropertyOverride => {
            if value.is_some() {
                return Err(TextFormatOverlayError::new(
                    "clear_text_format_property_override must not carry a value",
                ));
            }
            None
        }
    };

    let after_state = build_text_format_overlay_state_v1(
        state.story_id.clone(),
        state.base_revision_id.clone(),
        state.story_scalar_len,
        state.base_runs.clone(),
        provisional,
    )?;
    let after_effective =
        effective_property_segments_v1(&after_state, property, start_scalar, end_scalar)?;
    let after_state_hash = state_hash_v1(&after_state)?;

    Ok(TextFormatOperationReceiptV1 {
        protocol_version: RECEIPT_PROTOCOL_V1.to_owned(),
        command: TextFormatCommandV1 {
            protocol_version: OPERATION_PROTOCOL_V1.to_owned(),
            kind,
            story_id: state.story_id.clone(),
            start_scalar,
            end_scalar,
            property,
            value: normalized_value,
            expected_state_hash: expected_state_hash.to_owned(),
            before_state_hash,
            after_state_hash,
        },
        before_state: state.clone(),
        after_state,
        before_effective,
        after_effective,
        requires_authoritative_relayout: true,
        export_policy: EXPORT_POLICY_V1.to_owned(),
    })
}

pub fn set_text_format_property_v1(
    state: &TextFormatOverlayStateV1,
    start_scalar: u32,
    end_scalar: u32,
    property: FormatPropertyV1,
    value: FormatValueV1,
    expected_state_hash: &str,
) -> Result<TextFormatOperationReceiptV1> {
    apply_format_operation_v1(
        state,
        TextFormatOperationKindV1::SetTextFormatProperty,
        start_scalar,
        end_scalar,
        property,
        Some(value),
        expected_state_hash,
    )
}

pub fn clear_text_format_property_override_v1(
    state: &TextFormatOverlayStateV1,
    start_scalar: u32,
    end_scalar: u32,
    property: FormatPropertyV1,
    expected_state_hash: &str,
) -> Result<TextFormatOperationReceiptV1> {
    apply_format_operation_v1(
        state,
        TextFormatOperationKindV1::ClearTextFormatPropertyOverride,
        start_scalar,
        end_scalar,
        property,
        None,
        expected_state_hash,
    )
}

pub fn undo_text_format_operation_v1(
    receipt: &TextFormatOperationReceiptV1,
) -> TextFormatOverlayStateV1 {
    receipt.before_state.clone()
}

pub fn replay_text_format_operation_v1(
    receipt: &TextFormatOperationReceiptV1,
) -> Result<TextFormatOverlayStateV1> {
    let replay = match receipt.command.kind {
        TextFormatOperationKindV1::SetTextFormatProperty => set_text_format_property_v1(
            &receipt.before_state,
            receipt.command.start_scalar,
            receipt.command.end_scalar,
            receipt.command.property,
            receipt
                .command
                .value
                .clone()
                .ok_or_else(|| TextFormatOverlayError::new("replay set operation has no value"))?,
            &receipt.command.expected_state_hash,
        )?,
        TextFormatOperationKindV1::ClearTextFormatPropertyOverride => {
            clear_text_format_property_override_v1(
                &receipt.before_state,
                receipt.command.start_scalar,
                receipt.command.end_scalar,
                receipt.command.property,
                &receipt.command.expected_state_hash,
            )?
        }
    };
    if replay.after_state != receipt.after_state {
        return Err(TextFormatOverlayError::new(
            "format operation replay did not reproduce canonical state",
        ));
    }
    Ok(replay.after_state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(
        size: u64,
        bold: bool,
        italic: bool,
        color: &str,
        font: &str,
    ) -> BaseCharacterFormatV1 {
        BaseCharacterFormatV1 {
            font_resource_id: font.to_owned(),
            font_size_emu: size,
            bold,
            italic,
            text_color_rgb: color.to_owned(),
        }
    }

    fn state(
        base_runs: Vec<BaseFormatRunV1>,
        overrides: Vec<TextFormatOverrideRunV1>,
        story_len: u32,
    ) -> TextFormatOverlayStateV1 {
        build_text_format_overlay_state_v1(
            "story:1",
            "rev:1",
            story_len,
            base_runs,
            overrides,
        )
        .expect("valid overlay state")
    }

    fn one_base(format: BaseCharacterFormatV1) -> Vec<BaseFormatRunV1> {
        vec![BaseFormatRunV1 {
            start_scalar: 0,
            end_scalar: 6,
            format,
        }]
    }

    #[test]
    fn explicit_false_differs_from_inherit_when_base_is_true() {
        let state = state(one_base(fmt(12_000, true, false, "#000000", "font:resolved")), vec![], 6);
        let receipt = set_text_format_property_v1(
            &state,
            1,
            5,
            FormatPropertyV1::Bold,
            FormatValueV1::Bool(false),
            &state_hash_v1(&state).unwrap(),
        )
        .unwrap();
        assert_eq!(
            receipt.after_state.overrides,
            vec![TextFormatOverrideRunV1 {
                start_scalar: 1,
                end_scalar: 5,
                property: FormatPropertyV1::Bold,
                value: FormatValueV1::Bool(false),
            }]
        );
        let segments = effective_property_segments_v1(
            &receipt.after_state,
            FormatPropertyV1::Bold,
            1,
            5,
        )
        .unwrap();
        assert_eq!(segments[0].value, FormatValueV1::Bool(false));
        assert_eq!(
            segments[0].source,
            EffectivePropertySourceV1::ChapteraOverride
        );
    }

    #[test]
    fn redundant_explicit_value_equal_to_base_normalizes_away() {
        let state = state(one_base(fmt(12_000, false, false, "#000000", "font:resolved")), vec![], 6);
        let receipt = set_text_format_property_v1(
            &state,
            0,
            6,
            FormatPropertyV1::Bold,
            FormatValueV1::Bool(false),
            &state_hash_v1(&state).unwrap(),
        )
        .unwrap();
        assert!(receipt.after_state.overrides.is_empty());
    }

    #[test]
    fn clear_reveals_immutable_base_only_on_requested_range() {
        let state = state(
            one_base(fmt(12_000, true, false, "#000000", "font:resolved")),
            vec![TextFormatOverrideRunV1 {
                start_scalar: 0,
                end_scalar: 6,
                property: FormatPropertyV1::Bold,
                value: FormatValueV1::Bool(false),
            }],
            6,
        );
        let receipt = clear_text_format_property_override_v1(
            &state,
            2,
            4,
            FormatPropertyV1::Bold,
            &state_hash_v1(&state).unwrap(),
        )
        .unwrap();
        assert_eq!(
            receipt.after_state.overrides,
            vec![
                TextFormatOverrideRunV1 {
                    start_scalar: 0,
                    end_scalar: 2,
                    property: FormatPropertyV1::Bold,
                    value: FormatValueV1::Bool(false),
                },
                TextFormatOverrideRunV1 {
                    start_scalar: 4,
                    end_scalar: 6,
                    property: FormatPropertyV1::Bold,
                    value: FormatValueV1::Bool(false),
                },
            ]
        );
        let middle = effective_property_segments_v1(
            &receipt.after_state,
            FormatPropertyV1::Bold,
            2,
            4,
        )
        .unwrap();
        assert_eq!(middle[0].value, FormatValueV1::Bool(true));
        assert_eq!(middle[0].source, EffectivePropertySourceV1::Base);
    }

    #[test]
    fn adjacent_equal_overrides_coalesce_across_base_run_boundary() {
        let state = state(
            vec![
                BaseFormatRunV1 {
                    start_scalar: 0,
                    end_scalar: 3,
                    format: fmt(12_000, false, false, "#000000", "font:resolved"),
                },
                BaseFormatRunV1 {
                    start_scalar: 3,
                    end_scalar: 6,
                    format: fmt(12_000, false, false, "#111111", "font:resolved"),
                },
            ],
            vec![],
            6,
        );
        let receipt = set_text_format_property_v1(
            &state,
            0,
            6,
            FormatPropertyV1::Bold,
            FormatValueV1::Bool(true),
            &state_hash_v1(&state).unwrap(),
        )
        .unwrap();
        assert_eq!(
            receipt.after_state.overrides,
            vec![TextFormatOverrideRunV1 {
                start_scalar: 0,
                end_scalar: 6,
                property: FormatPropertyV1::Bold,
                value: FormatValueV1::Bool(true),
            }]
        );
    }

    #[test]
    fn overlap_history_normalizes_to_same_final_state() {
        let base = one_base(fmt(12_000, false, false, "#000000", "font:resolved"));
        let a0 = state(base.clone(), vec![], 6);
        let a1 = set_text_format_property_v1(
            &a0,
            0,
            6,
            FormatPropertyV1::Bold,
            FormatValueV1::Bool(true),
            &state_hash_v1(&a0).unwrap(),
        )
        .unwrap()
        .after_state;
        let a2 = clear_text_format_property_override_v1(
            &a1,
            2,
            4,
            FormatPropertyV1::Bold,
            &state_hash_v1(&a1).unwrap(),
        )
        .unwrap()
        .after_state;

        let b = state(
            base,
            vec![
                TextFormatOverrideRunV1 {
                    start_scalar: 0,
                    end_scalar: 2,
                    property: FormatPropertyV1::Bold,
                    value: FormatValueV1::Bool(true),
                },
                TextFormatOverrideRunV1 {
                    start_scalar: 4,
                    end_scalar: 6,
                    property: FormatPropertyV1::Bold,
                    value: FormatValueV1::Bool(true),
                },
            ],
            6,
        );
        assert_eq!(a2, b);
        assert_eq!(state_hash_v1(&a2).unwrap(), state_hash_v1(&b).unwrap());
    }

    #[test]
    fn zero_length_and_stale_state_fail_closed() {
        let state = state(one_base(fmt(12_000, false, false, "#000000", "font:resolved")), vec![], 6);
        let zero = set_text_format_property_v1(
            &state,
            2,
            2,
            FormatPropertyV1::Italic,
            FormatValueV1::Bool(true),
            &state_hash_v1(&state).unwrap(),
        )
        .unwrap_err();
        assert!(zero.to_string().contains("non-empty"));

        let stale = set_text_format_property_v1(
            &state,
            0,
            2,
            FormatPropertyV1::Italic,
            FormatValueV1::Bool(true),
            "deadbeef",
        )
        .unwrap_err();
        assert!(stale.to_string().contains("stale"));
    }

    #[test]
    fn unsupported_property_invalid_values_and_missing_font_fail_closed() {
        assert!(FormatPropertyV1::try_from("font_family").is_err());

        let state = state(one_base(fmt(12_000, false, false, "#000000", "font:resolved")), vec![], 6);
        assert!(
            set_text_format_property_v1(
                &state,
                0,
                2,
                FormatPropertyV1::TextColorRgb,
                FormatValueV1::String("red".to_owned()),
                &state_hash_v1(&state).unwrap(),
            )
            .is_err()
        );

        assert!(
            build_text_format_overlay_state_v1(
                "story:1",
                "rev:1",
                6,
                one_base(fmt(12_000, false, false, "#000000", "")),
                vec![],
            )
            .is_err()
        );
    }

    #[test]
    fn all_v1_properties_are_canonicalized() {
        let mut current =
            state(one_base(fmt(12_000, false, false, "#000000", "font:resolved")), vec![], 6);
        for (property, value) in [
            (FormatPropertyV1::FontSizeEmu, FormatValueV1::Integer(15_000)),
            (FormatPropertyV1::Bold, FormatValueV1::Bool(true)),
            (FormatPropertyV1::Italic, FormatValueV1::Bool(true)),
            (
                FormatPropertyV1::TextColorRgb,
                FormatValueV1::String("#aa00cc".to_owned()),
            ),
        ] {
            current = set_text_format_property_v1(
                &current,
                1,
                5,
                property,
                value,
                &state_hash_v1(&current).unwrap(),
            )
            .unwrap()
            .after_state;
        }
        assert_eq!(current.overrides.len(), 4);
        assert!(current.overrides.iter().any(|run| {
            run.property == FormatPropertyV1::TextColorRgb
                && run.value == FormatValueV1::String("#AA00CC".to_owned())
        }));
    }

    #[test]
    fn receipt_undo_and_replay_are_exact() {
        let state = state(one_base(fmt(12_000, true, false, "#000000", "font:resolved")), vec![], 6);
        let receipt = set_text_format_property_v1(
            &state,
            1,
            5,
            FormatPropertyV1::Bold,
            FormatValueV1::Bool(false),
            &state_hash_v1(&state).unwrap(),
        )
        .unwrap();
        assert_eq!(undo_text_format_operation_v1(&receipt), state);
        assert_eq!(
            replay_text_format_operation_v1(&receipt).unwrap(),
            receipt.after_state
        );
        assert!(receipt.requires_authoritative_relayout);
        assert_eq!(receipt.export_policy, EXPORT_POLICY_V1);
    }
}
