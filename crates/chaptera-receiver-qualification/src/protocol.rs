use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;

pub const RUNTIME_CONFIG_MANIFEST_V1: &str = "chaptera.receiver-runtime-config-manifest.v1";
pub const ORACLE_SCOPE_V1: &str = "chaptera.receiver-oracle-scope.v1";
pub const SEPARATION_EVIDENCE_V1: &str = "chaptera.receiver-separation-evidence.v1";
pub const QUALIFICATION_ASSERTION_RESULT_V1: &str =
    "chaptera.receiver-qualification-assertion-result.v1";
pub const QUALIFICATION_CLAIM_V1: &str = "chaptera.receiver-qualification-claim.v1";
pub const RECEIVER_QUALIFICATION_RECEIPT_V1: &str = "chaptera.receiver-qualification-receipt.v1";

const RUNTIME_MANIFEST_DOMAIN_V1: &[u8] = b"chaptera-receiver-runtime-manifest-v1\0";
const RECEIPT_DOMAIN_V1: &[u8] = b"chaptera-receiver-qualification-receipt-v1\0";
const EVIDENCE_DOMAIN_V1: &[u8] = b"chaptera-receiver-evidence-v1\0";

const MAX_TOKEN_BYTES: usize = 160;
const MAX_TEXT_BYTES: usize = 512;
const MAX_COLLECTION_ITEMS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: &'static str,
}

impl ProtocolError {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStateV1 {
    Supported,
    Unsupported,
    UnknownNotEvidenced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAvailabilityV1 {
    Available,
    Unknown,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OracleScopeLevelV1 {
    L0,
    L1,
    L2,
    L3,
    L4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleScopeV1 {
    pub schema_version: String,
    pub level: OracleScopeLevelV1,
    pub runtime_family: String,
    pub runtime_instance_hash: String,
    pub driver: Option<String>,
    pub target_device_equivalence: String,
}

impl OracleScopeV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        require_version(
            &self.schema_version,
            ORACLE_SCOPE_V1,
            "oracle_scope_version_invalid",
        )?;
        require_token(&self.runtime_family, "oracle_runtime_family_invalid")?;
        require_prefixed_sha256(&self.runtime_instance_hash, "oracle_runtime_hash_invalid")?;
        if let Some(driver) = &self.driver {
            require_text(driver, "oracle_driver_invalid")?;
        }
        require_token(
            &self.target_device_equivalence,
            "oracle_device_equivalence_invalid",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIdentityV1 {
    pub vendor: String,
    pub product: String,
    pub version: String,
    pub build: String,
    pub interpreter: String,
}

impl RuntimeIdentityV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        for (value, code) in [
            (&self.vendor, "runtime_vendor_invalid"),
            (&self.product, "runtime_product_invalid"),
            (&self.version, "runtime_version_invalid"),
            (&self.build, "runtime_build_invalid"),
            (&self.interpreter, "runtime_interpreter_invalid"),
        ] {
            require_text(value, code)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedDigestV1 {
    pub id: String,
    pub digest: String,
}

impl NamedDigestV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        require_text(&self.id, "named_digest_id_invalid")?;
        require_prefixed_sha256(&self.digest, "named_digest_hash_invalid")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeDependencyV1 {
    pub dependency_type: String,
    pub id: String,
    pub digest: String,
    pub required: bool,
    pub availability: EvidenceAvailabilityV1,
    pub provenance: String,
}

impl RuntimeDependencyV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        require_token(&self.dependency_type, "runtime_dependency_type_invalid")?;
        require_text(&self.id, "runtime_dependency_id_invalid")?;
        if self.availability == EvidenceAvailabilityV1::Available {
            require_prefixed_sha256(&self.digest, "runtime_dependency_digest_invalid")?;
        } else if !self.digest.is_empty() {
            require_prefixed_sha256(&self.digest, "runtime_dependency_digest_invalid")?;
        }
        require_text(&self.provenance, "runtime_dependency_provenance_invalid")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRouteConfigV1 {
    pub preset_id: String,
    pub preset_bytes_hash: Option<String>,
    pub expanded_settings_hash: String,
    pub queue_or_virtual_printer: String,
    pub ticket_mapping_hash: Option<String>,
}

impl JobRouteConfigV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        require_text(&self.preset_id, "job_route_preset_invalid")?;
        require_prefixed_sha256(
            &self.expanded_settings_hash,
            "job_route_expanded_hash_invalid",
        )?;
        require_text(&self.queue_or_virtual_printer, "job_route_queue_invalid")?;
        validate_optional_hash(&self.preset_bytes_hash, "job_route_preset_hash_invalid")?;
        validate_optional_hash(&self.ticket_mapping_hash, "job_route_ticket_hash_invalid")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterDriverConfigV1 {
    pub identity: String,
    pub version: String,
    pub bytes_or_vendor_digest: String,
    pub controller_compatibility: String,
}

impl PrinterDriverConfigV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        require_text(&self.identity, "driver_identity_invalid")?;
        require_text(&self.version, "driver_version_invalid")?;
        require_prefixed_sha256(&self.bytes_or_vendor_digest, "driver_digest_invalid")?;
        require_text(
            &self.controller_compatibility,
            "driver_controller_compatibility_invalid",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorConfigV1 {
    pub media_definition: String,
    pub output_profile: Option<NamedDigestV1>,
    pub calibration: Option<NamedDigestV1>,
    pub devicelinks: Vec<NamedDigestV1>,
    pub spot_libraries: Vec<NamedDigestV1>,
    pub color_adjustments: Vec<NamedDigestV1>,
    pub cmm_or_engine: String,
}

impl ColorConfigV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        require_text(&self.media_definition, "color_media_definition_invalid")?;
        require_text(&self.cmm_or_engine, "color_cmm_invalid")?;
        for item in self
            .output_profile
            .iter()
            .chain(self.calibration.iter())
            .chain(self.devicelinks.iter())
            .chain(self.spot_libraries.iter())
            .chain(self.color_adjustments.iter())
        {
            item.validate()?;
        }
        require_collection_bound(self.devicelinks.len(), "color_devicelinks_too_many")?;
        require_collection_bound(self.spot_libraries.len(), "color_spot_libraries_too_many")?;
        require_collection_bound(self.color_adjustments.len(), "color_adjustments_too_many")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecialInkConfigV1 {
    pub inkset: String,
    pub generated_role_rules_hash: Option<String>,
    pub layer_order: Vec<String>,
    pub choke_spread_hash: Option<String>,
}

impl SpecialInkConfigV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        require_text(&self.inkset, "special_inkset_invalid")?;
        validate_optional_hash(
            &self.generated_role_rules_hash,
            "special_generated_rules_hash_invalid",
        )?;
        validate_optional_hash(&self.choke_spread_hash, "special_choke_hash_invalid")?;
        require_collection_bound(self.layer_order.len(), "special_layer_order_too_many")?;
        for layer in &self.layer_order {
            require_text(layer, "special_layer_name_invalid")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishingConfigV1 {
    pub cutter_driver: Option<NamedDigestV1>,
    pub mark_schema_hash: Option<String>,
    pub device_adjustments_hash: Option<String>,
    pub jig_or_print_area_hash: Option<String>,
}

impl FinishingConfigV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(driver) = &self.cutter_driver {
            driver.validate()?;
        }
        validate_optional_hash(&self.mark_schema_hash, "finishing_mark_schema_hash_invalid")?;
        validate_optional_hash(
            &self.device_adjustments_hash,
            "finishing_adjustments_hash_invalid",
        )?;
        validate_optional_hash(
            &self.jig_or_print_area_hash,
            "finishing_print_area_hash_invalid",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfigManifestV1 {
    pub schema_version: String,
    pub runtime: RuntimeIdentityV1,
    pub job_route: JobRouteConfigV1,
    pub printer_driver: PrinterDriverConfigV1,
    pub color: ColorConfigV1,
    pub special_ink: SpecialInkConfigV1,
    pub finishing: FinishingConfigV1,
    pub dependencies: Vec<RuntimeDependencyV1>,
}

impl RuntimeConfigManifestV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        require_version(
            &self.schema_version,
            RUNTIME_CONFIG_MANIFEST_V1,
            "runtime_manifest_version_invalid",
        )?;
        self.runtime.validate()?;
        self.job_route.validate()?;
        self.printer_driver.validate()?;
        self.color.validate()?;
        self.special_ink.validate()?;
        self.finishing.validate()?;
        require_collection_bound(self.dependencies.len(), "runtime_dependencies_too_many")?;
        for dependency in &self.dependencies {
            dependency.validate()?;
            if dependency.required && dependency.availability != EvidenceAvailabilityV1::Available {
                return Err(ProtocolError::new("runtime_config_closure_partial"));
            }
        }
        Ok(())
    }

    pub fn manifest_hash(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        derive_hash(RUNTIME_MANIFEST_DOMAIN_V1, self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReferenceV1 {
    pub evidence_id: String,
    pub evidence_kind: String,
    pub content_hash: String,
    pub oracle_scope: OracleScopeV1,
    pub availability: EvidenceAvailabilityV1,
}

impl EvidenceReferenceV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        require_token(&self.evidence_id, "evidence_id_invalid")?;
        require_token(&self.evidence_kind, "evidence_kind_invalid")?;
        require_prefixed_sha256(&self.content_hash, "evidence_content_hash_invalid")?;
        self.oracle_scope.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeparationEvidenceLevelV1 {
    E2,
    E3,
    E4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TernaryEvidenceStateV1 {
    True,
    False,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationProducerV1 {
    pub vendor: String,
    pub product: String,
    pub version_build: String,
    pub interpreter: String,
    pub runtime_config_manifest_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationSourceV1 {
    pub input_artifact_hash: String,
    pub job_id: String,
    pub output_artifact_hash: String,
    pub native_format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationStageV1 {
    pub evidence_level: SeparationEvidenceLevelV1,
    pub pipeline_stage: String,
    pub after_color_conversion: TernaryEvidenceStateV1,
    pub after_special_role_generation: TernaryEvidenceStateV1,
    pub after_choke_spread: TernaryEvidenceStateV1,
    pub after_calibration: TernaryEvidenceStateV1,
    pub after_screening: TernaryEvidenceStateV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationPlaneV1 {
    pub canonical_colorant_id: String,
    pub receiver_colorant_name: String,
    pub colorant_class: String,
    pub plate_order: u32,
    pub identity_source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationRasterV1 {
    pub width_px: u32,
    pub height_px: u32,
    pub resolution_x_dpi: f64,
    pub resolution_y_dpi: f64,
    pub bits_per_sample: u16,
    pub polarity: String,
    pub tone: String,
    pub compression: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationGeometryV1 {
    pub physical_width_mm: f64,
    pub physical_height_mm: f64,
    pub origin_transform: Vec<f64>,
    pub page_to_raster_transform: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationProcessingV1 {
    pub output_profile_hash: Option<String>,
    pub calibration_id_hash: Option<String>,
    pub separation_strategy: String,
    pub black_generation: String,
    pub screening: String,
    pub runtime_generated_role: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceProvenanceV1 {
    pub metadata_raw_hash: String,
    pub extraction_method: String,
    pub confidence: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationEvidenceV1 {
    pub schema_version: String,
    pub oracle_scope: OracleScopeV1,
    pub producer: SeparationProducerV1,
    pub source: SeparationSourceV1,
    pub stage: SeparationStageV1,
    pub plane: SeparationPlaneV1,
    pub raster: SeparationRasterV1,
    pub geometry: SeparationGeometryV1,
    pub processing: SeparationProcessingV1,
    pub provenance: EvidenceProvenanceV1,
}

impl SeparationEvidenceV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        require_version(
            &self.schema_version,
            SEPARATION_EVIDENCE_V1,
            "separation_evidence_version_invalid",
        )?;
        self.oracle_scope.validate()?;
        for (value, code) in [
            (&self.producer.vendor, "separation_vendor_invalid"),
            (&self.producer.product, "separation_product_invalid"),
            (&self.producer.version_build, "separation_version_invalid"),
            (&self.producer.interpreter, "separation_interpreter_invalid"),
            (&self.source.job_id, "separation_job_id_invalid"),
            (
                &self.source.native_format,
                "separation_native_format_invalid",
            ),
            (
                &self.stage.pipeline_stage,
                "separation_pipeline_stage_invalid",
            ),
            (
                &self.plane.canonical_colorant_id,
                "separation_colorant_id_invalid",
            ),
            (
                &self.plane.receiver_colorant_name,
                "separation_receiver_colorant_invalid",
            ),
            (
                &self.plane.colorant_class,
                "separation_colorant_class_invalid",
            ),
            (
                &self.plane.identity_source,
                "separation_identity_source_invalid",
            ),
            (&self.raster.polarity, "separation_polarity_invalid"),
            (&self.raster.tone, "separation_tone_invalid"),
            (&self.raster.compression, "separation_compression_invalid"),
            (
                &self.processing.separation_strategy,
                "separation_strategy_invalid",
            ),
            (
                &self.processing.black_generation,
                "separation_black_generation_invalid",
            ),
            (&self.processing.screening, "separation_screening_invalid"),
            (
                &self.provenance.extraction_method,
                "separation_extraction_method_invalid",
            ),
            (&self.provenance.confidence, "separation_confidence_invalid"),
        ] {
            require_text(value, code)?;
        }
        for (value, code) in [
            (
                &self.producer.runtime_config_manifest_hash,
                "separation_runtime_manifest_hash_invalid",
            ),
            (
                &self.source.input_artifact_hash,
                "separation_input_hash_invalid",
            ),
            (
                &self.source.output_artifact_hash,
                "separation_output_hash_invalid",
            ),
            (
                &self.provenance.metadata_raw_hash,
                "separation_metadata_hash_invalid",
            ),
        ] {
            require_prefixed_sha256(value, code)?;
        }
        validate_optional_hash(
            &self.processing.output_profile_hash,
            "separation_output_profile_hash_invalid",
        )?;
        validate_optional_hash(
            &self.processing.calibration_id_hash,
            "separation_calibration_hash_invalid",
        )?;
        if self.raster.width_px == 0
            || self.raster.height_px == 0
            || self.raster.bits_per_sample == 0
        {
            return Err(ProtocolError::new("separation_raster_dimensions_invalid"));
        }
        for number in [
            self.raster.resolution_x_dpi,
            self.raster.resolution_y_dpi,
            self.geometry.physical_width_mm,
            self.geometry.physical_height_mm,
        ] {
            if !number.is_finite() || number <= 0.0 {
                return Err(ProtocolError::new("separation_numeric_value_invalid"));
            }
        }
        if self.geometry.origin_transform.len() != 6
            || self.geometry.page_to_raster_transform.len() != 6
        {
            return Err(ProtocolError::new("separation_transform_invalid"));
        }
        if self
            .geometry
            .origin_transform
            .iter()
            .chain(self.geometry.page_to_raster_transform.iter())
            .any(|value| !value.is_finite())
        {
            return Err(ProtocolError::new("separation_transform_invalid"));
        }
        Ok(())
    }

    pub fn evidence_hash(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        derive_hash(EVIDENCE_DOMAIN_V1, self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QualificationAssertionOutcomeV1 {
    Pass,
    Fail,
    Review,
    Unknown,
    Waived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationAssertionResultV1 {
    pub schema_version: String,
    pub assertion_id: String,
    pub outcome: QualificationAssertionOutcomeV1,
    pub required_scope: OracleScopeLevelV1,
    pub evidence: Vec<EvidenceReferenceV1>,
    pub reason: String,
    pub waiver_id: Option<String>,
}

impl QualificationAssertionResultV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        require_version(
            &self.schema_version,
            QUALIFICATION_ASSERTION_RESULT_V1,
            "assertion_result_version_invalid",
        )?;
        require_token(&self.assertion_id, "assertion_id_invalid")?;
        require_text(&self.reason, "assertion_reason_invalid")?;
        require_collection_bound(self.evidence.len(), "assertion_evidence_too_many")?;
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        if self.outcome == QualificationAssertionOutcomeV1::Pass {
            if self.evidence.is_empty() {
                return Err(ProtocolError::new("assertion_pass_without_evidence"));
            }
            if self.evidence.iter().any(|item| {
                item.availability != EvidenceAvailabilityV1::Available
                    || item.oracle_scope.level < self.required_scope
            }) {
                return Err(ProtocolError::new("assertion_pass_evidence_insufficient"));
            }
            if self.waiver_id.is_some() {
                return Err(ProtocolError::new("assertion_pass_with_waiver_invalid"));
            }
        }
        if self.outcome == QualificationAssertionOutcomeV1::Waived {
            let Some(waiver_id) = &self.waiver_id else {
                return Err(ProtocolError::new("assertion_waiver_id_required"));
            };
            require_token(waiver_id, "assertion_waiver_id_invalid")?;
        } else if self.waiver_id.is_some() {
            return Err(ProtocolError::new("assertion_waiver_id_unexpected"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationClaimV1 {
    pub schema_version: String,
    pub claim_id: String,
    pub capability: CapabilityStateV1,
    pub required_scope: OracleScopeLevelV1,
    pub assertion_results: Vec<QualificationAssertionResultV1>,
}

impl QualificationClaimV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        require_version(
            &self.schema_version,
            QUALIFICATION_CLAIM_V1,
            "qualification_claim_version_invalid",
        )?;
        require_token(&self.claim_id, "qualification_claim_id_invalid")?;
        if self.assertion_results.is_empty() {
            return Err(ProtocolError::new(
                "qualification_claim_assertions_required",
            ));
        }
        require_collection_bound(
            self.assertion_results.len(),
            "qualification_claim_assertions_too_many",
        )?;
        for assertion in &self.assertion_results {
            assertion.validate()?;
            if assertion.required_scope < self.required_scope {
                return Err(ProtocolError::new(
                    "qualification_claim_scope_underdeclared",
                ));
            }
        }
        if self.capability == CapabilityStateV1::Supported
            && self
                .assertion_results
                .iter()
                .any(|assertion| assertion.outcome != QualificationAssertionOutcomeV1::Pass)
        {
            return Err(ProtocolError::new(
                "qualification_supported_requires_all_pass",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverIdentityV1 {
    pub id: String,
    pub version: String,
    pub hash: String,
}

impl ReceiverIdentityV1 {
    fn validate(&self, code: &'static str) -> Result<(), ProtocolError> {
        require_token(&self.id, code)?;
        require_text(&self.version, code)?;
        require_prefixed_sha256(&self.hash, code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusIdentityV1 {
    pub id: String,
    pub version: String,
    pub hash: String,
}

impl CorpusIdentityV1 {
    fn validate(&self) -> Result<(), ProtocolError> {
        require_token(&self.id, "receipt_corpus_id_invalid")?;
        require_text(&self.version, "receipt_corpus_version_invalid")?;
        require_prefixed_sha256(&self.hash, "receipt_corpus_hash_invalid")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverQualificationReceiptV1 {
    pub schema_version: String,
    pub receiver_profile: ReceiverIdentityV1,
    pub runtime_profile: ReceiverIdentityV1,
    pub runtime_config_manifest_hash: String,
    pub corpus: CorpusIdentityV1,
    pub qualification_plan_hash: String,
    pub normalization_version: String,
    pub claims: Vec<QualificationClaimV1>,
    pub evidence: Vec<EvidenceReferenceV1>,
}

impl ReceiverQualificationReceiptV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        require_version(
            &self.schema_version,
            RECEIVER_QUALIFICATION_RECEIPT_V1,
            "qualification_receipt_version_invalid",
        )?;
        self.receiver_profile
            .validate("receipt_receiver_identity_invalid")?;
        self.runtime_profile
            .validate("receipt_runtime_identity_invalid")?;
        self.corpus.validate()?;
        require_prefixed_sha256(
            &self.runtime_config_manifest_hash,
            "receipt_runtime_manifest_hash_invalid",
        )?;
        require_prefixed_sha256(&self.qualification_plan_hash, "receipt_plan_hash_invalid")?;
        require_token(
            &self.normalization_version,
            "receipt_normalization_version_invalid",
        )?;
        if self.claims.is_empty() {
            return Err(ProtocolError::new("qualification_receipt_claims_required"));
        }
        require_collection_bound(self.claims.len(), "qualification_receipt_claims_too_many")?;
        require_collection_bound(
            self.evidence.len(),
            "qualification_receipt_evidence_too_many",
        )?;
        for claim in &self.claims {
            claim.validate()?;
        }
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        Ok(())
    }

    pub fn receipt_hash(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        derive_hash(RECEIPT_DOMAIN_V1, self)
    }
}

pub fn canonical_protocol_json_v1<T: Serialize + ?Sized>(
    value: &T,
) -> Result<Vec<u8>, ProtocolError> {
    let value = serde_json::to_value(value)
        .map_err(|_| ProtocolError::new("canonical_json_serialize_failed"))?;
    let mut out = Vec::new();
    write_canonical_value(&mut out, &value)
        .map_err(|_| ProtocolError::new("canonical_json_serialize_failed"))?;
    Ok(out)
}

fn derive_hash<T: Serialize + ?Sized>(domain: &[u8], value: &T) -> Result<String, ProtocolError> {
    let canonical = canonical_protocol_json_v1(value)?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(canonical);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn write_canonical_value(out: &mut Vec<u8>, value: &Value) -> Result<(), serde_json::Error> {
    match value {
        Value::Object(map) => {
            out.push(b'{');
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    out.push(b',');
                }
                serde_json::to_writer(&mut *out, key)?;
                out.push(b':');
                write_canonical_value(out, &map[key])?;
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    out.push(b',');
                }
                write_canonical_value(out, item)?;
            }
            out.push(b']');
        }
        scalar => serde_json::to_writer(out, scalar)?,
    }
    Ok(())
}

fn require_version(value: &str, expected: &str, code: &'static str) -> Result<(), ProtocolError> {
    if value != expected {
        return Err(ProtocolError::new(code));
    }
    Ok(())
}

fn require_token(value: &str, code: &'static str) -> Result<(), ProtocolError> {
    if value.is_empty()
        || value.len() > MAX_TOKEN_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/' | b'+')
        })
    {
        return Err(ProtocolError::new(code));
    }
    Ok(())
}

fn require_text(value: &str, code: &'static str) -> Result<(), ProtocolError> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(ProtocolError::new(code));
    }
    Ok(())
}

fn require_prefixed_sha256(value: &str, code: &'static str) -> Result<(), ProtocolError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(ProtocolError::new(code));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(ProtocolError::new(code));
    }
    Ok(())
}

fn validate_optional_hash(value: &Option<String>, code: &'static str) -> Result<(), ProtocolError> {
    if let Some(value) = value {
        require_prefixed_sha256(value, code)?;
    }
    Ok(())
}

fn require_collection_bound(len: usize, code: &'static str) -> Result<(), ProtocolError> {
    if len > MAX_COLLECTION_ITEMS {
        return Err(ProtocolError::new(code));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hash(ch: char) -> String {
        format!("sha256:{}", std::iter::repeat_n(ch, 64).collect::<String>())
    }

    fn scope(level: OracleScopeLevelV1) -> OracleScopeV1 {
        OracleScopeV1 {
            schema_version: ORACLE_SCOPE_V1.into(),
            level,
            runtime_family: "CalderaRIP".into(),
            runtime_instance_hash: hash('1'),
            driver: Some("File".into()),
            target_device_equivalence: "none".into(),
        }
    }

    fn evidence(
        level: OracleScopeLevelV1,
        availability: EvidenceAvailabilityV1,
    ) -> EvidenceReferenceV1 {
        EvidenceReferenceV1 {
            evidence_id: "plate.cyan".into(),
            evidence_kind: "separation".into(),
            content_hash: hash('2'),
            oracle_scope: scope(level),
            availability,
        }
    }

    #[test]
    fn canonical_json_profile_and_evidence_domain_are_frozen() {
        let value = json!({"z": 1, "a": [3, 1, 2], "unicode": "é"});
        let canonical = canonical_protocol_json_v1(&value).unwrap();
        assert_eq!(
            String::from_utf8(canonical.clone()).unwrap(),
            r#"{"a":[3,1,2],"unicode":"é","z":1}"#
        );
        let mut hasher = Sha256::new();
        hasher.update(EVIDENCE_DOMAIN_V1);
        hasher.update(canonical);
        assert_eq!(
            format!("{:x}", hasher.finalize()),
            "d1054550e170de6ab31f16dc6ebcb8904f4077c1453e7fa6dccd7fcbd7c4c4b8"
        );
    }

    #[test]
    fn pass_requires_available_evidence_at_required_scope() {
        let base = QualificationAssertionResultV1 {
            schema_version: QUALIFICATION_ASSERTION_RESULT_V1.into(),
            assertion_id: "white_plate_exists".into(),
            outcome: QualificationAssertionOutcomeV1::Pass,
            required_scope: OracleScopeLevelV1::L2,
            evidence: vec![evidence(
                OracleScopeLevelV1::L2,
                EvidenceAvailabilityV1::Available,
            )],
            reason: "required plate observed".into(),
            waiver_id: None,
        };
        base.validate().unwrap();

        let mut unknown = base.clone();
        unknown.evidence[0].availability = EvidenceAvailabilityV1::Unknown;
        assert_eq!(
            unknown.validate().unwrap_err().code,
            "assertion_pass_evidence_insufficient"
        );

        let mut low_scope = base;
        low_scope.evidence[0].oracle_scope.level = OracleScopeLevelV1::L1;
        assert_eq!(
            low_scope.validate().unwrap_err().code,
            "assertion_pass_evidence_insufficient"
        );
    }

    #[test]
    fn supported_claim_cannot_hide_unknown_or_waived_assertions() {
        let mut assertion = QualificationAssertionResultV1 {
            schema_version: QUALIFICATION_ASSERTION_RESULT_V1.into(),
            assertion_id: "cut_role".into(),
            outcome: QualificationAssertionOutcomeV1::Unknown,
            required_scope: OracleScopeLevelV1::L1,
            evidence: vec![evidence(
                OracleScopeLevelV1::L1,
                EvidenceAvailabilityV1::Unknown,
            )],
            reason: "runtime does not expose final cut role evidence".into(),
            waiver_id: None,
        };
        assertion.validate().unwrap();
        let claim = QualificationClaimV1 {
            schema_version: QUALIFICATION_CLAIM_V1.into(),
            claim_id: "receiver.cut_role".into(),
            capability: CapabilityStateV1::Supported,
            required_scope: OracleScopeLevelV1::L1,
            assertion_results: vec![assertion.clone()],
        };
        assert_eq!(
            claim.validate().unwrap_err().code,
            "qualification_supported_requires_all_pass"
        );

        assertion.outcome = QualificationAssertionOutcomeV1::Waived;
        assertion.waiver_id = Some("waiver-001".into());
        assertion.validate().unwrap();
        let waived = QualificationClaimV1 {
            assertion_results: vec![assertion],
            ..claim
        };
        assert_eq!(
            waived.validate().unwrap_err().code,
            "qualification_supported_requires_all_pass"
        );
    }

    #[test]
    fn missing_required_runtime_dependency_is_partial_closure() {
        let manifest = RuntimeConfigManifestV1 {
            schema_version: RUNTIME_CONFIG_MANIFEST_V1.into(),
            runtime: RuntimeIdentityV1 {
                vendor: "Caldera".into(),
                product: "CalderaRIP".into(),
                version: "19.3".into(),
                build: "test".into(),
                interpreter: "APPE".into(),
            },
            job_route: JobRouteConfigV1 {
                preset_id: "quickprint-1".into(),
                preset_bytes_hash: None,
                expanded_settings_hash: hash('3'),
                queue_or_virtual_printer: "File".into(),
                ticket_mapping_hash: None,
            },
            printer_driver: PrinterDriverConfigV1 {
                identity: "File".into(),
                version: "1".into(),
                bytes_or_vendor_digest: hash('4'),
                controller_compatibility: "generic".into(),
            },
            color: ColorConfigV1 {
                media_definition: "Contone".into(),
                output_profile: None,
                calibration: None,
                devicelinks: vec![],
                spot_libraries: vec![],
                color_adjustments: vec![],
                cmm_or_engine: "Caldera".into(),
            },
            special_ink: SpecialInkConfigV1 {
                inkset: "CMYK".into(),
                generated_role_rules_hash: None,
                layer_order: vec!["C".into(), "M".into(), "Y".into(), "K".into()],
                choke_spread_hash: None,
            },
            finishing: FinishingConfigV1 {
                cutter_driver: None,
                mark_schema_hash: None,
                device_adjustments_hash: None,
                jig_or_print_area_hash: None,
            },
            dependencies: vec![RuntimeDependencyV1 {
                dependency_type: "output_profile".into(),
                id: "profile-1".into(),
                digest: String::new(),
                required: true,
                availability: EvidenceAvailabilityV1::Unavailable,
                provenance: "runtime reference unresolved".into(),
            }],
        };
        assert_eq!(
            manifest.validate().unwrap_err().code,
            "runtime_config_closure_partial"
        );
    }

    #[test]
    fn unknown_wire_fields_are_rejected() {
        let raw = r#"{
          "schema_version":"chaptera.receiver-oracle-scope.v1",
          "level":"L1",
          "runtime_family":"CalderaRIP",
          "runtime_instance_hash":"sha256:1111111111111111111111111111111111111111111111111111111111111111",
          "driver":"File",
          "target_device_equivalence":"none",
          "pass":true
        }"#;
        assert!(serde_json::from_str::<OracleScopeV1>(raw).is_err());
    }
}
