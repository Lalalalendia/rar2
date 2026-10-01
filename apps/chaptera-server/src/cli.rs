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
    GuestReaderScene {
        #[arg(long)]
        session_id: String,
        #[arg(long)]
        expected_sha256: String,
        #[arg(long)]
        expected_byte_len: u64,
        #[arg(
            long,
            hide = true,
            requires_all = [
                "probe_font_sha256",
                "probe_source_family_sha256",
                "probe_font_resource_id"
            ]
        )]
        probe_font_path: Option<PathBuf>,
        #[arg(
            long,
            hide = true,
            requires_all = [
                "probe_font_path",
                "probe_source_family_sha256",
                "probe_font_resource_id"
            ]
        )]
        probe_font_sha256: Option<String>,
        #[arg(
            long,
            hide = true,
            requires_all = [
                "probe_font_path",
                "probe_font_sha256",
                "probe_font_resource_id"
            ]
        )]
        probe_source_family_sha256: Option<String>,
        #[arg(
            long,
            hide = true,
            requires_all = [
                "probe_font_path",
                "probe_font_sha256",
                "probe_source_family_sha256"
            ]
        )]
        probe_font_resource_id: Option<String>,
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
