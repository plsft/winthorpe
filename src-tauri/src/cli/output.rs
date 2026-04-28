//! JSON / human output helpers for the CLI.
//!
//! Every command funnels through `print` (for data) or `print_ok` (for
//! empty-success cases). Respecting `--json` and `--quiet` in one place
//! keeps command bodies free of formatting noise.

use anyhow::Result;
use serde::Serialize;

use super::args::Cli;

/// Render `value` to stdout. Human mode delegates to `human` for a
/// friendly text rendering; JSON mode always emits pretty JSON.
pub fn print<T: Serialize, F>(cli: &Cli, value: &T, human: F) -> Result<()>
where
    F: FnOnce(&T) -> String,
{
    if cli.json {
        let body = serde_json::to_string_pretty(value)?;
        println!("{body}");
    } else if !cli.quiet {
        let text = human(value);
        if !text.is_empty() {
            println!("{text}");
        }
    }
    Ok(())
}

/// Emit a confirmation message for an operation that returns nothing
/// useful. Honors `--json` by printing `{"ok": true}`.
pub fn print_ok(cli: &Cli, human: &str) {
    if cli.json {
        println!("{{\"ok\": true}}");
    } else if !cli.quiet && !human.is_empty() {
        println!("{human}");
    }
}

/// Emit a raw ID to stdout. Used by `workspace new`, `session new`, etc.
/// to keep shell pipelines ergonomic (`winthorpe workspace new ... | xargs`).
pub fn print_id(cli: &Cli, label: &str, id: &str) {
    if cli.json {
        let body = serde_json::json!({ label: id });
        println!("{body}");
    } else if cli.quiet {
        println!("{id}");
    } else {
        println!("{label}: {id}");
    }
}
