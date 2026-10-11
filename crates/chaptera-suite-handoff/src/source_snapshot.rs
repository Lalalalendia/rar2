//! One short Windows stable-read window, followed by same-object handle retention.
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub(crate) fn open_snapshot_file(path: &Path) -> io::Result<File> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Existing writers/deleters are incompatible; later ones cannot change this read.
        const FILE_SHARE_READ: u32 = 1;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
    }
    #[cfg(not(target_os = "windows"))]
    File::open(path)
}

pub(crate) fn read_bounded_snapshot(
    reader: &mut impl Read,
    max_bytes: u64,
    mut observer: impl FnMut(usize),
) -> io::Result<Vec<u8>> {
    let mut limited = reader.take(max_bytes.saturating_add(1));
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = match limited.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        observer(bytes.len());
    }
    Ok(bytes)
}

pub(crate) fn retain_admitted_handle(file: File) -> io::Result<File> {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::c_void;
        use std::os::windows::io::{AsRawHandle, FromRawHandle};

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn ReOpenFile(
                original: *mut c_void,
                access: u32,
                share: u32,
                flags: u32,
            ) -> *mut c_void;
        }

        const GENERIC_READ: u32 = 0x8000_0000;
        const SHARE_READ_WRITE_DELETE: u32 = 1 | 2 | 4;
        // SAFETY: file owns a live CreateFile handle. ReOpenFile binds that same object.
        let handle = unsafe {
            ReOpenFile(
                file.as_raw_handle(),
                GENERIC_READ,
                SHARE_READ_WRITE_DELETE,
                0,
            )
        };
        if handle == -1_isize as *mut c_void {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a successful ReOpenFile transfers a unique owned handle to this File.
        let retained = unsafe { File::from_raw_handle(handle) };
        drop(file);
        Ok(retained)
    }
    #[cfg(not(target_os = "windows"))]
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn source_read_keeps_only_one_sentinel_and_observes_materialized_bytes() {
        let mut source = Cursor::new(vec![0x5a; 200_000]);
        let mut observed = Vec::new();
        let bytes = read_bounded_snapshot(&mut source, 100_000, |n| observed.push(n))
            .expect("bounded read");
        assert_eq!(bytes.len(), 100_001);
        assert_eq!(source.position(), 100_001);
        assert_eq!(observed, vec![65_536, 100_001]);
    }

    #[test]
    fn empty_exact_and_short_source_reads_preserve_bytes() {
        for len in [0, 3, 8] {
            let expected = vec![0x42; len];
            let bytes = read_bounded_snapshot(&mut Cursor::new(&expected), 8, |_| {})
                .expect("bounded source");
            assert_eq!(bytes, expected);
        }
    }

    #[test]
    fn interrupted_read_is_retried_without_admitting_extra_bytes() {
        struct InterruptedOnce {
            interrupted: bool,
            bytes: Cursor<Vec<u8>>,
        }
        impl Read for InterruptedOnce {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                self.bytes.read(buffer)
            }
        }
        let mut source = InterruptedOnce {
            interrupted: false,
            bytes: Cursor::new(b"abc".to_vec()),
        };
        assert_eq!(
            read_bounded_snapshot(&mut source, 2, |_| {}).unwrap(),
            b"abc"
        );
    }
}
