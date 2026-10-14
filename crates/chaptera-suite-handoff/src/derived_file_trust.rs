use crate::destination::{AdmittedDestination, DestinationCommitReceipt, DestinationWriteError};
use crate::{AdmittedSource, identify_existing_path};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedFileTrustDisposition {
    Applied,
    NoOriginEvidence,
    FilesystemOrPolicyNotSupported,
    BlockedOrModified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedFileTrustReceipt {
    pub target_path: PathBuf,
    pub byte_len: u64,
    pub sha256: String,
    pub disposition: DerivedFileTrustDisposition,
}

#[derive(Debug)]
pub enum DerivedFileTrustError {
    SourceChanged(String),
    Destination(DestinationWriteError),
    BlockedOrModified { path: PathBuf, detail: String },
}

impl DerivedFileTrustError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SourceChanged(_) => "derived_file_source_changed",
            Self::Destination(error) => error.code(),
            Self::BlockedOrModified { .. } => "derived_file_blocked_or_modified",
        }
    }
}

impl fmt::Display for DerivedFileTrustError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceChanged(detail) => write!(f, "{detail}"),
            Self::Destination(error) => write!(f, "{error}"),
            Self::BlockedOrModified { path, detail } => write!(
                f,
                "derived-file trust application blocked or changed {}: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for DerivedFileTrustError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Destination(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DestinationWriteError> for DerivedFileTrustError {
    fn from(value: DestinationWriteError) -> Self {
        Self::Destination(value)
    }
}

pub(crate) fn commit_derived_output(
    source: &AdmittedSource,
    destination: &Path,
    bytes: &[u8],
) -> Result<DerivedFileTrustReceipt, DerivedFileTrustError> {
    commit_with_policy(source, destination, bytes, apply_platform_policy)
}

fn commit_with_policy(
    source: &AdmittedSource,
    destination: &Path,
    bytes: &[u8],
    policy: impl FnOnce(&Path, &Path) -> Result<DerivedFileTrustDisposition, String>,
) -> Result<DerivedFileTrustReceipt, DerivedFileTrustError> {
    source
        .verify_unchanged()
        .map_err(DerivedFileTrustError::SourceChanged)?;
    let source_identity = source.file_identity()?;
    let admitted = AdmittedDestination::admit(destination, &[source_identity])?;
    let committed = admitted.commit_bytes(bytes)?;

    let disposition = match identify_existing_path(source.locator_path()) {
        Ok(identity) if identity == source_identity => {
            let source_guard = crate::source_snapshot::open_snapshot_file(source.locator_path())
                .map_err(|error| blocked(source.locator_path(), "reopen admitted source", error))?;
            if crate::destination::identify_open_file(&source_guard)? != source_identity {
                DerivedFileTrustDisposition::NoOriginEvidence
            } else {
                policy(source.locator_path(), &committed.target_path).map_err(|detail| {
                    DerivedFileTrustError::BlockedOrModified {
                        path: committed.target_path.clone(),
                        detail,
                    }
                })?
            }
        }
        _ => DerivedFileTrustDisposition::NoOriginEvidence,
    };

    source
        .verify_unchanged()
        .map_err(DerivedFileTrustError::SourceChanged)?;
    admitted.verify_committed(&committed).map_err(|error| {
        DerivedFileTrustError::BlockedOrModified {
            path: committed.target_path.clone(),
            detail: error.to_string(),
        }
    })?;

    Ok(receipt(committed, disposition))
}

fn receipt(
    committed: DestinationCommitReceipt,
    disposition: DerivedFileTrustDisposition,
) -> DerivedFileTrustReceipt {
    DerivedFileTrustReceipt {
        target_path: committed.target_path,
        byte_len: committed.byte_len,
        sha256: committed.sha256,
        disposition,
    }
}

fn blocked(path: &Path, operation: &str, error: io::Error) -> DerivedFileTrustError {
    DerivedFileTrustError::BlockedOrModified {
        path: path.to_path_buf(),
        detail: format!("{operation}: {error}"),
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_platform_policy(
    _source: &Path,
    _destination: &Path,
) -> Result<DerivedFileTrustDisposition, String> {
    Ok(DerivedFileTrustDisposition::FilesystemOrPolicyNotSupported)
}

#[cfg(target_os = "windows")]
fn apply_platform_policy(
    source: &Path,
    destination: &Path,
) -> Result<DerivedFileTrustDisposition, String> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::{AttachmentServices, IAttachmentExecute};
    use windows::core::{GUID, PCWSTR};

    struct ComApartment(bool);
    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    let initialize = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let apartment = if initialize.is_ok() {
        ComApartment(true)
    } else if initialize.0 as u32 == 0x8001_0106 {
        ComApartment(false)
    } else {
        return Ok(DerivedFileTrustDisposition::FilesystemOrPolicyNotSupported);
    };
    let _apartment = apartment;
    let attachment: IAttachmentExecute =
        match unsafe { CoCreateInstance(&AttachmentServices, None, CLSCTX_INPROC_SERVER) } {
            Ok(value) => value,
            Err(_) => return Ok(DerivedFileTrustDisposition::FilesystemOrPolicyNotSupported),
        };

    fn attachment_services_path(path: &Path) -> Option<Vec<u16>> {
        const VERBATIM_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
        const VERBATIM_UNC_PREFIX: &[u16] = &[
            b'\\' as u16,
            b'\\' as u16,
            b'?' as u16,
            b'\\' as u16,
            b'U' as u16,
            b'N' as u16,
            b'C' as u16,
            b'\\' as u16,
        ];

        let encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        let mut normalized = if let Some(rest) = encoded.strip_prefix(VERBATIM_UNC_PREFIX) {
            [b'\\' as u16, b'\\' as u16]
                .into_iter()
                .chain(rest.iter().copied())
                .collect::<Vec<_>>()
        } else if let Some(rest) = encoded.strip_prefix(VERBATIM_PREFIX) {
            rest.to_vec()
        } else {
            encoded
        };
        if normalized.len() >= 260 {
            return None;
        }
        normalized.extend(iter::once(0));
        Some(normalized)
    }

    let Some(source) = attachment_services_path(source) else {
        return Ok(DerivedFileTrustDisposition::FilesystemOrPolicyNotSupported);
    };
    let Some(destination) = attachment_services_path(destination) else {
        return Ok(DerivedFileTrustDisposition::FilesystemOrPolicyNotSupported);
    };
    const CHAPTERA_CLIENT_ID: GUID = GUID::from_u128(0x430c9d4b_6187_4c83_91bd_17dc5719ec95);

    unsafe {
        attachment
            .SetClientGuid(&CHAPTERA_CLIENT_ID)
            .map_err(|error| format!("AttachmentServices SetClientGuid: {error}"))?;
        attachment
            .SetLocalPath(PCWSTR(destination.as_ptr()))
            .map_err(|error| format!("AttachmentServices SetLocalPath: {error}"))?;
        attachment
            .SetReferrer(PCWSTR(source.as_ptr()))
            .map_err(|error| format!("AttachmentServices SetReferrer: {error}"))?;
        attachment
            .Save()
            .map_err(|error| format!("AttachmentServices Save: {error}"))?;
    }
    Ok(DerivedFileTrustDisposition::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "chaptera-derived-trust-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn admitted(root: &Path) -> AdmittedSource {
        let source = root.join("source.pub");
        fs::write(&source, b"publisher-source").unwrap();
        AdmittedSource::open(&source).unwrap()
    }

    #[test]
    fn exact_output_is_committed_before_policy_receipt() {
        let root = root("exact");
        let source = admitted(&root);
        let output = root.join("output.bin");
        let receipt = commit_with_policy(&source, &output, b"derived", |_, _| {
            Ok(DerivedFileTrustDisposition::Applied)
        })
        .unwrap();
        assert_eq!(fs::read(output).unwrap(), b"derived");
        assert_eq!(receipt.disposition, DerivedFileTrustDisposition::Applied);
        assert_eq!(receipt.byte_len, 7);
        assert_eq!(receipt.sha256, format!("{:x}", Sha256::digest(b"derived")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_alias_is_rejected() {
        let root = root("alias");
        let source = admitted(&root);
        let error = source
            .commit_derived_output(source.locator_path(), b"replacement")
            .unwrap_err();
        assert_eq!(error.code(), "destination_protected_alias");
        assert_eq!(
            fs::read(source.locator_path()).unwrap(),
            b"publisher-source"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleted_output_after_policy_is_typed() {
        let root = root("deleted");
        let source = admitted(&root);
        let output = root.join("output.bin");
        let error = commit_with_policy(&source, &output, b"derived", |_, output| {
            fs::remove_file(output).unwrap();
            Ok(DerivedFileTrustDisposition::Applied)
        })
        .unwrap_err();
        assert_eq!(error.code(), "derived_file_blocked_or_modified");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn modified_output_after_policy_is_typed() {
        let root = root("modified");
        let source = admitted(&root);
        let output = root.join("output.bin");
        let error = commit_with_policy(&source, &output, b"derived", |_, output| {
            fs::write(output, b"modified").unwrap();
            Ok(DerivedFileTrustDisposition::Applied)
        })
        .unwrap_err();
        assert_eq!(error.code(), "derived_file_blocked_or_modified");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rebound_source_path_does_not_fabricate_origin() {
        let root = root("rebound");
        let source = admitted(&root);
        let original = source.locator_path().to_path_buf();
        let moved = root.join("original.pub");
        fs::rename(&original, &moved).unwrap();
        fs::write(&original, b"different-publisher-source").unwrap();
        let output = root.join("output.bin");
        let receipt = commit_with_policy(&source, &output, b"derived", |_, _| {
            panic!("policy must not run for a rebound source locator")
        })
        .unwrap();
        assert_eq!(
            receipt.disposition,
            DerivedFileTrustDisposition::NoOriginEvidence
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unsupported_platform_still_commits_exact_output() {
        let root = root("unsupported");
        let source = admitted(&root);
        let output = root.join("output.bin");
        let receipt = source.commit_derived_output(&output, b"derived").unwrap();
        assert_eq!(
            receipt.disposition,
            DerivedFileTrustDisposition::FilesystemOrPolicyNotSupported
        );
        assert_eq!(fs::read(output).unwrap(), b"derived");
        fs::remove_dir_all(root).unwrap();
    }
}
