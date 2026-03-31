use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Top-level CLI for daemon, diagnostics, and reload control.
#[derive(Debug, Parser)]
#[command(name = "mousefold")]
#[command(about = "Mouse to keyboard remapper daemon", version)]
pub struct Cli {
    /// Path to the YAML configuration file for daemon mode.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate the configuration and exit.
    Check {
        /// Path to the YAML configuration file.
        #[arg(short, long, value_name = "FILE")]
        config: PathBuf,
    },
    /// Print normalized mouse events without starting the daemon.
    Monitor {
        /// Path to the YAML configuration file.
        #[arg(short, long, value_name = "FILE")]
        config: PathBuf,
    },
    /// Request a running daemon to reload the configuration.
    Reload {
        /// Path to the YAML configuration file.
        #[arg(short, long, value_name = "FILE")]
        config: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_mode_accepts_top_level_config() {
        let cli = Cli::try_parse_from(["mousefold", "--config", "config.yaml"])
            .expect("cli should parse");

        assert!(cli.command.is_none());
        assert_eq!(
            cli.config.as_deref(),
            Some(PathBuf::from("config.yaml").as_path())
        );
    }

    #[test]
    fn check_subcommand_parses() {
        let cli = Cli::try_parse_from(["mousefold", "check", "--config", "config.yaml"])
            .expect("cli should parse");

        assert!(matches!(cli.command, Some(Command::Check { .. })));
    }
}
