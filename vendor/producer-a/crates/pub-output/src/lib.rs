//! Deterministic fixed-output font policy.
//!
//! This crate owns the boundary after layout/shaping and before a fixed-output
//! renderer. It never performs host-font discovery and never imports PUB source
//! parser state. Callers must provide explicit font bytes, a SHA-256 identity,
//! face index, technical embedding flags, and the glyph IDs actually used by
//! the resolved text run.

use harfrust::FontRef;
use pub_layout::{BoundedShapedText, font_fingerprint_sha256};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const OUTPUT_FONT_PLAN_SCHEMA_V0_1: &str = "0.1";
pub const FONT_OUTPUT_POLICY_REVISION_V0_1: &str = "font-output-v0.1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FontIdentity {
    pub fingerprint_sha256: String,
    pub face_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingLevel {
    Installable,
    PreviewPrint,
    Editable,
    Restricted,
    Unknown,
}

/// Explicit technical embedding flags supplied by the font/resource boundary.
///
/// These fields mirror the policy-relevant shape of OpenType embedding flags;
/// this crate does not make a legal licensing judgement from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TechnicalEmbeddingFlags {
    pub level: EmbeddingLevel,
    pub no_subsetting: bool,
    pub bitmap_only: bool,
}

impl TechnicalEmbeddingFlags {
    pub const fn installable() -> Self {
        Self {
            level: EmbeddingLevel::Installable,
            no_subsetting: false,
            bitmap_only: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExplicitFontResource<'a> {
    pub identity: FontIdentity,
    pub bytes: &'a [u8],
    pub embedding: TechnicalEmbeddingFlags,
}

#[derive(Debug, Clone)]
pub struct OutputFontRequest<'a> {
    /// Identity used by resolved shaping/layout state.
    pub source: FontIdentity,
    /// Exact source font bytes. None means the resolved font identity is known
    /// but the renderer has not been given authorized bytes.
    pub source_resource: Option<ExplicitFontResource<'a>>,
    /// Optional explicit substitute. There is deliberately no host lookup.
    pub fallback_resource: Option<ExplicitFontResource<'a>>,
    /// Glyph IDs used by resolved text. BTreeSet makes the subset input stable.
    pub used_glyph_ids: BTreeSet<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFontDisposition {
    EmbedFull,
    EmbedSubset,
    Outline,
    Substitute,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFontMaterialization {
    EmbedFull,
    EmbedSubset,
    Outline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferredEmbedding {
    Full,
    Subset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NonEmbeddableAction {
    Outline,
    Substitute,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingFontAction {
    Substitute,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedOutputFontProfile {
    pub target_format: String,
    pub profile: String,
    pub policy_revision: String,
    pub preferred_embedding: PreferredEmbedding,
    pub force_outline: bool,
    pub missing_font_action: MissingFontAction,
    pub restricted_font_action: NonEmbeddableAction,
    pub unknown_embedding_action: NonEmbeddableAction,
    pub bitmap_only_action: NonEmbeddableAction,
}

impl FixedOutputFontProfile {
    /// First PDF policy: subset technically embeddable fonts and fail closed
    /// for missing/restricted/unknown/bitmap-only sources.
    pub fn basic_pdf_v0_1() -> Self {
        Self {
            target_format: "pdf".into(),
            profile: "basic-fixed".into(),
            policy_revision: FONT_OUTPUT_POLICY_REVISION_V0_1.into(),
            preferred_embedding: PreferredEmbedding::Subset,
            force_outline: false,
            missing_font_action: MissingFontAction::Block,
            restricted_font_action: NonEmbeddableAction::Block,
            unknown_embedding_action: NonEmbeddableAction::Block,
            bitmap_only_action: NonEmbeddableAction::Block,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontOutputReason {
    EmbedFull,
    EmbedSubset,
    EmbedFullNoSubsetting,
    OutlinePolicy,
    OutlineRestricted,
    OutlineUnknownEmbedding,
    OutlineBitmapOnly,
    SubstituteMissingSource,
    SubstituteRestricted,
    SubstituteUnknownEmbedding,
    SubstituteBitmapOnly,
    BlockNoGlyphUsage,
    BlockDuplicateSourceRequest,
    BlockInvalidSourceIdentity,
    BlockSourceIdentityMismatch,
    BlockSourceFingerprintMismatch,
    BlockInvalidSourceFace,
    BlockMissingSource,
    BlockRestricted,
    BlockUnknownEmbedding,
    BlockBitmapOnly,
    BlockMissingFallback,
    BlockInvalidFallbackIdentity,
    BlockFallbackFingerprintMismatch,
    BlockInvalidFallbackFace,
    BlockFallbackNotEmbeddable,
}

impl FontOutputReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::EmbedFull => "font.output.embed_full",
            Self::EmbedSubset => "font.output.embed_subset",
            Self::EmbedFullNoSubsetting => "font.output.embed_full_no_subsetting",
            Self::OutlinePolicy => "font.output.outline_policy",
            Self::OutlineRestricted => "font.output.outline_restricted",
            Self::OutlineUnknownEmbedding => "font.output.outline_unknown_embedding",
            Self::OutlineBitmapOnly => "font.output.outline_bitmap_only",
            Self::SubstituteMissingSource => "font.output.substitute_missing_source",
            Self::SubstituteRestricted => "font.output.substitute_restricted",
            Self::SubstituteUnknownEmbedding => "font.output.substitute_unknown_embedding",
            Self::SubstituteBitmapOnly => "font.output.substitute_bitmap_only",
            Self::BlockNoGlyphUsage => "font.output.block_no_glyph_usage",
            Self::BlockDuplicateSourceRequest => "font.output.block_duplicate_source_request",
            Self::BlockInvalidSourceIdentity => "font.output.block_invalid_source_identity",
            Self::BlockSourceIdentityMismatch => "font.output.block_source_identity_mismatch",
            Self::BlockSourceFingerprintMismatch => "font.output.block_source_fingerprint_mismatch",
            Self::BlockInvalidSourceFace => "font.output.block_invalid_source_face",
            Self::BlockMissingSource => "font.output.block_missing_source",
            Self::BlockRestricted => "font.output.block_restricted",
            Self::BlockUnknownEmbedding => "font.output.block_unknown_embedding",
            Self::BlockBitmapOnly => "font.output.block_bitmap_only",
            Self::BlockMissingFallback => "font.output.block_missing_fallback",
            Self::BlockInvalidFallbackIdentity => "font.output.block_invalid_fallback_identity",
            Self::BlockFallbackFingerprintMismatch => {
                "font.output.block_fallback_fingerprint_mismatch"
            }
            Self::BlockInvalidFallbackFace => "font.output.block_invalid_fallback_face",
            Self::BlockFallbackNotEmbeddable => "font.output.block_fallback_not_embeddable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChosenOutputFont {
    pub identity: FontIdentity,
    pub materialization: OutputFontMaterialization,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedOutputFont {
    pub source: FontIdentity,
    pub disposition: OutputFontDisposition,
    pub reason: FontOutputReason,
    pub used_glyph_ids: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_embedding: Option<TechnicalEmbeddingFlags>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen_output: Option<ChosenOutputFont>,
    /// True when the planned output remains text rather than glyph outlines.
    pub text_searchability_preserved: bool,
    /// True only when the complete source font program is planned for output.
    pub source_font_program_recoverable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontOutputDiagnosticSeverity {
    Info,
    Loss,
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontOutputDiagnostic {
    pub source: FontIdentity,
    pub severity: FontOutputDiagnosticSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFontPlan {
    pub schema_version: String,
    pub target: FixedOutputFontProfile,
    pub fonts: Vec<PlannedOutputFont>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<FontOutputDiagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<FontOutputDiagnostic>,
}

impl OutputFontPlan {
    pub fn can_serialize(&self) -> bool {
        self.blockers.is_empty()
    }
}

/// Builds one font request from the current bounded shaping result.
///
/// LAYOUT-RESOLVE-01B1 currently fences one shaping run with the SHA-256 of
/// exactly the supplied font bytes. This helper preserves that explicit
/// identity and extracts a stable glyph set for output planning.
pub fn request_from_bounded_shaped_text<'a>(
    shaped: &BoundedShapedText,
    source_resource: Option<ExplicitFontResource<'a>>,
    fallback_resource: Option<ExplicitFontResource<'a>>,
) -> OutputFontRequest<'a> {
    OutputFontRequest {
        source: FontIdentity {
            fingerprint_sha256: shaped.environment.layout.font_set_fingerprint.clone(),
            face_index: shaped.environment.face_index,
        },
        source_resource,
        fallback_resource,
        used_glyph_ids: shaped.glyphs.iter().map(|glyph| glyph.glyph_id).collect(),
    }
}

/// Plans fixed-output font handling without serializing or discovering fonts.
///
/// Input order is non-semantic. Duplicate requests for the same source identity
/// fail closed instead of depending on which run happened to arrive first.
pub fn plan_output_fonts(
    profile: &FixedOutputFontProfile,
    mut requests: Vec<OutputFontRequest<'_>>,
) -> OutputFontPlan {
    requests.sort_by(|left, right| left.source.cmp(&right.source));

    let mut fonts = Vec::new();
    let mut diagnostics = Vec::new();
    let mut blockers = Vec::new();

    let mut start = 0usize;
    while start < requests.len() {
        let source = requests[start].source.clone();
        let mut end = start + 1;
        while end < requests.len() && requests[end].source == source {
            end += 1;
        }

        if end - start > 1 {
            let used_glyph_ids = requests[start..end]
                .iter()
                .flat_map(|request| request.used_glyph_ids.iter().copied())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let planned = blocked_font(
                source,
                used_glyph_ids,
                None,
                FontOutputReason::BlockDuplicateSourceRequest,
            );
            push_diagnostic(&planned, &mut diagnostics, &mut blockers);
            fonts.push(planned);
        } else {
            let planned = plan_one(profile, &requests[start]);
            push_diagnostic(&planned, &mut diagnostics, &mut blockers);
            fonts.push(planned);
        }

        start = end;
    }

    OutputFontPlan {
        schema_version: OUTPUT_FONT_PLAN_SCHEMA_V0_1.into(),
        target: profile.clone(),
        fonts,
        diagnostics,
        blockers,
    }
}

fn plan_one(
    profile: &FixedOutputFontProfile,
    request: &OutputFontRequest<'_>,
) -> PlannedOutputFont {
    let used_glyph_ids = request.used_glyph_ids.iter().copied().collect::<Vec<_>>();

    if used_glyph_ids.is_empty() {
        return blocked_font(
            request.source.clone(),
            used_glyph_ids,
            request
                .source_resource
                .as_ref()
                .map(|resource| resource.embedding),
            FontOutputReason::BlockNoGlyphUsage,
        );
    }

    if !is_sha256_hex(&request.source.fingerprint_sha256) {
        return blocked_font(
            request.source.clone(),
            used_glyph_ids,
            request
                .source_resource
                .as_ref()
                .map(|resource| resource.embedding),
            FontOutputReason::BlockInvalidSourceIdentity,
        );
    }

    let Some(source_resource) = request.source_resource.as_ref() else {
        return match profile.missing_font_action {
            MissingFontAction::Block => blocked_font(
                request.source.clone(),
                used_glyph_ids,
                None,
                FontOutputReason::BlockMissingSource,
            ),
            MissingFontAction::Substitute => substitute_font(
                profile,
                request,
                used_glyph_ids,
                FontOutputReason::SubstituteMissingSource,
            ),
        };
    };

    if source_resource.identity != request.source {
        return blocked_font(
            request.source.clone(),
            used_glyph_ids,
            Some(source_resource.embedding),
            FontOutputReason::BlockSourceIdentityMismatch,
        );
    }

    if let Some(reason) = validate_resource(source_resource, ResourceRole::Source) {
        return blocked_font(
            request.source.clone(),
            used_glyph_ids,
            Some(source_resource.embedding),
            reason,
        );
    }

    if source_resource.embedding.bitmap_only {
        return handle_non_embeddable(
            profile,
            request,
            used_glyph_ids,
            NonEmbeddableKind::BitmapOnly,
        );
    }

    match source_resource.embedding.level {
        EmbeddingLevel::Restricted => handle_non_embeddable(
            profile,
            request,
            used_glyph_ids,
            NonEmbeddableKind::Restricted,
        ),
        EmbeddingLevel::Unknown => {
            handle_non_embeddable(profile, request, used_glyph_ids, NonEmbeddableKind::Unknown)
        }
        EmbeddingLevel::Installable | EmbeddingLevel::PreviewPrint | EmbeddingLevel::Editable => {
            let (disposition, materialization, reason) = if profile.force_outline {
                (
                    OutputFontDisposition::Outline,
                    OutputFontMaterialization::Outline,
                    FontOutputReason::OutlinePolicy,
                )
            } else {
                match (
                    profile.preferred_embedding,
                    source_resource.embedding.no_subsetting,
                ) {
                    (PreferredEmbedding::Subset, false) => (
                        OutputFontDisposition::EmbedSubset,
                        OutputFontMaterialization::EmbedSubset,
                        FontOutputReason::EmbedSubset,
                    ),
                    (PreferredEmbedding::Subset, true) => (
                        OutputFontDisposition::EmbedFull,
                        OutputFontMaterialization::EmbedFull,
                        FontOutputReason::EmbedFullNoSubsetting,
                    ),
                    (PreferredEmbedding::Full, _) => (
                        OutputFontDisposition::EmbedFull,
                        OutputFontMaterialization::EmbedFull,
                        FontOutputReason::EmbedFull,
                    ),
                }
            };

            planned_font(
                request.source.clone(),
                used_glyph_ids,
                Some(source_resource.embedding),
                disposition,
                reason,
                Some(ChosenOutputFont {
                    identity: request.source.clone(),
                    materialization,
                }),
            )
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NonEmbeddableKind {
    Restricted,
    Unknown,
    BitmapOnly,
}

fn handle_non_embeddable(
    profile: &FixedOutputFontProfile,
    request: &OutputFontRequest<'_>,
    used_glyph_ids: Vec<u32>,
    kind: NonEmbeddableKind,
) -> PlannedOutputFont {
    let action = match kind {
        NonEmbeddableKind::Restricted => profile.restricted_font_action,
        NonEmbeddableKind::Unknown => profile.unknown_embedding_action,
        NonEmbeddableKind::BitmapOnly => profile.bitmap_only_action,
    };
    let embedding = request
        .source_resource
        .as_ref()
        .map(|resource| resource.embedding);

    match action {
        NonEmbeddableAction::Outline => {
            let reason = match kind {
                NonEmbeddableKind::Restricted => FontOutputReason::OutlineRestricted,
                NonEmbeddableKind::Unknown => FontOutputReason::OutlineUnknownEmbedding,
                NonEmbeddableKind::BitmapOnly => FontOutputReason::OutlineBitmapOnly,
            };
            planned_font(
                request.source.clone(),
                used_glyph_ids,
                embedding,
                OutputFontDisposition::Outline,
                reason,
                Some(ChosenOutputFont {
                    identity: request.source.clone(),
                    materialization: OutputFontMaterialization::Outline,
                }),
            )
        }
        NonEmbeddableAction::Substitute => {
            let reason = match kind {
                NonEmbeddableKind::Restricted => FontOutputReason::SubstituteRestricted,
                NonEmbeddableKind::Unknown => FontOutputReason::SubstituteUnknownEmbedding,
                NonEmbeddableKind::BitmapOnly => FontOutputReason::SubstituteBitmapOnly,
            };
            substitute_font(profile, request, used_glyph_ids, reason)
        }
        NonEmbeddableAction::Block => {
            let reason = match kind {
                NonEmbeddableKind::Restricted => FontOutputReason::BlockRestricted,
                NonEmbeddableKind::Unknown => FontOutputReason::BlockUnknownEmbedding,
                NonEmbeddableKind::BitmapOnly => FontOutputReason::BlockBitmapOnly,
            };
            blocked_font(request.source.clone(), used_glyph_ids, embedding, reason)
        }
    }
}

fn substitute_font(
    profile: &FixedOutputFontProfile,
    request: &OutputFontRequest<'_>,
    used_glyph_ids: Vec<u32>,
    reason: FontOutputReason,
) -> PlannedOutputFont {
    let source_embedding = request
        .source_resource
        .as_ref()
        .map(|resource| resource.embedding);
    let Some(fallback) = request.fallback_resource.as_ref() else {
        return blocked_font(
            request.source.clone(),
            used_glyph_ids,
            source_embedding,
            FontOutputReason::BlockMissingFallback,
        );
    };

    if !is_sha256_hex(&fallback.identity.fingerprint_sha256) {
        return blocked_font(
            request.source.clone(),
            used_glyph_ids,
            source_embedding,
            FontOutputReason::BlockInvalidFallbackIdentity,
        );
    }

    if let Some(validation_reason) = validate_resource(fallback, ResourceRole::Fallback) {
        return blocked_font(
            request.source.clone(),
            used_glyph_ids,
            source_embedding,
            validation_reason,
        );
    }

    if fallback.embedding.bitmap_only
        || matches!(
            fallback.embedding.level,
            EmbeddingLevel::Restricted | EmbeddingLevel::Unknown
        )
    {
        return blocked_font(
            request.source.clone(),
            used_glyph_ids,
            source_embedding,
            FontOutputReason::BlockFallbackNotEmbeddable,
        );
    }

    let materialization = if profile.force_outline {
        OutputFontMaterialization::Outline
    } else {
        match (
            profile.preferred_embedding,
            fallback.embedding.no_subsetting,
        ) {
            (PreferredEmbedding::Subset, false) => OutputFontMaterialization::EmbedSubset,
            (PreferredEmbedding::Subset, true) | (PreferredEmbedding::Full, _) => {
                OutputFontMaterialization::EmbedFull
            }
        }
    };

    planned_font(
        request.source.clone(),
        used_glyph_ids,
        source_embedding,
        OutputFontDisposition::Substitute,
        reason,
        Some(ChosenOutputFont {
            identity: fallback.identity.clone(),
            materialization,
        }),
    )
}

#[derive(Debug, Clone, Copy)]
enum ResourceRole {
    Source,
    Fallback,
}

fn validate_resource(
    resource: &ExplicitFontResource<'_>,
    role: ResourceRole,
) -> Option<FontOutputReason> {
    if !is_sha256_hex(&resource.identity.fingerprint_sha256) {
        return Some(match role {
            ResourceRole::Source => FontOutputReason::BlockInvalidSourceIdentity,
            ResourceRole::Fallback => FontOutputReason::BlockInvalidFallbackIdentity,
        });
    }

    let actual_fingerprint = font_fingerprint_sha256(resource.bytes);
    if actual_fingerprint != resource.identity.fingerprint_sha256 {
        return Some(match role {
            ResourceRole::Source => FontOutputReason::BlockSourceFingerprintMismatch,
            ResourceRole::Fallback => FontOutputReason::BlockFallbackFingerprintMismatch,
        });
    }

    if FontRef::from_index(resource.bytes, resource.identity.face_index).is_err() {
        return Some(match role {
            ResourceRole::Source => FontOutputReason::BlockInvalidSourceFace,
            ResourceRole::Fallback => FontOutputReason::BlockInvalidFallbackFace,
        });
    }

    None
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn planned_font(
    source: FontIdentity,
    used_glyph_ids: Vec<u32>,
    source_embedding: Option<TechnicalEmbeddingFlags>,
    disposition: OutputFontDisposition,
    reason: FontOutputReason,
    chosen_output: Option<ChosenOutputFont>,
) -> PlannedOutputFont {
    let text_searchability_preserved = chosen_output
        .as_ref()
        .is_some_and(|chosen| chosen.materialization != OutputFontMaterialization::Outline)
        && disposition != OutputFontDisposition::Block;
    let source_font_program_recoverable = chosen_output.as_ref().is_some_and(|chosen| {
        chosen.identity == source
            && chosen.materialization == OutputFontMaterialization::EmbedFull
            && disposition == OutputFontDisposition::EmbedFull
    });

    PlannedOutputFont {
        source,
        disposition,
        reason,
        used_glyph_ids,
        source_embedding,
        chosen_output,
        text_searchability_preserved,
        source_font_program_recoverable,
    }
}

fn blocked_font(
    source: FontIdentity,
    used_glyph_ids: Vec<u32>,
    source_embedding: Option<TechnicalEmbeddingFlags>,
    reason: FontOutputReason,
) -> PlannedOutputFont {
    planned_font(
        source,
        used_glyph_ids,
        source_embedding,
        OutputFontDisposition::Block,
        reason,
        None,
    )
}

fn push_diagnostic(
    planned: &PlannedOutputFont,
    diagnostics: &mut Vec<FontOutputDiagnostic>,
    blockers: &mut Vec<FontOutputDiagnostic>,
) {
    let severity = match planned.disposition {
        OutputFontDisposition::EmbedFull
            if planned.reason == FontOutputReason::EmbedFullNoSubsetting =>
        {
            Some(FontOutputDiagnosticSeverity::Info)
        }
        OutputFontDisposition::EmbedFull | OutputFontDisposition::EmbedSubset => None,
        OutputFontDisposition::Outline | OutputFontDisposition::Substitute => {
            Some(FontOutputDiagnosticSeverity::Loss)
        }
        OutputFontDisposition::Block => Some(FontOutputDiagnosticSeverity::Blocking),
    };

    let Some(severity) = severity else {
        return;
    };

    let diagnostic = FontOutputDiagnostic {
        source: planned.source.clone(),
        severity,
        code: planned.reason.code().into(),
        message: reason_message(planned.reason).into(),
    };
    diagnostics.push(diagnostic.clone());
    if severity == FontOutputDiagnosticSeverity::Blocking {
        blockers.push(diagnostic);
    }
}

fn reason_message(reason: FontOutputReason) -> &'static str {
    match reason {
        FontOutputReason::EmbedFull => "font is planned for full embedding",
        FontOutputReason::EmbedSubset => "font is planned for deterministic glyph subsetting",
        FontOutputReason::EmbedFullNoSubsetting => {
            "font forbids subsetting, so the complete font program is planned"
        }
        FontOutputReason::OutlinePolicy => "target profile explicitly requires glyph outlines",
        FontOutputReason::OutlineRestricted => {
            "font embedding is technically restricted; configured policy selects outlines"
        }
        FontOutputReason::OutlineUnknownEmbedding => {
            "font embedding permission is unknown; configured policy selects outlines"
        }
        FontOutputReason::OutlineBitmapOnly => {
            "font permits bitmap-only embedding; configured policy selects outlines"
        }
        FontOutputReason::SubstituteMissingSource => {
            "source font bytes are missing; configured explicit fallback is selected"
        }
        FontOutputReason::SubstituteRestricted => {
            "source font embedding is restricted; configured explicit fallback is selected"
        }
        FontOutputReason::SubstituteUnknownEmbedding => {
            "source font embedding permission is unknown; configured explicit fallback is selected"
        }
        FontOutputReason::SubstituteBitmapOnly => {
            "source font permits bitmap-only embedding; configured explicit fallback is selected"
        }
        FontOutputReason::BlockNoGlyphUsage => "font request contains no resolved glyph usage",
        FontOutputReason::BlockDuplicateSourceRequest => {
            "more than one request was supplied for the same source font identity"
        }
        FontOutputReason::BlockInvalidSourceIdentity => {
            "source font identity is not a lowercase SHA-256 fingerprint"
        }
        FontOutputReason::BlockSourceIdentityMismatch => {
            "supplied source font resource does not match the resolved source identity"
        }
        FontOutputReason::BlockSourceFingerprintMismatch => {
            "supplied source font bytes do not match the resolved SHA-256 fingerprint"
        }
        FontOutputReason::BlockInvalidSourceFace => {
            "supplied source font bytes do not contain the requested face index"
        }
        FontOutputReason::BlockMissingSource => {
            "source font bytes are unavailable and policy does not allow substitution"
        }
        FontOutputReason::BlockRestricted => {
            "font embedding is technically restricted and target policy blocks output"
        }
        FontOutputReason::BlockUnknownEmbedding => {
            "font embedding permission is unknown and target policy blocks output"
        }
        FontOutputReason::BlockBitmapOnly => {
            "font permits bitmap-only embedding and target policy blocks output"
        }
        FontOutputReason::BlockMissingFallback => {
            "font policy requires substitution but no explicit fallback bytes were supplied"
        }
        FontOutputReason::BlockInvalidFallbackIdentity => {
            "fallback font identity is not a lowercase SHA-256 fingerprint"
        }
        FontOutputReason::BlockFallbackFingerprintMismatch => {
            "fallback font bytes do not match their declared SHA-256 fingerprint"
        }
        FontOutputReason::BlockInvalidFallbackFace => {
            "fallback font bytes do not contain the requested face index"
        }
        FontOutputReason::BlockFallbackNotEmbeddable => {
            "explicit fallback is not directly embeddable under the current bounded policy"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_layout::{
        BoundedLayoutEnvironment, BoundedShapingRuntime, font_fingerprint_sha256, shape_bounded_ltr,
    };
    use pub_model::{EMU_PER_POINT, LengthEmu};

    fn font_identity(bytes: &[u8]) -> FontIdentity {
        FontIdentity {
            fingerprint_sha256: font_fingerprint_sha256(bytes),
            face_index: 0,
        }
    }

    fn resource(bytes: &[u8], embedding: TechnicalEmbeddingFlags) -> ExplicitFontResource<'_> {
        ExplicitFontResource {
            identity: font_identity(bytes),
            bytes,
            embedding,
        }
    }

    fn used(ids: &[u32]) -> BTreeSet<u32> {
        ids.iter().copied().collect()
    }

    fn request<'a>(
        bytes: &'a [u8],
        embedding: TechnicalEmbeddingFlags,
        glyphs: &[u32],
    ) -> OutputFontRequest<'a> {
        let font = resource(bytes, embedding);
        OutputFontRequest {
            source: font.identity.clone(),
            source_resource: Some(font),
            fallback_resource: None,
            used_glyph_ids: used(glyphs),
        }
    }

    #[test]
    fn bounded_shaping_handoff_uses_exact_identity_and_glyphs() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let fingerprint = font_fingerprint_sha256(bytes);
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: "layout-resolve-01b1".into(),
                font_set_fingerprint: fingerprint.clone(),
                resource_fingerprint: "resources:none".into(),
            },
            face_index: 0,
            font_size_emu: LengthEmu::new(12 * EMU_PER_POINT),
            font_bytes: bytes,
        };
        let shaped = shape_bounded_ltr("Hfix", &runtime).expect("pinned font shapes");
        let request = request_from_bounded_shaped_text(
            &shaped,
            Some(resource(bytes, TechnicalEmbeddingFlags::installable())),
            None,
        );

        assert_eq!(request.source.fingerprint_sha256, fingerprint);
        assert_eq!(request.source.face_index, 0);
        assert_eq!(
            request.used_glyph_ids,
            shaped.glyphs.iter().map(|glyph| glyph.glyph_id).collect()
        );
    }

    #[test]
    fn basic_pdf_subsets_installable_font_deterministically() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let left = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![request(
                bytes,
                TechnicalEmbeddingFlags::installable(),
                &[9, 2, 9, 4],
            )],
        );
        let right = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![request(
                bytes,
                TechnicalEmbeddingFlags::installable(),
                &[4, 9, 2],
            )],
        );

        assert_eq!(left, right);
        assert!(left.can_serialize());
        assert_eq!(
            left.fonts[0].disposition,
            OutputFontDisposition::EmbedSubset
        );
        assert_eq!(left.fonts[0].reason, FontOutputReason::EmbedSubset);
        assert_eq!(left.fonts[0].used_glyph_ids, vec![2, 4, 9]);
        assert!(left.fonts[0].text_searchability_preserved);
        assert!(!left.fonts[0].source_font_program_recoverable);
        assert!(left.diagnostics.is_empty());
        assert_eq!(
            serde_json::to_vec(&left).unwrap(),
            serde_json::to_vec(&right).unwrap()
        );
    }

    #[test]
    fn no_subsetting_flag_promotes_subset_policy_to_full_embed() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let plan = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![request(
                bytes,
                TechnicalEmbeddingFlags {
                    no_subsetting: true,
                    ..TechnicalEmbeddingFlags::installable()
                },
                &[2, 4],
            )],
        );

        assert!(plan.can_serialize());
        assert_eq!(plan.fonts[0].disposition, OutputFontDisposition::EmbedFull);
        assert_eq!(
            plan.fonts[0].reason,
            FontOutputReason::EmbedFullNoSubsetting
        );
        assert!(plan.fonts[0].source_font_program_recoverable);
        assert_eq!(
            plan.diagnostics[0].severity,
            FontOutputDiagnosticSeverity::Info
        );
    }

    #[test]
    fn missing_source_blocks_without_host_lookup() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let source = font_identity(bytes);
        let plan = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![OutputFontRequest {
                source,
                source_resource: None,
                fallback_resource: None,
                used_glyph_ids: used(&[1]),
            }],
        );

        assert!(!plan.can_serialize());
        assert_eq!(plan.fonts[0].disposition, OutputFontDisposition::Block);
        assert_eq!(plan.fonts[0].reason, FontOutputReason::BlockMissingSource);
        assert_eq!(plan.blockers.len(), 1);
    }

    #[test]
    fn missing_source_can_use_only_explicit_fallback() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let fallback = resource(bytes, TechnicalEmbeddingFlags::installable());
        let mut source = fallback.identity.clone();
        source.fingerprint_sha256 = "11".repeat(32);
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.missing_font_action = MissingFontAction::Substitute;

        let plan = plan_output_fonts(
            &profile,
            vec![OutputFontRequest {
                source,
                source_resource: None,
                fallback_resource: Some(fallback.clone()),
                used_glyph_ids: used(&[1, 2]),
            }],
        );

        assert!(plan.can_serialize());
        assert_eq!(plan.fonts[0].disposition, OutputFontDisposition::Substitute);
        assert_eq!(
            plan.fonts[0].reason,
            FontOutputReason::SubstituteMissingSource
        );
        assert_eq!(
            plan.fonts[0].chosen_output.as_ref().unwrap().identity,
            fallback.identity
        );
        assert!(plan.fonts[0].text_searchability_preserved);
        assert!(!plan.fonts[0].source_font_program_recoverable);
        assert_eq!(
            plan.diagnostics[0].severity,
            FontOutputDiagnosticSeverity::Loss
        );
    }

    #[test]
    fn restricted_font_is_explicit_block_by_default() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let plan = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![request(
                bytes,
                TechnicalEmbeddingFlags {
                    level: EmbeddingLevel::Restricted,
                    no_subsetting: false,
                    bitmap_only: false,
                },
                &[1],
            )],
        );

        assert!(!plan.can_serialize());
        assert_eq!(plan.fonts[0].reason, FontOutputReason::BlockRestricted);
        assert_eq!(plan.blockers[0].code, "font.output.block_restricted");
    }

    #[test]
    fn restricted_font_can_be_explicitly_outlined_by_profile() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.restricted_font_action = NonEmbeddableAction::Outline;

        let plan = plan_output_fonts(
            &profile,
            vec![request(
                bytes,
                TechnicalEmbeddingFlags {
                    level: EmbeddingLevel::Restricted,
                    no_subsetting: false,
                    bitmap_only: false,
                },
                &[1],
            )],
        );

        assert!(plan.can_serialize());
        assert_eq!(plan.fonts[0].disposition, OutputFontDisposition::Outline);
        assert_eq!(plan.fonts[0].reason, FontOutputReason::OutlineRestricted);
        assert!(!plan.fonts[0].text_searchability_preserved);
        assert_eq!(
            plan.diagnostics[0].severity,
            FontOutputDiagnosticSeverity::Loss
        );
    }

    #[test]
    fn force_outline_is_target_policy_not_renderer_guess() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.force_outline = true;

        let plan = plan_output_fonts(
            &profile,
            vec![request(
                bytes,
                TechnicalEmbeddingFlags::installable(),
                &[1, 2],
            )],
        );

        assert!(plan.can_serialize());
        assert_eq!(plan.fonts[0].disposition, OutputFontDisposition::Outline);
        assert_eq!(plan.fonts[0].reason, FontOutputReason::OutlinePolicy);
    }

    #[test]
    fn fingerprint_mismatch_blocks_instead_of_substituting() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let mut source_resource = resource(bytes, TechnicalEmbeddingFlags::installable());
        source_resource.identity.fingerprint_sha256 = "22".repeat(32);
        let source = source_resource.identity.clone();

        let plan = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![OutputFontRequest {
                source,
                source_resource: Some(source_resource),
                fallback_resource: None,
                used_glyph_ids: used(&[1]),
            }],
        );

        assert!(!plan.can_serialize());
        assert_eq!(
            plan.fonts[0].reason,
            FontOutputReason::BlockSourceFingerprintMismatch
        );
    }

    #[test]
    fn duplicate_source_requests_fail_closed_independent_of_order() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let a = request(bytes, TechnicalEmbeddingFlags::installable(), &[1]);
        let b = request(bytes, TechnicalEmbeddingFlags::installable(), &[2]);

        let left = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![a.clone(), b.clone()],
        );
        let right = plan_output_fonts(&FixedOutputFontProfile::basic_pdf_v0_1(), vec![b, a]);

        assert_eq!(left, right);
        assert!(!left.can_serialize());
        assert_eq!(
            left.fonts[0].reason,
            FontOutputReason::BlockDuplicateSourceRequest
        );
        assert_eq!(left.fonts[0].used_glyph_ids, vec![1, 2]);
    }

    #[test]
    fn request_order_is_non_semantic() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let source_a = request(bytes, TechnicalEmbeddingFlags::installable(), &[1]);

        let mut source_b_identity = font_identity(bytes);
        source_b_identity.fingerprint_sha256 = "33".repeat(32);
        let source_b = OutputFontRequest {
            source: source_b_identity,
            source_resource: None,
            fallback_resource: None,
            used_glyph_ids: used(&[2]),
        };

        let left = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![source_b.clone(), source_a.clone()],
        );
        let right = plan_output_fonts(
            &FixedOutputFontProfile::basic_pdf_v0_1(),
            vec![source_a, source_b],
        );

        assert_eq!(left, right);
        assert_eq!(left.fonts.len(), 2);
    }
}
