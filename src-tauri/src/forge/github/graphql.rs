//! GitHub GraphQL helpers used by in-app features that talk directly to
//! api.github.com (e.g. the commit button's post-stream PR verification).
//!
//! Unlike `github_cli.rs`, which shells out to `gh`, this module goes straight
//! to the v4 GraphQL endpoint using the OAuth access token persisted by the
//! device-flow identity stored in `auth.rs`. It exists so Winthorpe can look up
//! PR state without requiring `gh` to be installed on the user's machine.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

use crate::forge::{
    ActionProvider, ActionStatusKind, ChangeRequestInfo, ForgeActionItem, ForgeActionStatus,
    RemoteState,
};
use crate::{auth, error::ErrorCode, git_ops, models::workspaces as workspace_models};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubCheckRunDetail {
    details_url: Option<String>,
    html_url: Option<String>,
    output: Option<GithubCheckRunOutput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubCheckRunOutput {
    title: Option<String>,
    summary: Option<String>,
    text: Option<String>,
}

/// Send a blocking HTTP request with up to 2 retries on transient network
/// failures (TLS handshake, connect, timeout, connection reset). JSON bodies
/// are always cloneable via `try_clone`; on the rare non-cloneable case we
/// fall through to a single send.
fn send_with_retry(
    builder: reqwest::blocking::RequestBuilder,
) -> reqwest::Result<reqwest::blocking::Response> {
    const BACKOFF_MS: [u64; 2] = [200, 500];
    for &delay in &BACKOFF_MS {
        let Some(attempt) = builder.try_clone() else {
            return builder.send();
        };
        match attempt.send() {
            Ok(resp) => return Ok(resp),
            Err(e) if is_transient_network_error(&e) => {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            Err(e) => return Err(e),
        }
    }
    builder.send()
}

fn is_transient_network_error(err: &reqwest::Error) -> bool {
    if err.is_connect() || err.is_timeout() {
        return true;
    }
    let msg = err.to_string().to_lowercase();
    msg.contains("tls handshake")
        || msg.contains("handshake eof")
        || msg.contains("connection reset")
        || msg.contains("connection closed")
}
/// Look up the (most recent) pull request matching this workspace's current
/// branch on GitHub.
///
/// Returns:
///   - `Ok(Some(pr))` when a PR is found for `headRefName == branch`.
///   - `Ok(None)` when there's no matching PR, when the workspace has no
///     github.com remote, when the user isn't connected to GitHub, or when
///     the access token has been revoked.
///   - `Err(_)` only for unexpected transport / parse failures (so the caller
///     can surface a distinct "something went wrong" state).
pub fn lookup_workspace_pr(workspace_id: &str) -> Result<Option<ChangeRequestInfo>> {
    let Some(record) = workspace_models::load_workspace_record_by_id(workspace_id)? else {
        bail!("Workspace not found: {workspace_id}");
    };

    // A workspace in Phase 1 hasn't been pushed yet — there can't be a PR.
    // Short-circuit to match the post-ready answer and avoid a pointless
    // GitHub round-trip plus the UI flicker that would come with it.
    if record.state == crate::workspace_state::WorkspaceState::Initializing {
        return Ok(None);
    }

    let Some(remote_url) = record.remote_url.as_deref() else {
        return Ok(None);
    };
    let Some((owner, name)) = parse_github_remote(remote_url) else {
        // Not a github.com remote — nothing to query.
        return Ok(None);
    };
    let Some(branch) = record.branch.as_deref().filter(|b| !b.is_empty()) else {
        return Ok(None);
    };

    // No remote-tracking ref → this branch was never published, so any PR
    // GitHub returns for `headRefName == branch` belongs to a previous owner
    // of the name (e.g. a merged PR whose head branch was deleted). Skip.
    if !workspace_branch_has_remote_tracking(&record) {
        return Ok(None);
    }

    let Some(access_token) = auth::load_valid_github_access_token()? else {
        // User isn't connected, or their refresh token has expired.
        return Ok(None);
    };

    let client = Client::builder()
        .build()
        .context("Failed to build GitHub HTTP client")?;

    let query = r#"
query($owner: String!, $name: String!, $head: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(headRefName: $head, states: [OPEN, MERGED, CLOSED], first: 1, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        url
        number
        state
        title
        merged
      }
    }
  }
}
"#;

    let body = json!({
        "query": query,
        "variables": {
            "owner": owner,
            "name": name,
            "head": branch,
        },
    });

    let response = send_with_retry(
        client
            .post("https://api.github.com/graphql")
            .header(USER_AGENT, "Winthorpe")
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .json(&body),
    )
    .context("Failed to reach GitHub GraphQL API")?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        // Token was rejected — treat as "not connected" rather than erroring.
        return Ok(None);
    }
    if !status.is_success() {
        return Err(anyhow!(
            "GitHub GraphQL API returned HTTP {status}: {}",
            response.text().unwrap_or_default()
        ));
    }

    let parsed: GraphqlEnvelope = response
        .json()
        .context("Failed to decode GitHub GraphQL response")?;

    if let Some(errors) = &parsed.errors {
        if !errors.is_empty() {
            // "Could not resolve to a Repository" means the token doesn't
            // have access to this repo (private + insufficient scope) or the
            // repo doesn't exist. Treat like "not connected" — return None
            // so the caller degrades gracefully instead of surfacing an error.
            let is_repo_not_found = errors.iter().any(|e| {
                e.message.contains("Could not resolve to a Repository")
                    || e.message.contains("NOT_FOUND")
            });
            if is_repo_not_found {
                return Ok(None);
            }
            // Other GraphQL errors are unexpected — propagate.
            return Err(anyhow!(
                "GitHub GraphQL errors: {}",
                errors
                    .iter()
                    .map(|e| e.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }

    let Some(data) = parsed.data else {
        return Ok(None);
    };
    let Some(repository) = data.repository else {
        return Ok(None);
    };

    let Some(node) = repository.pull_requests.nodes.into_iter().next() else {
        return Ok(None);
    };

    Ok(Some(ChangeRequestInfo {
        url: node.url,
        number: node.number,
        state: node.state,
        title: node.title,
        is_merged: node.merged,
    }))
}

/// Full PR action status for the inspector Actions panel.
///
/// Missing GitHub configuration, missing OAuth, inaccessible repositories, and
/// "no PR for this branch" are represented in the returned status instead of
/// bubbling as command errors. That keeps the local Git rows usable even when
/// remote status cannot be queried.
pub fn lookup_workspace_pr_action_status(workspace_id: &str) -> Result<ForgeActionStatus> {
    let Some(record) = workspace_models::load_workspace_record_by_id(workspace_id)? else {
        bail!("Workspace not found: {workspace_id}");
    };

    // Phase 1 workspace: definitively no PR yet. Return the `no_pr` state
    // directly so the inspector paints the final empty review list from
    // the first frame, without a GitHub round-trip.
    if record.state == crate::workspace_state::WorkspaceState::Initializing {
        return Ok(ForgeActionStatus::no_change_request());
    }

    let Some(remote_url) = record.remote_url.as_deref() else {
        return Ok(ForgeActionStatus::unavailable("Workspace has no remote"));
    };
    let Some((owner, name)) = parse_github_remote(remote_url) else {
        return Ok(ForgeActionStatus::unavailable(
            "Workspace remote is not a GitHub repository",
        ));
    };
    let Some(branch) = record.branch.as_deref().filter(|b| !b.is_empty()) else {
        return Ok(ForgeActionStatus::unavailable(
            "Workspace has no current branch",
        ));
    };
    // Same guard as `lookup_workspace_pr` — without a remote-tracking ref the
    // branch was never published, so any PR returned would belong to a prior
    // owner of the same head ref. Surface as `no_pr` so the inspector hides
    // checks/deployments instead of showing a ghost PR's history.
    if !workspace_branch_has_remote_tracking(&record) {
        return Ok(ForgeActionStatus::no_change_request());
    }
    let Some(access_token) = auth::load_valid_github_access_token()? else {
        return Ok(ForgeActionStatus::unavailable(
            "GitHub account is not connected",
        ));
    };

    let client = Client::builder()
        .build()
        .context("Failed to build GitHub HTTP client")?;

    let status = query_workspace_pr_action_status(&client, &access_token, owner, name, branch)
        .unwrap_or_else(|error| ForgeActionStatus::error(format!("{error:#}")));

    Ok(status)
}

pub fn lookup_workspace_pr_check_insert_text(workspace_id: &str, item_id: &str) -> Result<String> {
    let Some(record) = workspace_models::load_workspace_record_by_id(workspace_id)? else {
        bail!("Workspace not found: {workspace_id}");
    };

    let Some(remote_url) = record.remote_url.as_deref() else {
        bail!("Workspace has no remote");
    };
    let Some((owner, name)) = parse_github_remote(remote_url) else {
        bail!("Workspace remote is not a GitHub repository");
    };
    let Some(branch) = record.branch.as_deref().filter(|b| !b.is_empty()) else {
        bail!("Workspace has no current branch");
    };
    let Some(access_token) = auth::load_valid_github_access_token()? else {
        crate::bail_coded!(
            ErrorCode::ForgeOnboarding,
            "GitHub account is not connected"
        );
    };

    let client = Client::builder()
        .build()
        .context("Failed to build GitHub HTTP client")?;

    let action_status = query_workspace_pr_action_status(
        &client,
        &access_token,
        owner.clone(),
        name.clone(),
        branch,
    )
    .context("Failed to load current PR action status")?;

    let item = action_status
        .checks
        .into_iter()
        .find(|check| check.id == item_id)
        .with_context(|| format!("Check item not found: {item_id}"))?;

    let detail = item
        .id
        .strip_prefix("check-run-")
        .and_then(|value| value.parse::<i64>().ok())
        .map(|check_run_id| {
            query_check_run_detail(&client, &access_token, &owner, &name, check_run_id)
        })
        .transpose()
        .context("Failed to load check run details")?;

    Ok(build_check_insert_text(&item, detail.as_ref()))
}

fn query_workspace_pr_action_status(
    client: &Client,
    access_token: &str,
    owner: String,
    name: String,
    branch: &str,
) -> Result<ForgeActionStatus> {
    let query = r#"
query($owner: String!, $name: String!, $head: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(headRefName: $head, states: [OPEN, MERGED, CLOSED], first: 1, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        url
        number
        state
        title
        merged
        reviewDecision
        mergeable
        commits(last: 1) {
          nodes {
            commit {
              statusCheckRollup {
                contexts(first: 50) {
                  nodes {
                    __typename
                    ... on CheckRun {
                      databaseId
                      name
                      status
                      conclusion
                      detailsUrl
                      startedAt
                      completedAt
                      checkSuite {
                        app { name }
                      }
                    }
                    ... on StatusContext {
                      context
                      state
                      targetUrl
                    }
                  }
                }
              }
              deployments(first: 20, orderBy: {field: CREATED_AT, direction: DESC}) {
                nodes {
                  id
                  environment
                  latestStatus {
                    state
                    logUrl
                    environmentUrl
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#;

    let body = json!({
        "query": query,
        "variables": {
            "owner": owner,
            "name": name,
            "head": branch,
        },
    });

    let response = send_with_retry(
        client
            .post("https://api.github.com/graphql")
            .header(USER_AGENT, "Winthorpe")
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .json(&body),
    )
    .context("Failed to reach GitHub GraphQL API")?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(ForgeActionStatus::unavailable("GitHub token was rejected"));
    }
    if !status.is_success() {
        return Ok(ForgeActionStatus::error(format!(
            "GitHub GraphQL API returned HTTP {status}: {}",
            response.text().unwrap_or_default()
        )));
    }

    let parsed: ActionGraphqlEnvelope = response
        .json()
        .context("Failed to decode GitHub GraphQL action status response")?;

    if let Some(errors) = &parsed.errors {
        if !errors.is_empty() {
            let message = errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let is_repo_not_found = errors.iter().any(|e| {
                e.message.contains("Could not resolve to a Repository")
                    || e.message.contains("NOT_FOUND")
            });
            if is_repo_not_found {
                return Ok(ForgeActionStatus::unavailable(message));
            }
            return Ok(ForgeActionStatus::error(message));
        }
    }

    let Some(data) = parsed.data else {
        return Ok(ForgeActionStatus::no_change_request());
    };
    let Some(repository) = data.repository else {
        return Ok(ForgeActionStatus::unavailable(
            "GitHub repository was not returned",
        ));
    };
    let Some(pr) = repository.pull_requests.nodes.into_iter().next() else {
        return Ok(ForgeActionStatus::no_change_request());
    };

    Ok(build_action_status(pr))
}

fn query_check_run_detail(
    client: &Client,
    access_token: &str,
    owner: &str,
    name: &str,
    check_run_id: i64,
) -> Result<GithubCheckRunDetail> {
    let response = send_with_retry(
        client
            .get(format!(
                "https://api.github.com/repos/{owner}/{name}/check-runs/{check_run_id}"
            ))
            .header(USER_AGENT, "Winthorpe")
            .header(ACCEPT, "application/vnd.github+json")
            .header(AUTHORIZATION, format!("Bearer {access_token}")),
    )
    .context("Failed to reach GitHub REST API")?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "GitHub check run API returned HTTP {status}: {}",
            response.text().unwrap_or_default()
        ));
    }

    response
        .json::<GithubCheckRunDetail>()
        .context("Failed to decode GitHub check run response")
}

/// Merge a workspace's open PR via the GitHub GraphQL `mergePullRequest`
/// mutation. Returns the updated `ChangeRequestInfo` on success, or `None`
/// when the PR can't be found / user isn't connected.
pub fn merge_workspace_pr(workspace_id: &str) -> Result<Option<ChangeRequestInfo>> {
    let pr = lookup_workspace_pr(workspace_id)?;
    let Some(pr) = pr else {
        return Ok(None);
    };
    if pr.state != "OPEN" {
        bail!("PR #{} is not open (state: {})", pr.number, pr.state);
    }

    let access_token = auth::load_valid_github_access_token()?;
    let Some(access_token) = access_token else {
        return Ok(None);
    };

    // We need the PR's GraphQL node ID. Re-query with node ID included.
    let Some(record) = workspace_models::load_workspace_record_by_id(workspace_id)? else {
        bail!("Workspace not found: {workspace_id}");
    };
    let Some(remote_url) = record.remote_url.as_deref() else {
        return Ok(None);
    };
    let Some((owner, name)) = parse_github_remote(remote_url) else {
        return Ok(None);
    };
    let Some(branch) = record.branch.as_deref().filter(|b| !b.is_empty()) else {
        return Ok(None);
    };

    let client = Client::builder()
        .build()
        .context("Failed to build GitHub HTTP client")?;

    // Fetch PR node ID
    let id_query = r#"
query($owner: String!, $name: String!, $head: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(headRefName: $head, states: [OPEN], first: 1) {
      nodes { id, url, number, state, title, merged }
    }
  }
}
"#;
    let id_body = json!({
        "query": id_query,
        "variables": { "owner": owner, "name": name, "head": branch },
    });

    let id_response: GraphqlEnvelope = send_with_retry(
        client
            .post("https://api.github.com/graphql")
            .header(USER_AGENT, "Winthorpe")
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .json(&id_body),
    )
    .context("Failed to reach GitHub GraphQL API")?
    .json()
    .context("Failed to decode GraphQL response")?;

    let pr_node_id = id_response
        .data
        .and_then(|d| d.repository)
        .and_then(|r| r.pull_requests.nodes.into_iter().next())
        .map(|n| n.id);
    let Some(pr_node_id) = pr_node_id.flatten() else {
        bail!("Could not resolve PR node ID for #{}", pr.number);
    };

    // Execute merge mutation
    let merge_mutation = r#"
mutation($prId: ID!) {
  mergePullRequest(input: { pullRequestId: $prId }) {
    pullRequest { url, number, state, title, merged }
  }
}
"#;
    let merge_body = json!({
        "query": merge_mutation,
        "variables": { "prId": pr_node_id },
    });

    let merge_response: serde_json::Value = send_with_retry(
        client
            .post("https://api.github.com/graphql")
            .header(USER_AGENT, "Winthorpe")
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .json(&merge_body),
    )
    .context("Failed to call mergePullRequest")?
    .json()
    .context("Failed to decode merge response")?;

    if let Some(errors) = merge_response.get("errors") {
        if let Some(arr) = errors.as_array() {
            if !arr.is_empty() {
                let msgs: Vec<&str> = arr
                    .iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect();
                bail!("mergePullRequest failed: {}", msgs.join("; "));
            }
        }
    }

    // Return refreshed PR info
    lookup_workspace_pr(workspace_id)
}

/// Close a workspace's open PR via the GitHub GraphQL `closePullRequest`
/// mutation. Returns the updated `ChangeRequestInfo` on success.
pub fn close_workspace_pr(workspace_id: &str) -> Result<Option<ChangeRequestInfo>> {
    let pr = lookup_workspace_pr(workspace_id)?;
    let Some(pr) = pr else {
        return Ok(None);
    };
    if pr.state != "OPEN" {
        bail!("PR #{} is not open (state: {})", pr.number, pr.state);
    }

    let access_token = auth::load_valid_github_access_token()?;
    let Some(access_token) = access_token else {
        return Ok(None);
    };

    let Some(record) = workspace_models::load_workspace_record_by_id(workspace_id)? else {
        bail!("Workspace not found: {workspace_id}");
    };
    let Some(remote_url) = record.remote_url.as_deref() else {
        return Ok(None);
    };
    let Some((owner, name)) = parse_github_remote(remote_url) else {
        return Ok(None);
    };
    let Some(branch) = record.branch.as_deref().filter(|b| !b.is_empty()) else {
        return Ok(None);
    };

    let client = Client::builder()
        .build()
        .context("Failed to build GitHub HTTP client")?;

    // Fetch PR node ID
    let id_query = r#"
query($owner: String!, $name: String!, $head: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(headRefName: $head, states: [OPEN], first: 1) {
      nodes { id, url, number, state, title, merged }
    }
  }
}
"#;
    let id_body = json!({
        "query": id_query,
        "variables": { "owner": owner, "name": name, "head": branch },
    });

    let id_response: GraphqlEnvelope = send_with_retry(
        client
            .post("https://api.github.com/graphql")
            .header(USER_AGENT, "Winthorpe")
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .json(&id_body),
    )
    .context("Failed to reach GitHub GraphQL API")?
    .json()
    .context("Failed to decode GraphQL response")?;

    let pr_node_id = id_response
        .data
        .and_then(|d| d.repository)
        .and_then(|r| r.pull_requests.nodes.into_iter().next())
        .map(|n| n.id);
    let Some(pr_node_id) = pr_node_id.flatten() else {
        bail!("Could not resolve PR node ID for #{}", pr.number);
    };

    let close_mutation = r#"
mutation($prId: ID!) {
  closePullRequest(input: { pullRequestId: $prId }) {
    pullRequest { url, number, state, title, merged }
  }
}
"#;
    let close_body = json!({
        "query": close_mutation,
        "variables": { "prId": pr_node_id },
    });

    let close_response: serde_json::Value = send_with_retry(
        client
            .post("https://api.github.com/graphql")
            .header(USER_AGENT, "Winthorpe")
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .json(&close_body),
    )
    .context("Failed to call closePullRequest")?
    .json()
    .context("Failed to decode close response")?;

    if let Some(errors) = close_response.get("errors") {
        if let Some(arr) = errors.as_array() {
            if !arr.is_empty() {
                let msgs: Vec<&str> = arr
                    .iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect();
                bail!("closePullRequest failed: {}", msgs.join("; "));
            }
        }
    }

    lookup_workspace_pr(workspace_id)
}

/// `true` when the workspace's local branch has a remote-tracking ref
/// (upstream config OR a `refs/remotes/<remote>/<branch>` known locally).
/// Used by both PR lookups to bail before hitting GitHub when the branch
/// can't possibly have a PR — avoids ghost matches against historical PRs
/// whose head branch happens to share the workspace's placeholder name.
fn workspace_branch_has_remote_tracking(record: &workspace_models::WorkspaceRecord) -> bool {
    let Ok(workspace_dir) =
        crate::data_dir::workspace_dir(&record.repo_name, &record.directory_name)
    else {
        return false;
    };
    if !workspace_dir.exists() {
        return false;
    }
    git_ops::resolve_remote_tracking_ref(&workspace_dir, record.remote.as_deref()).is_some()
}

/// Parse `https://github.com/owner/repo(.git)` and `git@github.com:owner/repo(.git)`
/// remotes into `(owner, repo)`. Returns `None` for non-GitHub remotes.
fn parse_github_remote(remote: &str) -> Option<(String, String)> {
    let remote = remote.trim();
    // SSH form: git@github.com:owner/repo(.git)
    if let Some(rest) = remote.strip_prefix("git@github.com:") {
        return split_owner_repo(rest.trim_end_matches(".git"));
    }
    // HTTPS form: https://github.com/owner/repo(.git)  or with auth prefix.
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "git://github.com/",
        "ssh://git@github.com/",
    ] {
        if let Some(rest) = remote.strip_prefix(prefix) {
            return split_owner_repo(rest.trim_end_matches(".git"));
        }
    }
    None
}

fn split_owner_repo(s: &str) -> Option<(String, String)> {
    let trimmed = s.trim_matches('/');
    let mut parts = trimmed.splitn(2, '/');
    let owner = parts.next()?.trim();
    let name = parts.next()?.trim();
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    Some((owner.to_string(), name.to_string()))
}

#[derive(Debug, Clone, Deserialize)]
struct GraphqlEnvelope {
    data: Option<GraphqlData>,
    errors: Option<Vec<GraphqlError>>,
}

#[derive(Debug, Clone, Deserialize)]
struct GraphqlData {
    repository: Option<Repository>,
}

#[derive(Debug, Clone, Deserialize)]
struct Repository {
    #[serde(rename = "pullRequests")]
    pull_requests: PullRequestConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct PullRequestConnection {
    nodes: Vec<PullRequestNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct PullRequestNode {
    /// GraphQL node ID (e.g. "PR_kwDO..."). Only populated when the query
    /// explicitly selects `id`; the lookup query omits it so this is
    /// `None` on the primary path.
    id: Option<String>,
    url: String,
    number: i64,
    state: String,
    title: String,
    merged: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GraphqlError {
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionGraphqlEnvelope {
    data: Option<ActionGraphqlData>,
    errors: Option<Vec<GraphqlError>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionGraphqlData {
    repository: Option<ActionRepository>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionRepository {
    #[serde(rename = "pullRequests")]
    pull_requests: ActionPullRequestConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionPullRequestConnection {
    nodes: Vec<ActionPullRequestNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionPullRequestNode {
    url: String,
    number: i64,
    state: String,
    title: String,
    merged: bool,
    review_decision: Option<String>,
    mergeable: Option<String>,
    commits: ActionCommitConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionCommitConnection {
    nodes: Vec<ActionPullRequestCommitNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionPullRequestCommitNode {
    commit: ActionCommitNode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionCommitNode {
    status_check_rollup: Option<ActionStatusCheckRollup>,
    deployments: ActionDeploymentConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionStatusCheckRollup {
    contexts: ActionCheckContextConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionCheckContextConnection {
    nodes: Vec<ActionCheckContextNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "__typename")]
enum ActionCheckContextNode {
    CheckRun(ActionCheckRunNode),
    StatusContext(ActionStatusContextNode),
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionCheckRunNode {
    database_id: Option<i64>,
    name: String,
    status: String,
    conclusion: Option<String>,
    details_url: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    check_suite: Option<ActionCheckSuite>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionCheckSuite {
    app: Option<ActionCheckApp>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionCheckApp {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionStatusContextNode {
    context: String,
    state: String,
    target_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionDeploymentConnection {
    nodes: Vec<ActionDeploymentNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionDeploymentNode {
    id: String,
    environment: Option<String>,
    latest_status: Option<ActionDeploymentStatusNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionDeploymentStatusNode {
    state: String,
    log_url: Option<String>,
    environment_url: Option<String>,
}

fn build_action_status(node: ActionPullRequestNode) -> ForgeActionStatus {
    let pr = ChangeRequestInfo {
        url: node.url,
        number: node.number,
        state: node.state,
        title: node.title,
        is_merged: node.merged,
    };
    let review_decision = node.review_decision;
    let mergeable = node.mergeable;
    let latest_commit = node
        .commits
        .nodes
        .into_iter()
        .next()
        .map(|node| node.commit);

    let checks = latest_commit
        .as_ref()
        .and_then(|commit| commit.status_check_rollup.as_ref())
        .map(|rollup| {
            dedupe_action_items(
                rollup
                    .contexts
                    .nodes
                    .iter()
                    .filter_map(normalize_check_context)
                    .collect(),
            )
        })
        .unwrap_or_default();

    let deployments = latest_commit
        .map(|commit| {
            commit
                .deployments
                .nodes
                .iter()
                .map(normalize_deployment)
                .collect()
        })
        .unwrap_or_default();

    ForgeActionStatus {
        change_request: Some(pr),
        review_decision,
        mergeable,
        deployments,
        checks,
        remote_state: RemoteState::Ok,
        message: None,
    }
}

fn normalize_check_context(node: &ActionCheckContextNode) -> Option<ForgeActionItem> {
    match node {
        ActionCheckContextNode::CheckRun(check) => {
            let app_name = check
                .check_suite
                .as_ref()
                .and_then(|suite| suite.app.as_ref())
                .map(|app| app.name.as_str());
            let url = check.details_url.clone();
            let provider = infer_provider(
                ActionProvider::Unknown,
                [Some(check.name.as_str()), app_name, url.as_deref()],
            );
            let provider = if provider == ActionProvider::Unknown {
                ActionProvider::Github
            } else {
                provider
            };
            Some(ForgeActionItem {
                id: check
                    .database_id
                    .map(|id| format!("check-run-{id}"))
                    .unwrap_or_else(|| format!("check-run-{}", check.name)),
                name: check.name.clone(),
                provider,
                status: normalize_check_run_status(&check.status, check.conclusion.as_deref()),
                duration: format_duration(
                    check.started_at.as_deref(),
                    check.completed_at.as_deref(),
                ),
                url,
            })
        }
        ActionCheckContextNode::StatusContext(status) => {
            let url = status.target_url.clone();
            let provider = infer_provider(
                ActionProvider::Github,
                [Some(status.context.as_str()), url.as_deref(), None],
            );
            Some(ForgeActionItem {
                id: format!("status-context-{}", status.context),
                name: status.context.clone(),
                provider,
                status: normalize_status_context_state(&status.state),
                duration: None,
                url,
            })
        }
        ActionCheckContextNode::Other => None,
    }
}

fn normalize_deployment(node: &ActionDeploymentNode) -> ForgeActionItem {
    let latest = node.latest_status.as_ref();
    let log_url = latest.and_then(|status| status.log_url.clone());
    let environment_url = latest.and_then(|status| status.environment_url.clone());
    let url = environment_url.or(log_url);
    let environment = node
        .environment
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Deployment");
    let provider = infer_provider(
        ActionProvider::Unknown,
        [
            Some(environment),
            url.as_deref(),
            latest.map(|status| status.state.as_str()),
        ],
    );

    ForgeActionItem {
        id: node.id.clone(),
        name: environment.to_string(),
        provider,
        status: latest
            .map(|status| normalize_deployment_state(&status.state))
            .unwrap_or(ActionStatusKind::Pending),
        duration: None,
        url,
    }
}

fn dedupe_action_items(items: Vec<ForgeActionItem>) -> Vec<ForgeActionItem> {
    let mut deduped = BTreeMap::<String, ForgeActionItem>::new();

    for item in items {
        let key = format!("{:?}::{}", item.provider, item.name);
        match deduped.get(&key) {
            Some(existing)
                if action_status_priority(existing.status)
                    < action_status_priority(item.status) => {}
            _ => {
                deduped.insert(key, item);
            }
        }
    }

    deduped.into_values().collect()
}

fn action_status_priority(status: ActionStatusKind) -> u8 {
    match status {
        ActionStatusKind::Failure => 0,
        ActionStatusKind::Running => 1,
        ActionStatusKind::Pending => 2,
        ActionStatusKind::Success => 3,
    }
}

fn normalize_check_run_status(status: &str, conclusion: Option<&str>) -> ActionStatusKind {
    match status {
        "COMPLETED" => match conclusion {
            Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => ActionStatusKind::Success,
            _ => ActionStatusKind::Failure,
        },
        "IN_PROGRESS" => ActionStatusKind::Running,
        "WAITING" | "REQUESTED" | "QUEUED" | "PENDING" => ActionStatusKind::Pending,
        _ => ActionStatusKind::Pending,
    }
}

fn normalize_status_context_state(state: &str) -> ActionStatusKind {
    match state {
        "SUCCESS" => ActionStatusKind::Success,
        "FAILURE" | "ERROR" => ActionStatusKind::Failure,
        "PENDING" => ActionStatusKind::Running,
        _ => ActionStatusKind::Pending,
    }
}

fn normalize_deployment_state(state: &str) -> ActionStatusKind {
    match state {
        "SUCCESS" => ActionStatusKind::Success,
        "FAILURE" | "ERROR" | "INACTIVE" => ActionStatusKind::Failure,
        "PENDING" | "QUEUED" => ActionStatusKind::Pending,
        _ => ActionStatusKind::Running,
    }
}

fn infer_provider<'a>(
    default_provider: ActionProvider,
    values: impl IntoIterator<Item = Option<&'a str>>,
) -> ActionProvider {
    let mut saw_github = false;
    for value in values.into_iter().flatten() {
        let value = value.to_ascii_lowercase();
        if value.contains("vercel") {
            return ActionProvider::Vercel;
        }
        if value.contains("github") {
            saw_github = true;
        }
    }
    if saw_github {
        ActionProvider::Github
    } else {
        default_provider
    }
}

fn format_duration(started_at: Option<&str>, completed_at: Option<&str>) -> Option<String> {
    let started = parse_github_datetime(started_at?)?;
    let completed = parse_github_datetime(completed_at?)?;
    let seconds = (completed - started).num_seconds();
    if seconds < 0 {
        return None;
    }
    if seconds < 60 {
        return Some(format!("{seconds}s"));
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return Some(format!("{minutes}m"));
    }
    Some(format!("{}h", minutes / 60))
}

fn build_check_insert_text(
    item: &ForgeActionItem,
    detail: Option<&GithubCheckRunDetail>,
) -> String {
    let url = detail
        .and_then(|run| run.details_url.as_deref().or(run.html_url.as_deref()))
        .or(item.url.as_deref());

    let mut sections = vec![format!(
        "Check: {}\nProvider: {}\nStatus: {}{}{}",
        item.name,
        action_provider_label(item.provider),
        action_status_label(item.status),
        item.duration
            .as_deref()
            .map(|duration| format!("\nDuration: {duration}"))
            .unwrap_or_default(),
        url.map(|value| format!("\nURL: {value}"))
            .unwrap_or_default(),
    )];

    if let Some(title) = detail
        .and_then(|run| run.output.as_ref())
        .and_then(|output| output.title.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Content Title:\n{title}"));
    }

    if let Some(summary) = detail
        .and_then(|run| run.output.as_ref())
        .and_then(|output| output.summary.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Content Summary:\n{summary}"));
    }

    if let Some(text) = detail
        .and_then(|run| run.output.as_ref())
        .and_then(|output| output.text.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Content Log:\n{text}"));
    }

    if sections.len() == 1 {
        sections
            .push("Content Log:\nDetailed log text is not available for this check.".to_string());
    }

    sections.join("\n\n")
}

fn action_provider_label(provider: ActionProvider) -> &'static str {
    match provider {
        ActionProvider::Github => "GitHub",
        ActionProvider::Gitlab => "GitLab",
        ActionProvider::Vercel => "Vercel",
        ActionProvider::Unknown => "Unknown",
    }
}

fn action_status_label(status: ActionStatusKind) -> &'static str {
    match status {
        ActionStatusKind::Success => "success",
        ActionStatusKind::Pending => "pending",
        ActionStatusKind::Running => "running",
        ActionStatusKind::Failure => "failure",
    }
}

fn parse_github_datetime(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|datetime| datetime.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_remote() {
        let parsed = parse_github_remote("https://github.com/octocat/hello-world.git");
        assert_eq!(
            parsed,
            Some(("octocat".to_string(), "hello-world".to_string()))
        );
    }

    #[test]
    fn parses_https_remote_without_git_suffix() {
        let parsed = parse_github_remote("https://github.com/octocat/hello-world");
        assert_eq!(
            parsed,
            Some(("octocat".to_string(), "hello-world".to_string()))
        );
    }

    #[test]
    fn parses_ssh_remote() {
        let parsed = parse_github_remote("git@github.com:octocat/hello-world.git");
        assert_eq!(
            parsed,
            Some(("octocat".to_string(), "hello-world".to_string()))
        );
    }

    #[test]
    fn rejects_non_github_remote() {
        assert_eq!(parse_github_remote("https://gitlab.com/foo/bar.git"), None);
    }

    #[test]
    fn rejects_malformed_remote() {
        assert_eq!(parse_github_remote("https://github.com/"), None);
        assert_eq!(parse_github_remote("git@github.com:incomplete"), None);
    }

    #[test]
    fn parses_ssh_scheme_form() {
        let parsed = parse_github_remote("ssh://git@github.com/octocat/hello-world.git");
        assert_eq!(
            parsed,
            Some(("octocat".to_string(), "hello-world".to_string()))
        );
    }

    #[test]
    fn handles_trailing_slash_on_remote() {
        let parsed = parse_github_remote("https://github.com/octocat/hello-world/");
        assert_eq!(
            parsed,
            Some(("octocat".to_string(), "hello-world".to_string()))
        );
    }

    #[test]
    fn parses_padded_remote_input() {
        let parsed = parse_github_remote("  https://github.com/octocat/hello-world.git  ");
        assert_eq!(
            parsed,
            Some(("octocat".to_string(), "hello-world".to_string()))
        );
    }

    #[test]
    fn rejects_other_forges() {
        assert_eq!(parse_github_remote("https://gitlab.com/foo/bar.git"), None);
        assert_eq!(parse_github_remote("git@bitbucket.org:foo/bar.git"), None);
        assert_eq!(parse_github_remote("https://example.com/foo/bar"), None);
    }

    #[test]
    fn split_owner_repo_trims_whitespace() {
        // Inner helper — important because `parse_github_remote` already
        // strips the prefix but doesn't sanitise inside.
        assert_eq!(
            split_owner_repo("  octocat / hello-world  "),
            Some(("octocat".to_string(), "hello-world".to_string()))
        );
    }

    #[test]
    fn split_owner_repo_rejects_blank_segments() {
        assert_eq!(split_owner_repo(" / hello-world"), None);
        assert_eq!(split_owner_repo("octocat / "), None);
        assert_eq!(split_owner_repo("/"), None);
    }

    #[test]
    fn normalize_check_run_status_treats_unknown_completed_as_failure() {
        // Anything that lands in COMPLETED but isn't a "good" conclusion is
        // a failure — a key invariant for the action status rollup.
        assert_eq!(
            normalize_check_run_status("COMPLETED", None),
            ActionStatusKind::Failure
        );
        assert_eq!(
            normalize_check_run_status("COMPLETED", Some("CANCELLED")),
            ActionStatusKind::Failure
        );
        assert_eq!(
            normalize_check_run_status("COMPLETED", Some("TIMED_OUT")),
            ActionStatusKind::Failure
        );
    }

    #[test]
    fn normalize_check_run_status_unknown_status_falls_back_to_pending() {
        assert_eq!(
            normalize_check_run_status("FUTURE_STATE", None),
            ActionStatusKind::Pending
        );
        assert_eq!(
            normalize_check_run_status("", None),
            ActionStatusKind::Pending
        );
    }

    #[test]
    fn normalize_check_run_status_completed_neutral_is_success() {
        // GitHub treats NEUTRAL as a non-failure conclusion (e.g. checks
        // that opt out of red status). We mirror that.
        assert_eq!(
            normalize_check_run_status("COMPLETED", Some("NEUTRAL")),
            ActionStatusKind::Success
        );
    }

    #[test]
    fn action_status_priority_orders_failure_first() {
        // Helps the rollup pick the most-attention-grabbing status.
        let priorities: Vec<u8> = [
            ActionStatusKind::Failure,
            ActionStatusKind::Running,
            ActionStatusKind::Pending,
            ActionStatusKind::Success,
        ]
        .iter()
        .map(|s| action_status_priority(*s))
        .collect();
        assert_eq!(priorities, vec![0, 1, 2, 3]);
    }

    #[test]
    fn format_duration_returns_none_when_completion_before_start() {
        // Clock skew between runners can push completed-at before started-at;
        // we should return None rather than emit a negative string.
        assert!(
            format_duration(Some("2026-04-10T00:01:00Z"), Some("2026-04-10T00:00:30Z")).is_none()
        );
    }

    #[test]
    fn format_duration_returns_none_when_either_input_is_invalid() {
        assert!(format_duration(Some("not-a-date"), Some("2026-04-10T00:00:00Z")).is_none());
        assert!(format_duration(Some("2026-04-10T00:00:00Z"), Some("garbage")).is_none());
    }

    #[test]
    fn infer_provider_falls_back_to_default_when_no_known_match() {
        let provider = infer_provider(ActionProvider::Gitlab, [Some("custom-runner"), None]);
        assert_eq!(provider, ActionProvider::Gitlab);
    }

    #[test]
    fn infer_provider_vercel_wins_over_github_in_same_input() {
        // Vercel deployments often live on GitHub — Vercel should still win.
        let provider = infer_provider(
            ActionProvider::Unknown,
            [Some("https://vercel.com/team/app"), Some("github.com/x")],
        );
        assert_eq!(provider, ActionProvider::Vercel);
    }

    #[test]
    fn normalizes_check_run_statuses() {
        assert_eq!(
            normalize_check_run_status("COMPLETED", Some("SUCCESS")),
            ActionStatusKind::Success
        );
        assert_eq!(
            normalize_check_run_status("COMPLETED", Some("SKIPPED")),
            ActionStatusKind::Success
        );
        assert_eq!(
            normalize_check_run_status("COMPLETED", Some("FAILURE")),
            ActionStatusKind::Failure
        );
        assert_eq!(
            normalize_check_run_status("IN_PROGRESS", None),
            ActionStatusKind::Running
        );
        assert_eq!(
            normalize_check_run_status("QUEUED", None),
            ActionStatusKind::Pending
        );
    }

    #[test]
    fn normalizes_status_context_and_deployment_states() {
        assert_eq!(
            normalize_status_context_state("SUCCESS"),
            ActionStatusKind::Success
        );
        assert_eq!(
            normalize_status_context_state("ERROR"),
            ActionStatusKind::Failure
        );
        assert_eq!(
            normalize_status_context_state("PENDING"),
            ActionStatusKind::Running
        );
        assert_eq!(
            normalize_deployment_state("SUCCESS"),
            ActionStatusKind::Success
        );
        assert_eq!(
            normalize_deployment_state("FAILURE"),
            ActionStatusKind::Failure
        );
        assert_eq!(
            normalize_deployment_state("IN_PROGRESS"),
            ActionStatusKind::Running
        );
    }

    #[test]
    fn infers_action_providers() {
        assert_eq!(
            infer_provider(
                ActionProvider::Unknown,
                [Some("Vercel – app"), Some("https://vercel.com/team/app")]
            ),
            ActionProvider::Vercel
        );
        assert_eq!(
            infer_provider(
                ActionProvider::Unknown,
                [
                    Some("GitHub Actions"),
                    Some("https://github.com/org/repo/actions")
                ]
            ),
            ActionProvider::Github
        );
        assert_eq!(
            infer_provider(ActionProvider::Unknown, [Some("custom-ci"), None]),
            ActionProvider::Unknown
        );
    }

    #[test]
    fn formats_check_run_durations() {
        assert_eq!(
            format_duration(Some("2026-04-10T00:00:00Z"), Some("2026-04-10T00:00:12Z")).as_deref(),
            Some("12s")
        );
        assert_eq!(
            format_duration(Some("2026-04-10T00:00:00Z"), Some("2026-04-10T00:02:03Z")).as_deref(),
            Some("2m")
        );
        assert_eq!(
            format_duration(Some("2026-04-10T00:00:00Z"), Some("2026-04-10T01:20:00Z")).as_deref(),
            Some("1h")
        );
        assert_eq!(format_duration(None, Some("2026-04-10T00:00:00Z")), None);
    }

    #[test]
    fn builds_action_status_with_review_and_mergeable_fields() {
        let status = build_action_status(ActionPullRequestNode {
            url: "https://github.com/octocat/hello-world/pull/1".to_string(),
            number: 1,
            state: "OPEN".to_string(),
            title: "Update".to_string(),
            merged: false,
            review_decision: Some("CHANGES_REQUESTED".to_string()),
            mergeable: Some("CONFLICTING".to_string()),
            commits: ActionCommitConnection {
                nodes: vec![ActionPullRequestCommitNode {
                    commit: ActionCommitNode {
                        status_check_rollup: Some(ActionStatusCheckRollup {
                            contexts: ActionCheckContextConnection {
                                nodes: vec![ActionCheckContextNode::CheckRun(ActionCheckRunNode {
                                    database_id: Some(42),
                                    name: "changes".to_string(),
                                    status: "COMPLETED".to_string(),
                                    conclusion: Some("SUCCESS".to_string()),
                                    details_url: Some(
                                        "https://github.com/octocat/hello-world/actions/runs/1"
                                            .to_string(),
                                    ),
                                    started_at: Some("2026-04-10T00:00:00Z".to_string()),
                                    completed_at: Some("2026-04-10T00:00:12Z".to_string()),
                                    check_suite: Some(ActionCheckSuite {
                                        app: Some(ActionCheckApp {
                                            name: "GitHub Actions".to_string(),
                                        }),
                                    }),
                                })],
                            },
                        }),
                        deployments: ActionDeploymentConnection {
                            nodes: vec![ActionDeploymentNode {
                                id: "deployment-1".to_string(),
                                environment: Some("Vercel Preview".to_string()),
                                latest_status: Some(ActionDeploymentStatusNode {
                                    state: "SUCCESS".to_string(),
                                    log_url: Some("https://vercel.com/log".to_string()),
                                    environment_url: Some("https://app.vercel.app".to_string()),
                                }),
                            }],
                        },
                    },
                }],
            },
        });

        assert_eq!(status.remote_state, RemoteState::Ok);
        assert_eq!(status.review_decision.as_deref(), Some("CHANGES_REQUESTED"));
        assert_eq!(status.mergeable.as_deref(), Some("CONFLICTING"));
        assert_eq!(status.checks.len(), 1);
        assert_eq!(status.checks[0].status, ActionStatusKind::Success);
        assert_eq!(status.checks[0].provider, ActionProvider::Github);
        assert_eq!(status.checks[0].duration.as_deref(), Some("12s"));
        assert_eq!(status.deployments.len(), 1);
        assert_eq!(status.deployments[0].provider, ActionProvider::Vercel);
    }

    #[test]
    fn deduplicates_duplicate_check_runs_with_same_name() {
        let status = build_action_status(ActionPullRequestNode {
            url: "https://github.com/octocat/hello-world/pull/1".to_string(),
            number: 1,
            state: "OPEN".to_string(),
            title: "Update".to_string(),
            merged: false,
            review_decision: None,
            mergeable: Some("MERGEABLE".to_string()),
            commits: ActionCommitConnection {
                nodes: vec![ActionPullRequestCommitNode {
                    commit: ActionCommitNode {
                        status_check_rollup: Some(ActionStatusCheckRollup {
                            contexts: ActionCheckContextConnection {
                                nodes: vec![
                                    ActionCheckContextNode::CheckRun(ActionCheckRunNode {
                                        database_id: Some(101),
                                        name: "Lint".to_string(),
                                        status: "COMPLETED".to_string(),
                                        conclusion: Some("SUCCESS".to_string()),
                                        details_url: Some(
                                            "https://github.com/octocat/hello-world/actions/runs/1"
                                                .to_string(),
                                        ),
                                        started_at: Some("2026-04-16T01:45:30Z".to_string()),
                                        completed_at: Some("2026-04-16T01:45:36Z".to_string()),
                                        check_suite: Some(ActionCheckSuite {
                                            app: Some(ActionCheckApp {
                                                name: "GitHub Actions".to_string(),
                                            }),
                                        }),
                                    }),
                                    ActionCheckContextNode::CheckRun(ActionCheckRunNode {
                                        database_id: Some(202),
                                        name: "Lint".to_string(),
                                        status: "IN_PROGRESS".to_string(),
                                        conclusion: None,
                                        details_url: Some(
                                            "https://github.com/octocat/hello-world/actions/runs/2"
                                                .to_string(),
                                        ),
                                        started_at: Some("2026-04-16T01:46:00Z".to_string()),
                                        completed_at: None,
                                        check_suite: Some(ActionCheckSuite {
                                            app: Some(ActionCheckApp {
                                                name: "GitHub Actions".to_string(),
                                            }),
                                        }),
                                    }),
                                ],
                            },
                        }),
                        deployments: ActionDeploymentConnection { nodes: vec![] },
                    },
                }],
            },
        });

        assert_eq!(status.checks.len(), 1);
        assert_eq!(status.checks[0].name, "Lint");
        assert_eq!(status.checks[0].status, ActionStatusKind::Running);
        assert_eq!(
            status.checks[0].url.as_deref(),
            Some("https://github.com/octocat/hello-world/actions/runs/2")
        );
    }

    #[test]
    fn builds_check_insert_text_with_detail_sections() {
        let text = build_check_insert_text(
            &ForgeActionItem {
                id: "check-run-42".to_string(),
                name: "changes".to_string(),
                provider: ActionProvider::Github,
                status: ActionStatusKind::Failure,
                duration: Some("12s".to_string()),
                url: Some("https://github.com/octocat/hello-world/actions/runs/1".to_string()),
            },
            Some(&GithubCheckRunDetail {
                details_url: Some(
                    "https://github.com/octocat/hello-world/actions/runs/1/job/99".to_string(),
                ),
                html_url: None,
                output: Some(GithubCheckRunOutput {
                    title: Some("Job failed".to_string()),
                    summary: Some("1 step failed".to_string()),
                    text: Some("Step 3: tests failed".to_string()),
                }),
            }),
        );

        assert!(text.contains("Check: changes"));
        assert!(text.contains("Provider: GitHub"));
        assert!(text.contains("Status: failure"));
        assert!(text.contains("Duration: 12s"));
        assert!(text.contains("Content Title:\nJob failed"));
        assert!(text.contains("Content Summary:\n1 step failed"));
        assert!(text.contains("Content Log:\nStep 3: tests failed"));
    }

    #[test]
    fn builds_check_insert_text_with_unavailable_log_fallback() {
        let text = build_check_insert_text(
            &ForgeActionItem {
                id: "status-context-ci".to_string(),
                name: "CI".to_string(),
                provider: ActionProvider::Github,
                status: ActionStatusKind::Pending,
                duration: None,
                url: None,
            },
            None,
        );

        assert!(text.contains("Check: CI"));
        assert!(text.contains("Status: pending"));
        assert!(text.contains("Content Log:\nDetailed log text is not available for this check."));
    }
}
