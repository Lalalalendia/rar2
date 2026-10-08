#[path = "../windows_dll_search.rs"]
mod windows_dll_search;

#[cfg(target_os = "windows")]
mod windows_probe {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(file_name: *const u16) -> *mut core::ffi::c_void;
        fn GetModuleFileNameW(
            module: *mut core::ffi::c_void,
            file_name: *mut u16,
            size: u32,
        ) -> u32;
        fn FreeLibrary(module: *mut core::ffi::c_void) -> i32;
        fn GetLastError() -> u32;
    }

    pub fn run() -> Result<(), String> {
        crate::windows_dll_search::install_process_policy()
            .map_err(|error| format!("install DLL search policy: {error}"))?;

        let mut args = std::env::args_os().skip(1);
        let Some(name) = args.next() else {
            return Err("usage: chaptera-dll-search-probe DLL-BASENAME".to_owned());
        };
        if args.next().is_some() {
            return Err("probe accepts exactly one DLL basename".to_owned());
        }
        if PathBuf::from(&name).components().count() != 1 {
            return Err("probe accepts a basename, not a path".to_owned());
        }

        let mut wide = OsStr::new(&name).encode_wide().collect::<Vec<_>>();
        wide.push(0);
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            let error = unsafe { GetLastError() };
            return Err(format!("LoadLibraryW failed with Win32 error {error}"));
        }

        let mut buffer = vec![0_u16; 32_768];
        let len = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) };
        let free_ok = unsafe { FreeLibrary(module) };
        if len == 0 {
            let error = unsafe { GetLastError() };
            return Err(format!(
                "GetModuleFileNameW failed with Win32 error {error}"
            ));
        }
        if free_ok == 0 {
            let error = unsafe { GetLastError() };
            return Err(format!("FreeLibrary failed with Win32 error {error}"));
        }

        let loaded = String::from_utf16_lossy(&buffer[..len as usize]);
        println!("{loaded}");
        Ok(())
    }
}

fn main() {
    #[cfg(target_os = "windows")]
    {
        if let Err(error) = windows_probe::run() {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("chaptera-dll-search-probe is Windows-only");
        std::process::exit(2);
    }
}
