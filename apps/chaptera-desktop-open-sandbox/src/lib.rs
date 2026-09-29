use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

pub const DEFAULT_PROCESS_MEMORY_BYTES: usize = 1024 * 1024 * 1024;
pub const DEFAULT_CPU_RATE_PERCENT: u32 = 80;
pub const DEFAULT_WALL_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_CAPTURE_BYTES: u64 = 320 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct SandboxReceipt {
    pub schema_version: &'static str,
    pub app_container: bool,
    pub zero_capabilities: bool,
    pub lpac_all_application_packages_opt_out: bool,
    pub exact_handle_allowlist_count: u32,
    pub child_process_restricted: bool,
    pub job_member_at_creation: bool,
    pub active_process_limit: u32,
    pub process_memory_limit_bytes: usize,
    pub cpu_rate_percent: u32,
    pub kill_on_job_close: bool,
    pub win32k_system_calls_disabled: bool,
    pub extension_points_disabled: bool,
    pub strict_handle_checks: bool,
    pub timed_out: bool,
    pub exit_code: u32,
}

#[derive(Debug)]
pub struct SandboxOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub receipt: SandboxReceipt,
}

pub fn launch_contained(
    executable: &Path,
    request: &[u8],
    timeout: Duration,
) -> Result<SandboxOutput> {
    platform::launch_contained(executable, request, timeout)
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn launch_contained(
        _executable: &Path,
        _request: &[u8],
        _timeout: Duration,
    ) -> Result<SandboxOutput> {
        bail!("Windows containment launcher is unavailable on this platform")
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use rappct::{
        AppContainerProfile,
        acl::{AccessMask, ResourcePath, grant_to_package},
    };
    use std::ffi::{OsStr, OsString, c_void};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::ptr::{null, null_mut};
    use std::thread;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };
    use windows_sys::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
    use windows_sys::Win32::Security::{
        FreeSid, GetTokenInformation, PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES,
        TOKEN_GROUPS, TOKEN_QUERY, TokenCapabilities, TokenIsAppContainer,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_GENERIC_EXECUTE, FILE_GENERIC_READ};
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, IsProcessInJob, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
        JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DETACHED_PROCESS,
        DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
        GetProcessMitigationPolicy, InitializeProcThreadAttributeList, OpenProcessToken,
        PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
        PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION,
        ProcessChildProcessPolicy, ProcessExtensionPointDisablePolicy,
        ProcessStrictHandleCheckPolicy, ProcessSystemCallDisablePolicy, STARTF_USESTDHANDLES,
        STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
    };
    use windows_sys::Win32::System::WindowsProgramming::{
        PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT,
        PROCESS_CREATION_CHILD_PROCESS_RESTRICTED,
    };

    const PROFILE_NAME: &str = "Chaptera.DesktopOpen.Parser.v1";
    const PROFILE_DISPLAY: &str = "Chaptera desktop PUB parser";
    const MITIGATION_STRICT_HANDLE_ALWAYS_ON: u64 = 1u64 << 24;
    const MITIGATION_WIN32K_DISABLE_ALWAYS_ON: u64 = 1u64 << 28;
    const MITIGATION_EXTENSION_POINT_DISABLE_ALWAYS_ON: u64 = 1u64 << 32;
    const WAIT_OBJECT_0_VALUE: u32 = 0;
    const WAIT_TIMEOUT_VALUE: u32 = 258;

    struct Handle(HANDLE);

    impl Handle {
        fn new(raw: HANDLE, stage: &str) -> Result<Self> {
            if raw.is_null() {
                bail!("{stage} failed: Win32 error {}", unsafe { GetLastError() });
            }
            Ok(Self(raw))
        }

        fn raw(&self) -> HANDLE {
            self.0
        }

        unsafe fn into_file(mut self) -> File {
            let raw = self.0;
            self.0 = null_mut();
            unsafe { File::from_raw_handle(raw.cast()) }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    struct Sid(PSID);

    impl Drop for Sid {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    FreeSid(self.0);
                }
            }
        }
    }

    struct AttributeList {
        storage: Vec<usize>,
    }

    impl AttributeList {
        fn new(count: u32) -> Result<Self> {
            let mut bytes = 0usize;
            unsafe {
                InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes);
            }
            if bytes == 0 {
                bail!("InitializeProcThreadAttributeList size query returned zero");
            }
            let words = bytes.div_ceil(size_of::<usize>());
            let mut storage = vec![0usize; words];
            let list = storage.as_mut_ptr().cast();
            let ok = unsafe { InitializeProcThreadAttributeList(list, count, 0, &mut bytes) };
            if ok == 0 {
                bail!(
                    "InitializeProcThreadAttributeList failed: Win32 error {}",
                    unsafe { GetLastError() }
                );
            }
            Ok(Self { storage })
        }

        fn raw(&mut self) -> *mut c_void {
            self.storage.as_mut_ptr().cast()
        }

        fn set<T>(&mut self, attribute: u32, value: &T) -> Result<()> {
            let ok = unsafe {
                UpdateProcThreadAttribute(
                    self.raw(),
                    0,
                    attribute as usize,
                    (value as *const T).cast(),
                    size_of::<T>(),
                    null_mut(),
                    null(),
                )
            };
            if ok == 0 {
                bail!(
                    "UpdateProcThreadAttribute({attribute}) failed: Win32 error {}",
                    unsafe { GetLastError() }
                );
            }
            Ok(())
        }

        fn set_slice<T>(&mut self, attribute: u32, value: &[T]) -> Result<()> {
            let ok = unsafe {
                UpdateProcThreadAttribute(
                    self.raw(),
                    0,
                    attribute as usize,
                    value.as_ptr().cast(),
                    std::mem::size_of_val(value),
                    null_mut(),
                    null(),
                )
            };
            if ok == 0 {
                bail!(
                    "UpdateProcThreadAttribute({attribute}) failed: Win32 error {}",
                    unsafe { GetLastError() }
                );
            }
            Ok(())
        }
    }

    impl Drop for AttributeList {
        fn drop(&mut self) {
            unsafe {
                DeleteProcThreadAttributeList(self.raw());
            }
        }
    }

    struct PipeSet {
        child_stdin: Handle,
        parent_stdin: Handle,
        parent_stdout: Handle,
        child_stdout: Handle,
        parent_stderr: Handle,
        child_stderr: Handle,
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn wide_str(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn make_pipe_set() -> Result<PipeSet> {
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let mut stdin_read = null_mut();
        let mut stdin_write = null_mut();
        let mut stdout_read = null_mut();
        let mut stdout_write = null_mut();
        let mut stderr_read = null_mut();
        let mut stderr_write = null_mut();

        for (read, write, name) in [
            (&mut stdin_read, &mut stdin_write, "stdin"),
            (&mut stdout_read, &mut stdout_write, "stdout"),
            (&mut stderr_read, &mut stderr_write, "stderr"),
        ] {
            let ok = unsafe { CreatePipe(read, write, &security, 0) };
            if ok == 0 {
                bail!("CreatePipe({name}) failed: Win32 error {}", unsafe {
                    GetLastError()
                });
            }
        }

        for (handle, name) in [
            (stdin_write, "parent stdin"),
            (stdout_read, "parent stdout"),
            (stderr_read, "parent stderr"),
        ] {
            let ok = unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
            if ok == 0 {
                bail!(
                    "SetHandleInformation({name}) failed: Win32 error {}",
                    unsafe { GetLastError() }
                );
            }
        }

        Ok(PipeSet {
            child_stdin: Handle::new(stdin_read, "stdin child handle")?,
            parent_stdin: Handle::new(stdin_write, "stdin parent handle")?,
            parent_stdout: Handle::new(stdout_read, "stdout parent handle")?,
            child_stdout: Handle::new(stdout_write, "stdout child handle")?,
            parent_stderr: Handle::new(stderr_read, "stderr parent handle")?,
            child_stderr: Handle::new(stderr_write, "stderr child handle")?,
        })
    }

    #[allow(clippy::field_reassign_with_default)]
    fn configure_job() -> Result<Handle> {
        let job = Handle::new(
            unsafe { CreateJobObjectW(null(), null()) },
            "CreateJobObjectW",
        )?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = DEFAULT_PROCESS_MEMORY_BYTES;
        let ok = unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            bail!(
                "SetInformationJobObject(extended) failed: Win32 error {}",
                unsafe { GetLastError() }
            );
        }

        let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
        cpu.ControlFlags =
            JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
        cpu.Anonymous.CpuRate = DEFAULT_CPU_RATE_PERCENT * 100;
        let ok = unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectCpuRateControlInformation,
                (&cpu as *const JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            bail!(
                "SetInformationJobObject(cpu) failed: Win32 error {}",
                unsafe { GetLastError() }
            );
        }
        Ok(job)
    }

    fn encode_environment(mut entries: Vec<(OsString, OsString)>) -> Vec<u16> {
        entries.sort_by(|a, b| {
            a.0.to_string_lossy()
                .to_uppercase()
                .cmp(&b.0.to_string_lossy().to_uppercase())
                .then_with(|| a.0.cmp(&b.0))
        });

        let mut block = Vec::new();
        for (key, value) in entries {
            block.extend(key.encode_wide());
            block.push(u16::from(b'='));
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    }

    fn minimal_environment(_cwd: &Path) -> Vec<u16> {
        // The contained worker is launched by exact lpApplicationName and does not
        // perform shell/PATH lookup. Keep the inherited environment bounded to
        // Windows runtime identity plus LOCALAPPDATA, which hosted AppContainer
        // CreateProcessW requires even before untrusted PUB bytes are written.
        let entries = [
            "SystemRoot",
            "windir",
            "ComSpec",
            "PATHEXT",
            "TEMP",
            "TMP",
            "PATH",
            "LOCALAPPDATA",
        ]
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (OsString::from(key), value)))
        .collect::<Vec<_>>();
        encode_environment(entries)
    }

    fn derive_sid(profile_name: &str) -> Result<Sid> {
        let name = wide_str(profile_name);
        let mut sid = null_mut();
        let hr = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
        if hr < 0 || sid.is_null() {
            bail!("DeriveAppContainerSidFromAppContainerName failed: HRESULT 0x{hr:08X}");
        }
        Ok(Sid(sid))
    }

    fn get_token_u32(token: HANDLE, class: i32) -> Result<u32> {
        let mut value = 0u32;
        let mut returned = 0u32;
        let ok = unsafe {
            GetTokenInformation(
                token,
                class,
                (&mut value as *mut u32).cast(),
                size_of::<u32>() as u32,
                &mut returned,
            )
        };
        if ok == 0 {
            bail!(
                "GetTokenInformation({class}) failed: Win32 error {}",
                unsafe { GetLastError() }
            );
        }
        Ok(value)
    }

    fn capability_count(token: HANDLE) -> Result<u32> {
        let mut needed = 0u32;
        unsafe {
            GetTokenInformation(token, TokenCapabilities, null_mut(), 0, &mut needed);
        }
        if needed < size_of::<u32>() as u32 {
            bail!("TokenCapabilities size query returned {needed}");
        }
        let mut buffer = vec![0u8; needed as usize];
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenCapabilities,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        };
        if ok == 0 {
            bail!(
                "GetTokenInformation(TokenCapabilities) failed: Win32 error {}",
                unsafe { GetLastError() }
            );
        }
        let groups = unsafe { &*(buffer.as_ptr().cast::<TOKEN_GROUPS>()) };
        Ok(groups.GroupCount)
    }

    fn mitigation_flags(process: HANDLE, policy: i32) -> Result<u32> {
        let mut flags = 0u32;
        let ok = unsafe {
            GetProcessMitigationPolicy(
                process,
                policy,
                (&mut flags as *mut u32).cast(),
                size_of::<u32>(),
            )
        };
        if ok == 0 {
            bail!(
                "GetProcessMitigationPolicy({policy}) failed: Win32 error {}",
                unsafe { GetLastError() }
            );
        }
        Ok(flags)
    }

    fn inspect_process(
        process: HANDLE,
        job: HANDLE,
        require_strict_mitigations: bool,
        lpac_opt_out: bool,
    ) -> Result<SandboxReceipt> {
        let mut token_raw = null_mut();
        let ok = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token_raw) };
        if ok == 0 {
            bail!("OpenProcessToken failed: Win32 error {}", unsafe {
                GetLastError()
            });
        }
        let token = Handle::new(token_raw, "process token")?;
        let app_container = get_token_u32(token.raw(), TokenIsAppContainer)? != 0;
        let zero_capabilities = capability_count(token.raw())? == 0;

        let mut in_job = 0;
        let ok = unsafe { IsProcessInJob(process, job, &mut in_job) };
        if ok == 0 {
            bail!("IsProcessInJob failed: Win32 error {}", unsafe {
                GetLastError()
            });
        }

        let win32k = mitigation_flags(process, ProcessSystemCallDisablePolicy)?;
        let extension = mitigation_flags(process, ProcessExtensionPointDisablePolicy)?;
        let strict = mitigation_flags(process, ProcessStrictHandleCheckPolicy)?;
        let child = mitigation_flags(process, ProcessChildProcessPolicy)?;

        if !app_container
            || !zero_capabilities
            || in_job == 0
            || child & 1 == 0
            || (require_strict_mitigations
                && (win32k & 1 == 0 || extension & 1 == 0 || strict & 1 == 0))
        {
            bail!(
                "containment receipt incomplete: app_container={app_container} zero_caps={zero_capabilities} in_job={in_job} win32k={win32k:#x} extension={extension:#x} strict={strict:#x} child={child:#x}"
            );
        }

        Ok(SandboxReceipt {
            schema_version: "chaptera-desktop-pub-containment-receipt-v1",
            app_container,
            zero_capabilities,
            lpac_all_application_packages_opt_out: lpac_opt_out,
            exact_handle_allowlist_count: 3,
            child_process_restricted: true,
            job_member_at_creation: in_job != 0,
            active_process_limit: 1,
            process_memory_limit_bytes: DEFAULT_PROCESS_MEMORY_BYTES,
            cpu_rate_percent: DEFAULT_CPU_RATE_PERCENT,
            kill_on_job_close: true,
            win32k_system_calls_disabled: win32k & 1 != 0,
            extension_points_disabled: extension & 1 != 0,
            strict_handle_checks: strict & 1 != 0,
            timed_out: false,
            exit_code: u32::MAX,
        })
    }

    fn read_bounded(mut file: File, label: &'static str) -> thread::JoinHandle<Result<Vec<u8>>> {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            std::io::Read::by_ref(&mut file)
                .take(MAX_CAPTURE_BYTES + 1)
                .read_to_end(&mut bytes)
                .with_context(|| format!("read {label}"))?;
            if bytes.len() as u64 > MAX_CAPTURE_BYTES {
                bail!("{label} exceeded bounded capture limit");
            }
            Ok(bytes)
        })
    }

    pub fn launch_contained(
        executable: &Path,
        request: &[u8],
        timeout: Duration,
    ) -> Result<SandboxOutput> {
        rappct::supports_lpac().context("LPAC support is required")?;

        let executable = executable
            .canonicalize()
            .with_context(|| format!("canonicalize {}", executable.display()))?;
        if !executable.is_absolute() || !executable.is_file() {
            bail!("contained executable must be an existing absolute file");
        }
        let cwd = executable
            .parent()
            .ok_or_else(|| anyhow::anyhow!("contained executable has no parent"))?
            .to_path_buf();

        let profile = AppContainerProfile::ensure(
            PROFILE_NAME,
            PROFILE_DISPLAY,
            Some("Chaptera source-path-free desktop PUB parser"),
        )
        .context("ensure AppContainer profile")?;

        let access = AccessMask(FILE_GENERIC_READ | FILE_GENERIC_EXECUTE);
        grant_to_package(ResourcePath::Directory(cwd.clone()), &profile.sid, access)
            .context("grant package read/execute to worker directory")?;
        grant_to_package(ResourcePath::File(executable.clone()), &profile.sid, access)
            .context("grant package read/execute to worker executable")?;

        let sid = derive_sid(&profile.name)?;
        let security_capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: sid.0,
            Capabilities: null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };

        let job = configure_job()?;
        let pipes = make_pipe_set()?;

        let lpac_policy = PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
        let mitigation_policy = MITIGATION_STRICT_HANDLE_ALWAYS_ON
            | MITIGATION_WIN32K_DISABLE_ALWAYS_ON
            | MITIGATION_EXTENSION_POINT_DISABLE_ALWAYS_ON;
        let require_strict_mitigations = true;
        let child_policy = PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;
        let handle_list = [
            pipes.child_stdin.raw(),
            pipes.child_stdout.raw(),
            pipes.child_stderr.raw(),
        ];
        let job_list = [job.raw()];

        let mut attributes = AttributeList::new(6)?;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            &security_capabilities,
        )?;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            &lpac_policy,
        )?;
        attributes.set_slice(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handle_list)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, &child_policy)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY, &mitigation_policy)?;
        attributes.set_slice(PROC_THREAD_ATTRIBUTE_JOB_LIST, &job_list)?;

        let executable_w = wide(executable.as_os_str());
        let cwd_w = wide(cwd.as_os_str());
        let mut command_line = wide(OsStr::new(&format!("\"{}\"", executable.display())));
        let environment = minimal_environment(&cwd);
        let cwd_ptr = cwd_w.as_ptr();

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = pipes.child_stdin.raw();
        startup.StartupInfo.hStdOutput = pipes.child_stdout.raw();
        startup.StartupInfo.hStdError = pipes.child_stderr.raw();
        startup.lpAttributeList = attributes.raw();

        let mut process_info: PROCESS_INFORMATION = unsafe { zeroed() };
        let creation_flags =
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | DETACHED_PROCESS;
        let ok = unsafe {
            CreateProcessW(
                executable_w.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                creation_flags,
                environment.as_ptr().cast(),
                cwd_ptr,
                &startup.StartupInfo,
                &mut process_info,
            )
        };
        if ok == 0 {
            bail!("CreateProcessW failed closed: Win32 error {}", unsafe {
                GetLastError()
            });
        }

        let process = Handle::new(process_info.hProcess, "process handle")?;
        let thread_handle = Handle::new(process_info.hThread, "thread handle")?;
        drop(thread_handle);

        drop(pipes.child_stdin);
        drop(pipes.child_stdout);
        drop(pipes.child_stderr);

        let mut receipt = inspect_process(
            process.raw(),
            job.raw(),
            require_strict_mitigations,
            lpac_policy == PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT,
        )?;

        let mut stdin = unsafe { pipes.parent_stdin.into_file() };
        let stdout = unsafe { pipes.parent_stdout.into_file() };
        let stderr = unsafe { pipes.parent_stderr.into_file() };

        let stdout_thread = read_bounded(stdout, "worker stdout");
        let stderr_thread = read_bounded(stderr, "worker stderr");

        stdin.write_all(request).context("write worker request")?;
        drop(stdin);

        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        let wait = unsafe { WaitForSingleObject(process.raw(), timeout_ms) };
        if wait == WAIT_TIMEOUT_VALUE {
            receipt.timed_out = true;
            unsafe {
                TerminateJobObject(job.raw(), 124);
                WaitForSingleObject(process.raw(), 5_000);
            }
        } else if wait != WAIT_OBJECT_0_VALUE {
            bail!("WaitForSingleObject failed: result {wait:#x}");
        }

        let mut exit_code = 0u32;
        let ok = unsafe { GetExitCodeProcess(process.raw(), &mut exit_code) };
        if ok == 0 {
            bail!("GetExitCodeProcess failed: Win32 error {}", unsafe {
                GetLastError()
            });
        }
        receipt.exit_code = exit_code;

        let stdout = stdout_thread
            .join()
            .map_err(|_| anyhow::anyhow!("worker stdout reader panicked"))??;
        let stderr = stderr_thread
            .join()
            .map_err(|_| anyhow::anyhow!("worker stderr reader panicked"))??;

        if receipt.timed_out {
            bail!("contained worker exceeded wall timeout");
        }

        Ok(SandboxOutput {
            stdout,
            stderr,
            receipt,
        })
    }
}
