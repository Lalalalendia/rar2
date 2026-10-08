//! Bounded Quill typography and paragraph projection into source-neutral Reader DTOs.
//!
//! Story construction, PAGE roles, OfficeArt paint, geometry, tables and
//! rendering remain outside this seam.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubScriptFontEntryDisposition {
    Resolved,
    UnresolvedSentinel,
    InvalidFontOrdinal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubScriptFontEntry {
    pub script_slot: u16,
    pub source_font_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_font_name: Option<String>,
    pub disposition: PubScriptFontEntryDisposition,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubScriptFontMap {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub entries: Vec<PubScriptFontEntry>,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubParagraphAlignment {
    Center,
    Right,
    InterWord,
    Distribute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubParagraphAlignmentRun {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub alignment: PubParagraphAlignment,
    pub source_value: u16,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PubParagraphLineSpacing {
    Proportional { point_equivalent_emu: u32 },
    Absolute { spacing_emu: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubParagraphLineSpacingRun {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub line_spacing: PubParagraphLineSpacing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_value: Option<u32>,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubParagraphFlowConstraint {
    StartInNextTextBox,
    KeepLinesTogether,
    KeepWithNext,
    WidowControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubParagraphFlowRun {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub constraint: PubParagraphFlowConstraint,
    pub source_value: u32,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTypographyBooleanV1 {
    pub local_toggle: bool,
    pub inherited_value: bool,
    pub effective_value: bool,
}

pub(super) fn project_effective_boolean_v1(
    source: &QuillEffectiveBoolean,
) -> PubTypographyBooleanV1 {
    PubTypographyBooleanV1 {
        local_toggle: source.local_toggle,
        inherited_value: source.inherited_value,
        effective_value: source.effective_value,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTypographyRun {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub source_font_index: u32,
    pub source_font_name: String,
    pub text_size_emu: u32,
    pub font_inherited: bool,
    pub size_inherited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_rgb: Option<[u8; 3]>,
    /// Source Quill Publisher scheme slot 0..7, retained separately from
    /// the resolved publication-scheme RGB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_scheme_slot: Option<u8>,
    #[serde(default)]
    pub color_inherited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<PubTypographyBooleanV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<PubTypographyBooleanV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTypographySizeRun {
    pub story_id: StoryId,
    pub story_utf16_start: u32,
    pub story_utf16_end: u32,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub text_size_emu: u32,
    pub size_inherited: bool,
}

pub(super) struct PubTypographyProjection {
    pub(super) typography_runs: Vec<PubTypographyRun>,
    pub(super) typography_size_runs: Vec<PubTypographySizeRun>,
    pub(super) paragraph_alignments: Vec<PubParagraphAlignmentRun>,
    pub(super) paragraph_line_spacings: Vec<PubParagraphLineSpacingRun>,
    pub(super) paragraph_flow_runs: Vec<PubParagraphFlowRun>,
    pub(super) script_font_maps: Vec<PubScriptFontMap>,
}

pub(super) fn project_typography_catalog(
    typography_catalog: Option<pub_quill::QuillTypographyCatalog>,
    story_by_syid: &BTreeMap<u32, StoryId>,
    graph: &PubSourceGraph,
    color_scheme: Option<&MatureColorScheme>,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> PubTypographyProjection {
    let mut typography_runs = Vec::new();
    let mut typography_size_runs = Vec::new();
    let mut paragraph_alignments = Vec::new();
    let mut paragraph_line_spacings = Vec::new();
    let mut paragraph_flow_runs = Vec::new();
    let mut script_font_maps = Vec::new();
    if let Some(catalog) = typography_catalog {
        for map in &catalog.script_font_maps {
            let syid = map.story_syid.0;
            let Some(story_id) = story_by_syid.get(&syid).copied() else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("script-font map references missing Story SYID {syid}"),
                });
                continue;
            };
            let Some(story) = graph.stories.get(&story_id) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("script-font map Story {story_id:?} is absent"),
                });
                continue;
            };
            let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                &story.text,
                map.story_start_utf16,
                map.story_end_utf16,
            ) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!(
                        "script-font map range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                        map.story_start_utf16, map.story_end_utf16
                    ),
                });
                continue;
            };

            let object_key = quill_story_object_key(syid);
            let entries = map
                .entries
                .iter()
                .map(|entry| PubScriptFontEntry {
                    script_slot: entry.script_slot,
                    source_font_index: entry.font_index,
                    source_font_name: entry.font_name.clone(),
                    disposition: match entry.disposition {
                        QuillScriptFontEntryDisposition::Resolved => {
                            PubScriptFontEntryDisposition::Resolved
                        }
                        QuillScriptFontEntryDisposition::UnresolvedSentinel => {
                            PubScriptFontEntryDisposition::UnresolvedSentinel
                        }
                        QuillScriptFontEntryDisposition::InvalidFontOrdinal => {
                            PubScriptFontEntryDisposition::InvalidFontOrdinal
                        }
                    },
                    source_ref: source_ref(
                        &graph.source,
                        &entry.source,
                        Some(object_key.clone()),
                        Some(format!(
                            "FDPC/ScriptFonts/script-slot/{}",
                            entry.script_slot
                        )),
                        SourceRole::Semantic,
                        AuthorityClass::Authoritative,
                        ReadConfidence::Exact,
                    ),
                })
                .collect::<Vec<_>>();

            script_font_maps.push(PubScriptFontMap {
                story_id,
                story_utf16_start: map.story_start_utf16,
                story_utf16_end: map.story_end_utf16,
                story_scalar_start,
                story_scalar_end,
                entries,
                source_ref: source_ref(
                    &graph.source,
                    &map.fdpc_style_source,
                    Some(object_key),
                    Some("FDPC/ScriptFonts".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            });
        }
        for run in &catalog.paragraph_alignments {
            let syid = run.story_syid.0;
            let Some(story_id) = story_by_syid.get(&syid).copied() else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("paragraph alignment references missing Story SYID {syid}"),
                });
                continue;
            };
            let Some(story) = graph.stories.get(&story_id) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("paragraph alignment Story {story_id:?} is absent"),
                });
                continue;
            };
            let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                &story.text,
                run.story_start_utf16,
                run.story_end_utf16,
            ) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!(
                        "paragraph alignment range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                        run.story_start_utf16, run.story_end_utf16
                    ),
                });
                continue;
            };
            paragraph_alignments.push(PubParagraphAlignmentRun {
                story_id,
                story_utf16_start: run.story_start_utf16,
                story_utf16_end: run.story_end_utf16,
                story_scalar_start,
                story_scalar_end,
                alignment: match run.alignment {
                    QuillParagraphAlignment::Center => PubParagraphAlignment::Center,
                    QuillParagraphAlignment::Right => PubParagraphAlignment::Right,
                    QuillParagraphAlignment::InterWord => PubParagraphAlignment::InterWord,
                    QuillParagraphAlignment::Distribute => PubParagraphAlignment::Distribute,
                },
                source_value: run.source_value,
                source_ref: source_ref(
                    &graph.source,
                    &run.fdpp_style_source,
                    Some(quill_story_object_key(syid)),
                    Some("FDPP/ParagraphAlignment".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            });
        }
        for run in &catalog.paragraph_line_spacings {
            let syid = run.story_syid.0;
            let Some(story_id) = story_by_syid.get(&syid).copied() else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("paragraph line spacing references missing Story SYID {syid}"),
                });
                continue;
            };
            let Some(story) = graph.stories.get(&story_id) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("paragraph line spacing Story {story_id:?} is absent"),
                });
                continue;
            };
            let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                &story.text,
                run.story_start_utf16,
                run.story_end_utf16,
            ) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!(
                        "paragraph line spacing range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                        run.story_start_utf16, run.story_end_utf16
                    ),
                });
                continue;
            };
            paragraph_line_spacings.push(PubParagraphLineSpacingRun {
                story_id,
                story_utf16_start: run.story_start_utf16,
                story_utf16_end: run.story_end_utf16,
                story_scalar_start,
                story_scalar_end,
                line_spacing: match run.line_spacing {
                    QuillParagraphLineSpacing::Proportional {
                        point_equivalent_emu,
                    } => PubParagraphLineSpacing::Proportional {
                        point_equivalent_emu,
                    },
                    QuillParagraphLineSpacing::Absolute { spacing_emu } => {
                        PubParagraphLineSpacing::Absolute { spacing_emu }
                    }
                },
                source_value: run.source_value,
                source_ref: source_ref(
                    &graph.source,
                    &run.fdpp_style_source,
                    Some(quill_story_object_key(syid)),
                    Some("FDPP/ParagraphLineSpacing".into()),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            });
        }
        for run in &catalog.paragraph_flow_runs {
            let syid = run.story_syid.0;
            let Some(story_id) = story_by_syid.get(&syid).copied() else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("paragraph flow references missing Story SYID {syid}"),
                });
                continue;
            };
            let Some(story) = graph.stories.get(&story_id) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("paragraph flow Story {story_id:?} is absent"),
                });
                continue;
            };
            let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                &story.text,
                run.story_start_utf16,
                run.story_end_utf16,
            ) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!(
                        "paragraph flow range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                        run.story_start_utf16, run.story_end_utf16
                    ),
                });
                continue;
            };
            let (constraint, semantic_name) = match run.constraint {
                QuillParagraphFlowConstraint::StartInNextTextBox => (
                    PubParagraphFlowConstraint::StartInNextTextBox,
                    "StartInNextTextBox",
                ),
                QuillParagraphFlowConstraint::KeepLinesTogether => (
                    PubParagraphFlowConstraint::KeepLinesTogether,
                    "KeepLinesTogether",
                ),
                QuillParagraphFlowConstraint::KeepWithNext => {
                    (PubParagraphFlowConstraint::KeepWithNext, "KeepWithNext")
                }
                QuillParagraphFlowConstraint::WidowControl => {
                    (PubParagraphFlowConstraint::WidowControl, "WidowControl")
                }
            };
            paragraph_flow_runs.push(PubParagraphFlowRun {
                story_id,
                story_utf16_start: run.story_start_utf16,
                story_utf16_end: run.story_end_utf16,
                story_scalar_start,
                story_scalar_end,
                constraint,
                source_value: run.source_value,
                source_ref: source_ref(
                    &graph.source,
                    &run.property_source,
                    Some(quill_story_object_key(syid)),
                    Some(format!("FDPP/ParagraphFlow/{semantic_name}")),
                    SourceRole::Semantic,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            });
        }
        for run in &catalog.size_only_runs {
            let syid = run.story_syid.0;
            let Some(story_id) = story_by_syid.get(&syid).copied() else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("size-only typography references missing Story SYID {syid}"),
                });
                continue;
            };
            let Some(story) = graph.stories.get(&story_id) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!("size-only typography Story {story_id:?} is absent"),
                });
                continue;
            };
            let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                &story.text,
                run.story_start_utf16,
                run.story_end_utf16,
            ) else {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: format!(
                        "size-only typography range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                        run.story_start_utf16, run.story_end_utf16
                    ),
                });
                continue;
            };
            typography_size_runs.push(PubTypographySizeRun {
                story_id,
                story_utf16_start: run.story_start_utf16,
                story_utf16_end: run.story_end_utf16,
                story_scalar_start,
                story_scalar_end,
                text_size_emu: run.text_size_emu,
                size_inherited: run.text_size_source == QuillTypographyValueSource::InheritedStsh1,
            });
        }
        if !catalog.effective_runs.is_empty() {
            for run in catalog.effective_runs {
                let syid = run.story_syid.0;
                let Some(story_id) = story_by_syid.get(&syid).copied() else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!(
                            "effective typography references missing Story SYID {syid}"
                        ),
                    });
                    continue;
                };
                let Some(story) = graph.stories.get(&story_id) else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!("effective typography Story {story_id:?} is absent"),
                    });
                    continue;
                };
                let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                    &story.text,
                    run.story_start_utf16,
                    run.story_end_utf16,
                ) else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!(
                            "effective typography range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                            run.story_start_utf16, run.story_end_utf16
                        ),
                    });
                    continue;
                };
                let color_rgb =
                    bounded_quill_text_rgb(run.color_rgb, run.color_scheme_slot, color_scheme);
                typography_runs.push(PubTypographyRun {
                    story_id,
                    story_utf16_start: run.story_start_utf16,
                    story_utf16_end: run.story_end_utf16,
                    story_scalar_start,
                    story_scalar_end,
                    source_font_index: run.font_index,
                    source_font_name: run.font_name,
                    text_size_emu: run.text_size_emu,
                    font_inherited: run.font_source == QuillTypographyValueSource::InheritedStsh1,
                    size_inherited: run.text_size_source
                        == QuillTypographyValueSource::InheritedStsh1,
                    color_rgb,
                    color_scheme_slot: run.color_scheme_slot,
                    color_inherited: run.color_inherited,
                    bold: run.bold.as_ref().map(project_effective_boolean_v1),
                    italic: run.italic.as_ref().map(project_effective_boolean_v1),
                });
            }
        } else {
            for run in catalog.explicit_runs {
                let syid = run.story_syid.0;
                let Some(story_id) = story_by_syid.get(&syid).copied() else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!("explicit typography references missing Story SYID {syid}"),
                    });
                    continue;
                };
                let Some(story) = graph.stories.get(&story_id) else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!("explicit typography Story {story_id:?} is absent"),
                    });
                    continue;
                };
                let Some((story_scalar_start, story_scalar_end)) = utf16_range_to_scalar_range(
                    &story.text,
                    run.story_start_utf16,
                    run.story_end_utf16,
                ) else {
                    diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                        reason: format!(
                            "explicit typography range {}..{} splits a UTF-16 scalar boundary for Story SYID {syid}",
                            run.story_start_utf16, run.story_end_utf16
                        ),
                    });
                    continue;
                };
                let color_rgb =
                    bounded_quill_text_rgb(run.color_rgb, run.color_scheme_slot, color_scheme);
                typography_runs.push(PubTypographyRun {
                    story_id,
                    story_utf16_start: run.story_start_utf16,
                    story_utf16_end: run.story_end_utf16,
                    story_scalar_start,
                    story_scalar_end,
                    source_font_index: run.font_index,
                    source_font_name: run.font_name,
                    text_size_emu: run.text_size_emu,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb,
                    color_scheme_slot: run.color_scheme_slot,
                    color_inherited: run.color_inherited,
                    bold: None,
                    italic: None,
                });
            }
        }
    }

    PubTypographyProjection {
        typography_runs,
        typography_size_runs,
        paragraph_alignments,
        paragraph_line_spacings,
        paragraph_flow_runs,
        script_font_maps,
    }
}

fn utf16_range_to_scalar_range(text: &str, start_utf16: u32, end_utf16: u32) -> Option<(u32, u32)> {
    if start_utf16 > end_utf16 {
        return None;
    }

    fn boundary(text: &str, target_utf16: u32) -> Option<u32> {
        if target_utf16 == 0 {
            return Some(0);
        }

        let mut utf16_cursor = 0_u32;
        let mut scalar_cursor = 0_u32;
        for scalar in text.chars() {
            utf16_cursor = utf16_cursor.checked_add(scalar.len_utf16() as u32)?;
            scalar_cursor = scalar_cursor.checked_add(1)?;
            if utf16_cursor == target_utf16 {
                return Some(scalar_cursor);
            }
            if utf16_cursor > target_utf16 {
                return None;
            }
        }
        (utf16_cursor == target_utf16).then_some(scalar_cursor)
    }

    Some((boundary(text, start_utf16)?, boundary(text, end_utf16)?))
}

fn bounded_quill_text_rgb(
    direct_rgb: Option<[u8; 3]>,
    scheme_slot: Option<u8>,
    color_scheme: Option<&MatureColorScheme>,
) -> Option<[u8; 3]> {
    match (direct_rgb, scheme_slot) {
        (Some(rgb), None) => Some(rgb),
        (None, Some(slot)) => color_scheme?.slots.get(usize::from(slot))?.rgb,
        // Both carriers at once are not a grounded Quill state; neither is
        // absence of both. Keep those cases fail-closed.
        _ => None,
    }
}
