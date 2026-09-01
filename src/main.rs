mod policy;

#[cfg(windows)]
mod windows_app;

use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Monitor audio sessions until the process is stopped.
    Run {
        #[command(flatten)]
        settings: Settings,
    },
    /// Copy the executable to LocalAppData and start it at sign-in.
    Install {
        #[command(flatten)]
        settings: Settings,
    },
    /// Disable automatic start and stop the running guard.
    Uninstall,
    /// Show whether automatic start and the guard process are enabled.
    Status,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct Settings {
    /// Target session volume, from 0 to 100.
    #[arg(long, default_value_t = 30)]
    pub(crate) volume: u8,

    /// Cap every new session above the target, not only sessions at 100%.
    #[arg(long)]
    pub(crate) cap: bool,

    /// Also lower sessions that already exist when the guard starts.
    #[arg(long)]
    pub(crate) include_existing: bool,

    /// Also process the Windows System Sounds session.
    #[arg(long)]
    pub(crate) include_system: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 30,
            cap: false,
            include_existing: false,
            include_system: false,
        }
    }
}

impl Settings {
    fn validate(&self) -> Result<()> {
        if self.volume > 100 {
            bail!("--volume must be between 0 and 100");
        }
        Ok(())
    }
}

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<()> {
    let command = Cli::parse().command.unwrap_or(Command::Run {
        settings: Settings::default(),
    });

    match &command {
        Command::Run { settings } | Command::Install { settings } => settings.validate()?,
        Command::Uninstall | Command::Status => {}
    }

    dispatch(command)
}

#[cfg(windows)]
fn dispatch(command: Command) -> Result<()> {
    match command {
        Command::Run { settings } => windows_app::run(settings),
        Command::Install { settings } => windows_app::install(settings),
        Command::Uninstall => windows_app::uninstall(),
        Command::Status => windows_app::status(),
    }
}

#[cfg(not(windows))]
fn dispatch(_command: Command) -> Result<()> {
    bail!("windows-volume-guard only runs on Windows 10 or Windows 11")
}
