use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const READER_COMPATIBILITY_REPORT_V1: &str = "chaptera.reader-compatibility-report.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderCompatibilityReportV1 {
    pub protocol_version: &'static str,
    pub source_sha256: String,
    pub state: &'static str,
    pub engine_classification: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_summary: Option<ReaderCompatibilityContentSummaryV1>,
    pub limitations: Vec<ReaderCompatibilityLimitationV1>,
    pub output_routes: ReaderCompatibilityOutputRoutesV1,
    pub recommended_next_step: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderCompatibilityContentSummaryV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_frame_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_frame_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_node_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub story_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_resource_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_text_range_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_image_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_geometry_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderCompatibilityLimitationV1 {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderCompatibilityOutputRoutesV1 {
    pub read_only_preview: &'static str,
    pub salvage_recovery: &'static str,
    pub editable_idml: &'static str,
    pub editable_odg: &'static str,
}

pub const READER_EDITABLE_ROUTES_ASSESSMENT_V1: &str =
    "chaptera.reader-editable-routes.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderEditableRoutesAssessmentV1 {
    pub protocol_version: String,
    pub source_sha256: String,
    pub idml: ReaderEditableTargetAssessmentV1,
    pub odg: ReaderEditableTargetAssessmentV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderEditableTargetAssessmentV1 {
    pub state: String,
    pub reason_code: String,
}

impl ReaderEditableRoutesAssessmentV1 {
    pub fn validate_for_source(&self, source_sha256: &str) -> Result<(), String> {
        validate_source_sha256(source_sha256)?;
        if self.protocol_version != READER_EDITABLE_ROUTES_ASSESSMENT_V1
            || self.source_sha256 != source_sha256
        {
            return Err("editable route assessment identity differs from compatibility source".to_owned());
        }
        validate_target_assessment(&self.idml)?;
        validate_target_assessment(&self.odg)?;
        Ok(())
    }
}

pub fn build_reader_compatibility_report(
    source_sha256: &str,
    classification: &str,
    scene: Option<&Value>,
    salvage: Option<&Value>,
) -> Result<ReaderCompatibilityReportV1, String> {
    build_reader_compatibility_report_with_routes(
        source_sha256,
        classification,
        scene,
        salvage,
        None,
    )
}

pub fn build_reader_compatibility_report_with_routes(
    source_sha256: &str,
    classification: &str,
    scene: Option<&Value>,
    salvage: Option<&Value>,
    editable_routes: Option<&ReaderEditableRoutesAssessmentV1>,
) -> Result<ReaderCompatibilityReportV1, String> {
    validate_source_sha256(source_sha256)?;
    if let Some(routes) = editable_routes {
        routes.validate_for_source(source_sha256)?;
    }

    let (state, content_summary, mut limitations, output_routes, recommended_next_step) =
        match classification {
            "supported" | "partial" => {
                if salvage.is_some() {
                    return Err(
                        "normal compatibility report cannot carry salvage observation".to_owned(),
                    );
                }
                let scene = scene.ok_or_else(|| {
                    "normal compatibility report is missing Reader Scene".to_owned()
                })?;
                validate_scene_identity(scene, source_sha256)?;
                let summary = summarize_scene(scene)?;
                let mut limitations = scene_limitations(scene);
                if classification == "partial" && limitations.is_empty() {
                    limitations.push(ReaderCompatibilityLimitationV1 {
                        code: "preview_limitations",
                        message: "The preview has known limitations and should be reviewed before migration.",
                    });
                }
                append_editable_route_limitations(&mut limitations, editable_routes);
                (
                    if classification == "supported" {
                        "opens_normally"
                    } else {
                        "needs_review"
                    },
                    Some(summary),
                    limitations,
                    ReaderCompatibilityOutputRoutesV1 {
                        read_only_preview: if classification == "supported" {
                            "available"
                        } else {
                            "available_with_limitations"
                        },
                        salvage_recovery: "not_applicable",
                        editable_idml: editable_route_state(editable_routes.map(|routes| &routes.idml))?,
                        editable_odg: editable_route_state(editable_routes.map(|routes| &routes.odg))?,
                    },
                    if classification == "supported" {
                        "migration_pilot_preview"
                    } else {
                        "review_preview_before_migration"
                    },
                )
            }
            "salvage" => {
                if editable_routes.is_some() {
                    return Err("salvage compatibility report cannot carry editable route assessment".to_owned());
                }
                if scene.is_some() {
                    return Err("salvage compatibility report cannot carry Reader Scene".to_owned());
                }
                let salvage = salvage.ok_or_else(|| {
                    "salvage compatibility report is missing recovery observation".to_owned()
                })?;
                validate_salvage_identity(salvage, source_sha256)?;
                (
                    "opens_with_salvage",
                    Some(summarize_salvage(salvage)?),
                    salvage_limitations(salvage),
                    ReaderCompatibilityOutputRoutesV1 {
                        read_only_preview: "unavailable",
                        salvage_recovery: "available",
                        editable_idml: "not_verified",
                        editable_odg: "not_verified",
                    },
                    "rescue_review",
                )
            }
            "unsupported" => {
                if editable_routes.is_some() {
                    return Err("unsupported compatibility report cannot carry editable route assessment".to_owned());
                }
                if scene.is_some() || salvage.is_some() {
                    return Err(
                        "unsupported compatibility report cannot carry document payload".to_owned(),
                    );
                }
                (
                    "unsupported",
                    None,
                    vec![ReaderCompatibilityLimitationV1 {
                        code: "automatic_open_unavailable",
                        message: "Chaptera cannot currently produce a trustworthy preview for this file.",
                    }],
                    ReaderCompatibilityOutputRoutesV1 {
                        read_only_preview: "unavailable",
                        salvage_recovery: "unavailable",
                        editable_idml: "not_verified",
                        editable_odg: "not_verified",
                    },
                    "unsupported_or_manual_review",
                )
            }
            _ => {
                return Err("unsupported Reader classification for compatibility report".to_owned());
            }
        };

    Ok(ReaderCompatibilityReportV1 {
        protocol_version: READER_COMPATIBILITY_REPORT_V1,
        source_sha256: source_sha256.to_owned(),
        state,
        engine_classification: classification.to_owned(),
        content_summary,
        limitations: {
            if limitations.len() > 16 {
                limitations.truncate(16);
            }
            limitations
        },
        output_routes,
        recommended_next_step,
    })
}

fn validate_target_assessment(value: &ReaderEditableTargetAssessmentV1) -> Result<(), String> {
    let valid = matches!(
        (value.state.as_str(), value.reason_code.as_str()),
        ("available_with_declared_losses", "serializable")
            | ("unavailable", "blocking_losses")
            | ("unavailable", "editor_profile_unavailable")
            | ("not_verified", "assessment_failed")
            | ("not_verified", "source_identity_mismatch")
    );
    if !valid {
        return Err("editable route assessment state/reason pair is invalid".to_owned());
    }
    Ok(())
}

fn editable_route_state(
    value: Option<&ReaderEditableTargetAssessmentV1>,
) -> Result<&'static str, String> {
    let Some(value) = value else {
        return Ok("not_verified");
    };
    match value.state.as_str() {
        "available_with_declared_losses" => Ok("available_with_declared_losses"),
        "unavailable" => Ok("unavailable"),
        "not_verified" => Ok("not_verified"),
        _ => Err("editable route assessment state is invalid".to_owned()),
    }
}

fn append_editable_route_limitations(
    limitations: &mut Vec<ReaderCompatibilityLimitationV1>,
    routes: Option<&ReaderEditableRoutesAssessmentV1>,
) {
    let Some(routes) = routes else {
        return;
    };
    append_target_route_limitation(limitations, "idml", &routes.idml);
    append_target_route_limitation(limitations, "odg", &routes.odg);
}

fn append_target_route_limitation(
    limitations: &mut Vec<ReaderCompatibilityLimitationV1>,
    target: &'static str,
    assessment: &ReaderEditableTargetAssessmentV1,
) {
    let item = match (target, assessment.state.as_str(), assessment.reason_code.as_str()) {
        ("idml", "unavailable", "blocking_losses") => Some(ReaderCompatibilityLimitationV1 {
            code: "idml_editable_export_blocked",
            message: "IDML editable export is blocked because required document semantics would be lost.",
        }),
        ("odg", "unavailable", "blocking_losses") => Some(ReaderCompatibilityLimitationV1 {
            code: "odg_editable_export_blocked",
            message: "ODG editable export is blocked because required document semantics would be lost.",
        }),
        ("idml", "unavailable", "editor_profile_unavailable") => Some(ReaderCompatibilityLimitationV1 {
            code: "idml_editable_export_unavailable",
            message: "IDML editable export is not available for this Publisher profile.",
        }),
        ("odg", "unavailable", "editor_profile_unavailable") => Some(ReaderCompatibilityLimitationV1 {
            code: "odg_editable_export_unavailable",
            message: "ODG editable export is not available for this Publisher profile.",
        }),
        ("idml", "not_verified", _) => Some(ReaderCompatibilityLimitationV1 {
            code: "idml_editable_export_not_verified",
            message: "IDML editable export could not be verified for this file.",
        }),
        ("odg", "not_verified", _) => Some(ReaderCompatibilityLimitationV1 {
            code: "odg_editable_export_not_verified",
            message: "ODG editable export could not be verified for this file.",
        }),
        _ => None,
    };
    if let Some(item) = item {
        if !limitations.iter().any(|existing| existing.code == item.code) {
            limitations.push(item);
        }
    }
}

fn validate_source_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("compatibility report source SHA-256 is invalid".to_owned());
    }
    Ok(())
}

fn validate_scene_identity(scene: &Value, source_sha256: &str) -> Result<(), String> {
    if scene.get("protocol_version").and_then(Value::as_str) != Some("chaptera.reader-scene.v1")
        || scene.get("source_hash").and_then(Value::as_str) != Some(source_sha256)
    {
        return Err("Reader Scene identity differs from compatibility source".to_owned());
    }
    Ok(())
}

fn validate_salvage_identity(salvage: &Value, source_sha256: &str) -> Result<(), String> {
    if salvage.get("schema_version").and_then(Value::as_str)
        != Some("chaptera.reader-partial-source-graph.v1")
        || salvage.get("source_sha256").and_then(Value::as_str) != Some(source_sha256)
    {
        return Err("salvage identity differs from compatibility source".to_owned());
    }
    Ok(())
}

fn summarize_scene(scene: &Value) -> Result<ReaderCompatibilityContentSummaryV1, String> {
    let pages = required_array(scene, "pages")?;
    let nodes = required_array(scene, "nodes")?;
    let stories = required_array(scene, "stories")?;
    let resources = optional_array(scene, "resources")?;

    let mut text_frames = 0_u64;
    let mut picture_frames = 0_u64;
    let mut tables = 0_u64;
    let mut other = 0_u64;
    for node in nodes {
        let object = node
            .as_object()
            .ok_or_else(|| "Reader Scene node is not an object".to_owned())?;
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if object.get("table").is_some_and(|value| !value.is_null()) || kind == "table" {
            tables += 1;
        } else if kind == "text_frame" {
            text_frames += 1;
        } else if kind == "picture_frame" {
            picture_frames += 1;
        } else {
            other += 1;
        }
    }

    Ok(ReaderCompatibilityContentSummaryV1 {
        page_count: Some(count(pages.len())?),
        text_frame_count: Some(text_frames),
        picture_frame_count: Some(picture_frames),
        table_count: Some(tables),
        other_node_count: Some(other),
        story_count: Some(count(stories.len())?),
        image_resource_count: Some(count(resources.len())?),
        recovered_text_range_count: None,
        recovered_image_count: None,
        recovered_geometry_count: None,
    })
}

fn summarize_salvage(salvage: &Value) -> Result<ReaderCompatibilityContentSummaryV1, String> {
    let facts = required_array(salvage, "facts")?;
    let mut text_ranges = 0_u64;
    let mut images = 0_u64;
    let mut geometry = 0_u64;
    for fact in facts {
        match fact.get("kind").and_then(Value::as_str) {
            Some("text_range") => text_ranges += 1,
            Some("verified_image") => images += 1,
            Some("grounded_geometry") => geometry += 1,
            Some(_) | None => {
                return Err("salvage fact uses an unsupported compatibility kind".to_owned());
            }
        }
    }

    Ok(ReaderCompatibilityContentSummaryV1 {
        page_count: None,
        text_frame_count: None,
        picture_frame_count: None,
        table_count: None,
        other_node_count: None,
        story_count: None,
        image_resource_count: None,
        recovered_text_range_count: Some(text_ranges),
        recovered_image_count: Some(images),
        recovered_geometry_count: Some(geometry),
    })
}

fn scene_limitations(scene: &Value) -> Vec<ReaderCompatibilityLimitationV1> {
    let mut limitations = Vec::new();
    let mut seen = HashSet::new();
    let reasons = scene
        .get("fidelity")
        .and_then(|value| value.get("reasons"))
        .and_then(Value::as_array);
    for reason in reasons.into_iter().flatten() {
        let (code, message) = match reason.as_str() {
            Some("stacking_order_unavailable") => (
                "overlap_order_may_differ",
                "Overlapping objects may appear in a different order than in Publisher.",
            ),
            Some("node_kind_partial") => (
                "object_identification_partial",
                "Some document objects are only partially identified in this preview.",
            ),
            Some("image_resource_not_inline") => (
                "image_preview_unavailable",
                "Some embedded images are unavailable in this preview.",
            ),
            Some("text_layout_partial") | Some("shared_text_layout_unavailable") => (
                "text_layout_may_differ",
                "Some text layout may differ from Microsoft Publisher.",
            ),
            Some("explicit_fallback_font_substitution") => (
                "font_substitution",
                "Some text uses a substitute font and may wrap or size differently.",
            ),
            Some("viewer_fidelity_warnings") => (
                "preview_fidelity_warning",
                "The preview contains known display limitations.",
            ),
            Some(_) | None => (
                "preview_limitation_other",
                "The preview contains an additional limitation that should be reviewed before migration.",
            ),
        };
        push_unique(&mut limitations, &mut seen, code, message);
    }
    limitations
}

fn salvage_limitations(salvage: &Value) -> Vec<ReaderCompatibilityLimitationV1> {
    let mut limitations = Vec::new();
    let mut seen = HashSet::new();
    let gaps = salvage.get("gaps").and_then(Value::as_array);
    for gap in gaps.into_iter().flatten() {
        let (code, message) = match gap.as_str() {
            Some("text_unavailable") => (
                "recovered_text_incomplete",
                "Some text could not be recovered from the source file.",
            ),
            Some("text_semantic_ambiguity") => (
                "recovered_text_needs_review",
                "Recovered text exists, but some text relationships remain ambiguous.",
            ),
            Some("image_facts_unavailable") => (
                "recovered_images_incomplete",
                "Some image facts could not be recovered from the source file.",
            ),
            Some("geometry_facts_unavailable") => (
                "recovered_geometry_incomplete",
                "Page placement and geometry could not be fully recovered.",
            ),
            Some(_) | None => (
                "recovery_limitation_other",
                "Recovery contains an additional limitation that requires review.",
            ),
        };
        push_unique(&mut limitations, &mut seen, code, message);
    }
    if limitations.is_empty() {
        push_unique(
            &mut limitations,
            &mut seen,
            "recovery_mode",
            "Only source-backed recovered facts are available; normal page layout is not claimed.",
        );
    }
    limitations
}

fn push_unique(
    limitations: &mut Vec<ReaderCompatibilityLimitationV1>,
    seen: &mut HashSet<&'static str>,
    code: &'static str,
    message: &'static str,
) {
    if seen.insert(code) {
        limitations.push(ReaderCompatibilityLimitationV1 { code, message });
    }
}

fn required_array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("compatibility source is missing array {key}"))
}

fn optional_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    match value.get(key) {
        Some(value) => value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("compatibility source field {key} is not an array")),
        None => Ok(&[]),
    }
}

fn count(value: usize) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| "compatibility count exceeds u64".to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_string};

    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn supported_scene_report_is_source_safe_and_counted() {
        let scene = json!({
            "protocol_version": "chaptera.reader-scene.v1",
            "source_hash": SHA,
            "fidelity": {"state": "supported", "reasons": []},
            "pages": [{"page_id": "p1"}],
            "nodes": [
                {"kind": "text_frame"},
                {"kind": "picture_frame"},
                {"kind": "table", "table": {}},
                {"kind": "unknown"}
            ],
            "stories": [{"story_id": "s1", "text": "customer secret text"}],
            "resources": [{"resource_id": "r1"}]
        });
        let report =
            build_reader_compatibility_report(SHA, "supported", Some(&scene), None).unwrap();

        assert_eq!(report.state, "opens_normally");
        assert_eq!(report.recommended_next_step, "migration_pilot_preview");
        let summary = report.content_summary.as_ref().unwrap();
        assert_eq!(summary.page_count, Some(1));
        assert_eq!(summary.text_frame_count, Some(1));
        assert_eq!(summary.picture_frame_count, Some(1));
        assert_eq!(summary.table_count, Some(1));
        assert_eq!(summary.other_node_count, Some(1));
        assert_eq!(summary.story_count, Some(1));
        assert_eq!(summary.image_resource_count, Some(1));
        let encoded = to_string(&report).unwrap();
        assert!(!encoded.contains("customer secret text"));
        assert!(!encoded.contains("story_id"));
        assert!(!encoded.contains("resource_id"));
    }

    #[test]
    fn partial_scene_maps_only_stable_customer_limitations() {
        let scene = json!({
            "protocol_version": "chaptera.reader-scene.v1",
            "source_hash": SHA,
            "fidelity": {
                "state": "partial",
                "reasons": [
                    "text_layout_partial",
                    "viewer_fidelity_warnings",
                    "new_internal_reason"
                ]
            },
            "pages": [{"page_id": "p1"}],
            "nodes": [],
            "stories": []
        });
        let report = build_reader_compatibility_report(SHA, "partial", Some(&scene), None).unwrap();

        assert_eq!(report.state, "needs_review");
        let codes = report
            .limitations
            .iter()
            .map(|item| item.code)
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            vec![
                "text_layout_may_differ",
                "preview_fidelity_warning",
                "preview_limitation_other"
            ]
        );
    }

    #[test]
    fn salvage_report_counts_only_recovered_fact_kinds() {
        let salvage = json!({
            "schema_version": "chaptera.reader-partial-source-graph.v1",
            "source_sha256": SHA,
            "subsystems": {},
            "facts": [
                {"kind": "text_range", "text": "private text"},
                {"kind": "verified_image", "resource_key": "private-resource"},
                {"kind": "grounded_geometry", "node_key": "private-node"}
            ],
            "gaps": ["geometry_facts_unavailable"]
        });
        let report =
            build_reader_compatibility_report(SHA, "salvage", None, Some(&salvage)).unwrap();

        assert_eq!(report.state, "opens_with_salvage");
        let summary = report.content_summary.as_ref().unwrap();
        assert_eq!(summary.recovered_text_range_count, Some(1));
        assert_eq!(summary.recovered_image_count, Some(1));
        assert_eq!(summary.recovered_geometry_count, Some(1));
        let encoded = to_string(&report).unwrap();
        assert!(!encoded.contains("private text"));
        assert!(!encoded.contains("private-resource"));
        assert!(!encoded.contains("private-node"));
    }

    #[test]
    fn unsupported_report_advertises_no_unverified_conversion() {
        let report = build_reader_compatibility_report(SHA, "unsupported", None, None).unwrap();

        assert_eq!(report.state, "unsupported");
        assert_eq!(report.output_routes.read_only_preview, "unavailable");
        assert_eq!(report.output_routes.editable_idml, "not_verified");
        assert_eq!(report.output_routes.editable_odg, "not_verified");
        assert!(report.content_summary.is_none());
    }

    #[test]
    fn source_identity_mismatch_fails_closed() {
        let scene = json!({
            "protocol_version": "chaptera.reader-scene.v1",
            "source_hash": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "fidelity": {"state": "supported", "reasons": []},
            "pages": [],
            "nodes": [],
            "stories": []
        });
        assert!(build_reader_compatibility_report(SHA, "supported", Some(&scene), None).is_err());
    }
}
