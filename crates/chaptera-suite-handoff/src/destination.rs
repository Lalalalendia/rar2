use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SIBLING_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    primary: u64,
    secondary: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedDestination {
    requested_path: PathBuf,
    requested_parent: PathBuf,
    canonical_parent: PathBuf,
    parent_identity: FileIdentity,
    target_path: PathBuf,
    expected_target_identity: Option<FileIdentity>,
    protected: Vec<FileIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationCommitReceipt {
    pub target_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub byte_len: u64,
    pub sha256: String,
}

#[derive(Debug)]
pub enum DestinationWriteError {
    InvalidDestination(&'static str),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    UnsupportedPlatform,
    ParentChanged {
        requested_parent: PathBuf,
    },
    ReparseTarget {
        path: PathBuf,
    },
    ProtectedAlias {
        path: PathBuf,
    },
    TargetChanged {
        path: PathBuf,
    },
    VerificationFailed {
        path: PathBuf,
        reason: &'static str,
    },
    ReplaceInterrupted {
        target: PathBuf,
        candidate: PathBuf,
        backup: PathBuf,
        source: io::Error,
    },
    RecoveryRequired {
        target: PathBuf,
        candidate: Option<PathBuf>,
        backup: Option<PathBuf>,
        reason: &'static str,
    },
}

impl DestinationWriteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidDestination(_) => "destination_invalid",
            Self::Io { .. } => "destination_io",
            Self::UnsupportedPlatform => "destination_identity_unsupported",
            Self::ParentChanged { .. } => "destination_parent_changed",
            Self::ReparseTarget { .. } => "destination_reparse_target",
            Self::ProtectedAlias { .. } => "destination_protected_alias",
            Self::TargetChanged { .. } => "destination_target_changed",
            Self::VerificationFailed { .. } => "destination_verification_failed",
            Self::ReplaceInterrupted { .. } => "destination_replace_interrupted",
            Self::RecoveryRequired { .. } => "destination_recovery_required",
        }
    }
}

impl fmt::Display for DestinationWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDestination(reason) => write!(f, "invalid destination: {reason}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "{operation} {}: {source}", path.display()),
            Self::UnsupportedPlatform => {
                write!(
                    f,
                    "destination file identity is unsupported on this platform"
                )
            }
            Self::ParentChanged { requested_parent } => write!(
                f,
                "destination parent identity changed after admission: {}",
                requested_parent.display()
            ),
            Self::ReparseTarget { path } => {
                write!(
                    f,
                    "destination target is a reparse/symlink: {}",
                    path.display()
                )
            }
            Self::ProtectedAlias { path } => {
                write!(
                    f,
                    "destination aliases a protected file: {}",
                    path.display()
                )
            }
            Self::TargetChanged { path } => {
                write!(
                    f,
                    "destination target changed after admission: {}",
                    path.display()
                )
            }
            Self::VerificationFailed { path, reason } => {
                write!(
                    f,
                    "destination verification failed for {}: {reason}",
                    path.display()
                )
            }
            Self::ReplaceInterrupted {
                target,
                candidate,
                backup,
                source,
            } => write!(
                f,
                "destination replace interrupted for {} (candidate {}, backup {}): {source}",
                target.display(),
                candidate.display(),
                backup.display()
            ),
            Self::RecoveryRequired {
                target,
                candidate,
                backup,
                reason,
            } => write!(
                f,
                "destination recovery required for {} (candidate={:?}, backup={:?}): {reason}",
                target.display(),
                candidate,
                backup
            ),
        }
    }
}

impl std::error::Error for DestinationWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } | Self::ReplaceInterrupted { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> DestinationWriteError {
    DestinationWriteError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

pub fn identify_existing_path(path: &Path) -> Result<FileIdentity, DestinationWriteError> {
    platform_file_identity(path)
}

impl AdmittedDestination {
    pub fn admit(
        requested_path: &Path,
        protected: &[FileIdentity],
    ) -> Result<Self, DestinationWriteError> {
        let target_name = requested_path
            .file_name()
            .ok_or(DestinationWriteError::InvalidDestination(
                "destination must have a file name",
            ))?
            .to_os_string();
        let requested_parent = requested_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let canonical_parent = fs::canonicalize(&requested_parent).map_err(|error| {
            io_error("canonicalize destination parent", &requested_parent, error)
        })?;
        let parent_metadata = fs::metadata(&canonical_parent)
            .map_err(|error| io_error("stat destination parent", &canonical_parent, error))?;
        if !parent_metadata.is_dir() {
            return Err(DestinationWriteError::InvalidDestination(
                "destination parent is not a directory",
            ));
        }
        let parent_identity = identify_existing_path(&canonical_parent)?;
        let target_path = canonical_parent.join(target_name);
        let expected_target_identity = inspect_target(&target_path, protected)?;

        Ok(Self {
            requested_path: requested_path.to_path_buf(),
            requested_parent,
            canonical_parent,
            parent_identity,
            target_path,
            expected_target_identity,
            protected: protected.to_vec(),
        })
    }

    pub fn requested_path(&self) -> &Path {
        &self.requested_path
    }

    pub fn target_path(&self) -> &Path {
        &self.target_path
    }

    pub fn commit_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<DestinationCommitReceipt, DestinationWriteError> {
        self.revalidate_parent()?;

        let (candidate_path, mut candidate_file) = create_unique_candidate(&self.canonical_parent)?;
        candidate_file
            .write_all(bytes)
            .map_err(|error| io_error("write destination candidate", &candidate_path, error))?;
        candidate_file
            .sync_all()
            .map_err(|error| io_error("flush destination candidate", &candidate_path, error))?;
        drop(candidate_file);

        let expected_sha256 = sha256_bytes(bytes);
        verify_exact_file(&candidate_path, bytes.len() as u64, &expected_sha256)?;
        let candidate_identity = identify_existing_path(&candidate_path)?;

        if let Err(error) = self.revalidate_target() {
            let _ = fs::remove_file(&candidate_path);
            return Err(error);
        }

        let backup_path = if self.expected_target_identity.is_some() {
            let backup = reserve_backup_name(&self.canonical_parent)?;
            if let Err(error) = replace_existing(&self.target_path, &candidate_path, &backup) {
                return Err(DestinationWriteError::ReplaceInterrupted {
                    target: self.target_path.clone(),
                    candidate: candidate_path,
                    backup,
                    source: error,
                });
            }
            Some(backup)
        } else {
            publish_absent_target(&candidate_path, &self.target_path)?;
            None
        };

        if let Err(error) =
            verify_exact_file(&self.target_path, bytes.len() as u64, &expected_sha256)
        {
            return Err(DestinationWriteError::RecoveryRequired {
                target: self.target_path.clone(),
                candidate: candidate_path.exists().then_some(candidate_path),
                backup: backup_path,
                reason: match error {
                    DestinationWriteError::VerificationFailed { reason, .. } => reason,
                    _ => "final exact-byte verification failed",
                },
            });
        }

        let final_identity = identify_existing_path(&self.target_path)?;
        if final_identity != candidate_identity {
            return Err(DestinationWriteError::RecoveryRequired {
                target: self.target_path.clone(),
                candidate: candidate_path.exists().then_some(candidate_path),
                backup: backup_path,
                reason: "published target identity does not match admitted candidate",
            });
        }

        if let (Some(expected), Some(backup)) =
            (self.expected_target_identity, backup_path.as_ref())
        {
            let backup_identity = identify_existing_path(backup)?;
            if backup_identity != expected {
                return Err(DestinationWriteError::RecoveryRequired {
                    target: self.target_path.clone(),
                    candidate: candidate_path.exists().then_some(candidate_path),
                    backup: backup_path,
                    reason: "backup identity does not match admitted predecessor",
                });
            }
        }

        Ok(DestinationCommitReceipt {
            target_path: self.target_path.clone(),
            backup_path,
            byte_len: bytes.len() as u64,
            sha256: expected_sha256,
        })
    }

    fn revalidate_parent(&self) -> Result<(), DestinationWriteError> {
        let now = fs::canonicalize(&self.requested_parent).map_err(|_| {
            DestinationWriteError::ParentChanged {
                requested_parent: self.requested_parent.clone(),
            }
        })?;
        if now != self.canonical_parent
            || identify_existing_path(&self.canonical_parent)? != self.parent_identity
        {
            return Err(DestinationWriteError::ParentChanged {
                requested_parent: self.requested_parent.clone(),
            });
        }
        Ok(())
    }

    fn revalidate_target(&self) -> Result<(), DestinationWriteError> {
        let actual = inspect_target(&self.target_path, &self.protected)?;
        if actual != self.expected_target_identity {
            return Err(DestinationWriteError::TargetChanged {
                path: self.target_path.clone(),
            });
        }
        Ok(())
    }
}

fn inspect_target(
    path: &Path,
    protected: &[FileIdentity],
) -> Result<Option<FileIdentity>, DestinationWriteError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("stat destination target", path, error)),
    };
    if metadata.is_dir() {
        return Err(DestinationWriteError::InvalidDestination(
            "destination target is a directory",
        ));
    }
    if is_reparse_or_symlink(&metadata) {
        return Err(DestinationWriteError::ReparseTarget {
            path: path.to_path_buf(),
        });
    }
    let identity = identify_existing_path(path)?;
    if protected.contains(&identity) {
        return Err(DestinationWriteError::ProtectedAlias {
            path: path.to_path_buf(),
        });
    }
    Ok(Some(identity))
}

fn create_unique_candidate(parent: &Path) -> Result<(PathBuf, File), DestinationWriteError> {
    for _ in 0..64 {
        let sequence = SIBLING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            ".chaptera-write-{}-{sequence}.candidate",
            std::process::id()
        );
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("create destination candidate", &path, error)),
        }
    }
    Err(DestinationWriteError::InvalidDestination(
        "could not allocate a unique sibling candidate",
    ))
}

fn reserve_backup_name(parent: &Path) -> Result<PathBuf, DestinationWriteError> {
    for _ in 0..64 {
        let sequence = SIBLING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".chaptera-write-{}-{sequence}.backup",
            std::process::id()
        ));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Ok(_) => continue,
            Err(error) => return Err(io_error("stat destination backup", &path, error)),
        }
    }
    Err(DestinationWriteError::InvalidDestination(
        "could not allocate a unique sibling backup",
    ))
}

fn verify_exact_file(
    path: &Path,
    expected_len: u64,
    expected_sha256: &str,
) -> Result<(), DestinationWriteError> {
    let mut file =
        File::open(path).map_err(|error| io_error("reopen written file", path, error))?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error("read written file", path, error))?;
        if read == 0 {
            break;
        }
        total =
            total
                .checked_add(read as u64)
                .ok_or(DestinationWriteError::VerificationFailed {
                    path: path.to_path_buf(),
                    reason: "written byte length overflow",
                })?;
        digest.update(&buffer[..read]);
    }
    if total != expected_len {
        return Err(DestinationWriteError::VerificationFailed {
            path: path.to_path_buf(),
            reason: "written byte length mismatch",
        });
    }
    if format!("{:x}", digest.finalize()) != expected_sha256 {
        return Err(DestinationWriteError::VerificationFailed {
            path: path.to_path_buf(),
            reason: "written SHA-256 mismatch",
        });
    }
    Ok(())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn publish_absent_target(candidate: &Path, target: &Path) -> Result<(), DestinationWriteError> {
    // A hard-link publication is a create-if-absent operation on both Unix and
    // Windows. Unlike rename(), it cannot silently replace a target that
    // appeared after admission/revalidation.
    fs::hard_link(candidate, target)
        .map_err(|error| io_error("publish absent destination", target, error))?;
    fs::remove_file(candidate)
        .map_err(|error| io_error("remove published candidate link", candidate, error))?;
    Ok(())
}

#[cfg(windows)]
fn replace_existing(target: &Path, candidate: &Path, backup: &Path) -> io::Result<()> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }

    let target = wide(target);
    let candidate = wide(candidate);
    let backup = wide(backup);
    // REPLACEFILE_WRITE_THROUGH is documented as unsupported. Flags stay 0.
    let ok = unsafe {
        replace_file_w(
            target.as_ptr(),
            candidate.as_ptr(),
            backup.as_ptr(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_existing(target: &Path, candidate: &Path, backup: &Path) -> io::Result<()> {
    fs::rename(target, backup)?;
    if let Err(error) = fs::rename(candidate, target) {
        let _ = fs::rename(backup, target);
        return Err(error);
    }
    Ok(())
}

#[cfg(unix)]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(unix)]
fn platform_file_identity(path: &Path) -> Result<FileIdentity, DestinationWriteError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).map_err(|error| io_error("identify file", path, error))?;
    Ok(FileIdentity {
        primary: metadata.dev(),
        secondary: metadata.ino(),
    })
}

#[cfg(windows)]
fn platform_file_identity(path: &Path) -> Result<FileIdentity, DestinationWriteError> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let handle = unsafe {
        create_file_w(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        return Err(io_error("identify file", path, io::Error::last_os_error()));
    }

    let mut information = ByHandleFileInformation::default();
    let ok = unsafe { get_file_information_by_handle(handle, &mut information) };
    let info_error = (ok == 0).then(io::Error::last_os_error);
    unsafe {
        close_handle(handle);
    }
    if let Some(error) = info_error {
        return Err(io_error("identify file", path, error));
    }

    Ok(FileIdentity {
        primary: u64::from(information.volume_serial_number),
        secondary: (u64::from(information.file_index_high) << 32)
            | u64::from(information.file_index_low),
    })
}

#[cfg(not(any(unix, windows)))]
fn platform_file_identity(_path: &Path) -> Result<FileIdentity, DestinationWriteError> {
    Err(DestinationWriteError::UnsupportedPlatform)
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct ByHandleFileInformation {
    file_attributes: u32,
    creation_time: FileTime,
    last_access_time: FileTime,
    last_write_time: FileTime,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "CreateFileW"]
    fn create_file_w(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut std::ffi::c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;

    #[link_name = "GetFileInformationByHandle"]
    fn get_file_information_by_handle(
        file: *mut std::ffi::c_void,
        information: *mut ByHandleFileInformation,
    ) -> i32;

    #[link_name = "CloseHandle"]
    fn close_handle(handle: *mut std::ffi::c_void) -> i32;

    #[link_name = "ReplaceFileW"]
    fn replace_file_w(
        replaced_file_name: *const u16,
        replacement_file_name: *const u16,
        backup_file_name: *const u16,
        replace_flags: u32,
        exclude: *mut std::ffi::c_void,
        reserved: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "chaptera-destination-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn absent_destination_publishes_exact_bytes_without_clobber_semantics() {
        let root = temp_dir("absent");
        let target = root.join("output.json");
        let admitted = AdmittedDestination::admit(&target, &[]).expect("admit absent");
        let receipt = admitted.commit_bytes(b"chaptera").expect("commit");

        assert_eq!(fs::read(&target).unwrap(), b"chaptera");
        assert_eq!(receipt.target_path, target);
        assert!(receipt.backup_path.is_none());
        assert_eq!(receipt.byte_len, 8);
        assert_eq!(
            receipt.sha256,
            "fa0bba13982a384d4c42ef19105ae2bf88c66d63b9b085d1d4022842235d17f8"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_destination_is_replaced_only_after_identity_revalidation() {
        let root = temp_dir("replace");
        let target = root.join("project.json");
        fs::write(&target, b"old").unwrap();
        let old_identity = identify_existing_path(&target).unwrap();
        let admitted = AdmittedDestination::admit(&target, &[]).expect("admit existing");
        let receipt = admitted.commit_bytes(b"new").expect("replace");

        assert_eq!(fs::read(&target).unwrap(), b"new");
        let backup = receipt.backup_path.expect("bounded predecessor backup");
        assert_eq!(fs::read(&backup).unwrap(), b"old");
        assert_eq!(identify_existing_path(&backup).unwrap(), old_identity);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn protected_hardlink_alias_is_rejected_before_candidate_write() {
        let root = temp_dir("hardlink");
        let protected = root.join("source.pub");
        let alias = root.join("looks-safe.json");
        fs::write(&protected, b"immutable").unwrap();
        fs::hard_link(&protected, &alias).unwrap();
        let identity = identify_existing_path(&protected).unwrap();

        let error = AdmittedDestination::admit(&alias, &[identity]).unwrap_err();
        assert_eq!(error.code(), "destination_protected_alias");
        assert_eq!(fs::read(&protected).unwrap(), b"immutable");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn target_swap_after_admission_fails_closed() {
        let root = temp_dir("swap");
        let target = root.join("output.json");
        let replacement = root.join("replacement.tmp");
        fs::write(&target, b"old").unwrap();
        let admitted = AdmittedDestination::admit(&target, &[]).expect("admit");
        fs::write(&replacement, b"attacker").unwrap();
        fs::remove_file(&target).unwrap();
        fs::rename(&replacement, &target).unwrap();

        let error = admitted.commit_bytes(b"new").unwrap_err();
        assert_eq!(error.code(), "destination_target_changed");
        assert_eq!(fs::read(&target).unwrap(), b"attacker");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn parent_symlink_retarget_after_admission_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("parent-retarget");
        let first = root.join("first");
        let second = root.join("second");
        let link = root.join("selected");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        symlink(&first, &link).unwrap();

        let target = link.join("project.json");
        let admitted = AdmittedDestination::admit(&target, &[]).expect("admit");
        fs::remove_file(&link).unwrap();
        symlink(&second, &link).unwrap();

        let error = admitted.commit_bytes(b"new").unwrap_err();
        assert_eq!(error.code(), "destination_parent_changed");
        assert!(!first.join("project.json").exists());
        assert!(!second.join("project.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
