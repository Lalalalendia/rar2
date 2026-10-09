//! Exact physical-font resource authoring gate within the canonical overlay.
//!
//! A browser selector is only a candidate. The server independently obtains
//! the original full font bytes, face count and authoring permission from its
//! trusted registry. This module does not perform host lookup, font installation,
//! licensing inference, layout, PDF rendering or native PUB mutation.

use super::{
    FormatPropertyV1, FormatValueV1, Result, TextFormatOperationKindV1,
    TextFormatOperationReceiptV1, TextFormatOverlayError, TextFormatOverlayStateV1,
    apply_format_operation_v1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const FONT_CANDIDATE_PROTOCOL_V1: &str = "chaptera.font-replacement-candidate.v1";
pub const FONT_CANDIDATE_AUTHORITY_V1: &str = "candidate_only_server_validation_required";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontResourceIdentityV1 {
    pub resource_id: String,
    pub font_fingerprint: String,
    pub content_hash: String,
    pub face_index: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontAuthoringScopeV1 {
    pub document_id: String,
    pub revision_id: String,
    pub scene_snapshot_id: String,
    pub layout_environment_id: String,
    pub font_set_fingerprint: String,
}

// Exact wire shape of the client-only candidate from #2494. The client never
// controls the independent trusted resource record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontReplacementCandidateV1 {
    pub protocol_version: String,
    pub document_id: String,
    pub expected_revision_id: String,
    pub scene_snapshot_id: String,
    pub layout_environment_id: String,
    pub font_set_fingerprint: String,
    pub resource_id: String,
    pub font_fingerprint: String,
    pub content_hash: String,
    pub face_index: u16,
    pub authority: String,
}

// A server-owned entry, deliberately NOT Deserialize: no HTTP/EditorProject
// payload may impersonate the trusted registry. The caller must only construct
// this from independently sourced, policy-admitted original bytes.
#[derive(Debug)]
pub struct ServerFontResourceV1<'a> {
    pub identity: &'a FontResourceIdentityV1,
    pub full_font_bytes: &'a [u8],
    pub face_count: u32,
    pub is_full_resource: bool,
    pub authoring_admitted: bool,
}

fn is_lower_hex(value: &str, digits: usize) -> bool {
    value.len() == digits
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_lower_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

pub(super) fn validate_font_resource_identity_v1(identity: &FontResourceIdentityV1) -> Result<()> {
    if !is_lower_uuid(&identity.resource_id) {
        return Err(TextFormatOverlayError::new(
            "font resource ID must be a canonical lowercase UUID",
        ));
    }
    if !identity
        .font_fingerprint
        .strip_prefix("sha256:")
        .is_some_and(|hash| is_lower_hex(hash, 64))
        || !is_lower_hex(&identity.content_hash, 64)
    {
        return Err(TextFormatOverlayError::new(
            "font resource requires exact lowercase SHA-256 identity",
        ));
    }
    Ok(())
}

fn validate_scope(
    scope: &FontAuthoringScopeV1,
    candidate: &FontReplacementCandidateV1,
) -> Result<()> {
    if candidate.protocol_version != FONT_CANDIDATE_PROTOCOL_V1
        || candidate.authority != FONT_CANDIDATE_AUTHORITY_V1
    {
        return Err(TextFormatOverlayError::new(
            "not a canonical untrusted font replacement candidate",
        ));
    }
    if scope.document_id != candidate.document_id
        || scope.revision_id != candidate.expected_revision_id
        || scope.scene_snapshot_id != candidate.scene_snapshot_id
        || scope.layout_environment_id != candidate.layout_environment_id
        || scope.font_set_fingerprint != candidate.font_set_fingerprint
    {
        return Err(TextFormatOverlayError::new(
            "font authoring candidate is stale or belongs to another layout environment",
        ));
    }
    Ok(())
}

pub fn set_admitted_font_resource_v1(
    state: &TextFormatOverlayStateV1,
    start_scalar: u32,
    end_scalar: u32,
    candidate: &FontReplacementCandidateV1,
    current_scope: &FontAuthoringScopeV1,
    server_resource: &ServerFontResourceV1<'_>,
    expected_state_hash: &str,
) -> Result<TextFormatOperationReceiptV1> {
    validate_scope(current_scope, candidate)?;
    let identity = FontResourceIdentityV1 {
        resource_id: candidate.resource_id.clone(),
        font_fingerprint: candidate.font_fingerprint.clone(),
        content_hash: candidate.content_hash.clone(),
        face_index: candidate.face_index,
    };
    validate_font_resource_identity_v1(&identity)?;
    validate_font_resource_identity_v1(server_resource.identity)?;
    if !server_resource.authoring_admitted
        || !server_resource.is_full_resource
        || server_resource.full_font_bytes.is_empty()
    {
        return Err(TextFormatOverlayError::new(
            "font resource has no independent full-file authoring admission",
        ));
    }
    if server_resource.identity != &identity {
        return Err(TextFormatOverlayError::new(
            "requested font identity differs from server-owned resource",
        ));
    }
    if server_resource.face_count == 0
        || u32::from(identity.face_index) >= server_resource.face_count
    {
        return Err(TextFormatOverlayError::new(
            "font face index not admitted by server resource",
        ));
    }
    let actual_content_hash = format!("{:x}", Sha256::digest(server_resource.full_font_bytes));
    if actual_content_hash != identity.content_hash {
        return Err(TextFormatOverlayError::new(
            "server font bytes disagree with admitted content SHA-256",
        ));
    }
    // Reuse the canonical overlay operation/state/hash normalization. No
    // parallel format state, unvalidated property setter or CLI file lookup.
    apply_format_operation_v1(
        state,
        TextFormatOperationKindV1::SetTextFormatProperty,
        start_scalar,
        end_scalar,
        FormatPropertyV1::FontResource,
        Some(FormatValueV1::FontResource(identity)),
        expected_state_hash,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BaseCharacterFormatV1, BaseFormatRunV1, EffectivePropertySourceV1,
        build_text_format_overlay_state_v1, clear_text_format_property_override_v1,
        effective_property_segments_v1, replay_text_format_operation_v1,
        set_text_format_property_v1, state_hash_v1, undo_text_format_operation_v1,
    };

    const DOC: &str = "11111111-1111-4111-8111-111111111111";
    const RESOURCE: &str = "82222222-2222-4222-8222-222222222222";

    fn fixture_state() -> TextFormatOverlayStateV1 {
        build_text_format_overlay_state_v1(
            "story:1",
            "source-revision",
            6,
            vec![BaseFormatRunV1 {
                start_scalar: 0,
                end_scalar: 6,
                format: BaseCharacterFormatV1 {
                    font_resource_id: "font:source-unavailable".to_owned(),
                    font_size_emu: 12000,
                    bold: false,
                    italic: false,
                    text_color_rgb: "#000000".to_owned(),
                },
            }],
            vec![],
        )
        .unwrap()
    }

    fn identity(bytes: &[u8]) -> FontResourceIdentityV1 {
        FontResourceIdentityV1 {
            resource_id: RESOURCE.to_owned(),
            font_fingerprint: "sha256:".to_owned() + &"b".repeat(64),
            content_hash: format!("{:x}", Sha256::digest(bytes)),
            face_index: 0,
        }
    }

    fn scope() -> FontAuthoringScopeV1 {
        FontAuthoringScopeV1 {
            document_id: DOC.to_owned(),
            revision_id: "sha256:".to_owned() + &"1".repeat(64),
            scene_snapshot_id: "sha256:".to_owned() + &"2".repeat(64),
            layout_environment_id: "sha256:".to_owned() + &"3".repeat(64),
            font_set_fingerprint: "sha256:".to_owned() + &"4".repeat(64),
        }
    }

    fn candidate(
        scope: &FontAuthoringScopeV1,
        id: &FontResourceIdentityV1,
    ) -> FontReplacementCandidateV1 {
        FontReplacementCandidateV1 {
            protocol_version: FONT_CANDIDATE_PROTOCOL_V1.to_owned(),
            document_id: scope.document_id.clone(),
            expected_revision_id: scope.revision_id.clone(),
            scene_snapshot_id: scope.scene_snapshot_id.clone(),
            layout_environment_id: scope.layout_environment_id.clone(),
            font_set_fingerprint: scope.font_set_fingerprint.clone(),
            resource_id: id.resource_id.clone(),
            font_fingerprint: id.font_fingerprint.clone(),
            content_hash: id.content_hash.clone(),
            face_index: id.face_index,
            authority: FONT_CANDIDATE_AUTHORITY_V1.to_owned(),
        }
    }

    #[test]
    fn admitted_exact_bytes_create_one_canonical_overlay_operation_and_clear_restores_base() {
        let bytes = b"test font file bytes from the private server registry";
        let id = identity(bytes);
        let context = scope();
        let request = candidate(&context, &id);
        let trusted = ServerFontResourceV1 {
            identity: &id,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let source = fixture_state();
        let original_hash = state_hash_v1(&source).unwrap();
        let receipt = set_admitted_font_resource_v1(
            &source,
            1,
            5,
            &request,
            &context,
            &trusted,
            &original_hash,
        )
        .unwrap();
        assert_eq!(receipt.after_state.overrides.len(), 1);
        assert_eq!(
            receipt.after_state.overrides[0].property,
            FormatPropertyV1::FontResource
        );
        assert_eq!(
            receipt.after_state.overrides[0].value,
            FormatValueV1::FontResource(id),
        );
        assert!(receipt.requires_authoritative_relayout);
        assert_eq!(undo_text_format_operation_v1(&receipt), source);
        assert_eq!(
            replay_text_format_operation_v1(&receipt).unwrap(),
            receipt.after_state
        );
        let after = receipt.after_state;
        let effective =
            effective_property_segments_v1(&after, FormatPropertyV1::FontResource, 1, 5).unwrap();
        assert_eq!(
            effective[0].source,
            EffectivePropertySourceV1::ChapteraOverride
        );
        let cleared = clear_text_format_property_override_v1(
            &after,
            1,
            5,
            FormatPropertyV1::FontResource,
            &state_hash_v1(&after).unwrap(),
        )
        .unwrap();
        assert_eq!(cleared.after_state, source);
        let revealed = effective_property_segments_v1(
            &cleared.after_state,
            FormatPropertyV1::FontResource,
            1,
            5,
        )
        .unwrap();
        assert_eq!(
            revealed[0].value,
            FormatValueV1::String("font:source-unavailable".to_owned())
        );
        assert_eq!(revealed[0].source, EffectivePropertySourceV1::Base);
    }

    #[test]
    fn untrusted_generic_font_mutation_cannot_skip_server_admission() {
        let bytes = b"only registered original bytes can be used";
        let id = identity(bytes);
        let source = fixture_state();
        let result = set_text_format_property_v1(
            &source,
            0,
            6,
            FormatPropertyV1::FontResource,
            FormatValueV1::FontResource(id),
            &state_hash_v1(&source).unwrap(),
        )
        .unwrap_err();
        assert!(
            result
                .to_string()
                .contains("independent server resource admission")
        );
    }

    #[test]
    fn stale_scope_and_same_name_or_different_identity_fail_closed() {
        let bytes = b"registered font bytes";
        let id = identity(bytes);
        let context = scope();
        let trusted = ServerFontResourceV1 {
            identity: &id,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let source = fixture_state();
        let mut request = candidate(&context, &id);
        request.expected_revision_id = "sha256:".to_owned() + &"9".repeat(64);
        let e = set_admitted_font_resource_v1(
            &source,
            0,
            6,
            &request,
            &context,
            &trusted,
            &state_hash_v1(&source).unwrap(),
        )
        .unwrap_err();
        assert!(e.to_string().contains("stale"));

        let mut request = candidate(&context, &id);
        request.font_fingerprint = "sha256:".to_owned() + &"a".repeat(64);
        let e = set_admitted_font_resource_v1(
            &source,
            0,
            6,
            &request,
            &context,
            &trusted,
            &state_hash_v1(&source).unwrap(),
        )
        .unwrap_err();
        assert!(e.to_string().contains("differs"));
    }

    #[test]
    fn missing_authoring_permission_subset_and_byte_drift_fail_closed() {
        let bytes = b"registered font bytes";
        let id = identity(bytes);
        let context = scope();
        let request = candidate(&context, &id);
        let source = fixture_state();
        let original_hash = state_hash_v1(&source).unwrap();
        for (permitted, full_file) in [(false, true), (true, false)] {
            let trusted = ServerFontResourceV1 {
                identity: &id,
                full_font_bytes: bytes,
                face_count: 1,
                is_full_resource: full_file,
                authoring_admitted: permitted,
            };
            assert!(
                set_admitted_font_resource_v1(
                    &source,
                    0,
                    6,
                    &request,
                    &context,
                    &trusted,
                    &original_hash,
                )
                .is_err()
            );
        }
        let corrupted = ServerFontResourceV1 {
            identity: &id,
            full_font_bytes: b"other font",
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        assert!(
            set_admitted_font_resource_v1(
                &source,
                0,
                6,
                &request,
                &context,
                &corrupted,
                &original_hash,
            )
            .unwrap_err()
            .to_string()
            .contains("content SHA-256")
        );
    }

    #[test]
    fn wrong_face_noncanonical_id_stale_state_and_wrong_type_fail_closed() {
        let bytes = b"registered font bytes";
        let id = identity(bytes);
        let context = scope();
        let mut request = candidate(&context, &id);
        let trusted = ServerFontResourceV1 {
            identity: &id,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let source = fixture_state();
        let hash = state_hash_v1(&source).unwrap();
        request.face_index = 1;
        assert!(
            set_admitted_font_resource_v1(&source, 0, 6, &request, &context, &trusted, &hash,)
                .is_err()
        );

        let mut request = candidate(&context, &id);
        request.resource_id = "same font name".to_owned();
        assert!(
            set_admitted_font_resource_v1(&source, 0, 6, &request, &context, &trusted, &hash,)
                .is_err()
        );
        assert!(
            set_admitted_font_resource_v1(
                &source,
                0,
                6,
                &candidate(&context, &id),
                &context,
                &trusted,
                "stale",
            )
            .unwrap_err()
            .to_string()
            .contains("stale")
        );
        assert!(
            crate::set_text_format_property_v1(
                &source,
                1,
                5,
                FormatPropertyV1::Bold,
                FormatValueV1::FontResource(id),
                &hash,
            )
            .is_err()
        );
    }

    #[test]
    fn client_candidate_wire_carries_no_resource_bytes_or_host_path() {
        let bytes = b"private resource bytes";
        let id = identity(bytes);
        let encoded = serde_json::to_value(candidate(&scope(), &id)).unwrap();
        assert_eq!(encoded["resource_id"], RESOURCE);
        assert!(encoded.get("full_font_bytes").is_none());
        assert!(encoded.get("source_path").is_none());
        assert!(encoded.get("font_family").is_none());
        assert_eq!(encoded["authority"], FONT_CANDIDATE_AUTHORITY_V1);
        // A real raw byte SHA is not presumed equal to the provider's
        // face-specific font fingerprint.
        assert_ne!(encoded["content_hash"], id.font_fingerprint);
    }
}
