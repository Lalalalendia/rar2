//! Exact glyphs for independently admitted physical font overrides.
//!
//! This is not a text-flow or PDF authority. Publisher source font bindings
//! are names/indices without independently verified font bytes. They remain
//! unresolved instead of silently borrowing the admitted replacement font.
//! Only the canonical current EditorSession FontResource overlay may select
//! a replacement; glyph clusters retain whole-Story Unicode-scalar indices.

use super::DesktopShapedFlowRuntimeError;
use chaptera_text_format_overlay::{
    EffectivePropertySegmentV1, EffectivePropertySourceV1, FontResourceIdentityV1,
    FormatPropertyV1, FormatValueV1, ServerFontResourceV1, TextFormatOverlayStateV1,
    effective_property_segments_v1, state_hash_v1,
};
use pub_editor::EditorSession;
use pub_layout::{
    BoundedLayoutEnvironment, BoundedShapedText, BoundedShapingRuntime, font_fingerprint_sha256,
    shape_bounded_ltr_segment,
};
use pub_model::{LengthEmu, StoryId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CURRENT_FONT_SPANS_V1: &str = "chaptera.current-exact-font-spans.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurrentPhysicalFontSpanV1 {
    SourceUnresolved {
        start_scalar: u32,
        end_scalar: u32,
        source_font_binding_id: String,
    },
    AdmittedExact {
        start_scalar: u32,
        end_scalar: u32,
        identity: FontResourceIdentityV1,
        font_size_emu: LengthEmu,
        shaped: BoundedShapedText,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentPhysicalFontSpansV1 {
    pub protocol_version: String,
    pub story_id: String,
    pub story_format_state_hash: String,
    pub story_scalar_len: u32,
    pub spans: Vec<CurrentPhysicalFontSpanV1>,
    /// True means all Story scalars were shaped with exact, admitted full
    /// physical bytes. It does not establish line breaks or frame allocation.
    pub all_scalars_shaped: bool,
    pub authoritative_line_breaks: bool,
    pub fixed_pdf_allowed: bool,
}

fn blocked(code: &'static str, reason: impl Into<String>) -> DesktopShapedFlowRuntimeError {
    DesktopShapedFlowRuntimeError::new(code, reason)
}

fn covering<'a>(
    segments: &'a [EffectivePropertySegmentV1],
    start: u32,
    end: u32,
) -> Result<&'a EffectivePropertySegmentV1, DesktopShapedFlowRuntimeError> {
    segments
        .iter()
        .find(|part| part.start_scalar <= start && end <= part.end_scalar)
        .ok_or_else(|| {
            blocked(
                "current_font_span_partition_invalid",
                "effective font property does not cover a current Story interval",
            )
        })
}

/// Read and shape only canonical FontResource overrides. The trusted caller
/// must construct this full-font grant independently of the EditorProject and
/// the browser candidate. Missing, changed, subset or non-admitted bytes deny
/// the whole projection, not a silently substituted partial glyph result.
pub fn shape_current_exact_font_override_spans_v1(
    editor: &EditorSession,
    story_id: StoryId,
    trusted: &ServerFontResourceV1<'_>,
) -> Result<CurrentPhysicalFontSpansV1, DesktopShapedFlowRuntimeError> {
    let story = editor
        .graph()
        .stories
        .get(&story_id)
        .ok_or_else(|| blocked("story_missing", "current EditorSession Story is missing"))?;
    let overlay = editor
        .current_text_format_overlay_v1(story_id)
        .map_err(|error| {
            blocked(
                "current_font_overlay_unavailable",
                format!("current Story format history is unavailable: {error}"),
            )
        })?;
    project_exact_font_spans_from_overlay_v1(&overlay, &story.text, trusted)
}

fn project_exact_font_spans_from_overlay_v1(
    state: &TextFormatOverlayStateV1,
    text: &str,
    trusted: &ServerFontResourceV1<'_>,
) -> Result<CurrentPhysicalFontSpansV1, DesktopShapedFlowRuntimeError> {
    let scalars = text.chars().collect::<Vec<_>>();
    let text_len = u32::try_from(scalars.len())
        .map_err(|_| blocked("story_extent_overflow", "Story scalar extent exceeds u32"))?;
    if text_len != state.story_scalar_len {
        return Err(blocked(
            "current_font_story_stale",
            "current Story text differs from canonical format scalar extent",
        ));
    }
    let state_hash = state_hash_v1(state)
        .map_err(|error| blocked("current_font_overlay_invalid", error.to_string()))?;
    if text_len == 0 {
        return Ok(CurrentPhysicalFontSpansV1 {
            protocol_version: CURRENT_FONT_SPANS_V1.to_owned(),
            story_id: state.story_id.clone(),
            story_format_state_hash: state_hash,
            story_scalar_len: 0,
            spans: Vec::new(),
            all_scalars_shaped: true,
            authoritative_line_breaks: false,
            fixed_pdf_allowed: false,
        });
    }

    let resource_segments =
        effective_property_segments_v1(state, FormatPropertyV1::FontResource, 0, text_len)
            .map_err(|error| blocked("current_font_resource_invalid", error.to_string()))?;
    let size_segments =
        effective_property_segments_v1(state, FormatPropertyV1::FontSizeEmu, 0, text_len)
            .map_err(|error| blocked("current_font_size_invalid", error.to_string()))?;
    let mut boundaries = BTreeSet::from([0, text_len]);
    for run in resource_segments.iter().chain(size_segments.iter()) {
        boundaries.insert(run.start_scalar);
        boundaries.insert(run.end_scalar);
    }

    let identity = trusted.identity;
    let has_override = resource_segments
        .iter()
        .any(|part| part.source == EffectivePropertySourceV1::ChapteraOverride);
    let fingerprint = if has_override {
        if !trusted.authoring_admitted
            || !trusted.is_full_resource
            || trusted.full_font_bytes.is_empty()
            || trusted.face_count == 0
            || u32::from(identity.face_index) >= trusted.face_count
        {
            return Err(blocked(
                "current_font_resource_not_admitted",
                "full physical font bytes and independent authoring admission are required",
            ));
        }
        let actual = font_fingerprint_sha256(trusted.full_font_bytes);
        if identity.content_hash != actual
            || identity.font_fingerprint != format!("sha256:{actual}")
        {
            return Err(blocked(
                "current_font_resource_bytes_changed",
                "trusted font resource SHA-256 does not match the original resource identity",
            ));
        }
        Some(actual)
    } else {
        None
    };

    let points = boundaries.into_iter().collect::<Vec<_>>();
    let mut spans = Vec::with_capacity(points.len().saturating_sub(1));
    let mut all_scalars_shaped = true;
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start == end {
            continue;
        }
        let property = covering(&resource_segments, start, end)?;
        let size = covering(&size_segments, start, end)?;
        let size_emu = match &size.value {
            FormatValueV1::Integer(value) => i64::try_from(*value)
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    blocked(
                        "current_font_size_invalid",
                        "font size exceeds valid EMU domain",
                    )
                })?,
            _ => {
                return Err(blocked(
                    "current_font_size_invalid",
                    "effective font size must be integer EMU",
                ));
            }
        };

        match (&property.source, &property.value) {
            (EffectivePropertySourceV1::Base, FormatValueV1::String(binding))
                if !binding.is_empty() =>
            {
                all_scalars_shaped = false;
                spans.push(CurrentPhysicalFontSpanV1::SourceUnresolved {
                    start_scalar: start,
                    end_scalar: end,
                    source_font_binding_id: binding.clone(),
                });
            }
            (EffectivePropertySourceV1::ChapteraOverride, FormatValueV1::FontResource(current)) => {
                if current != identity {
                    return Err(blocked(
                        "current_font_resource_missing",
                        "Story override requires a different independently admitted physical resource",
                    ));
                }
                let start_index = usize::try_from(start).map_err(|_| {
                    blocked("story_extent_overflow", "invalid Unicode scalar start")
                })?;
                let end_index = usize::try_from(end)
                    .map_err(|_| blocked("story_extent_overflow", "invalid Unicode scalar end"))?;
                let interval: String = scalars[start_index..end_index].iter().collect();
                let runtime = BoundedShapingRuntime {
                    layout: BoundedLayoutEnvironment {
                        engine_revision: CURRENT_FONT_SPANS_V1.to_owned(),
                        font_set_fingerprint: fingerprint
                            .as_ref()
                            .expect("font override is admitted before shaping")
                            .clone(),
                        resource_fingerprint: identity.resource_id.clone(),
                    },
                    face_index: u32::from(identity.face_index),
                    font_size_emu: LengthEmu::new(size_emu),
                    font_bytes: trusted.full_font_bytes,
                };
                let shaped = shape_bounded_ltr_segment(&interval, start, &runtime)
                    .map_err(|error| blocked("current_font_shaping_failed", error.to_string()))?;
                spans.push(CurrentPhysicalFontSpanV1::AdmittedExact {
                    start_scalar: start,
                    end_scalar: end,
                    identity: current.clone(),
                    font_size_emu: LengthEmu::new(size_emu),
                    shaped,
                });
            }
            _ => {
                return Err(blocked(
                    "current_font_property_unverifiable",
                    "source font labels and admitted physical identities cannot be interchanged",
                ));
            }
        }
    }
    Ok(CurrentPhysicalFontSpansV1 {
        protocol_version: CURRENT_FONT_SPANS_V1.to_owned(),
        story_id: state.story_id.clone(),
        story_format_state_hash: state_hash,
        story_scalar_len: text_len,
        spans,
        all_scalars_shaped,
        authoritative_line_breaks: false,
        fixed_pdf_allowed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chaptera_text_format_overlay::{
        BaseCharacterFormatV1, BaseFormatRunV1, FontAuthoringScopeV1, FontReplacementCandidateV1,
        TextFormatOverrideRunV1, build_text_format_overlay_state_v1,
    };

    fn fixture_identity(font: &[u8]) -> FontResourceIdentityV1 {
        let sha = font_fingerprint_sha256(font);
        FontResourceIdentityV1 {
            resource_id: "f27a8036-8492-480f-8fa6-d2e775cc9bcfda".to_owned(),
            font_fingerprint: format!("sha256:{sha}"),
            content_hash: sha,
            face_index: 0,
        }
    }

    fn base(size: u64, resource: &str) -> BaseCharacterFormatV1 {
        BaseCharacterFormatV1 {
            font_resource_id: resource.to_owned(),
            font_size_emu: size,
            bold: false,
            italic: false,
            text_color_rgb: "#000000".to_owned(),
        }
    }

    fn state(identity: &FontResourceIdentityV1, start: u32, end: u32) -> TextFormatOverlayStateV1 {
        build_text_format_overlay_state_v1(
            "story-test",
            "source-story-revision",
            4,
            vec![
                BaseFormatRunV1 {
                    start_scalar: 0,
                    end_scalar: 2,
                    format: base(120_000, "pub-source-font:first"),
                },
                BaseFormatRunV1 {
                    start_scalar: 2,
                    end_scalar: 4,
                    format: base(240_000, "pub-source-font:second"),
                },
            ],
            vec![TextFormatOverrideRunV1 {
                start_scalar: start,
                end_scalar: end,
                property: FormatPropertyV1::FontResource,
                value: FormatValueV1::FontResource(identity.clone()),
            }],
        )
        .expect("canonical mixed source and physical font history")
    }

    #[test]
    fn mixed_ranges_shape_exact_glyphs_with_global_scalar_clusters_and_effective_sizes() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let identity = fixture_identity(bytes);
        let resource = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let before = state(&identity, 1, 3);
        let result = project_exact_font_spans_from_overlay_v1(&before, "Aéfi", &resource)
            .expect("shape bounded partial font override");
        assert_eq!(result.protocol_version, CURRENT_FONT_SPANS_V1);
        assert!(!result.all_scalars_shaped);
        assert!(!result.authoritative_line_breaks);
        assert!(!result.fixed_pdf_allowed);
        assert_eq!(result.spans.len(), 4);
        assert!(matches!(
            result.spans[0],
            CurrentPhysicalFontSpanV1::SourceUnresolved {
                start_scalar: 0,
                end_scalar: 1,
                ..
            }
        ));
        assert!(matches!(
            result.spans[3],
            CurrentPhysicalFontSpanV1::SourceUnresolved {
                start_scalar: 3,
                end_scalar: 4,
                ..
            }
        ));
        for (position, expected_size) in [(1, 120_000_i64), (2, 240_000_i64)] {
            match &result.spans[position] {
                CurrentPhysicalFontSpanV1::AdmittedExact {
                    start_scalar,
                    end_scalar,
                    font_size_emu,
                    shaped,
                    identity: admitted,
                } => {
                    assert_eq!(admitted, &identity);
                    assert_eq!(*start_scalar, position as u32);
                    assert_eq!(*end_scalar, position as u32 + 1);
                    assert_eq!(font_size_emu.get(), expected_size);
                    assert!(!shaped.glyphs.is_empty());
                    assert!(
                        shaped
                            .glyphs
                            .iter()
                            .all(|glyph| *start_scalar <= glyph.cluster
                                && glyph.cluster < *end_scalar)
                    );
                }
                _ => panic!("physical span was silently replaced by source font"),
            }
        }
        assert_eq!(
            result,
            project_exact_font_spans_from_overlay_v1(&before, "Aéfi", &resource)
                .expect("identical full bytes must produce deterministic glyphs"),
        );
    }

    #[test]
    fn altered_missing_subset_or_unadmitted_physical_resource_never_shapes_as_fallback() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let identity = fixture_identity(bytes);
        let overlay = state(&identity, 1, 3);
        for (full, allowed, payload, face_count) in [
            (false, true, bytes, 1_u32),
            (true, false, bytes, 1),
            (true, true, b"wrong-font-bytes".as_slice(), 1),
            (true, true, bytes, 0),
        ] {
            let grant = ServerFontResourceV1 {
                identity: &identity,
                full_font_bytes: payload,
                face_count,
                is_full_resource: full,
                authoring_admitted: allowed,
            };
            assert!(project_exact_font_spans_from_overlay_v1(&overlay, "Aéfi", &grant).is_err());
        }
        let wrong_identity = FontResourceIdentityV1 {
            resource_id: "00000000-0000-4000-8000-000000000000".to_owned(),
            ..identity.clone()
        };
        let wrong = ServerFontResourceV1 {
            identity: &wrong_identity,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        assert!(project_exact_font_spans_from_overlay_v1(&overlay, "Aéfi", &wrong).is_err());
        assert!(
            project_exact_font_spans_from_overlay_v1(
                &overlay,
                "Aé",
                &ServerFontResourceV1 {
                    identity: &identity,
                    full_font_bytes: bytes,
                    face_count: 1,
                    is_full_resource: true,
                    authoring_admitted: true,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn even_whole_story_physical_font_does_not_certify_line_break_or_pdf() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let identity = fixture_identity(bytes);
        let trusted = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let overlay = state(&identity, 0, 4);
        let result = project_exact_font_spans_from_overlay_v1(&overlay, "Aéfi", &trusted)
            .expect("all scalar ranges have an admitted physical font");
        assert!(result.all_scalars_shaped);
        assert!(
            result
                .spans
                .iter()
                .all(|span| matches!(span, CurrentPhysicalFontSpanV1::AdmittedExact { .. }))
        );
        assert!(!result.authoritative_line_breaks);
        assert!(!result.fixed_pdf_allowed);
    }

    #[test]
    fn real_newsletter_font_history_shapes_only_admitted_scalar_and_survives_reopen() {
        use pub_editor::{EditorProjectFontReopenGrantV1, Sha256Digest, open_mature_0x2c_editor};
        use sha2::{Digest, Sha256};
        use std::{collections::BTreeMap, env, fs};
        let Some(path) = env::var_os("CHAPTERA_SAMPLE_NEWSLETTER") else {
            eprintln!("real Newsletter gate is run with CHAPTERA_SAMPLE_NEWSLETTER");
            return;
        };
        let source_bytes = fs::read(path).expect("real pinned PUB source");
        let source_hash = Sha256Digest::from_bytes(Sha256::digest(&source_bytes).into());
        let mut editor = open_mature_0x2c_editor(&source_bytes, source_hash)
            .expect("real source-backed EditorSession");
        let story_id = editor
            .graph()
            .stories
            .keys()
            .copied()
            .find(|id| {
                editor.graph().stories[id].text.chars().count() >= 2
                    && editor.current_text_format_overlay_v1(*id).is_ok()
            })
            .expect("real PUB Story with a fully bounded source typography base");
        let font_bytes = include_bytes!("../../../assets/fonts/ofl/abel/Abel-Regular.ttf");
        let identity = fixture_identity(font_bytes);
        let grant = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: font_bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let scope = FontAuthoringScopeV1 {
            document_id: editor
                .project()
                .identity
                .as_ref()
                .unwrap()
                .document_id
                .clone(),
            revision_id: "sha256:".to_owned() + &"1".repeat(64),
            scene_snapshot_id: "sha256:".to_owned() + &"2".repeat(64),
            layout_environment_id: "sha256:".to_owned() + &"3".repeat(64),
            font_set_fingerprint: "sha256:".to_owned() + &"4".repeat(64),
        };
        let candidate = FontReplacementCandidateV1 {
            protocol_version: "chaptera.font-replacement-candidate.v1".to_owned(),
            document_id: scope.document_id.clone(),
            expected_revision_id: scope.revision_id.clone(),
            scene_snapshot_id: scope.scene_snapshot_id.clone(),
            layout_environment_id: scope.layout_environment_id.clone(),
            font_set_fingerprint: scope.font_set_fingerprint.clone(),
            resource_id: identity.resource_id.clone(),
            font_fingerprint: identity.font_fingerprint.clone(),
            content_hash: identity.content_hash.clone(),
            face_index: identity.face_index,
            authority: "candidate_only_server_validation_required".to_owned(),
        };
        let original = shape_current_exact_font_override_spans_v1(&editor, story_id, &grant)
            .expect("source binding stays unresolved without physical bytes");
        assert!(!original.all_scalars_shaped);
        assert!(
            original
                .spans
                .iter()
                .all(|x| matches!(x, CurrentPhysicalFontSpanV1::SourceUnresolved { .. }))
        );
        let before = editor.current_text_format_state_hash_v1(story_id).unwrap();
        editor
            .set_admitted_font_resource_v1(story_id, 0, 1, &candidate, &scope, &grant, &before)
            .expect("real authorized one-scalar font edit");
        let edited = shape_current_exact_font_override_spans_v1(&editor, story_id, &grant)
            .expect("current exact font override must yield real glyph positions");
        assert!(!edited.all_scalars_shaped);
        assert!(edited.spans.iter().any(|x| matches!(
            x,
            CurrentPhysicalFontSpanV1::AdmittedExact {
                start_scalar: 0,
                end_scalar: 1,
                ..
            }
        )));
        let project = editor.project();
        editor.undo().expect("canonical Undo");
        let undone = shape_current_exact_font_override_spans_v1(&editor, story_id, &grant)
            .expect("source typography back after Undo");
        assert_eq!(undone, original);
        editor.redo().expect("canonical Redo");
        assert_eq!(
            shape_current_exact_font_override_spans_v1(&editor, story_id, &grant)
                .expect("reapply exact glyphs"),
            edited,
        );
        let mut fresh =
            open_mature_0x2c_editor(&source_bytes, source_hash).expect("fresh real PUB source");
        let reopen = EditorProjectFontReopenGrantV1 {
            source_hash,
            project_document_id: &project.identity.as_ref().unwrap().document_id,
            resource: ServerFontResourceV1 {
                identity: &identity,
                full_font_bytes: font_bytes,
                face_count: 1,
                is_full_resource: true,
                authoring_admitted: true,
            },
        };
        fresh
            .apply_project_with_admitted_font_resources_v1(&project, &BTreeMap::new(), &[reopen])
            .expect("fresh project re-admitted by independent exact byte grant");
        assert_eq!(
            shape_current_exact_font_override_spans_v1(&fresh, story_id, &grant)
                .expect("fresh reopened Story must yield same glyph spans"),
            edited,
        );
        assert_eq!(source_hash, editor.source_hash());
        println!(
            "REAL_PUB_FONT_SPANS_OK {}",
            serde_json::json!({
                "protocol_version": CURRENT_FONT_SPANS_V1,
                "real_source": true,
                "exact_glyphs": true,
                "one_scalar_only": true,
                "undo_redo": true,
                "fresh_reopen": true,
                "line_breaks_verified": false,
                "fixed_pdf_allowed": false,
            })
        );
    }
}
