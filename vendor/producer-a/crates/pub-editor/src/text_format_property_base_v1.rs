use super::EditOperation;
use chaptera_text_format_overlay::{
    EffectivePropertySegmentV1, EffectivePropertySourceV1, FormatPropertyV1, FormatValueV1,
};
use pub_model::StoryId;
use pub_reader::PubTypographyRun;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const TEXT_FORMAT_PROPERTY_STATE_V1: &str = "chaptera.text-format-property-state.v1";

fn validate_supported_property_v1(property: FormatPropertyV1) -> Result<(), String> {
    match property {
        FormatPropertyV1::Bold | FormatPropertyV1::Italic => Ok(()),
        FormatPropertyV1::FontSizeEmu | FormatPropertyV1::TextColorRgb => {
            Err("property-scoped text-format state v1 supports only bold/italic".to_owned())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatPropertyBaseRunV1 {
    pub start_scalar: u32,
    pub end_scalar: u32,
    pub value: FormatValueV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatPropertyOverrideRunV1 {
    pub start_scalar: u32,
    pub end_scalar: u32,
    pub value: FormatValueV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFormatPropertyStateV1 {
    pub protocol_version: String,
    pub story_id: String,
    pub base_revision_id: String,
    pub story_scalar_len: u32,
    pub property: FormatPropertyV1,
    pub base_runs: Vec<TextFormatPropertyBaseRunV1>,
    pub overrides: Vec<TextFormatPropertyOverrideRunV1>,
}

fn validate_property_value(
    property: FormatPropertyV1,
    value: &FormatValueV1,
) -> Result<(), String> {
    validate_supported_property_v1(property)?;
    match (property, value) {
        (FormatPropertyV1::Bold | FormatPropertyV1::Italic, FormatValueV1::Bool(_)) => Ok(()),
        (FormatPropertyV1::Bold, _) => Err("bold must be boolean".to_owned()),
        (FormatPropertyV1::Italic, _) => Err("italic must be boolean".to_owned()),
        (FormatPropertyV1::FontSizeEmu | FormatPropertyV1::TextColorRgb, _) => {
            unreachable!("unsupported properties are fenced above")
        }
    }
}

fn source_property_value(
    run: &PubTypographyRun,
    property: FormatPropertyV1,
) -> Result<FormatValueV1, String> {
    match property {
        FormatPropertyV1::Bold => {
            let value = run
                .bold
                .as_ref()
                .ok_or_else(|| "bounded effective bold is unavailable".to_owned())?;
            if value.effective_value != (value.inherited_value ^ value.local_toggle) {
                return Err("Reader bold projection violates the admitted XOR invariant".to_owned());
            }
            Ok(FormatValueV1::Bool(value.effective_value))
        }
        FormatPropertyV1::Italic => {
            let value = run
                .italic
                .as_ref()
                .ok_or_else(|| "bounded effective italic is unavailable".to_owned())?;
            if value.effective_value != (value.inherited_value ^ value.local_toggle) {
                return Err(
                    "Reader italic projection violates the admitted XOR invariant".to_owned(),
                );
            }
            Ok(FormatValueV1::Bool(value.effective_value))
        }
        FormatPropertyV1::FontSizeEmu => {
            if run.text_size_emu == 0 {
                return Err("bounded effective font size is unavailable".to_owned());
            }
            Ok(FormatValueV1::Integer(u64::from(run.text_size_emu)))
        }
        FormatPropertyV1::TextColorRgb => {
            let [red, green, blue] = run.color_rgb.ok_or_else(|| {
                "bounded effective direct-RGB text color is unavailable".to_owned()
            })?;
            Ok(FormatValueV1::String(format!(
                "#{red:02X}{green:02X}{blue:02X}"
            )))
        }
    }
}

fn push_base_run(
    out: &mut Vec<TextFormatPropertyBaseRunV1>,
    start_scalar: u32,
    end_scalar: u32,
    value: FormatValueV1,
) {
    if let Some(previous) = out.last_mut()
        && previous.end_scalar == start_scalar
        && previous.value == value
    {
        previous.end_scalar = end_scalar;
        return;
    }
    out.push(TextFormatPropertyBaseRunV1 {
        start_scalar,
        end_scalar,
        value,
    });
}

pub fn build_source_text_format_property_state_v1(
    story_id: StoryId,
    base_revision_id: &str,
    story_scalar_len: u32,
    source_runs: &[PubTypographyRun],
    property: FormatPropertyV1,
) -> Result<TextFormatPropertyStateV1, String> {
    validate_supported_property_v1(property)?;
    if base_revision_id.is_empty() {
        return Err("base_revision_id is required".to_owned());
    }
    if story_scalar_len == 0 {
        return Ok(TextFormatPropertyStateV1 {
            protocol_version: TEXT_FORMAT_PROPERTY_STATE_V1.to_owned(),
            story_id: story_id.as_canonical().to_string(),
            base_revision_id: base_revision_id.to_owned(),
            story_scalar_len,
            property,
            base_runs: Vec::new(),
            overrides: Vec::new(),
        });
    }

    let mut runs = source_runs
        .iter()
        .filter(|run| run.story_id == story_id)
        .collect::<Vec<_>>();
    runs.sort_by_key(|run| (run.story_scalar_start, run.story_scalar_end));
    if runs.is_empty() {
        return Err("no source effective typography runs".to_owned());
    }

    let mut cursor = 0_u32;
    let mut base_runs = Vec::with_capacity(runs.len());
    for run in runs {
        if run.story_scalar_start != cursor
            || run.story_scalar_end <= run.story_scalar_start
            || run.story_scalar_end > story_scalar_len
        {
            return Err(
                "source effective typography does not form one contiguous scalar partition"
                    .to_owned(),
            );
        }
        let value = source_property_value(run, property)?;
        validate_property_value(property, &value)?;
        push_base_run(
            &mut base_runs,
            run.story_scalar_start,
            run.story_scalar_end,
            value,
        );
        cursor = run.story_scalar_end;
    }
    if cursor != story_scalar_len {
        return Err("source effective typography does not cover the full Story".to_owned());
    }

    Ok(TextFormatPropertyStateV1 {
        protocol_version: TEXT_FORMAT_PROPERTY_STATE_V1.to_owned(),
        story_id: story_id.as_canonical().to_string(),
        base_revision_id: base_revision_id.to_owned(),
        story_scalar_len,
        property,
        base_runs,
        overrides: Vec::new(),
    })
}

fn state_hash_bytes_v1(state: &TextFormatPropertyStateV1) -> Result<Vec<u8>, String> {
    serde_json::to_vec(state).map_err(|error| error.to_string())
}

pub fn text_format_property_state_hash_v1(
    state: &TextFormatPropertyStateV1,
) -> Result<String, String> {
    let digest = Sha256::digest(state_hash_bytes_v1(state)?);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").map_err(|error| error.to_string())?;
    }
    Ok(format!("sha256:{encoded}"))
}

fn validate_range(state: &TextFormatPropertyStateV1, start: u32, end: u32) -> Result<(), String> {
    if end <= start || end > state.story_scalar_len {
        return Err("format range must be non-empty and within Story scalar extent".to_owned());
    }
    Ok(())
}

fn base_value_at(state: &TextFormatPropertyStateV1, scalar: u32) -> Result<FormatValueV1, String> {
    state
        .base_runs
        .iter()
        .find(|run| run.start_scalar <= scalar && scalar < run.end_scalar)
        .map(|run| run.value.clone())
        .ok_or_else(|| "property base does not cover requested scalar".to_owned())
}

fn override_value_at(
    state: &TextFormatPropertyStateV1,
    scalar: u32,
) -> Result<Option<FormatValueV1>, String> {
    let mut found = None;
    for run in &state.overrides {
        if !(run.start_scalar <= scalar && scalar < run.end_scalar) {
            continue;
        }
        if found.is_some() {
            return Err("property override state overlaps".to_owned());
        }
        found = Some(run.value.clone());
    }
    Ok(found)
}

#[derive(Clone, Copy)]
enum PropertyEdit<'a> {
    Set {
        start: u32,
        end: u32,
        value: &'a FormatValueV1,
    },
    Clear {
        start: u32,
        end: u32,
    },
}

fn normalize_after_edit(
    state: &TextFormatPropertyStateV1,
    edit: PropertyEdit<'_>,
) -> Result<Vec<TextFormatPropertyOverrideRunV1>, String> {
    let (edit_start, edit_end) = match edit {
        PropertyEdit::Set { start, end, .. } | PropertyEdit::Clear { start, end } => (start, end),
    };
    validate_range(state, edit_start, edit_end)?;

    let mut boundaries = BTreeSet::from([0, state.story_scalar_len, edit_start, edit_end]);
    for run in &state.base_runs {
        boundaries.insert(run.start_scalar);
        boundaries.insert(run.end_scalar);
    }
    for run in &state.overrides {
        boundaries.insert(run.start_scalar);
        boundaries.insert(run.end_scalar);
    }
    let points = boundaries.into_iter().collect::<Vec<_>>();
    let mut out: Vec<TextFormatPropertyOverrideRunV1> = Vec::new();

    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start == end {
            continue;
        }
        let explicit = if edit_start <= start && end <= edit_end {
            match edit {
                PropertyEdit::Set { value, .. } => Some(value.clone()),
                PropertyEdit::Clear { .. } => None,
            }
        } else {
            override_value_at(state, start)?
        };
        let Some(explicit) = explicit else {
            continue;
        };
        if explicit == base_value_at(state, start)? {
            continue;
        }
        if let Some(previous) = out.last_mut()
            && previous.end_scalar == start
            && previous.value == explicit
        {
            previous.end_scalar = end;
        } else {
            out.push(TextFormatPropertyOverrideRunV1 {
                start_scalar: start,
                end_scalar: end,
                value: explicit,
            });
        }
    }
    Ok(out)
}

pub fn set_text_format_property_state_v1(
    state: &TextFormatPropertyStateV1,
    start: u32,
    end: u32,
    value: FormatValueV1,
) -> Result<TextFormatPropertyStateV1, String> {
    validate_property_value(state.property, &value)?;
    let overrides = normalize_after_edit(
        state,
        PropertyEdit::Set {
            start,
            end,
            value: &value,
        },
    )?;
    let mut after = state.clone();
    after.overrides = overrides;
    Ok(after)
}

pub fn clear_text_format_property_state_v1(
    state: &TextFormatPropertyStateV1,
    start: u32,
    end: u32,
) -> Result<TextFormatPropertyStateV1, String> {
    let overrides = normalize_after_edit(state, PropertyEdit::Clear { start, end })?;
    let mut after = state.clone();
    after.overrides = overrides;
    Ok(after)
}

pub fn effective_text_format_property_segments_v1(
    state: &TextFormatPropertyStateV1,
    start: u32,
    end: u32,
) -> Result<Vec<EffectivePropertySegmentV1>, String> {
    validate_range(state, start, end)?;
    let mut boundaries = BTreeSet::from([start, end]);
    for run in &state.base_runs {
        if start < run.end_scalar && run.start_scalar < end {
            boundaries.insert(start.max(run.start_scalar));
            boundaries.insert(end.min(run.end_scalar));
        }
    }
    for run in &state.overrides {
        if start < run.end_scalar && run.start_scalar < end {
            boundaries.insert(start.max(run.start_scalar));
            boundaries.insert(end.min(run.end_scalar));
        }
    }
    let points = boundaries.into_iter().collect::<Vec<_>>();
    let mut out: Vec<EffectivePropertySegmentV1> = Vec::new();
    for pair in points.windows(2) {
        let segment_start = pair[0];
        let segment_end = pair[1];
        if segment_start == segment_end {
            continue;
        }
        let override_value = override_value_at(state, segment_start)?;
        let (value, source) = if let Some(value) = override_value {
            (value, EffectivePropertySourceV1::ChapteraOverride)
        } else {
            (
                base_value_at(state, segment_start)?,
                EffectivePropertySourceV1::Base,
            )
        };
        if let Some(previous) = out.last_mut()
            && previous.end_scalar == segment_start
            && previous.value == value
            && previous.source == source
        {
            previous.end_scalar = segment_end;
        } else {
            out.push(EffectivePropertySegmentV1 {
                start_scalar: segment_start,
                end_scalar: segment_end,
                property: state.property,
                value,
                source,
            });
        }
    }
    Ok(out)
}

fn operation_for_state<'a>(
    state: &TextFormatPropertyStateV1,
    operation: &'a EditOperation,
) -> Option<(&'a str, &'a str)> {
    match operation {
        EditOperation::SetTextFormatProperty {
            story_id,
            property,
            before_state_hash,
            after_state_hash,
            ..
        }
        | EditOperation::ClearTextFormatPropertyOverride {
            story_id,
            property,
            before_state_hash,
            after_state_hash,
            ..
        }
        | EditOperation::SetTextFormatPropertyScopedV1 {
            story_id,
            property,
            before_state_hash,
            after_state_hash,
            ..
        }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            story_id,
            property,
            before_state_hash,
            after_state_hash,
            ..
        } if story_id.as_canonical().to_string() == state.story_id
            && *property == state.property =>
        {
            Some((before_state_hash, after_state_hash))
        }
        _ => None,
    }
}

pub fn apply_text_format_property_operation_semantic_v1(
    state: &TextFormatPropertyStateV1,
    operation: &EditOperation,
) -> Result<TextFormatPropertyStateV1, String> {
    match operation {
        EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            ..
        }
        | EditOperation::SetTextFormatPropertyScopedV1 {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            ..
        } if story_id.as_canonical().to_string() == state.story_id
            && *property == state.property =>
        {
            set_text_format_property_state_v1(state, *start_scalar, *end_scalar, value.clone())
        }
        EditOperation::ClearTextFormatPropertyOverride {
            story_id,
            start_scalar,
            end_scalar,
            property,
            ..
        }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            story_id,
            start_scalar,
            end_scalar,
            property,
            ..
        } if story_id.as_canonical().to_string() == state.story_id
            && *property == state.property =>
        {
            clear_text_format_property_state_v1(state, *start_scalar, *end_scalar)
        }
        _ => Err("text-format operation does not target this property state".to_owned()),
    }
}

pub fn apply_text_format_property_operation_checked_v1(
    state: &TextFormatPropertyStateV1,
    operation: &EditOperation,
) -> Result<TextFormatPropertyStateV1, String> {
    let (before_hash, after_hash) = operation_for_state(state, operation)
        .ok_or_else(|| "text-format operation does not target this property state".to_owned())?;
    if text_format_property_state_hash_v1(state)? != before_hash {
        return Err("property-scoped text-format operation is stale".to_owned());
    }
    let after = apply_text_format_property_operation_semantic_v1(state, operation)?;
    if text_format_property_state_hash_v1(&after)? != after_hash {
        return Err("property-scoped text-format operation after hash is invalid".to_owned());
    }
    Ok(after)
}

pub fn fold_text_format_property_history_v1(
    mut state: TextFormatPropertyStateV1,
    operations: &[EditOperation],
) -> Result<TextFormatPropertyStateV1, String> {
    for operation in operations {
        if operation_for_state(&state, operation).is_some() {
            state = apply_text_format_property_operation_semantic_v1(&state, operation)?;
        }
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_reader::PubTypographyBooleanV1;

    fn story_id() -> StoryId {
        serde_json::from_str("\"11111111-1111-5111-8111-111111111111\"").expect("canonical StoryId")
    }

    fn run(
        start: u32,
        end: u32,
        bold: Option<bool>,
        italic: Option<bool>,
        color_rgb: Option<[u8; 3]>,
    ) -> PubTypographyRun {
        PubTypographyRun {
            story_id: story_id(),
            story_utf16_start: start,
            story_utf16_end: end,
            story_scalar_start: start,
            story_scalar_end: end,
            source_font_index: 4,
            source_font_name: "Montserrat".to_owned(),
            text_size_emu: 304_800,
            font_inherited: true,
            size_inherited: true,
            color_rgb,
            color_inherited: true,
            bold: bold.map(|effective_value| PubTypographyBooleanV1 {
                local_toggle: false,
                inherited_value: effective_value,
                effective_value,
            }),
            italic: italic.map(|effective_value| PubTypographyBooleanV1 {
                local_toggle: false,
                inherited_value: effective_value,
                effective_value,
            }),
        }
    }

    #[test]
    fn bold_property_base_does_not_require_unrelated_color() {
        let state = build_source_text_format_property_state_v1(
            story_id(),
            "sha256:source-story",
            4,
            &[run(0, 4, Some(true), Some(false), None)],
            FormatPropertyV1::Bold,
        )
        .expect("known Bold must remain usable when color is unresolved");

        assert_eq!(state.base_runs.len(), 1);
        assert_eq!(state.base_runs[0].value, FormatValueV1::Bool(true));
        assert!(state.overrides.is_empty());
    }

    #[test]
    fn property_state_v1_rejects_unrelated_size_and_color_domains() {
        let size_error = build_source_text_format_property_state_v1(
            story_id(),
            "sha256:source-story",
            4,
            &[run(0, 4, Some(false), Some(false), Some([0, 0, 0]))],
            FormatPropertyV1::FontSizeEmu,
        )
        .expect_err("v1 is bounded to Bold/Italic");
        assert!(size_error.contains("only bold/italic"));

        let color_error = build_source_text_format_property_state_v1(
            story_id(),
            "sha256:source-story",
            4,
            &[run(0, 4, Some(false), Some(false), Some([0, 0, 0]))],
            FormatPropertyV1::TextColorRgb,
        )
        .expect_err("v1 is bounded to Bold/Italic");
        assert!(color_error.contains("only bold/italic"));
    }

    #[test]
    fn bold_property_base_still_fails_closed_when_bold_is_unknown() {
        let error = build_source_text_format_property_state_v1(
            story_id(),
            "sha256:source-story",
            4,
            &[run(0, 4, None, Some(false), Some([0, 0, 0]))],
            FormatPropertyV1::Bold,
        )
        .expect_err("unknown Bold must remain unavailable");

        assert!(error.contains("bold"));
    }

    #[test]
    fn scoped_persisted_kinds_replay_in_property_hash_domain() {
        let source = build_source_text_format_property_state_v1(
            story_id(),
            "sha256:source-story",
            4,
            &[run(0, 4, Some(false), Some(false), None)],
            FormatPropertyV1::Bold,
        )
        .expect("source Bold state");
        let source_hash =
            text_format_property_state_hash_v1(&source).expect("source property hash");
        let after_set =
            set_text_format_property_state_v1(&source, 0, 4, FormatValueV1::Bool(true))
                .expect("set scoped Bold");
        let set_hash =
            text_format_property_state_hash_v1(&after_set).expect("edited property hash");
        let set_operation = EditOperation::SetTextFormatPropertyScopedV1 {
            story_id: story_id(),
            start_scalar: 0,
            end_scalar: 4,
            property: FormatPropertyV1::Bold,
            value: FormatValueV1::Bool(true),
            before_state_hash: source_hash.clone(),
            after_state_hash: set_hash.clone(),
        };

        assert_eq!(
            apply_text_format_property_operation_checked_v1(&source, &set_operation)
                .expect("checked scoped Set replay"),
            after_set
        );

        let after_clear =
            clear_text_format_property_state_v1(&after_set, 0, 4).expect("clear scoped Bold");
        assert_eq!(after_clear, source);
        let clear_operation = EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            story_id: story_id(),
            start_scalar: 0,
            end_scalar: 4,
            property: FormatPropertyV1::Bold,
            before_state_hash: set_hash,
            after_state_hash: source_hash,
        };
        assert_eq!(
            apply_text_format_property_operation_checked_v1(&after_set, &clear_operation)
                .expect("checked scoped Clear replay"),
            source
        );
        assert_eq!(
            fold_text_format_property_history_v1(
                source.clone(),
                &[set_operation, clear_operation],
            )
            .expect("scoped Set/Clear history projection"),
            source
        );
    }

    #[test]
    fn scoped_set_clear_normalizes_against_only_the_target_property_base() {
        let source = build_source_text_format_property_state_v1(
            story_id(),
            "sha256:source-story",
            6,
            &[
                run(0, 3, Some(false), Some(false), None),
                run(3, 6, Some(true), Some(false), None),
            ],
            FormatPropertyV1::Bold,
        )
        .expect("source Bold state");

        let set = set_text_format_property_state_v1(&source, 1, 5, FormatValueV1::Bool(true))
            .expect("set Bold");
        assert_eq!(
            effective_text_format_property_segments_v1(&set, 0, 6).expect("effective Bold"),
            vec![
                EffectivePropertySegmentV1 {
                    start_scalar: 0,
                    end_scalar: 1,
                    property: FormatPropertyV1::Bold,
                    value: FormatValueV1::Bool(false),
                    source: EffectivePropertySourceV1::Base,
                },
                EffectivePropertySegmentV1 {
                    start_scalar: 1,
                    end_scalar: 3,
                    property: FormatPropertyV1::Bold,
                    value: FormatValueV1::Bool(true),
                    source: EffectivePropertySourceV1::ChapteraOverride,
                },
                EffectivePropertySegmentV1 {
                    start_scalar: 3,
                    end_scalar: 6,
                    property: FormatPropertyV1::Bold,
                    value: FormatValueV1::Bool(true),
                    source: EffectivePropertySourceV1::Base,
                },
            ]
        );

        let cleared = clear_text_format_property_state_v1(&set, 1, 5).expect("clear Bold");
        assert_eq!(cleared, source);
    }
}
