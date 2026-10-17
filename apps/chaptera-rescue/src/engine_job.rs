//! A process-wide Windows GUI Job: every subprocess inherits containment at
//! CreateProcess time, before arbitrary worker code can execute. Its handle
//! stays non-inheritable and alive until OS process exit; the Python engine
//! creates a nested memory-bounded Job and cleans its own workers on exit.
#![cfg(windows)]

use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::sync::Mutex;

// Statics are not dropped at process shutdown. Keeping the GUI job here
// ensures its LAST handle closes only when Windows tears down the GUI process.
// Dropping a Job containing our own live process would kill the GUI itself.
static GUI_LIFETIME_JOB: Mutex<Option<EngineJob>> = Mutex::new(None);

const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

#[repr(C)]
#[derive(Default)]
struct BasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[repr(C)]
#[derive(Default)]
struct ExtendedLimitInformation {
    basic_limit_information: BasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> *mut c_void;
    fn SetInformationJobObject(
        job: *mut c_void,
        info_class: i32,
        info: *const c_void,
        info_length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}

/// Non-inheritable GUI lifetime handle, stored in a static until process exit.
/// Never close it while the GUI is running: the GUI belongs to this Job too.
pub struct EngineJob(OwnedHandle);

impl EngineJob {
    fn create() -> Result<Self, String> {
        // NULL security attributes => the job handle is NOT inheritable by
        // the engine or any worker; only the GUI keeps it open.
        let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if raw.is_null() {
            return Err("Cannot establish GUI lifetime Job Object".to_owned());
        }
        let job = Self(unsafe { OwnedHandle::from_raw_handle(raw) });
        let mut limits = ExtendedLimitInformation::default();
        limits.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&raw const limits).cast::<c_void>(),
                size_of::<ExtendedLimitInformation>() as u32,
            )
        };
        if set == 0 {
            return Err("Cannot configure GUI lifetime Job Object".to_owned());
        }
        Ok(job)
    }

    /// Install before starting eframe. Every subsequent CreateProcess child
    /// inherits this job AT CREATION, removing the spawn->attach window. This
    /// is a process-lifetime guard; it must never be dropped while GUI runs.
    ///
    /// The GUI may itself already belong to an outer Windows Job (e.g. a CI
    /// runner); Windows 10/11 allow a nested child Job in that case.
    pub fn protect_gui_process() -> Result<(), String> {
        let mut keep = GUI_LIFETIME_JOB
            .lock()
            .map_err(|_| "GUI lifetime Job lock failed".to_owned())?;
        if keep.is_some() {
            return Ok(());
        }
        let job = Self::create()?;
        if unsafe { AssignProcessToJobObject(job.0.as_raw_handle(), GetCurrentProcess()) } == 0 {
            return Err("Cannot assign GUI itself to lifetime Job".to_owned());
        }
        *keep = Some(job);
        Ok(())
    }

    /// Fail closed if another entrypoint tries launching BatchUi without
    /// first activating the process-wide Windows lifetime protection.
    pub fn gui_is_guarded() -> bool {
        GUI_LIFETIME_JOB
            .lock()
            .map(|state| state.is_some())
            .unwrap_or(false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn layout_matches_win64_job_object_abi() {
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size_of::<ExtendedLimitInformation>(), 144);
    }

    #[test]
    fn helper_process_installs_early_gui_guard() {
        // This helper runs in its own Rust test process, NEVER in the parent
        // cargo test process. An early guard would kill other tests on close.
        let Some(ready) = std::env::var_os("RESCUE_GUI_JOB_HELPER_READY") else {
            return;
        };
        EngineJob::protect_gui_process().expect("guard before any worker exists");
        EngineJob::protect_gui_process().expect("idempotent same-process install");
        let ready = std::path::PathBuf::from(ready);
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let code = concat!(
            "import os,pathlib,subprocess,sys,time\n",
            "from tools import rescue_windows_job_guard as guard\n",
            "assert guard.activate(512)\n",
            "worker=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'],",
            "stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)\n",
            "pathlib.Path(sys.argv[1]).write_text(str(os.getpid())+' '+str(worker.pid))\n",
            "time.sleep(30)\n",
        );
        let mut engine = Command::new("python")
            .args(["-u", "-c", code])
            .arg(&ready)
            .current_dir(repo)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn engine as a GUI Job member");

        // Prove membership *at spawn time*, without attaching this child to
        // any other GUI per-batch Job. This tests the original race boundary.
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        }
        let holder = GUI_LIFETIME_JOB.lock().expect("GUI Job holder");
        let job = holder.as_ref().expect("installed GUI Job");
        let mut inside: i32 = 0;
        assert_ne!(
            unsafe {
                IsProcessInJob(engine.as_raw_handle(), job.0.as_raw_handle(), &raw mut inside)
            },
            0,
            "IsProcessInJob failed"
        );
        assert_eq!(inside, 1, "engine was not inherited into GUI Job at spawn");
        drop(holder);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.is_file() && Instant::now() < deadline {
            assert!(engine.try_wait().unwrap().is_none(), "engine exited before child spawn");
            thread::sleep(Duration::from_millis(25));
        }
        assert!(ready.is_file(), "nested worker never launched");
        // Helper exits normally. OS closes its static GUI job handle, killing
        // both nested engine and worker. Child handles do not keep Job alive.
    }

    #[test]
    fn gui_exit_reaps_children_inherited_before_per_batch_attachment() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let ready = std::env::temp_dir().join(format!(
            "rescue-gui-early-{}-{nonce}.ready",
            std::process::id()
        ));
        // Run the test helper as a fresh process so the GUI-level Job
        // membership is scoped to that helper's lifetime only.
        let status = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "engine_job::tests::helper_process_installs_early_gui_guard",
                "--nocapture",
            ])
            .env("RESCUE_GUI_JOB_HELPER_READY", &ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("start GUI fixture subprocess");
        assert!(status.success(), "GUI fixture failed to install early Job");
        let pids = fs::read_to_string(&ready).expect("child PID receipt");
        let pids: Vec<u32> = pids
            .split_whitespace()
            .map(|pid| pid.parse().expect("numeric process ID"))
            .collect();
        assert_eq!(pids.len(), 2);
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, id: u32) -> *mut c_void;
            fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        for pid in pids {
            let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) };
            if !handle.is_null() {
                assert_eq!(
                    unsafe { WaitForSingleObject(handle, 5000) },
                    0,
                    "GUI process exit left inherited engine/worker alive"
                );
                unsafe { CloseHandle(handle) };
            }
        }
        let _ = fs::remove_file(ready);
    }


}
