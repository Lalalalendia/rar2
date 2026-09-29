#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum LaunchError {
    Io(io::Error),
    ExecutableNotAbsolute(PathBuf),
    ExecutableMissing(PathBuf),
    ExecutableIdentityChanged(PathBuf),
    WorkingDirectoryMissing(PathBuf),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "process launch I/O error: {error}"),
            Self::ExecutableNotAbsolute(path) => {
                write!(formatter, "process executable is not absolute: {}", path.display())
            }
            Self::ExecutableMissing(path) => {
                write!(formatter, "process executable is missing: {}", path.display())
            }
            Self::ExecutableIdentityChanged(path) => write!(
                formatter,
                "process executable identity changed before launch: {}",
                path.display()
            ),
            Self::WorkingDirectoryMissing(path) => write!(
                formatter,
                "process working directory is missing or changed: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LaunchError {}

impl From<io::Error> for LaunchError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, LaunchError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundProgram {
    executable: PathBuf,
    executable_sha256: String,
    working_directory: PathBuf,
}

impl BoundProgram {
    pub fn bind(executable: &Path, working_directory: &Path) -> Result<Self> {
        if !executable.is_absolute() {
            return Err(LaunchError::ExecutableNotAbsolute(executable.to_path_buf()));
        }
        let executable = fs::canonicalize(executable)
            .map_err(|_| LaunchError::ExecutableMissing(executable.to_path_buf()))?;
        if !executable.is_absolute() || !executable.is_file() {
            return Err(LaunchError::ExecutableMissing(executable));
        }

        let working_directory = fs::canonicalize(working_directory)
            .map_err(|_| LaunchError::WorkingDirectoryMissing(working_directory.to_path_buf()))?;
        if !working_directory.is_absolute() || !working_directory.is_dir() {
            return Err(LaunchError::WorkingDirectoryMissing(working_directory));
        }

        Ok(Self {
            executable_sha256: sha256_file(&executable)?,
            executable,
            working_directory,
        })
    }

    pub fn from_expected(
        executable: PathBuf,
        executable_sha256: String,
        working_directory: PathBuf,
    ) -> Result<Self> {
        let bound = Self {
            executable,
            executable_sha256,
            working_directory,
        };
        bound.revalidate()?;
        Ok(bound)
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn executable_sha256(&self) -> &str {
        &self.executable_sha256
    }

    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    pub fn revalidate(&self) -> Result<()> {
        if !self.executable.is_absolute() {
            return Err(LaunchError::ExecutableNotAbsolute(self.executable.clone()));
        }
        let canonical = fs::canonicalize(&self.executable)
            .map_err(|_| LaunchError::ExecutableMissing(self.executable.clone()))?;
        if canonical != self.executable || !canonical.is_file() {
            return Err(LaunchError::ExecutableIdentityChanged(
                self.executable.clone(),
            ));
        }
        if sha256_file(&canonical)? != self.executable_sha256 {
            return Err(LaunchError::ExecutableIdentityChanged(
                self.executable.clone(),
            ));
        }

        let canonical_cwd = fs::canonicalize(&self.working_directory)
            .map_err(|_| LaunchError::WorkingDirectoryMissing(self.working_directory.clone()))?;
        if canonical_cwd != self.working_directory || !canonical_cwd.is_dir() {
            return Err(LaunchError::WorkingDirectoryMissing(
                self.working_directory.clone(),
            ));
        }
        Ok(())
    }

    pub fn command<I, K, V>(&self, environment: I) -> Result<Command>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.revalidate()?;
        let mut command = Command::new(&self.executable);
        command.current_dir(&self.working_directory).env_clear();
        for (key, value) in environment {
            command.env(key, value);
        }
        Ok(command)
    }
}

pub fn current_environment_allowlist(keys: &[&str]) -> Vec<(OsString, OsString)> {
    keys.iter()
        .filter_map(|key| {
            std::env::var_os(key).map(|value| (OsString::from(*key), value))
        })
        .collect()
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("chaptera-process-launch-{label}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn relative_executable_is_rejected() {
        let cwd = std::env::current_dir().expect("current dir");
        assert!(matches!(
            BoundProgram::bind(Path::new("relative.exe"), &cwd),
            Err(LaunchError::ExecutableNotAbsolute(_))
        ));
    }

    #[test]
    fn changed_executable_fails_revalidation() {
        let root = temp_dir("identity");
        let executable = root.join("helper.exe");
        fs::write(&executable, b"first").expect("write first");
        let bound = BoundProgram::bind(&executable, &root).expect("bind");
        fs::write(&executable, b"second").expect("replace");
        assert!(matches!(
            bound.revalidate(),
            Err(LaunchError::ExecutableIdentityChanged(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn command_clears_ambient_environment_and_sets_explicit_entries() {
        let root = temp_dir("environment");
        let executable = root.join("helper.exe");
        fs::write(&executable, b"stub").expect("write helper");
        let bound = BoundProgram::bind(&executable, &root).expect("bind");
        let command = bound
            .command([(OsString::from("A"), OsString::from("B"))])
            .expect("command");
        assert_eq!(command.get_current_dir(), Some(root.as_path()));
        let env = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(env, vec![("A".to_owned(), Some("B".to_owned()))]);
        let _ = fs::remove_dir_all(root);
    }
}
