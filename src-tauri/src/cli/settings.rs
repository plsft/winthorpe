//! `winthorpe settings` — read/write entries in the `settings` table.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use rusqlite::params;

use crate::settings as settings_store;
use crate::ui_sync::UiMutationEvent;

use super::args::{Cli, SettingsAction};
use super::{notify_ui_event, output};

pub fn dispatch(action: &SettingsAction, cli: &Cli) -> Result<()> {
    match action {
        SettingsAction::Get { key } => get(key, cli),
        SettingsAction::Set { key, value } => set(key, value, cli),
        SettingsAction::List { all } => list(*all, cli),
        SettingsAction::Delete { key } => delete(key, cli),
    }
}

fn get(key: &str, cli: &Cli) -> Result<()> {
    let value = settings_store::load_setting_value(key)?;
    output::print(cli, &value, |v| match v {
        Some(s) => s.clone(),
        None => String::new(),
    })
}

fn set(key: &str, value: &str, cli: &Cli) -> Result<()> {
    settings_store::upsert_setting_value(key, value)?;
    notify_ui_event(UiMutationEvent::SettingsChanged {
        key: Some(key.to_string()),
    });
    output::print_ok(cli, &format!("Set {key}"));
    Ok(())
}

fn list(all: bool, cli: &Cli) -> Result<()> {
    let conn = crate::models::db::read_conn()?;
    let mut stmt = if all {
        conn.prepare("SELECT key, value FROM settings ORDER BY key ASC")?
    } else {
        conn.prepare(
            "SELECT key, value FROM settings \
             WHERE key LIKE 'app.%' OR key LIKE 'branch_prefix_%' \
             ORDER BY key ASC",
        )?
    };
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let map: BTreeMap<String, String> = rows.filter_map(|r| r.ok()).collect();

    output::print(cli, &map, |m| {
        m.iter()
            .map(|(k, v)| format!("{k}\t{v}"))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn delete(key: &str, cli: &Cli) -> Result<()> {
    let conn = crate::models::db::write_conn()?;
    let removed = conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    if removed == 0 {
        bail!("No setting with key '{key}'");
    }
    notify_ui_event(UiMutationEvent::SettingsChanged {
        key: Some(key.to_string()),
    });
    output::print_ok(cli, &format!("Deleted {key}"));
    Ok(())
}
