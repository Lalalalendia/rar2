use serde::{Deserialize, Serialize};

pub const CLASSIFIER_PROTOCOL_V1: &str = "chaptera.failure-classifier.v1";
pub const INTAKE_PROTOCOL_V1: &str = "chaptera.intake-capability-request.v1";
pub const INTAKE_RECEIPT_V1: &str = "chaptera.intake-receipt.v1";
pub const CONSENT_VERSION_V1: &str = "chaptera-intake-consent-v1";
pub const RETENTION_POLICY_V1: &str = "chaptera-intake-retention-v1";

const MAX_REASON_FLAGS: usize = 16;
const MAX_REASON_FLAG_BYTES: usize = 96;
const MAX_FAILURE_CODE_BYTES: usize = 96;
const MAX_OPAQUE_ID_BYTES: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureClassV1 {
    PubHighValue,
    PubDamaged,
    PubPossible,
    ArchiveWithPub,
    NotPub,
    #[serde(rename = "SUSPICIOUS/POLYGLOT")]
    SuspiciousPolyglot,
}

impl FailureClassV1 {
    pub const fn exact_file_intake_eligible(self) -> bool {
        matches!(self, Self::PubHighValue | Self::PubDamaged)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfidenceBandV1 {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureClassificationV1 {
    pub protocol_version: String,
    pub class: FailureClassV1,
    pub confidence: ConfidenceBandV1,
    pub reason_flags: Vec<String>,
}

impl FailureClassificationV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_version != CLASSIFIER_PROTOCOL_V1 {
            return Err(ProtocolError::new("classifier_protocol_version_invalid"));
        }
        if self.reason_flags.is_empty() || self.reason_flags.len() > MAX_REASON_FLAGS {
            return Err(ProtocolError::new("classifier_reason_count_invalid"));
        }
        for flag in &self.reason_flags {
            require_stable_token(flag, MAX_REASON_FLAG_BYTES, "classifier_reason_invalid")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeConsentRequestV1 {
    pub protocol_version: String,
    pub consent_version: String,
}

impl IntakeConsentRequestV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_version != INTAKE_PROTOCOL_V1 {
            return Err(ProtocolError::new("intake_protocol_version_invalid"));
        }
        if self.consent_version != CONSENT_VERSION_V1 {
            return Err(ProtocolError::new("intake_consent_version_invalid"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerIntakeEvidenceV1 {
    pub classification: FailureClassificationV1,
    pub failure_code: String,
}

impl ServerIntakeEvidenceV1 {
    pub fn validate_and_authorize(
        &self,
        consent: &IntakeConsentRequestV1,
    ) -> Result<(), ProtocolError> {
        consent.validate()?;
        self.classification.validate()?;
        if !self.classification.class.exact_file_intake_eligible() {
            return Err(ProtocolError::new("intake_class_ineligible"));
        }
        require_stable_token(
            &self.failure_code,
            MAX_FAILURE_CODE_BYTES,
            "intake_failure_code_invalid",
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExactByteDispositionV1 {
    NewExactBytes,
    DuplicateExactBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterDispositionV1 {
    NewCluster,
    ExistingCluster,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeReceiptV1 {
    pub protocol_version: String,
    pub submission_id: String,
    pub server_sha256: String,
    pub exact_byte_disposition: ExactByteDispositionV1,
    pub cluster_disposition: ClusterDispositionV1,
    pub retention_policy: String,
}

impl IntakeReceiptV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_version != INTAKE_RECEIPT_V1 {
            return Err(ProtocolError::new("intake_receipt_version_invalid"));
        }
        require_opaque_id(&self.submission_id)?;
        require_sha256(&self.server_sha256)?;
        if self.retention_policy != RETENTION_POLICY_V1 {
            return Err(ProtocolError::new("intake_retention_policy_invalid"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: &'static str,
}

impl ProtocolError {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for ProtocolError {}

fn require_stable_token(
    value: &str,
    max_bytes: usize,
    code: &'static str,
) -> Result<(), ProtocolError> {
    if value.is_empty()
        || value.len() > max_bytes
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
    {
        return Err(ProtocolError::new(code));
    }
    Ok(())
}

fn require_opaque_id(value: &str) -> Result<(), ProtocolError> {
    if value.len() < 16 || value.len() > MAX_OPAQUE_ID_BYTES {
        return Err(ProtocolError::new("intake_submission_id_invalid"));
    }
    require_stable_token(
        value,
        MAX_OPAQUE_ID_BYTES,
        "intake_submission_id_invalid",
    )
}

fn require_sha256(value: &str) -> Result<(), ProtocolError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(ProtocolError::new("intake_server_sha256_invalid"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classification(class: FailureClassV1) -> FailureClassificationV1 {
        FailureClassificationV1 {
            protocol_version: CLASSIFIER_PROTOCOL_V1.to_owned(),
            class,
            confidence: ConfidenceBandV1::High,
            reason_flags: vec!["publisher_contents_present".to_owned()],
        }
    }

    fn consent() -> IntakeConsentRequestV1 {
        IntakeConsentRequestV1 {
            protocol_version: INTAKE_PROTOCOL_V1.to_owned(),
            consent_version: CONSENT_VERSION_V1.to_owned(),
        }
    }

    fn evidence(class: FailureClassV1) -> ServerIntakeEvidenceV1 {
        ServerIntakeEvidenceV1 {
            classification: classification(class),
            failure_code: "reader_scene_open_failed".to_owned(),
        }
    }

    #[test]
    fn exact_file_eligibility_is_fail_closed_to_two_classes() {
        for class in [
            FailureClassV1::PubHighValue,
            FailureClassV1::PubDamaged,
        ] {
            evidence(class)
                .validate_and_authorize(&consent())
                .expect("eligible class");
        }

        for class in [
            FailureClassV1::PubPossible,
            FailureClassV1::ArchiveWithPub,
            FailureClassV1::NotPub,
            FailureClassV1::SuspiciousPolyglot,
        ] {
            assert_eq!(
                evidence(class)
                    .validate_and_authorize(&consent())
                    .unwrap_err()
                    .code,
                "intake_class_ineligible"
            );
        }
    }

    #[test]
    fn consent_version_is_exact_and_mandatory() {
        let mut value = consent();
        value.consent_version = "chaptera-intake-consent-v0".to_owned();
        assert_eq!(
            evidence(FailureClassV1::PubDamaged)
                .validate_and_authorize(&value)
                .unwrap_err()
                .code,
            "intake_consent_version_invalid"
        );
    }

    #[test]
    fn consent_wire_contains_no_client_claim_about_file_or_eligibility() {
        let value = serde_json::to_value(consent()).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 2);
        for forbidden in [
            "classification",
            "class",
            "failure_code",
            "filename",
            "path",
            "sha256",
            "client_sha256",
            "bytes",
            "object_url",
            "storage_key",
        ] {
            assert!(!object.contains_key(forbidden));
        }
    }

    #[test]
    fn classifier_tokens_match_canonical_contract() {
        let cases = [
            (FailureClassV1::PubHighValue, "\"PUB_HIGH_VALUE\""),
            (FailureClassV1::PubDamaged, "\"PUB_DAMAGED\""),
            (FailureClassV1::PubPossible, "\"PUB_POSSIBLE\""),
            (FailureClassV1::ArchiveWithPub, "\"ARCHIVE_WITH_PUB\""),
            (FailureClassV1::NotPub, "\"NOT_PUB\""),
            (
                FailureClassV1::SuspiciousPolyglot,
                "\"SUSPICIOUS/POLYGLOT\"",
            ),
        ];
        for (class, expected) in cases {
            assert_eq!(serde_json::to_string(&class).unwrap(), expected);
        }
    }

    #[test]
    fn receipt_requires_server_identity_and_authoritative_retention_policy() {
        let receipt = IntakeReceiptV1 {
            protocol_version: INTAKE_RECEIPT_V1.to_owned(),
            submission_id: "submission_01JZZZZZZZZZZZZZZZZZZZZZZZ".to_owned(),
            server_sha256: "a".repeat(64),
            exact_byte_disposition: ExactByteDispositionV1::NewExactBytes,
            cluster_disposition: ClusterDispositionV1::Deferred,
            retention_policy: RETENTION_POLICY_V1.to_owned(),
        };
        receipt.validate().expect("valid receipt");

        let mut wrong_policy = receipt.clone();
        wrong_policy.retention_policy = "custom-retention".to_owned();
        assert_eq!(
            wrong_policy.validate().unwrap_err().code,
            "intake_retention_policy_invalid"
        );

        let mut uppercase_hash = receipt.clone();
        uppercase_hash.server_sha256 = "A".repeat(64);
        assert_eq!(
            uppercase_hash.validate().unwrap_err().code,
            "intake_server_sha256_invalid"
        );

        let mut bad_hash = receipt;
        bad_hash.server_sha256 = "client-value".to_owned();
        assert_eq!(
            bad_hash.validate().unwrap_err().code,
            "intake_server_sha256_invalid"
        );
    }

    #[test]
    fn unknown_wire_fields_are_rejected() {
        let raw = r#"{
            "protocol_version":"chaptera.intake-capability-request.v1",
            "consent_version":"chaptera-intake-consent-v1",
            "class":"PUB_HIGH_VALUE"
        }"#;
        assert!(serde_json::from_str::<IntakeConsentRequestV1>(raw).is_err());
    }
}
