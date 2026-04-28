//! `winthorpe github` — GitHub auth + PR operations.

use anyhow::Result;

use crate::auth;
use crate::github_cli;
use crate::github_graphql;
use crate::service;
use crate::ui_sync::UiMutationEvent;

use super::args::{Cli, GithubAction, GithubAuthAction, GithubPrAction};
use super::{notify_ui_event, output};

pub fn dispatch(action: &GithubAction, cli: &Cli) -> Result<()> {
    match action {
        GithubAction::Auth { action } => auth_dispatch(action, cli),
        GithubAction::Pr { action } => pr_dispatch(action, cli),
        GithubAction::Repos => list_repos(cli),
        GithubAction::CliStatus => gh_cli_status(cli),
    }
}

fn auth_dispatch(action: &GithubAuthAction, cli: &Cli) -> Result<()> {
    match action {
        GithubAuthAction::Status => auth_status(cli),
        GithubAuthAction::Logout => logout(cli),
    }
}

fn auth_status(cli: &Cli) -> Result<()> {
    let snapshot = auth::get_github_identity_session()?;
    output::print(cli, &snapshot, |s| match s {
        auth::GithubIdentitySnapshot::Connected { session } => format!(
            "Connected as @{} ({}{})",
            session.login,
            session.name.clone().unwrap_or_default(),
            session
                .primary_email
                .as_deref()
                .map(|e| format!(" <{e}>"))
                .unwrap_or_default(),
        ),
        auth::GithubIdentitySnapshot::Disconnected => "Not connected".to_string(),
        auth::GithubIdentitySnapshot::Unconfigured { message } => {
            format!("Unconfigured: {message}")
        }
        auth::GithubIdentitySnapshot::Error { message } => format!("Error: {message}"),
    })
}

fn logout(cli: &Cli) -> Result<()> {
    auth::disconnect_github_identity_headless()?;
    notify_ui_event(UiMutationEvent::GithubIdentityChanged);
    output::print_ok(cli, "Disconnected from GitHub");
    Ok(())
}

fn list_repos(cli: &Cli) -> Result<()> {
    let repos = github_cli::list_github_accessible_repositories()?;
    output::print(cli, &repos, |items| {
        if items.is_empty() {
            "No accessible repositories.".to_string()
        } else {
            items
                .iter()
                .map(|r| format!("{}\t{}", r.full_name, r.html_url))
                .collect::<Vec<_>>()
                .join("\n")
        }
    })
}

fn gh_cli_status(cli: &Cli) -> Result<()> {
    let status = github_cli::get_github_cli_status()?;
    output::print(cli, &status, |_| format!("{status:?}"))
}

fn pr_dispatch(action: &GithubPrAction, cli: &Cli) -> Result<()> {
    match action {
        GithubPrAction::Show { workspace_ref } => pr_show(workspace_ref, cli),
        GithubPrAction::Status { workspace_ref } => pr_status(workspace_ref, cli),
        GithubPrAction::Merge { workspace_ref } => pr_merge(workspace_ref, cli),
        GithubPrAction::Close { workspace_ref } => pr_close(workspace_ref, cli),
    }
}

fn pr_show(workspace_ref: &str, cli: &Cli) -> Result<()> {
    let id = service::resolve_workspace_ref(workspace_ref)?;
    let pr = github_graphql::lookup_workspace_pr(&id)?;
    output::print(cli, &pr, |value| match value {
        Some(pr) => format!(
            "#{} {}\nURL:    {}\nState:  {}{}",
            pr.number,
            pr.title,
            pr.url,
            pr.state,
            if pr.is_merged { " (merged)" } else { "" },
        ),
        None => "No PR linked to this workspace.".to_string(),
    })
}

fn pr_status(workspace_ref: &str, cli: &Cli) -> Result<()> {
    let id = service::resolve_workspace_ref(workspace_ref)?;
    let status = github_graphql::lookup_workspace_pr_action_status(&id)?;
    output::print(cli, &status, |s| format!("{s:?}"))
}

fn pr_merge(workspace_ref: &str, cli: &Cli) -> Result<()> {
    let id = service::resolve_workspace_ref(workspace_ref)?;
    let pr = github_graphql::merge_workspace_pr(&id)?;
    notify_ui_event(UiMutationEvent::WorkspaceChangeRequestChanged {
        workspace_id: id.clone(),
    });
    output::print(cli, &pr, |value| match value {
        Some(pr) => format!("Merged PR #{}: {}", pr.number, pr.url),
        None => "No PR to merge.".to_string(),
    })
}

fn pr_close(workspace_ref: &str, cli: &Cli) -> Result<()> {
    let id = service::resolve_workspace_ref(workspace_ref)?;
    let pr = github_graphql::close_workspace_pr(&id)?;
    notify_ui_event(UiMutationEvent::WorkspaceChangeRequestChanged {
        workspace_id: id.clone(),
    });
    output::print(cli, &pr, |value| match value {
        Some(pr) => format!("Closed PR #{}: {}", pr.number, pr.url),
        None => "No PR to close.".to_string(),
    })
}
