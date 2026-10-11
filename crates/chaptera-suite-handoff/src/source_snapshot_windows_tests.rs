use super::*;
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "chaptera-source-snapshot-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn source(&self, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("source.pub");
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn emit_proof(case: &str, facts: serde_json::Value) {
    if let Some(directory) = std::env::var_os("CHAPTERA_SOURCE_SNAPSHOT_PROOF_DIR") {
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory).unwrap();
        let receipt = serde_json::json!({
            "schema": "chaptera.win-source-snapshot.v1",
            "case": case,
            "git_commit": std::env::var("CHAPTERA_CANDIDATE_SHA").ok(),
            "facts": facts,
            "source_bytes_serialized": false,
            "source_path_serialized": false,
        });
        fs::write(directory.join(format!("{case}.json")), receipt.to_string()).unwrap();
    }
}

#[test]
fn windows_preexisting_writer_is_source_busy_then_retry_admits_new_version() {
    let fixture = Fixture::new("busy");
    let source = fixture.source(b"old source");
    let mut writer = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(1 | 2 | 4)
        .open(&source)
        .unwrap();
    let error = AdmittedSource::open(&source).expect_err("active writer cannot admit snapshot");
    assert!(error.starts_with("source_busy:"), "{error}");
    writer.set_len(0).unwrap();
    std::io::Write::write_all(&mut writer, b"complete new version").unwrap();
    writer.sync_all().unwrap();
    drop(writer);
    let admitted = AdmittedSource::open(&source).expect("retry after writer closes");
    assert_eq!(admitted.bytes(), b"complete new version");
    assert_eq!(admitted.sha256(), sha256_bytes(b"complete new version"));
    emit_proof(
        "busy_writer",
        serde_json::json!({
            "existing_writer_rejected_as_source_busy": true,
            "retry_admitted_exact_new_version": true,
        }),
    );
}

#[test]
fn windows_preexisting_writable_mapping_is_rejected_before_read() {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileMappingW(
            file: *mut c_void,
            attributes: *const c_void,
            protect: u32,
            size_high: u32,
            size_low: u32,
            name: *const u16,
        ) -> *mut c_void;
        fn MapViewOfFile(
            mapping: *mut c_void,
            access: u32,
            offset_high: u32,
            offset_low: u32,
            bytes: usize,
        ) -> *mut c_void;
        fn UnmapViewOfFile(base: *const c_void) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    struct Mapping {
        handle: *mut c_void,
        view: *mut c_void,
    }
    impl Drop for Mapping {
        fn drop(&mut self) {
            // SAFETY: these are the unique successful mapping/view handles owned below.
            unsafe {
                if !self.view.is_null() {
                    UnmapViewOfFile(self.view);
                }
                CloseHandle(self.handle);
            }
        }
    }

    let fixture = Fixture::new("mapped-writer");
    let source = fixture.source(&vec![0x41; 4096]);
    let writer = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(1 | 2 | 4)
        .open(&source)
        .unwrap();
    // SAFETY: writer is live and readable/writable; null pointers mean no name/attributes.
    let handle = unsafe {
        CreateFileMappingW(
            writer.as_raw_handle(),
            std::ptr::null(),
            4,
            0,
            0,
            std::ptr::null(),
        )
    };
    assert!(!handle.is_null(), "{}", std::io::Error::last_os_error());
    let mut mapping = Mapping {
        handle,
        view: std::ptr::null_mut(),
    };
    // SAFETY: mapping owns a live PAGE_READWRITE section with a 4096-byte backing file.
    mapping.view = unsafe { MapViewOfFile(handle, 2, 0, 0, 4096) };
    assert!(
        !mapping.view.is_null(),
        "{}",
        std::io::Error::last_os_error()
    );
    drop(writer); // The writable view remains after the ordinary writer handle closes.
    let error = AdmittedSource::open(&source).expect_err("writable mapping cannot admit snapshot");
    assert!(error.starts_with("source_busy:"), "{error}");
    drop(mapping);
    let admitted = AdmittedSource::open(&source).expect("retry after writable view closes");
    assert_eq!(admitted.bytes(), vec![0x41; 4096]);
    emit_proof(
        "mapped_writer",
        serde_json::json!({
            "writable_view_survived_original_writer_handle": true,
            "writable_mapping_rejected_as_source_busy": true,
            "retry_after_unmap_admitted_exact_bytes": true,
        }),
    );
}

// The parent invokes this same test executable as a separate process during a
// paused real admission read. An ordinary harness invocation has no mutation job.
#[test]
fn windows_external_mutator_helper() {
    let Some(source) = std::env::var_os("CHAPTERA_SOURCE_SNAPSHOT_MUTATOR_SOURCE") else {
        return;
    };
    let source = PathBuf::from(source);
    let output =
        PathBuf::from(std::env::var_os("CHAPTERA_SOURCE_SNAPSHOT_MUTATOR_RECEIPT").unwrap());
    let write = OpenOptions::new().write(true).open(&source).unwrap_err();
    let truncate = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&source)
        .unwrap_err();
    let rename = fs::rename(&source, source.with_extension("renamed.pub")).unwrap_err();
    for error in [&write, &truncate, &rename] {
        assert_eq!(
            error.raw_os_error(),
            Some(32),
            "expected sharing violation: {error}"
        );
    }
    fs::write(
        output,
        serde_json::json!({
            "separate_process": true,
            "write_blocked": true,
            "truncate_blocked": true,
            "rename_blocked": true,
            "windows_error_codes": [32, 32, 32],
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn windows_slow_source_admission_blocks_external_write_truncate_and_rename() {
    let fixture = Fixture::new("external-race");
    let original: Vec<u8> = (0..262_144).map(|n| (n % 251) as u8).collect();
    let source = fixture.source(&original);
    let child_receipt = fixture.0.join("child.json");
    let mut proven = false;
    let admitted = AdmittedSource::open_with_read_observer(&source, |read| {
        if proven {
            return;
        }
        assert!(
            read > 0 && read < original.len(),
            "pause inside the real read window"
        );
        // Read-only observers remain legal while mutation is excluded.
        let reader = File::open(&source).expect("concurrent read is admitted");
        drop(reader);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_snapshot_windows_tests::windows_external_mutator_helper",
                "--nocapture",
            ])
            .env("CHAPTERA_SOURCE_SNAPSHOT_MUTATOR_SOURCE", &source)
            .env("CHAPTERA_SOURCE_SNAPSHOT_MUTATOR_RECEIPT", &child_receipt)
            .spawn()
            .expect("launch independent Windows mutator");
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "external mutation proof failed");
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("external mutator exceeded bounded timeout");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(&child_receipt).unwrap()).unwrap();
        for flag in [
            "separate_process",
            "write_blocked",
            "truncate_blocked",
            "rename_blocked",
        ] {
            assert_eq!(receipt[flag], true, "missing actual child proof {flag}");
        }
        proven = true;
    })
    .expect("stable source read completes");
    assert!(proven, "external writer must actually run during admission");
    assert_eq!(admitted.bytes(), original);
    assert_eq!(admitted.sha256(), sha256_bytes(&original));
    assert_eq!(fs::read(&source).unwrap(), original);
    emit_proof(
        "external_mutator",
        serde_json::json!({
            "separate_process": true,
            "paused_after_first_chunk_before_eof": true,
            "write_blocked": true,
            "truncate_blocked": true,
            "rename_blocked": true,
            "read_only_observer_allowed": true,
            "snapshot_equals_exact_original": true,
            "source_unchanged": true,
        }),
    );
}

#[test]
fn windows_post_window_retention_releases_lock_without_rebinding_source_path() {
    let fixture = Fixture::new("retain-handle");
    let original = b"original admitted source";
    let source = fixture.source(original);
    let admitted = AdmittedSource::open(&source).expect("stable admission");
    let moved = fixture.0.join("original-moved.pub");
    fs::rename(&source, &moved).expect("short delete exclusion is released");
    fs::write(&source, b"different object now owns the path").unwrap();
    assert_eq!(admitted.bytes(), original);
    assert_eq!(
        admitted.rehash_opened_file().unwrap(),
        sha256_bytes(original)
    );
    fs::write(
        &moved,
        b"original object changed after the admission window",
    )
    .expect("short write exclusion is released");
    assert_ne!(admitted.rehash_opened_file().unwrap(), admitted.sha256());
    emit_proof(
        "post_window_handle",
        serde_json::json!({
            "post_admission_rename_allowed": true,
            "post_admission_write_allowed": true,
            "retained_handle_rehashes_original_object": true,
            "replacement_path_did_not_rebind_identity": true,
            "post_window_original_mutation_detected": true,
        }),
    );
}
