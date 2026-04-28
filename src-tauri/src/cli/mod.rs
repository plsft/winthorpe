//! Command-line interface for Winthorpe.
//!
//! The binary at `src/bin/winthorpe-cli.rs` is a thin dispatcher — every
//! command body lives here so it can reach crate-private domain logic
//! (`workspace::*`, `models::*`, `agents::*`, `github::*`, `git::*`).
//!
//! # Architecture
//!
//! Each command domain gets its own sub-module (`repo`, `workspace`,
//! `session`, `files`, `send`, `github`, `settings`, `scripts`,
//! `conductor`, `system`, `data`). Shared helpers live in `output` (JSON
//! / human formatting) and `refs` (UUID / name disambiguation).

pub mod args;
mod conductor;
mod data;
mod files;
mod github;
mod output;
mod refs;
mod repo;
mod scripts;
mod send;
mod session;
mod settings;
mod system;
mod workspace;

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{CommandFactory, FromArgMatches};

pub use self::args::{Cli, Commands};
use crate::ui_sync::UiMutationEvent;

fn installed_cli_name() -> &'static str {
    if crate::data_dir::is_dev() {
        "winthorpe-dev"
    } else {
        "winthorpe"
    }
}

pub(crate) fn notify_ui_event(event: UiMutationEvent) {
    let _ = crate::ui_sync::notify_running_app(event);
}

pub(crate) fn notify_ui_events(events: impl IntoIterator<Item = UiMutationEvent>) {
    for event in events {
        notify_ui_event(event);
    }
}

/// Entry point. Parses arguments, initialises the data directory and
/// schema, then dispatches. Returns a process exit code.
pub fn run() -> ExitCode {
    let cli = {
        let command_name = installed_cli_name();
        let matches = Cli::command()
            .name(command_name)
            .bin_name(command_name)
            .get_matches();
        Cli::from_arg_matches(&matches).expect("command matches should parse into Cli")
    };

    if let Some(ref dir) = cli.data_dir {
        // SAFETY: called in main() before any threads are spawned.
        unsafe { std::env::set_var("WINTHORPE_DATA_DIR", dir) };
    }

    match dispatch(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if cli.json {
                let body = serde_json::json!({ "error": format!("{error:#}") });
                eprintln!("{body}");
            } else {
                eprintln!("error: {error:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: &Cli) -> Result<()> {
    ensure_ready()?;

    use args::Commands as C;
    match &cli.command {
        C::Data { action } => data::dispatch(action, cli),
        C::Completions { shell } => system::completions(*shell),
        C::CliStatus => system::cli_status(cli),
        C::Quit => system::quit(),
        C::Settings { action } => settings::dispatch(action, cli),
        C::Repo { action } => repo::dispatch(action, cli),
        C::Workspace { action } => workspace::dispatch(action, cli),
        C::Session { action } => session::dispatch(action, cli),
        C::Files { action } => files::dispatch(action, cli),
        C::Send(opts) => send::send(opts, cli),
        C::Models { action } => send::dispatch_models(action, cli),
        C::Github { action } => github::dispatch(action, cli),
        C::Scripts { action } => scripts::dispatch(action, cli),
        C::Conductor { action } => conductor::dispatch(action, cli),
        C::Mcp => crate::mcp::run_mcp_server(),
    }
}

/// Make sure the data directory and schema are ready. Shared across all
/// commands — a typo in one dispatcher arm shouldn't leave the DB
/// half-initialised, so this runs unconditionally.
fn ensure_ready() -> Result<()> {
    crate::data_dir::ensure_directory_structure()?;
    let db_path = crate::data_dir::db_path()?;
    let conn = rusqlite::Connection::open(&db_path)
        .with_context(|| format!("Failed to open database at {db_path:?}"))?;
    crate::schema_init(&conn);
    Ok(())
}
