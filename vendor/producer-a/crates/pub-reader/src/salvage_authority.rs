use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const READER_EVIDENCE_REGISTRY_SCHEMA_V1: &str =
    "chaptera.reader-evidence-registry.v1";
const READER_EVIDENCE_REGISTRY_JSON: &str =
    include_str!("../data/reader-evidence-registry.json");

#[derive(Debug, Deserialize)]
struct EvidenceRegistry {
    schema: String,
    entries: Vec<EvidenceEntry>,
}

#[derive(Debug, Deserialize)]
struct EvidenceEntry {
    source_sha256: String,
    owner: String,
    evidence_class: String,
    authority_receipt: EvidenceAuthorityReceipt,
    disposition: String,
}

#[derive(Debug, Deserialize)]
struct EvidenceAuthorityReceipt {
    kind: String,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    run_id: Option<u64>,
    #[serde(default)]
    artifact_id: Option<u64>,
    #[serde(default)]
    evidence_digest: Option<String>,
    #[serde(default)]
    classification: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderSalvageAuthority {
    pub source_sha256: String,
    pub owner: String,
    pub task_id: String,
    pub run_id: u64,
    pub artifact_id: u64,
    pub evidence_digest: String,
    pub classification: String,
}

pub fn typed_corruption_authority(source_sha256: &str) -> Option<ReaderSalvageAuthority> {
    if !is_sha256_hex(source_sha256) {
        return None;
    }

    let registry = evidence_registry()?;
    let mut matches = registry.entries.iter().filter(|entry| {
        entry.source_sha256 == source_sha256
            && entry.evidence_class == "typed_corruption"
            && entry.disposition == "existing_typed_corruption_evidence"
    });
    let entry = matches.next()?;
    if matches.next().is_some() {
        return None;
    }

    if entry.authority_receipt.kind != "closed_discriminator" {
        return None;
    }
    let task_id = entry.authority_receipt.task_id.as_ref()?;
    let run_id = entry.authority_receipt.run_id?;
    let artifact_id = entry.authority_receipt.artifact_id?;
    let evidence_digest = entry.authority_receipt.evidence_digest.as_ref()?;
    let classification = entry.authority_receipt.classification.as_ref()?;
    if task_id.trim().is_empty()
        || run_id == 0
        || artifact_id == 0
        || !is_sha256_digest(evidence_digest)
        || classification.trim().is_empty()
        || entry.owner.trim().is_empty()
    {
        return None;
    }

    Some(ReaderSalvageAuthority {
        source_sha256: entry.source_sha256.clone(),
        owner: entry.owner.clone(),
        task_id: task_id.clone(),
        run_id,
        artifact_id,
        evidence_digest: evidence_digest.clone(),
        classification: classification.clone(),
    })
}

fn evidence_registry() -> Option<&'static EvidenceRegistry> {
    static REGISTRY: OnceLock<Option<EvidenceRegistry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let registry: EvidenceRegistry =
                serde_json::from_str(READER_EVIDENCE_REGISTRY_JSON).ok()?;
            (registry.schema == READER_EVIDENCE_REGISTRY_SCHEMA_V1).then_some(registry)
        })
        .as_ref()
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(is_sha256_hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPNHOUS_SHA256: &str =
        "227961e2fba4a6fb814aa2da47e79b19d55ff04e87e49d9c5ceef8d07ce36d0e";

    #[test]
    fn exact_opnhous_authority_resolves_with_pinned_digest() {
        let authority =
            typed_corruption_authority(OPNHOUS_SHA256).expect("OPNHOUS typed authority");
        assert_eq!(authority.source_sha256, OPNHOUS_SHA256);
        assert_eq!(
            authority.evidence_digest,
            "sha256:b814c65752ecdbfcbafcfb16b869f000313fdb7327a4a6a6ad9865fb294a4aed"
        );
        assert_eq!(
            authority.classification,
            "malformed-or-stale-publisher97-media-variant"
        );
        assert_eq!(authority.task_id, "PUB-T-650");
        assert_eq!(authority.run_id, 36072949588);
        assert_eq!(authority.artifact_id, 10838639727);
    }

    #[test]
    fn format_gap_and_unknown_sha_do_not_authorize_salvage() {
        assert!(
            typed_corruption_authority(
                "211c2c6b4bf432fcc85fafa41b6219d328541f1a6e1fa2aaa8cb2134949e3157"
            )
            .is_none()
        );
        assert!(typed_corruption_authority(&"f".repeat(64)).is_none());
    }

    #[test]
    fn malformed_sha_fails_closed() {
        assert!(typed_corruption_authority("not-a-sha").is_none());
        assert!(typed_corruption_authority(&"A".repeat(64)).is_none());
    }
}
