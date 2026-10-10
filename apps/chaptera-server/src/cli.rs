use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::build_info::BUILD_IDENTITY;

#[derive(Debug, Parser)]
#[command(
    name = "chaptera",
    about = "Chaptera Cloud server runtime shell",
    version = BUILD_IDENTITY,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Load one typed Chaptera TOML configuration file.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install this exact executable as an immutable Linux release.
    Install {
        /// Populate a staging root instead of mutating the live host.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Start the private HTTP runtime.
    Serve,
    /// Start the durable background worker runtime.
    Worker,
    /// Inspect or advance the durable schema.
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Run read-only startup/operator diagnostics.
    Doctor,
    #[command(name = "untrusted-pub-inspect", hide = true)]
    UntrustedPubInspect {
        #[arg(long)]
        max_file_bytes: u64,
        #[arg(long)]
        max_cfb_entries: u64,
        #[arg(long)]
        max_declared_stream_bytes: u64,
    },
    #[command(hide = true)]
    SourceBaseline {
        #[arg(long)]
        document_id: String,
        #[arg(long)]
        expected_sha256: String,
        #[arg(long)]
        expected_byte_len: u64,
    },
    #[command(hide = true)]
    MigrationEditableRoutes {
        #[arg(long)]
        document_id: String,
        #[arg(long)]
        expected_sha256: String,
        #[arg(long)]
        expected_byte_len: u64,
    },
    /// Replay an exact canonical product project in a confined process.
    #[command(name = "product-isolated-replay", hide = true)]
    ProductIsolatedReplay {
        #[arg(long)]
        document_id: String,
        #[arg(long)]
        expected_sha256: String,
        #[arg(long)]
        expected_byte_len: u64,
        /// Omit both options to derive a baseline project in isolation.
        #[arg(long, requires = "expected_project_sha256")]
        project_json: Option<PathBuf>,
        #[arg(long, requires = "project_json")]
        expected_project_sha256: Option<String>,
        #[arg(long, requires_all = ["move_x_emu", "move_y_emu", "project_json"])]
        move_node_id: Option<String>,
        #[arg(long, requires = "move_node_id")]
        move_x_emu: Option<i64>,
        #[arg(long, requires = "move_node_id")]
        move_y_emu: Option<i64>,
    },
    #[command(hide = true)]
    GuestReaderScene {
        #[arg(long)]
        session_id: String,
        #[arg(long)]
        expected_sha256: String,
        #[arg(long)]
        expected_byte_len: u64,
        #[arg(long)]
        font_registry: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum MigrateAction {
    Status,
    Up,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Command, MigrateAction};

    #[test]
    fn parses_all_operator_commands() {
        assert!(matches!(
            Cli::try_parse_from([
                "chaptera",
                "install",
                "--config",
                "/etc/chaptera/chaptera.toml",
                "--root",
                "/tmp/chaptera-root",
            ])
            .unwrap()
            .command,
            Command::Install { root: Some(_) }
        ));
        assert!(matches!(
            Cli::try_parse_from(["chaptera", "serve"]).unwrap().command,
            Command::Serve
        ));
        assert!(matches!(
            Cli::try_parse_from(["chaptera", "worker"]).unwrap().command,
            Command::Worker
        ));
        assert!(matches!(
            Cli::try_parse_from(["chaptera", "doctor"]).unwrap().command,
            Command::Doctor
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "chaptera",
                "untrusted-pub-inspect",
                "--max-file-bytes",
                "1024",
                "--max-cfb-entries",
                "32",
                "--max-declared-stream-bytes",
                "2048",
            ])
            .unwrap()
            .command,
            Command::UntrustedPubInspect { .. }
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "chaptera",
                "migration-editable-routes",
                "--document-id",
                "document:one",
                "--expected-sha256",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "--expected-byte-len",
                "123"
            ])
            .unwrap()
            .command,
            Command::MigrationEditableRoutes { .. }
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "chaptera",
                "product-isolated-replay",
                "--document-id",
                "document-one",
                "--expected-sha256",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "--expected-byte-len",
                "1024",
                "--project-json",
                "/tmp/project.json",
                "--expected-project-sha256",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ])
            .unwrap()
            .command,
            Command::ProductIsolatedReplay { .. }
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "chaptera",
                "product-isolated-replay",
                "--document-id",
                "document-one",
                "--expected-sha256",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "--expected-byte-len",
                "1024",
            ])
            .unwrap()
            .command,
            Command::ProductIsolatedReplay {
                project_json: None,
                expected_project_sha256: None,
                ..
            }
        ));
        assert!(
            Cli::try_parse_from([
                "chaptera",
                "product-isolated-replay",
                "--document-id",
                "document-one",
                "--expected-sha256",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "--expected-byte-len",
                "1024",
                "--project-json",
                "/tmp/project.json",
            ])
            .is_err()
        );

        assert!(matches!(
            Cli::try_parse_from([
                "chaptera",
                "product-isolated-replay",
                "--document-id", "document-one",
                "--expected-sha256", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "--expected-byte-len", "1024",
                "--project-json", "/tmp/project.json",
                "--expected-project-sha256", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "--move-node-id", "00112233-4455-6677-8899-aabbccddeeff",
                "--move-x-emu", "100",
                "--move-y-emu", "-50",
            ])
            .unwrap()
            .command,
            Command::ProductIsolatedReplay {
                move_node_id: Some(_),
                move_x_emu: Some(100),
                move_y_emu: Some(-50),
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["chaptera", "migrate", "status"])
                .unwrap()
                .command,
            Command::Migrate {
                action: MigrateAction::Status
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["chaptera", "migrate", "up"])
                .unwrap()
                .command,
            Command::Migrate {
                action: MigrateAction::Up
            }
        ));
    }

    #[test]
    fn config_flag_is_global() {
        let cli = Cli::try_parse_from([
            "chaptera",
            "serve",
            "--config",
            "/etc/chaptera/chaptera.toml",
        ])
        .unwrap();

        assert_eq!(
            cli.config.unwrap(),
            PathBuf::from("/etc/chaptera/chaptera.toml")
        );
        assert!(matches!(cli.command, Command::Serve));
    }
}
