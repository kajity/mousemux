mod app;
mod config;
mod device;
mod error;
mod router;
mod virtual_keyboard;
mod virtual_mouse;

use app::{Cli, run};
use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    match run(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("[ERROR] {err}");
            std::process::ExitCode::FAILURE
        }
    }
}
