use std::io::{Cursor, Read};

use chaptera_failure_intake_protocol::{
    CLASSIFIER_PROTOCOL_V1, ConfidenceBandV1, FailureClassV1, FailureClassificationV1,
    ServerIntakeEvidenceV1,
};

const CFB_MAGIC: &[u8; 8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
const CONTENTS_0X22_MAGIC: [u8; 4] = [0xE8, 0xAC, 0x22, 0x00];
const CONTENTS_0X2C_MAGIC: [u8; 4] = [0xE8, 0xAC, 0x2C, 0x00];
const MAX_ARCHIVE_MEMBERS: usize = 128;
const MAX_ARCHIVE_MEMBER_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ARCHIVE_INSPECTED_BYTES: u64 = 64 * 1024 * 1024;

pub fn guest_failure_intake_evidence(
    bytes: &[u8],
    reader_classification: &str,
    terminal_code: Option<&str>,
) -> Option<ServerIntakeEvidenceV1> {
    if reader_classification != "unsupported" {
        return None;
    }
    let failure_code = terminal_code?;
    if !matches!(
        failure_code,
        "reader_scene_open_failed" | "reader_scene_projection_failed"
    ) {
        return None;
    }

    Some(ServerIntakeEvidenceV1 {
        classification: classify_failed_source(bytes),
        failure_code: failure_code.to_owned(),
    })
}

pub fn classify_failed_source(bytes: &[u8]) -> FailureClassificationV1 {
    if obvious_non_pub(bytes) {
        return classification(
            FailureClassV1::NotPub,
            ConfidenceBandV1::High,
            &["obvious_non_pub_signature"],
        );
    }

    if is_zip(bytes) {
        return classify_archive(bytes);
    }

    if bytes.starts_with(CFB_MAGIC) {
        return classify_cfb(bytes);
    }

    classification(
        FailureClassV1::PubPossible,
        ConfidenceBandV1::Low,
        &["unrecognized_binary_without_publisher_proof"],
    )
}

fn classify_cfb(bytes: &[u8]) -> FailureClassificationV1 {
    let mut compound = match cfb::CompoundFile::open(Cursor::new(bytes)) {
        Ok(compound) => compound,
        Err(_) => {
            return classification(
                FailureClassV1::PubDamaged,
                ConfidenceBandV1::High,
                &["cfb_signature_present", "cfb_parse_failed"],
            );
        }
    };

    if publisher_contents_magic(&mut compound) {
        return classification(
            FailureClassV1::PubHighValue,
            ConfidenceBandV1::High,
            &["publisher_contents_magic", "reader_terminal_failure"],
        );
    }

    let contents_name_present = compound
        .walk()
        .any(|entry| entry.is_stream() && entry.path().to_string_lossy() == "/Contents");
    classification(
        FailureClassV1::PubPossible,
        if contents_name_present {
            ConfidenceBandV1::Medium
        } else {
            ConfidenceBandV1::Low
        },
        if contents_name_present {
            &["cfb_contents_name_without_publisher_magic"]
        } else {
            &["generic_cfb_without_publisher_markers"]
        },
    )
}

fn publisher_contents_magic<F>(compound: &mut cfb::CompoundFile<F>) -> bool
where
    F: std::io::Read + std::io::Seek,
{
    let mut stream = match compound.open_stream("/Contents") {
        Ok(stream) => stream,
        Err(_) => return false,
    };
    let mut magic = [0_u8; 4];
    if stream.read_exact(&mut magic).is_err() {
        return false;
    }
    magic == CONTENTS_0X22_MAGIC || magic == CONTENTS_0X2C_MAGIC
}

fn classify_archive(bytes: &[u8]) -> FailureClassificationV1 {
    let cursor = Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(archive) => archive,
        Err(_) => {
            return classification(
                FailureClassV1::PubPossible,
                ConfidenceBandV1::Low,
                &["zip_signature_parse_failed"],
            );
        }
    };

    if archive.len() > MAX_ARCHIVE_MEMBERS {
        return classification(
            FailureClassV1::PubPossible,
            ConfidenceBandV1::Low,
            &["archive_member_limit_exceeded"],
        );
    }

    let mut inspected = 0_u64;
    for index in 0..archive.len() {
        let mut member = match archive.by_index(index) {
            Ok(member) => member,
            Err(_) => continue,
        };
        if member.is_dir() || member.size() == 0 || member.size() > MAX_ARCHIVE_MEMBER_BYTES {
            continue;
        }
        let next_total = match inspected.checked_add(member.size()) {
            Some(value) if value <= MAX_ARCHIVE_INSPECTED_BYTES => value,
            _ => {
                return classification(
                    FailureClassV1::PubPossible,
                    ConfidenceBandV1::Low,
                    &["archive_inspection_byte_limit_exceeded"],
                );
            }
        };
        inspected = next_total;

        let mut candidate = Vec::with_capacity(member.size() as usize);
        if member.read_to_end(&mut candidate).is_err() {
            continue;
        }
        if !candidate.starts_with(CFB_MAGIC) {
            continue;
        }
        if let Ok(mut compound) = cfb::CompoundFile::open(Cursor::new(candidate.as_slice()))
            && publisher_contents_magic(&mut compound)
        {
            return classification(
                FailureClassV1::ArchiveWithPub,
                ConfidenceBandV1::High,
                &["archive_member_publisher_contents_magic"],
            );
        }
    }

    classification(
        FailureClassV1::PubPossible,
        ConfidenceBandV1::Low,
        &["archive_without_proven_publisher_member"],
    )
}

fn obvious_non_pub(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"%PDF-")
        || bytes.starts_with(b"MZ")
        || bytes.starts_with(b"\x89PNG\r\n\x1A\n")
        || bytes.starts_with(b"\xFF\xD8\xFF")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
    {
        return true;
    }

    let prefix = bytes
        .iter()
        .copied()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .take(512)
        .collect::<Vec<_>>();
    let lowered = prefix
        .iter()
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if lowered.starts_with(b"<!doctype")
        || lowered.starts_with(b"<html")
        || lowered.starts_with(b"<?xml")
    {
        return true;
    }

    bytes.len() >= 8
        && bytes.len() <= 1024 * 1024
        && !bytes.contains(&0)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_whitespace() || byte.is_ascii_graphic())
}

fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

fn classification(
    class: FailureClassV1,
    confidence: ConfidenceBandV1,
    reasons: &[&str],
) -> FailureClassificationV1 {
    FailureClassificationV1 {
        protocol_version: CLASSIFIER_PROTOCOL_V1.to_owned(),
        class,
        confidence,
        reason_flags: reasons.iter().map(|reason| (*reason).to_owned()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use chaptera_failure_intake_protocol::{
        CONSENT_VERSION_V1, INTAKE_PROTOCOL_V1, IntakeConsentRequestV1,
    };

    use super::*;

    fn publisher_cfb(contents_magic: [u8; 4]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut compound = cfb::CompoundFile::create(cursor).expect("create CFB");
        {
            let mut stream = compound.create_stream("/Contents").expect("Contents");
            stream.write_all(&contents_magic).expect("write Contents");
            stream.write_all(&[0_u8; 32]).expect("write payload");
        }
        compound.into_inner().into_inner()
    }

    fn generic_cfb() -> Vec<u8> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut compound = cfb::CompoundFile::create(cursor).expect("create CFB");
        {
            let mut stream = compound.create_stream("/Other").expect("Other");
            stream.write_all(b"not publisher").expect("write Other");
        }
        compound.into_inner().into_inner()
    }

    fn consent() -> IntakeConsentRequestV1 {
        IntakeConsentRequestV1 {
            protocol_version: INTAKE_PROTOCOL_V1.to_owned(),
            consent_version: CONSENT_VERSION_V1.to_owned(),
        }
    }

    #[test]
    fn publisher_cfb_with_reader_failure_is_high_value_and_authorizable() {
        let bytes = publisher_cfb(CONTENTS_0X2C_MAGIC);
        let evidence =
            guest_failure_intake_evidence(&bytes, "unsupported", Some("reader_scene_open_failed"))
                .expect("server evidence");
        assert_eq!(evidence.classification.class, FailureClassV1::PubHighValue);
        evidence
            .validate_and_authorize(&consent())
            .expect("high-value exact file is eligible");
    }

    #[test]
    fn malformed_cfb_signature_is_damaged_and_authorizable() {
        let mut bytes = CFB_MAGIC.to_vec();
        bytes.extend_from_slice(&[0_u8; 32]);
        let evidence =
            guest_failure_intake_evidence(&bytes, "unsupported", Some("reader_scene_open_failed"))
                .expect("server evidence");
        assert_eq!(evidence.classification.class, FailureClassV1::PubDamaged);
        evidence
            .validate_and_authorize(&consent())
            .expect("damaged exact file is eligible");
    }

    #[test]
    fn generic_cfb_stays_possible_and_fails_closed() {
        let evidence = guest_failure_intake_evidence(
            &generic_cfb(),
            "unsupported",
            Some("reader_scene_open_failed"),
        )
        .expect("server evidence");
        assert_eq!(evidence.classification.class, FailureClassV1::PubPossible);
        assert_eq!(
            evidence
                .validate_and_authorize(&consent())
                .expect_err("generic CFB is not eligible")
                .code,
            "intake_class_ineligible"
        );
    }

    #[test]
    fn obvious_non_pub_stays_ineligible() {
        let evidence = guest_failure_intake_evidence(
            b"%PDF-1.7\nnot a publisher document",
            "unsupported",
            Some("reader_scene_open_failed"),
        )
        .expect("server evidence");
        assert_eq!(evidence.classification.class, FailureClassV1::NotPub);
        assert!(!evidence.classification.class.exact_file_intake_eligible());
    }

    #[test]
    fn successful_or_partial_reader_outcome_has_no_failure_intake_evidence() {
        let bytes = publisher_cfb(CONTENTS_0X22_MAGIC);
        for state in ["supported", "partial"] {
            assert!(
                guest_failure_intake_evidence(&bytes, state, None).is_none(),
                "{state} must not mint failure intake evidence"
            );
        }
    }

    #[test]
    fn unknown_terminal_code_has_no_failure_intake_evidence() {
        let bytes = publisher_cfb(CONTENTS_0X22_MAGIC);
        assert!(
            guest_failure_intake_evidence(&bytes, "unsupported", Some("client_claim")).is_none()
        );
    }
}
