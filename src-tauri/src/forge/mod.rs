//! Forge abstraction — unifies GitHub and GitLab (pull requests / merge
//! requests, CI status, CLI install + auth).
//!
//! Layout:
//!
//! - [`types`] — serialisable public types (`ForgeProvider`, `ForgeDetection`,
//!   `ForgeCliStatus`, `DetectionSignal`, `ForgeLabels`, change-request and
//!   action-status shapes).
//! - [`remote`] — git remote URL parsing.
//! - [`command`] — bounded subprocess execution for forge CLIs.
//! - [`detect`] — the layered detector that classifies a repo's forge at
//!   creation time and backs the "Why do we think so?" tooltip.
//! - [`cli_status`] — gh / glab CLI probes + install paths.
//! - [`workspace`] — per-workspace router that dispatches change-request calls
//!   to the right backend once a provider is resolved.
//! - [`github`] — GitHub SDK (auth, CLI helpers, GraphQL). Moved here from
//!   the old crate-root `github` module so everything forge-shaped lives
//!   in one place. The crate-root aliases (`github_cli`, `github_graphql`,
//!   `auth`) in `lib.rs` still resolve, so existing call sites don't need
//!   to change.
//! - [`gitlab`] — GitLab REST client using `glab api`.

mod bundled;
mod cli_status;
pub(crate) mod command;
mod detect;
pub mod github;
mod gitlab;
mod provider;
pub(crate) mod remote;
pub(crate) mod status_cache;
mod types;
mod workspace;

pub use bundled::init as init_bundled_cli_paths;
pub(crate) use cli_status::forge_cli_auth_command;
pub use cli_status::{get_forge_cli_status, open_forge_cli_auth_terminal};
pub use detect::detect_provider_for_repo;
pub(crate) use detect::detect_provider_for_repo_offline;
pub use types::{
    ActionProvider, ActionStatusKind, ChangeRequestInfo, DetectionSignal, ForgeActionItem,
    ForgeActionStatus, ForgeCliStatus, ForgeDetection, ForgeLabels, ForgeProvider, RemoteState,
};
pub use workspace::{
    close_workspace_change_request, get_workspace_forge, lookup_workspace_forge_action_status,
    lookup_workspace_forge_check_insert_text, merge_workspace_change_request,
    refresh_workspace_change_request,
};
