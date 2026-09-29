#[cfg(target_os = "windows")]
mod imp {
    use std::fmt;

    const LOAD_LIBRARY_SEARCH_APPLICATION_DIR: u32 = 0x0000_0200;
    const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;
    const CHAPTERA_DEFAULT_DLL_DIRECTORIES: u32 =
        LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetDefaultDllDirectories(directory_flags: u32) -> i32;
        fn SetDllDirectoryW(path_name: *const u16) -> i32;
        fn GetLastError() -> u32;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DllSearchPolicyError {
        operation: &'static str,
        win32_error: u32,
    }

    impl fmt::Display for DllSearchPolicyError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "{} failed with Win32 error {}",
                self.operation, self.win32_error
            )
        }
    }

    impl std::error::Error for DllSearchPolicyError {}

    fn last_error(operation: &'static str) -> DllSearchPolicyError {
        DllSearchPolicyError {
            operation,
            win32_error: unsafe { GetLastError() },
        }
    }

    pub fn install_process_policy() -> Result<(), DllSearchPolicyError> {
        let default_ok = unsafe { SetDefaultDllDirectories(CHAPTERA_DEFAULT_DLL_DIRECTORIES) };
        if default_ok == 0 {
            return Err(last_error("SetDefaultDllDirectories"));
        }

        // An empty directory string explicitly removes the current directory
        // from the legacy process DLL search order. Combined with the default
        // directory policy above, basename loads are admitted only from the
        // application directory and System32 unless a future reviewed call
        // supplies explicit LOAD_LIBRARY_SEARCH_* flags.
        let empty = [0_u16];
        let cwd_ok = unsafe { SetDllDirectoryW(empty.as_ptr()) };
        if cwd_ok == 0 {
            return Err(last_error("SetDllDirectoryW"));
        }

        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn policy_can_be_reapplied_idempotently() {
            install_process_policy().expect("first DLL-search policy install");
            install_process_policy().expect("second DLL-search policy install");
        }

        #[test]
        fn default_roots_are_exactly_application_dir_plus_system32() {
            assert_eq!(
                CHAPTERA_DEFAULT_DLL_DIRECTORIES,
                LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32
            );
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::install_process_policy;

#[cfg(not(target_os = "windows"))]
pub fn install_process_policy() -> Result<(), std::convert::Infallible> {
    Ok(())
}
