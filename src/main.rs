mod app;
mod cli;
mod config;
mod device;
mod error;
mod router;
mod virtual_keyboard;
mod virtual_mouse;

use app::run;
use clap::Parser;
use cli::Cli;

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
