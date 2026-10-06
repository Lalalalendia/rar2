use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Cursor;

pub const SECURITY_PROFILE_V1: &str = "chaptera-untrusted-pub-v1";
pub const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_MAX_CFB_ENTRIES: u64 = 8_192;
pub const DEFAULT_MAX_DECLARED_STREAM_BYTES: u64 = 512 * 1024 * 1024;

const CFB_SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const CFB_FREE_SECTOR: u32 = 0xffff_ffff;
const CFB_END_OF_CHAIN: u32 = 0xffff_fffe;
const CFB_FAT_SECTOR: u32 = 0xffff_fffd;
const CFB_DIFAT_SECTOR: u32 = 0xffff_fffc;
const CFB_MAX_REGULAR_SECTOR: u32 = 0xffff_fffa;
const CFB_MINI_STREAM_CUTOFF: u32 = 4096;
const CFB_HEADER_DIFAT_ENTRIES: usize = 109;
const CFB_MAX_PATH_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubScanPolicyV1 {
    pub max_file_bytes: u64,
    pub max_cfb_entries: u64,
    pub max_declared_stream_bytes: u64,
}

impl Default for PubScanPolicyV1 {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_cfb_entries: DEFAULT_MAX_CFB_ENTRIES,
            max_declared_stream_bytes: DEFAULT_MAX_DECLARED_STREAM_BYTES,
        }
    }
}

impl PubScanPolicyV1 {
    pub fn validate(self) -> Result<Self, String> {
        if self.max_file_bytes == 0 {
            return Err("max_file_bytes must be positive".to_owned());
        }
        if self.max_cfb_entries == 0 {
            return Err("max_cfb_entries must be positive".to_owned());
        }
        if self.max_declared_stream_bytes == 0 {
            return Err("max_declared_stream_bytes must be positive".to_owned());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubScanStatusV1 {
    AcceptedCfb,
    ParseFailed,
    RejectedByPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubScanResultV1 {
    pub protocol_version: String,
    pub security_profile: String,
    pub status: PubScanStatusV1,
    pub byte_len: u64,
    pub sha256: String,
    pub cfb_entry_count: Option<u64>,
    pub declared_stream_bytes: Option<u64>,
    pub filesystem_confinement: bool,
    pub security_event: Option<String>,
    pub error: Option<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn inspect_pub_bytes_v1(
    bytes: &[u8],
    policy: PubScanPolicyV1,
    filesystem_confinement: bool,
) -> PubScanResultV1 {
    let policy = match policy.validate() {
        Ok(policy) => policy,
        Err(error) => {
            return result(
                bytes,
                PubScanStatusV1::RejectedByPolicy,
                None,
                None,
                filesystem_confinement,
                Some("invalid_policy".to_owned()),
                Some(error),
            );
        }
    };

    let byte_len = bytes.len() as u64;
    if byte_len > policy.max_file_bytes {
        return result(
            bytes,
            PubScanStatusV1::RejectedByPolicy,
            None,
            None,
            filesystem_confinement,
            Some(format!(
                "input_size_limit: {byte_len} > {}",
                policy.max_file_bytes
            )),
            Some("PUB input exceeds admitted byte limit".to_owned()),
        );
    }

    if let Err(violation) = inspect_cfb_firewall(bytes) {
        return result(
            bytes,
            PubScanStatusV1::ParseFailed,
            None,
            None,
            filesystem_confinement,
            Some(format!("cfb_firewall:{}", violation.code)),
            Some(violation.message),
        );
    }

    let compound = match cfb::CompoundFile::open(Cursor::new(bytes)) {
        Ok(compound) => compound,
        Err(error) => {
            return result(
                bytes,
                PubScanStatusV1::ParseFailed,
                None,
                None,
                filesystem_confinement,
                None,
                Some(format!("CFB parse failed: {error}")),
            );
        }
    };

    let mut entry_count = 0_u64;
    let mut declared_stream_bytes = 0_u64;
    for entry in compound.walk() {
        entry_count = match entry_count.checked_add(1) {
            Some(value) => value,
            None => {
                return result(
                    bytes,
                    PubScanStatusV1::RejectedByPolicy,
                    None,
                    None,
                    filesystem_confinement,
                    Some("cfb_entry_count_overflow".to_owned()),
                    Some("CFB entry count overflowed u64".to_owned()),
                );
            }
        };
        if entry_count > policy.max_cfb_entries {
            return result(
                bytes,
                PubScanStatusV1::RejectedByPolicy,
                Some(entry_count),
                None,
                filesystem_confinement,
                Some(format!(
                    "cfb_entry_limit: {entry_count} > {}",
                    policy.max_cfb_entries
                )),
                Some("CFB entry count exceeds admitted limit".to_owned()),
            );
        }

        let path_depth = entry.path().components().count();
        if path_depth > CFB_MAX_PATH_DEPTH {
            return result(
                bytes,
                PubScanStatusV1::RejectedByPolicy,
                Some(entry_count),
                Some(declared_stream_bytes),
                filesystem_confinement,
                Some(format!(
                    "cfb_path_depth_limit: {path_depth} > {CFB_MAX_PATH_DEPTH}"
                )),
                Some("CFB path depth exceeds admitted limit".to_owned()),
            );
        }

        if entry.is_stream() {
            if entry.len() > byte_len {
                return result(
                    bytes,
                    PubScanStatusV1::ParseFailed,
                    Some(entry_count),
                    Some(declared_stream_bytes),
                    filesystem_confinement,
                    Some("cfb_firewall:stream_length_exceeds_file".to_owned()),
                    Some(format!(
                        "CFB stream {:?} declares {} bytes in a {} byte file",
                        entry.path(),
                        entry.len(),
                        byte_len
                    )),
                );
            }
            declared_stream_bytes = match declared_stream_bytes.checked_add(entry.len()) {
                Some(value) => value,
                None => {
                    return result(
                        bytes,
                        PubScanStatusV1::RejectedByPolicy,
                        Some(entry_count),
                        None,
                        filesystem_confinement,
                        Some("cfb_declared_stream_bytes_overflow".to_owned()),
                        Some("CFB declared stream byte sum overflowed u64".to_owned()),
                    );
                }
            };
            if declared_stream_bytes > policy.max_declared_stream_bytes {
                return result(
                    bytes,
                    PubScanStatusV1::RejectedByPolicy,
                    Some(entry_count),
                    Some(declared_stream_bytes),
                    filesystem_confinement,
                    Some(format!(
                        "cfb_declared_stream_bytes_limit: {declared_stream_bytes} > {}",
                        policy.max_declared_stream_bytes
                    )),
                    Some("CFB declared stream bytes exceed admitted limit".to_owned()),
                );
            }
        }
    }

    result(
        bytes,
        PubScanStatusV1::AcceptedCfb,
        Some(entry_count),
        Some(declared_stream_bytes),
        filesystem_confinement,
        None,
        None,
    )
}

#[derive(Debug)]
struct CfbFirewallViolation {
    code: &'static str,
    message: String,
}

fn firewall_violation(code: &'static str, message: impl Into<String>) -> CfbFirewallViolation {
    CfbFirewallViolation {
        code,
        message: message.into(),
    }
}

fn inspect_cfb_firewall(bytes: &[u8]) -> Result<(), CfbFirewallViolation> {
    if bytes.len() < 512 {
        return Err(firewall_violation(
            "header_truncated",
            "CFB input is shorter than the 512-byte header",
        ));
    }
    if bytes.get(..8) != Some(CFB_SIGNATURE.as_slice()) {
        return Err(firewall_violation(
            "signature_invalid",
            "CFB signature does not match the Compound File Binary format",
        ));
    }

    let major = cfb_u16(bytes, 26)?;
    if !matches!(major, 3 | 4) {
        return Err(firewall_violation(
            "major_version_invalid",
            format!("unsupported CFB major version {major}"),
        ));
    }
    let byte_order = cfb_u16(bytes, 28)?;
    if byte_order != 0xfffe {
        return Err(firewall_violation(
            "byte_order_invalid",
            format!("unsupported CFB byte order {byte_order:#06x}"),
        ));
    }

    let sector_shift = cfb_u16(bytes, 30)?;
    let sector_len = match (major, sector_shift) {
        (3, 9) => 512usize,
        (4, 12) => 4096usize,
        _ => {
            return Err(firewall_violation(
                "sector_size_invalid",
                format!("unsupported CFB major/sector pair {major}/{sector_shift}"),
            ));
        }
    };
    if cfb_u16(bytes, 32)? != 6 {
        return Err(firewall_violation(
            "mini_sector_size_invalid",
            "CFB mini-sector shift must be 6",
        ));
    }
    if bytes.len() < sector_len || !bytes.len().is_multiple_of(sector_len) {
        return Err(firewall_violation(
            "file_alignment_invalid",
            format!(
                "CFB byte length {} is not aligned to sector size {sector_len}",
                bytes.len()
            ),
        ));
    }

    let num_sectors = bytes.len() / sector_len - 1;
    let num_directory_sectors = cfb_u32(bytes, 40)? as usize;
    if major == 3 && num_directory_sectors != 0 {
        return Err(firewall_violation(
            "v3_directory_sector_count_invalid",
            "CFB version 3 must declare zero directory sectors in the header",
        ));
    }

    let num_fat_sectors = cfb_u32(bytes, 44)? as usize;
    if num_fat_sectors > num_sectors {
        return Err(firewall_violation(
            "fat_sector_count_exceeds_file",
            format!(
                "CFB declares {num_fat_sectors} FAT sectors for only {num_sectors} physical sectors"
            ),
        ));
    }
    let fat_entries_per_sector = sector_len / 4;
    if num_fat_sectors
        .checked_mul(fat_entries_per_sector)
        .is_none_or(|capacity| capacity < num_sectors)
    {
        return Err(firewall_violation(
            "fat_capacity_too_small",
            "declared FAT sectors cannot address all physical sectors",
        ));
    }

    require_regular_sector(
        cfb_u32(bytes, 48)?,
        num_sectors,
        "directory",
        "directory_sector_invalid",
    )?;

    if cfb_u32(bytes, 56)? != CFB_MINI_STREAM_CUTOFF {
        return Err(firewall_violation(
            "mini_stream_cutoff_invalid",
            "CFB mini-stream cutoff must be 4096 bytes",
        ));
    }

    let first_mini_fat = cfb_u32(bytes, 60)?;
    let num_mini_fat = cfb_u32(bytes, 64)? as usize;
    if num_mini_fat > num_sectors {
        return Err(firewall_violation(
            "mini_fat_sector_count_exceeds_file",
            "CFB MiniFAT sector count exceeds physical sector count",
        ));
    }
    if num_mini_fat > 0 {
        require_regular_sector(
            first_mini_fat,
            num_sectors,
            "MiniFAT",
            "mini_fat_sector_invalid",
        )?;
    } else if !matches!(first_mini_fat, CFB_END_OF_CHAIN | CFB_FREE_SECTOR) {
        return Err(firewall_violation(
            "mini_fat_empty_chain_invalid",
            "CFB with zero MiniFAT sectors has a non-terminal MiniFAT start",
        ));
    }

    let first_difat = cfb_u32(bytes, 68)?;
    let num_difat = cfb_u32(bytes, 72)? as usize;
    if num_difat > num_sectors {
        return Err(firewall_violation(
            "difat_sector_count_exceeds_file",
            "CFB DIFAT sector count exceeds physical sector count",
        ));
    }
    if num_difat > 0 {
        require_regular_sector(first_difat, num_sectors, "DIFAT", "difat_sector_invalid")?;
    } else if !matches!(first_difat, CFB_END_OF_CHAIN | CFB_FREE_SECTOR) {
        return Err(firewall_violation(
            "difat_empty_chain_invalid",
            "CFB with zero DIFAT sectors has a non-terminal DIFAT start",
        ));
    }

    let difat_slots_per_sector = fat_entries_per_sector.saturating_sub(1);
    let max_fat_sector_ids = CFB_HEADER_DIFAT_ENTRIES
        .checked_add(
            num_difat
                .checked_mul(difat_slots_per_sector)
                .ok_or_else(|| {
                    firewall_violation(
                        "difat_capacity_overflow",
                        "CFB DIFAT capacity overflowed usize",
                    )
                })?,
        )
        .ok_or_else(|| {
            firewall_violation(
                "difat_capacity_overflow",
                "CFB DIFAT capacity overflowed usize",
            )
        })?;
    if num_fat_sectors > max_fat_sector_ids {
        return Err(firewall_violation(
            "difat_capacity_too_small",
            "CFB DIFAT cannot name the declared number of FAT sectors",
        ));
    }

    let mut header_fat_ids = std::collections::BTreeSet::new();
    for index in 0..CFB_HEADER_DIFAT_ENTRIES {
        let sector = cfb_u32(bytes, 76 + index * 4)?;
        if sector == CFB_FREE_SECTOR {
            continue;
        }
        require_regular_sector(
            sector,
            num_sectors,
            "header DIFAT",
            "header_fat_sector_invalid",
        )?;
        if !header_fat_ids.insert(sector) {
            return Err(firewall_violation(
                "duplicate_header_fat_sector",
                format!("CFB header names FAT sector {sector} more than once"),
            ));
        }
    }
    if header_fat_ids.len() > num_fat_sectors {
        return Err(firewall_violation(
            "header_fat_count_exceeds_declared",
            "CFB header names more FAT sectors than num_fat_sectors declares",
        ));
    }

    Ok(())
}

fn cfb_u16(bytes: &[u8], offset: usize) -> Result<u16, CfbFirewallViolation> {
    let raw = bytes.get(offset..offset + 2).ok_or_else(|| {
        firewall_violation(
            "header_truncated",
            format!("CFB u16 field at offset {offset} lies outside the header"),
        )
    })?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn cfb_u32(bytes: &[u8], offset: usize) -> Result<u32, CfbFirewallViolation> {
    let raw = bytes.get(offset..offset + 4).ok_or_else(|| {
        firewall_violation(
            "header_truncated",
            format!("CFB u32 field at offset {offset} lies outside the header"),
        )
    })?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn require_regular_sector(
    sector: u32,
    num_sectors: usize,
    label: &str,
    code: &'static str,
) -> Result<(), CfbFirewallViolation> {
    let valid = sector <= CFB_MAX_REGULAR_SECTOR
        && !matches!(
            sector,
            CFB_FREE_SECTOR | CFB_END_OF_CHAIN | CFB_FAT_SECTOR | CFB_DIFAT_SECTOR
        )
        && usize::try_from(sector)
            .ok()
            .is_some_and(|value| value < num_sectors);
    if valid {
        Ok(())
    } else {
        Err(firewall_violation(
            code,
            format!("{label} references invalid physical sector {sector}"),
        ))
    }
}

fn result(
    bytes: &[u8],
    status: PubScanStatusV1,
    cfb_entry_count: Option<u64>,
    declared_stream_bytes: Option<u64>,
    filesystem_confinement: bool,
    security_event: Option<String>,
    error: Option<String>,
) -> PubScanResultV1 {
    PubScanResultV1 {
        protocol_version: "chaptera.untrusted-pub-scan-result.v1".to_owned(),
        security_profile: SECURITY_PROFILE_V1.to_owned(),
        status,
        byte_len: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        cfb_entry_count,
        declared_stream_bytes,
        filesystem_confinement,
        security_event,
        error,
    }
}

pub fn filesystem_default_deny_supported() -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
}

pub fn install_post_read_filesystem_default_deny() -> std::io::Result<()> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        linux::install_filesystem_default_deny()
    }

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "post-read filesystem default-deny is implemented only on Linux x86_64",
        ))
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    const BPF_LD_W_ABS: u16 = 0x20;
    const BPF_JMP_JEQ_K: u16 = 0x15;
    const BPF_RET_K: u16 = 0x06;
    const SECCOMP_DATA_NR_OFFSET: u32 = 0;
    const SECCOMP_DATA_ARCH_OFFSET: u32 = 4;
    const SECCOMP_MODE_FILTER: libc::c_ulong = 2;
    const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
    const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
    const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;

    pub(super) fn install_filesystem_default_deny() -> std::io::Result<()> {
        let denied_syscalls: &[libc::c_long] = &[
            libc::SYS_open,
            libc::SYS_openat,
            libc::SYS_openat2,
            libc::SYS_creat,
            libc::SYS_open_by_handle_at,
            libc::SYS_stat,
            libc::SYS_lstat,
            libc::SYS_newfstatat,
            libc::SYS_statx,
            libc::SYS_access,
            libc::SYS_faccessat,
            libc::SYS_faccessat2,
            libc::SYS_readlink,
            libc::SYS_readlinkat,
            libc::SYS_getdents,
            libc::SYS_getdents64,
            libc::SYS_unlink,
            libc::SYS_unlinkat,
            libc::SYS_rename,
            libc::SYS_renameat,
            libc::SYS_renameat2,
            libc::SYS_link,
            libc::SYS_linkat,
            libc::SYS_symlink,
            libc::SYS_symlinkat,
            libc::SYS_mkdir,
            libc::SYS_mkdirat,
            libc::SYS_rmdir,
            libc::SYS_mknod,
            libc::SYS_mknodat,
            libc::SYS_chmod,
            libc::SYS_fchmod,
            libc::SYS_fchmodat,
            libc::SYS_chown,
            libc::SYS_fchown,
            libc::SYS_lchown,
            libc::SYS_fchownat,
            libc::SYS_truncate,
            libc::SYS_ftruncate,
            libc::SYS_utime,
            libc::SYS_utimes,
            libc::SYS_futimesat,
            libc::SYS_utimensat,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_pivot_root,
            libc::SYS_chroot,
            libc::SYS_execve,
            libc::SYS_execveat,
            libc::SYS_fork,
            libc::SYS_vfork,
            libc::SYS_clone,
            libc::SYS_clone3,
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
        ];
        install_errno_filter(denied_syscalls)
    }

    fn install_errno_filter(denied_syscalls: &[libc::c_long]) -> std::io::Result<()> {
        let mut filter = Vec::with_capacity(5 + denied_syscalls.len() * 2);
        filter.push(statement(BPF_LD_W_ABS, SECCOMP_DATA_ARCH_OFFSET));
        filter.push(jump(BPF_JMP_JEQ_K, AUDIT_ARCH_X86_64, 1, 0));
        filter.push(statement(BPF_RET_K, SECCOMP_RET_KILL_PROCESS));
        filter.push(statement(BPF_LD_W_ABS, SECCOMP_DATA_NR_OFFSET));

        for &syscall_number in denied_syscalls {
            filter.push(jump(BPF_JMP_JEQ_K, syscall_number as u32, 0, 1));
            filter.push(statement(
                BPF_RET_K,
                SECCOMP_RET_ERRNO | (libc::EPERM as u32),
            ));
        }
        filter.push(statement(BPF_RET_K, SECCOMP_RET_ALLOW));

        let mut program = libc::sock_fprog {
            len: filter
                .len()
                .try_into()
                .expect("seccomp filter length must fit u16"),
            filter: filter.as_mut_ptr(),
        };

        let no_new_privs = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if no_new_privs != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let result = unsafe {
            libc::prctl(
                libc::PR_SET_SECCOMP,
                SECCOMP_MODE_FILTER,
                &mut program as *mut libc::sock_fprog,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    fn statement(code: u16, k: u32) -> libc::sock_filter {
        libc::sock_filter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }

    fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
        libc::sock_filter { code, jt, jf, k }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_identity_is_stable() {
        assert_eq!(
            sha256_hex(b"pub"),
            "0017dea7770f7ecff7ab3c20506546129e96bdeba2f544bb8e5414eb79786122"
        );
    }

    #[test]
    fn invalid_cfb_is_a_parse_failure_not_a_policy_acceptance() {
        let result = inspect_pub_bytes_v1(b"not a cfb", PubScanPolicyV1::default(), false);
        assert_eq!(result.status, PubScanStatusV1::ParseFailed);
        assert!(result.sha256.len() == 64);
    }

    #[test]
    fn input_size_limit_is_enforced_before_cfb_parse() {
        let result = inspect_pub_bytes_v1(
            b"12345",
            PubScanPolicyV1 {
                max_file_bytes: 4,
                ..PubScanPolicyV1::default()
            },
            false,
        );
        assert_eq!(result.status, PubScanStatusV1::RejectedByPolicy);
        assert!(
            result
                .security_event
                .as_deref()
                .is_some_and(|value| value.starts_with("input_size_limit:"))
        );
    }

    #[test]
    fn firewall_rejects_impossible_fat_count_before_cfb_parser() {
        let mut bytes = vec![0_u8; 512];
        bytes[..8].copy_from_slice(&CFB_SIGNATURE);
        bytes[26..28].copy_from_slice(&3_u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&0xfffe_u16.to_le_bytes());
        bytes[30..32].copy_from_slice(&9_u16.to_le_bytes());
        bytes[32..34].copy_from_slice(&6_u16.to_le_bytes());
        bytes[44..48].copy_from_slice(&1_u32.to_le_bytes());
        bytes[48..52].copy_from_slice(&0_u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&CFB_MINI_STREAM_CUTOFF.to_le_bytes());
        bytes[60..64].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());
        bytes[68..72].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());
        for index in 0..CFB_HEADER_DIFAT_ENTRIES {
            let offset = 76 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
        }

        let result = inspect_pub_bytes_v1(&bytes, PubScanPolicyV1::default(), false);
        assert_eq!(result.status, PubScanStatusV1::ParseFailed);
        assert_eq!(
            result.security_event.as_deref(),
            Some("cfb_firewall:fat_sector_count_exceeds_file")
        );
    }

    #[test]
    fn firewall_rejects_unaligned_cfb_before_cfb_parser() {
        let mut bytes = vec![0_u8; 513];
        bytes[..8].copy_from_slice(&CFB_SIGNATURE);
        bytes[26..28].copy_from_slice(&3_u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&0xfffe_u16.to_le_bytes());
        bytes[30..32].copy_from_slice(&9_u16.to_le_bytes());
        bytes[32..34].copy_from_slice(&6_u16.to_le_bytes());

        let result = inspect_pub_bytes_v1(&bytes, PubScanPolicyV1::default(), false);
        assert_eq!(result.status, PubScanStatusV1::ParseFailed);
        assert_eq!(
            result.security_event.as_deref(),
            Some("cfb_firewall:file_alignment_invalid")
        );
    }

    #[test]
    fn platform_claim_matches_implemented_post_read_sandbox() {
        assert_eq!(
            filesystem_default_deny_supported(),
            cfg!(all(target_os = "linux", target_arch = "x86_64"))
        );
    }
}
