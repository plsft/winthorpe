//! `winthorpe conductor` — migrate from Winthorpe v1 (Conductor).

use anyhow::Result;

use crate::import;

use super::args::{Cli, ConductorAction};
use super::output;

pub fn dispatch(action: &ConductorAction, cli: &Cli) -> Result<()> {
    match action {
        ConductorAction::Status => status(cli),
        ConductorAction::Repos => repos(cli),
        ConductorAction::Workspaces => workspaces(cli),
    }
}

fn status(cli: &Cli) -> Result<()> {
    let available = import::conductor_source_available();
    output::print(cli, &serde_json::json!({"available": available}), |_| {
        if available {
            "Conductor data source detected.".to_string()
        } else {
            "No Conductor data source found.".to_string()
        }
    })
}

fn repos(cli: &Cli) -> Result<()> {
    let items = import::list_conductor_repos()?;
    output::print(cli, &items, |rows| {
        if rows.is_empty() {
            "No Conductor repositories.".to_string()
        } else {
            rows.iter()
                .map(|r| {
                    format!(
                        "{}\t{}\t{} workspaces ({} already imported)",
                        r.id, r.name, r.workspace_count, r.already_imported_count,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    })
}

fn workspaces(cli: &Cli) -> Result<()> {
    let repos = import::list_conductor_repos()?;
    #[derive(serde::Serialize)]
    struct Row {
        repo_id: String,
        workspace: import::ConductorWorkspace,
    }
    let mut rows = Vec::new();
    for repo in &repos {
        let wss = import::list_conductor_workspaces(&repo.id)?;
        for ws in wss {
            rows.push(Row {
                repo_id: repo.id.clone(),
                workspace: ws,
            });
        }
    }
    output::print(cli, &rows, |rs| {
        if rs.is_empty() {
            "No Conductor workspaces.".to_string()
        } else {
            rs.iter()
                .map(|r| {
                    format!(
                        "{}\t{}\t{}\t{}{}",
                        r.repo_id,
                        r.workspace.id,
                        r.workspace.directory_name,
                        r.workspace.state,
                        if r.workspace.already_imported {
                            " [imported]"
                        } else {
                            ""
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    })
}
