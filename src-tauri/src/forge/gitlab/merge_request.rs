//! Merge-request-shaped operations: look up the current workspace's MR,
//! convert a GitLab MR record into Winthorpe's neutral `ChangeRequestInfo`,
//! and translate GitLab's mergeable/state enums.

use anyhow::{bail, Context, Result};

use crate::error::ErrorCode;

use super::super::types::ChangeRequestInfo;

use super::api::{
    command_detail, encode_path_component, encode_query_value, glab_api, looks_like_auth_error,
    looks_like_missing_error,
};
use super::context::GitlabContext;
use super::types::GitlabMergeRequest;

/// Fetch the most recently updated MR that has the workspace's branch as
/// its source. Returns `None` when there is no MR (or when glab can't
/// authenticate / the repo isn't reachable — those cases look the same
/// to callers and should degrade gracefully).
pub(super) fn find_workspace_mr(context: &GitlabContext) -> Result<Option<GitlabMergeRequest>> {
    let endpoint = format!(
        "projects/{}/merge_requests?source_branch={}&state=all&order_by=updated_at&sort=desc&per_page=1",
        encode_path_component(&context.full_path),
        encode_query_value(&context.branch),
    );
    let output = glab_api(&context.remote.host, [endpoint.as_str()])?;
    if !output.success {
        let detail = command_detail(&output);
        if looks_like_auth_error(&detail) {
            crate::bail_coded!(
                ErrorCode::ForgeOnboarding,
                "GitLab CLI authentication required: {detail}"
            );
        }
        if looks_like_missing_error(&detail) {
            return Ok(None);
        }
        bail!("GitLab MR lookup failed: {detail}");
    }

    let mut items = serde_json::from_str::<Vec<GitlabMergeRequest>>(&output.stdout)
        .context("Failed to decode GitLab merge request response")?;
    let Some(mr) = items.pop() else {
        return Ok(None);
    };
    fetch_mr_detail(context, mr.iid).map(Some)
}

fn fetch_mr_detail(context: &GitlabContext, iid: i64) -> Result<GitlabMergeRequest> {
    let endpoint = format!(
        "projects/{}/merge_requests/{iid}",
        encode_path_component(&context.full_path),
    );
    let output = glab_api(&context.remote.host, [endpoint.as_str()])?;
    if !output.success {
        let detail = command_detail(&output);
        if looks_like_auth_error(&detail) {
            crate::bail_coded!(
                ErrorCode::ForgeOnboarding,
                "GitLab CLI authentication required: {detail}"
            );
        }
        bail!("GitLab MR detail lookup failed: {detail}");
    }
    serde_json::from_str::<GitlabMergeRequest>(&output.stdout)
        .context("Failed to decode GitLab merge request detail response")
}

/// Collapse a GitLab MR record into the neutral shape the frontend
/// expects — the same struct GitHub's GraphQL path returns so the
/// inspector can render either provider without knowing which backend
/// produced the data.
pub(super) fn mr_info(mr: &GitlabMergeRequest) -> ChangeRequestInfo {
    ChangeRequestInfo {
        url: mr.web_url.clone(),
        number: mr.iid,
        state: gitlab_mr_state(&mr.state).to_string(),
        title: mr.title.clone(),
        is_merged: mr.state == "merged" || mr.merged_at.is_some(),
    }
}

pub(super) fn gitlab_mr_state(state: &str) -> &'static str {
    match state {
        "opened" => "OPEN",
        "merged" => "MERGED",
        "closed" => "CLOSED",
        _ => "UNKNOWN",
    }
}

/// Map GitLab's merge status to the same three-way enum GitHub's
/// `mergeable` field uses (`MERGEABLE` / `CONFLICTING` / `UNKNOWN`).
pub(super) fn gitlab_mergeable(mr: &GitlabMergeRequest) -> Option<String> {
    if mr.has_conflicts.unwrap_or(false) {
        return Some("CONFLICTING".to_string());
    }

    let status = mr
        .detailed_merge_status
        .as_deref()
        .or(mr.merge_status.as_deref())?;
    match status {
        "can_be_merged" | "mergeable" => Some("MERGEABLE".to_string()),
        "checking" | "unchecked" | "ci_must_pass" | "not_open" => Some("UNKNOWN".to_string()),
        value if value.contains("conflict") => Some("CONFLICTING".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_gitlab_mr_state_to_existing_pr_state_shape() {
        assert_eq!(gitlab_mr_state("opened"), "OPEN");
        assert_eq!(gitlab_mr_state("merged"), "MERGED");
        assert_eq!(gitlab_mr_state("closed"), "CLOSED");
        assert_eq!(gitlab_mr_state("locked"), "UNKNOWN");
        assert_eq!(gitlab_mr_state("draft"), "UNKNOWN");
    }

    fn mr_with_merge_status(
        merge_status: Option<&str>,
        detailed_merge_status: Option<&str>,
        has_conflicts: Option<bool>,
    ) -> GitlabMergeRequest {
        GitlabMergeRequest {
            iid: 1,
            title: "MR".to_string(),
            state: "opened".to_string(),
            web_url: "https://gitlab.example.com/acme/repo/-/merge_requests/1".to_string(),
            merged_at: None,
            merge_status: merge_status.map(str::to_string),
            detailed_merge_status: detailed_merge_status.map(str::to_string),
            has_conflicts,
            head_pipeline: None,
        }
    }

    #[test]
    fn maps_gitlab_mergeable_status_to_existing_shape() {
        let cases = [
            (Some("can_be_merged"), None, None, Some("MERGEABLE")),
            (Some("mergeable"), None, None, Some("MERGEABLE")),
            (Some("checking"), None, None, Some("UNKNOWN")),
            (Some("unchecked"), None, None, Some("UNKNOWN")),
            (Some("ci_must_pass"), None, None, Some("UNKNOWN")),
            (Some("not_open"), None, None, Some("UNKNOWN")),
            (Some("has_conflicts"), None, None, Some("CONFLICTING")),
            (Some("mergeable"), None, Some(true), Some("CONFLICTING")),
            (Some("mergeable"), Some("checking"), None, Some("UNKNOWN")),
            (Some("unexpected"), None, None, None),
        ];

        for (merge_status, detailed_status, has_conflicts, expected) in cases {
            let mr = mr_with_merge_status(merge_status, detailed_status, has_conflicts);
            assert_eq!(gitlab_mergeable(&mr).as_deref(), expected);
        }
    }
}
