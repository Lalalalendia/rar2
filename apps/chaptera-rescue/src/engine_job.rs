//! A GUI-owned Windows Job Object: the engine and its descendants die if the
//! desktop exits, including a hard crash. The handle is deliberately NOT
//! inherited by children. The Python engine may create a nested memory-capped
//! job independently; this job owns lifetime, not memory policy.
#![cfg(windows)]

use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;
use std::ptr;

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
}

/// The GUI alone owns this non-inheritable handle. Dropping it terminates the
/// attached batch engine and all children inherited into its nested job tree.
pub struct EngineJob(OwnedHandle);

impl EngineJob {
    pub fn attach(child: &Child) -> Result<Self, String> {
        // Null security attributes create a non-inheritable handle; a handle
        // inherited by the engine would defeat KILL_ON_JOB_CLOSE on GUI crash.
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
        if unsafe { AssignProcessToJobObject(job.0.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err("Cannot attach batch engine to GUI lifetime Job Object".to_owned());
        }
        Ok(job)
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
    fn closing_gui_job_reaps_nested_python_engine_and_worker() {
        // This is a physical Windows test, not an ABI mock. The Python engine
        // joins its separate 512 MiB Job and creates a grandchild. Closing the
        // GUI-owned outer Job must kill both. No private PUB bytes are involved.
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let ready = std::env::temp_dir().join(format!(
            "rescue-gui-job-{}-{nonce}.ready",
            std::process::id()
        ));
        let code = concat!(
            "import pathlib, subprocess, sys, time\n",
            "from tools import rescue_windows_job_guard as g\n",
            "assert g.activate(512)\n",
            "child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\n",
            "pathlib.Path(sys.argv[1]).write_text(str(child.pid))\n",
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
            .expect("launch nested Windows Python guard");
        let job = EngineJob::attach(&engine).expect("nested Windows GUI/engine jobs");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() && Instant::now() < deadline {
            assert!(
                engine.try_wait().unwrap().is_none(),
                "nested engine crashed"
            );
            thread::sleep(Duration::from_millis(25));
        }
        assert!(ready.is_file(), "Python engine never spawned the worker");
        let worker_pid: u32 = fs::read_to_string(&ready)
            .expect("child pid")
            .parse()
            .expect("numeric PID");
        drop(job);
        let deadline = Instant::now() + Duration::from_secs(5);
        while engine.try_wait().unwrap().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            engine.try_wait().unwrap().is_some(),
            "GUI close left engine alive"
        );

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, id: u32) -> *mut c_void;
            fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        let handle = unsafe { OpenProcess(0x0010_0000, 0, worker_pid) }; // SYNCHRONIZE
        if !handle.is_null() {
            assert_eq!(
                unsafe { WaitForSingleObject(handle, 5000) },
                0,
                "GUI close left a salvage grandchild alive"
            );
            unsafe { CloseHandle(handle) };
        }
        let _ = fs::remove_file(ready);
    }
}
