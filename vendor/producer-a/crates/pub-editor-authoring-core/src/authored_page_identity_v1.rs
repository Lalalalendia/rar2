//! Source-neutral identity/provenance law for Chaptera-created publication pages.
//!
//! This primitive is intentionally session-neutral and does not add page
//! lifecycle semantics. It proves only that an editor-created PageId is an
//! explicit UUIDv7 identity with author-created provenance, without inventing
//! Publisher-native allocation state or changing pub-model::Page wire shape.

use super::create_shape_runtime_v1::AuthoredEntityProvenanceV1;
use pub_model::PageId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredPageIdentityV1 {
    pub page_id: PageId,
    pub provenance: AuthoredEntityProvenanceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoredPageIdentityValidationErrorV1 {
    PageIdNotUuidV7,
    NonAuthorCreatedProvenance,
}

pub fn is_editor_created_uuid_v7_page_id(page_id: PageId) -> bool {
    let bytes = page_id.as_canonical().as_bytes();
    (bytes[6] & 0xf0) == 0x70 && (bytes[8] & 0xc0) == 0x80
}

pub fn validate_authored_page_identity_v1(
    identity: &AuthoredPageIdentityV1,
) -> Result<(), AuthoredPageIdentityValidationErrorV1> {
    if !is_editor_created_uuid_v7_page_id(identity.page_id) {
        return Err(AuthoredPageIdentityValidationErrorV1::PageIdNotUuidV7);
    }
    if identity.provenance != AuthoredEntityProvenanceV1::AuthorCreated {
        return Err(AuthoredPageIdentityValidationErrorV1::NonAuthorCreatedProvenance);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::CanonicalId;

    fn page_id_with(version_nibble: u8, variant_bits: u8) -> PageId {
        let mut bytes = [0x11; 16];
        bytes[6] = (version_nibble << 4) | 0x0a;
        bytes[8] = variant_bits | 0x01;
        PageId::from_canonical(CanonicalId::from_bytes(bytes))
    }

    #[test]
    fn author_created_page_identity_accepts_uuid_v7_and_explicit_provenance() {
        let identity = AuthoredPageIdentityV1 {
            page_id: page_id_with(0x7, 0x80),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };

        assert_eq!(validate_authored_page_identity_v1(&identity), Ok(()));
        assert!(is_editor_created_uuid_v7_page_id(identity.page_id));
    }

    #[test]
    fn page_identity_rejects_non_uuid_v7_identity() {
        let identity = AuthoredPageIdentityV1 {
            page_id: page_id_with(0x4, 0x80),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };

        assert_eq!(
            validate_authored_page_identity_v1(&identity),
            Err(AuthoredPageIdentityValidationErrorV1::PageIdNotUuidV7)
        );
    }

    #[test]
    fn page_identity_rejects_non_rfc4122_variant() {
        let identity = AuthoredPageIdentityV1 {
            page_id: page_id_with(0x7, 0x00),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };

        assert_eq!(
            validate_authored_page_identity_v1(&identity),
            Err(AuthoredPageIdentityValidationErrorV1::PageIdNotUuidV7)
        );
    }

    #[test]
    fn page_identity_never_inferrs_author_created_from_absent_source_state() {
        let identity = AuthoredPageIdentityV1 {
            page_id: page_id_with(0x7, 0x80),
            provenance: AuthoredEntityProvenanceV1::SourceBacked,
        };

        assert_eq!(
            validate_authored_page_identity_v1(&identity),
            Err(AuthoredPageIdentityValidationErrorV1::NonAuthorCreatedProvenance)
        );
    }

    #[test]
    fn wire_shape_is_explicit_and_does_not_touch_pub_model_page() {
        let identity = AuthoredPageIdentityV1 {
            page_id: page_id_with(0x7, 0x80),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let value = serde_json::to_value(identity).expect("serialize authored page identity");

        assert!(value.get("page_id").is_some());
        assert_eq!(value["provenance"]["kind"], "author_created");
        assert_eq!(value.as_object().expect("object").len(), 2);
    }
}
