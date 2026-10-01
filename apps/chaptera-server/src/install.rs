use std::{
    env,
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
};

use sha2::{Digest, Sha256};

use crate::{build_info::BUILD_IDENTITY, config::ChapteraConfig};

const CHAPTERA_SLICE: &str = include_str!("../../../deploy/systemd/chaptera.slice");
const CHAPTERA_WEB_SERVICE: &str = include_str!("../../../deploy/systemd/chaptera-web.service");
const CHAPTERA_WORKER_SERVICE: &str =
    include_str!("../../../deploy/systemd/chaptera-worker.service");
const CHAPTERA_TARGET: &str = include_str!("../../../deploy/systemd/chaptera.target");
const CHAPTERA_TMPFILES: &str = include_str!("../../../deploy/tmpfiles/chaptera.conf");
const CHAPTERA_SYSUSERS: &str = include_str!("../../../deploy/sysusers/chaptera.conf");
const ISOLATION_HARNESS: &[u8] = include_bytes!("../../../tools/migration_pdf_worker_isolation.py");

const CANONICAL_CONFIG: &str = "/etc/chaptera/chaptera.toml";

const HOST_FILES: [(&str, &str, u32); 6] = [
    ("/etc/systemd/system/chaptera.slice", CHAPTERA_SLICE, 0o644),
    (
        "/etc/systemd/system/chaptera-web.service",
        CHAPTERA_WEB_SERVICE,
        0o644,
    ),
    (
        "/etc/systemd/system/chaptera-worker.service",
        CHAPTERA_WORKER_SERVICE,
        0o644,
    ),
    (
        "/etc/systemd/system/chaptera.target",
        CHAPTERA_TARGET,
        0o644,
    ),
    ("/etc/tmpfiles.d/chaptera.conf", CHAPTERA_TMPFILES, 0o644),
    ("/etc/sysusers.d/chaptera.conf", CHAPTERA_SYSUSERS, 0o644),
];

pub fn run(config_path: Option<&Path>, staging_root: Option<&Path>) -> Result<(), Box<dyn Error>> {
    ensure_linux()?;

    let config_path = config_path.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "install requires --config /etc/chaptera/chaptera.toml",
        )
    })?;
    ChapteraConfig::load(config_path)?;

    let (root, staging) = match staging_root {
        Some(root) => {
            validate_staging_root(root)?;
            (root, true)
        }
        None => (Path::new("/"), false),
    };

    if !staging {
        if config_path != Path::new(CANONICAL_CONFIG) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "host install requires the canonical config path {CANONICAL_CONFIG}; use --root for CI/staging acceptance"
                ),
            )
            .into());
        }
        preflight_systemd_host()?;
    }

    let source_exe = env::current_exe().map_err(|error| {
        io::Error::other(format!(
            "cannot resolve running chaptera executable for self-install: {error}"
        ))
    })?;

    install_from(&source_exe, root, BUILD_IDENTITY, staging)?;

    if staging {
        println!(
            "chaptera install: staged build {BUILD_IDENTITY} under {}",
            root.display()
        );
    } else {
        println!(
            "chaptera install: installed build {BUILD_IDENTITY}; start with: systemctl enable --now chaptera.target"
        );
    }
    Ok(())
}

fn install_from(
    source_exe: &Path,
    root: &Path,
    build_id: &str,
    staging: bool,
) -> Result<(), Box<dyn Error>> {
    validate_build_id(build_id)?;

    let release_dir = rooted(root, &format!("/opt/chaptera/releases/{build_id}"))?;
    fs::create_dir_all(&release_dir)?;
    set_mode(&release_dir, 0o755)?;

    let installed_exe = release_dir.join("chaptera");
    install_exact_executable(source_exe, &installed_exe)?;
    atomic_write(
        &release_dir.join("tools/migration_pdf_worker_isolation.py"),
        ISOLATION_HARNESS,
        0o755,
    )?;

    for (absolute_path, contents, mode) in HOST_FILES {
        let path = rooted(root, absolute_path)?;
        atomic_write(&path, contents.as_bytes(), mode)?;
    }

    if !staging {
        run_checked("systemd-sysusers", &["/etc/sysusers.d/chaptera.conf"])?;
        run_checked(
            "systemd-tmpfiles",
            &["--create", "/etc/tmpfiles.d/chaptera.conf"],
        )?;
        run_checked("systemctl", &["daemon-reload"])?;
    }

    switch_current(root, build_id)?;
    Ok(())
}

fn ensure_linux() -> io::Result<()> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "chaptera install is supported only on Linux systemd hosts",
        ))
    }
}

fn validate_staging_root(root: &Path) -> io::Result<()> {
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--root must be an absolute staging directory",
        ));
    }
    if root == Path::new("/") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--root / is forbidden; omit --root for a real host install",
        ));
    }
    Ok(())
}

fn preflight_systemd_host() -> io::Result<()> {
    if !Path::new("/run/systemd/system").is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "systemd is not the active host init; refusing to install",
        ));
    }

    for command in ["systemctl", "systemd-tmpfiles", "systemd-sysusers"] {
        let status = ProcessCommand::new(command)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("required host command {command} is unavailable: {error}"),
                )
            })?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "required host command {command} failed its preflight"
            )));
        }
    }
    Ok(())
}

fn run_checked(program: &str, args: &[&str]) -> io::Result<()> {
    let status = ProcessCommand::new(program)
        .args(args)
        .stdin(Stdio::null())
        .status()
        .map_err(|error| io::Error::other(format!("failed to execute {program}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{program} exited unsuccessfully while installing Chaptera"
        )))
    }
}

fn validate_build_id(build_id: &str) -> io::Result<()> {
    if build_id.is_empty()
        || !build_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-' | b'_'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "build identity is not safe as a release directory name",
        ));
    }
    Ok(())
}

fn rooted(root: &Path, absolute: &str) -> io::Result<PathBuf> {
    let path = Path::new(absolute);
    let relative = path.strip_prefix("/").map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("install path must be absolute: {absolute}"),
        )
    })?;
    Ok(root.join(relative))
}

fn install_exact_executable(source: &Path, destination: &Path) -> io::Result<()> {
    let source_metadata = fs::metadata(source).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot stat running executable {}: {error}",
                source.display()
            ),
        )
    })?;
    if !source_metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "running executable {} is not a regular file",
                source.display()
            ),
        ));
    }

    if destination.exists() {
        let destination_metadata = fs::metadata(destination)?;
        if destination_metadata.len() == source_metadata.len()
            && sha256_file(destination)? == sha256_file(source)?
        {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "immutable release {} already exists with different bytes",
                destination.display()
            ),
        ));
    }

    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "release executable has no parent: {}",
                destination.display()
            ),
        )
    })?;
    fs::create_dir_all(parent)?;

    let temp = temp_sibling(destination)?;
    remove_stale_temp(&temp)?;
    fs::copy(source, &temp)?;
    set_mode(&temp, 0o755)?;
    OpenOptions::new().write(true).open(&temp)?.sync_all()?;
    fs::rename(&temp, destination)?;
    sync_directory(parent)?;
    Ok(())
}

fn sha256_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    if let Ok(existing) = fs::read(path) {
        if existing == contents {
            set_mode(path, mode)?;
            return Ok(());
        }
    }

    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("install file has no parent: {}", path.display()),
        )
    })?;
    fs::create_dir_all(parent)?;
    let temp = temp_sibling(path)?;
    remove_stale_temp(&temp)?;
    fs::write(&temp, contents)?;
    set_mode(&temp, mode)?;
    OpenOptions::new().write(true).open(&temp)?.sync_all()?;
    fs::rename(&temp, path)?;
    sync_directory(parent)?;
    Ok(())
}

fn switch_current(root: &Path, build_id: &str) -> io::Result<()> {
    let chaptera_root = rooted(root, "/opt/chaptera")?;
    fs::create_dir_all(&chaptera_root)?;
    let current = chaptera_root.join("current");
    let previous = chaptera_root.join("previous");
    let desired = PathBuf::from(format!("releases/{build_id}"));

    let prior = match fs::symlink_metadata(&current) {
        Ok(metadata) => {
            if !metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{} exists but is not a symlink; refusing to replace it",
                        current.display()
                    ),
                ));
            }
            Some(fs::read_link(&current)?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    if prior.as_ref() == Some(&desired) {
        return Ok(());
    }

    if let Some(prior) = prior {
        atomic_symlink(&previous, &prior)?;
    }
    atomic_symlink(&current, &desired)?;
    sync_directory(&chaptera_root)?;
    Ok(())
}

fn temp_sibling(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path has no parent: {}", path.display()),
        )
    })?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-UTF-8 install path"))?;
    Ok(parent.join(format!(".{name}.tmp-{}", std::process::id())))
}

fn remove_stale_temp(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("temporary install path is a directory: {}", path.display()),
        )),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn atomic_symlink(path: &Path, target: &Path) -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("symlink path has no parent: {}", path.display()),
        )
    })?;
    fs::create_dir_all(parent)?;
    let temp = temp_sibling(path)?;
    remove_stale_temp(&temp)?;
    symlink(target, &temp)?;
    fs::rename(&temp, path)?;
    sync_directory(parent)
}

#[cfg(not(unix))]
fn atomic_symlink(_path: &Path, _target: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic release symlinks require a Unix host",
    ))
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("chaptera-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn source(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = root.join(name);
        fs::write(&path, bytes).unwrap();
        set_mode(&path, 0o755).unwrap();
        path
    }

    #[test]
    fn staging_install_is_idempotent_and_upgrade_records_previous() {
        let root = test_root("upgrade");
        let source_a = source(&root, "chaptera-a", b"chaptera-build-a");
        let source_b = source(&root, "chaptera-b", b"chaptera-build-b");

        install_from(&source_a, &root, "1.0.0+aaaa", true).unwrap();
        install_from(&source_a, &root, "1.0.0+aaaa", true).unwrap();

        let current = root.join("opt/chaptera/current");
        assert_eq!(
            fs::read_link(&current).unwrap(),
            PathBuf::from("releases/1.0.0+aaaa")
        );
        assert!(!root.join("opt/chaptera/previous").exists());

        install_from(&source_b, &root, "1.0.1+bbbb", true).unwrap();

        assert_eq!(
            fs::read_link(&current).unwrap(),
            PathBuf::from("releases/1.0.1+bbbb")
        );
        assert_eq!(
            fs::read_link(root.join("opt/chaptera/previous")).unwrap(),
            PathBuf::from("releases/1.0.0+aaaa")
        );
        assert_eq!(
            fs::read(root.join("opt/chaptera/current/chaptera")).unwrap(),
            b"chaptera-build-b"
        );
        assert_eq!(
            fs::read(root.join("opt/chaptera/current/tools/migration_pdf_worker_isolation.py"))
                .unwrap(),
            ISOLATION_HARNESS
        );
        assert_eq!(
            fs::read(root.join("etc/systemd/system/chaptera.target")).unwrap(),
            CHAPTERA_TARGET.as_bytes()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn conflicting_immutable_release_does_not_move_current() {
        let root = test_root("conflict");
        let source_a = source(&root, "chaptera-a", b"chaptera-build-a");
        let source_b = source(&root, "chaptera-b", b"chaptera-build-b");

        install_from(&source_a, &root, "1.0.0+aaaa", true).unwrap();
        let error = install_from(&source_b, &root, "1.0.0+aaaa", true).unwrap_err();
        assert!(error.to_string().contains("immutable release"));
        assert_eq!(
            fs::read_link(root.join("opt/chaptera/current")).unwrap(),
            PathBuf::from("releases/1.0.0+aaaa")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_build_identity_is_rejected() {
        let root = test_root("identity");
        let source = source(&root, "chaptera", b"chaptera-build");
        let error = install_from(&source, &root, "../escape", true).unwrap_err();
        assert!(error.to_string().contains("build identity"));
        fs::remove_dir_all(root).unwrap();
    }
}
