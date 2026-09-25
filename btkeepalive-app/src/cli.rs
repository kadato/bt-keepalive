//! Command line interface.
//!
//! Light commands (`--config-path`, `--dry-run`, `--render-wav`,
//! `--list-devices`) never touch audio hardware or the tray, so they
//! work on any OS including headless CI.

use clap::Parser;
use std::path::PathBuf;

/// BT KeepAlive: keep Bluetooth headphones awake on Windows.
#[derive(Debug, Parser)]
#[command(name = "BTKeepAlive", version, about)]
pub struct Cli {
    /// Print the config file path and exit.
    #[arg(long)]
    pub config_path: bool,

    /// Do not start audio until Play is chosen.
    #[arg(long)]
    pub no_autoplay: bool,

    /// Render 5 seconds of the current preset to a temp WAV and exit.
    /// Verifies the full DSP path without sound hardware.
    #[arg(long)]
    pub dry_run: bool,

    /// Render 5 seconds of the current preset to this WAV file and exit.
    #[arg(long, value_name = "PATH")]
    pub render_wav: Option<PathBuf>,

    /// List output devices and exit.
    #[arg(long)]
    pub list_devices: bool,
}
