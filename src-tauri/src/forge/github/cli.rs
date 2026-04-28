use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crate::forge::command::run_command;
use crate::forge::status_cache::{self, CacheableStatus, CachedEntry};

const GITHUB_HOST: &str = "github.com";
const GITHUB_REPOS_ENDPOINT: &str =
    "/user/repos?per_page=100&sort=updated&affiliation=owner,collaborator,organization_member";
const GITHUB_CLI_STATUS_CACHE_TTL: Duration = Duration::from_secs(2);
const GITHUB_CLI_READY_DOWNGRADE_GRACE: Duration = Duration::from_secs(600);

type GithubStatusCache = Mutex<HashMap<&'static str, CachedEntry<GithubCliStatus>>>;
static SYSTEM_GH_STATUS_CACHE: LazyLock<GithubStatusCache> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl CacheableStatus for GithubCliStatus {
    fn is_ready(&self) -> bool {
        matches!(self, GithubCliStatus::Ready { .. })
    }
    fn should_debounce_ready_downgrade(&self) -> bool {
        // `Unauthenticated` is conclusive — `gh auth status` reads the local
        // hosts.yml so it should surface immediately on a `gh auth logout`.
        // Only `Error` (network blip, gh momentarily wedged) is debounced.
        matches!(self, GithubCliStatus::Error { .. })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum GithubCliStatus {
    Ready {
        host: String,
        login: String,
        version: String,
        message: String,
    },
    Unauthenticated {
        host: String,
        version: Option<String>,
        message: String,
    },
    Unavailable {
        host: String,
        message: String,
    },
    Error {
        host: String,
        version: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubCliUser {
    pub login: String,
    pub id: i64,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepositorySummary {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub owner_login: String,
    pub private: bool,
    pub default_branch: Option<String>,
    pub html_url: String,
    pub updated_at: Option<String>,
    pub pushed_at: Option<String>,
}

#[derive(Debug, Clone)]
struct GhCommandOutput {
    stdout: String,
}

#[derive(Debug, Clone)]
enum GhCommandError {
    NotFound,
    Failed {
        stdout: String,
        stderr: String,
        code: Option<i32>,
    },
    Other(String),
}

trait GhCommandRunner {
    fn run<I, S>(&self, args: I) -> std::result::Result<GhCommandOutput, GhCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>;
}

pub fn get_github_cli_status() -> Result<GithubCliStatus> {
    status_cache::load_cached(
        &SYSTEM_GH_STATUS_CACHE,
        GITHUB_HOST,
        GITHUB_CLI_STATUS_CACHE_TTL,
        GITHUB_CLI_READY_DOWNGRADE_GRACE,
        || get_github_cli_status_with(&SystemGhRunner),
    )
}

pub fn refresh_github_cli_status() -> Result<GithubCliStatus> {
    status_cache::refresh_cached(&SYSTEM_GH_STATUS_CACHE, GITHUB_HOST, || {
        get_github_cli_status_with(&SystemGhRunner)
    })
}

pub fn get_github_cli_user() -> Result<Option<GithubCliUser>> {
    let status = get_github_cli_status()?;
    get_github_cli_user_with_status(&SystemGhRunner, &status)
}

pub fn list_github_accessible_repositories() -> Result<Vec<GithubRepositorySummary>> {
    let status = get_github_cli_status()?;
    list_github_accessible_repositories_with_status(&SystemGhRunner, &status)
}

fn get_github_cli_status_with(runner: &impl GhCommandRunner) -> Result<GithubCliStatus> {
    tracing::debug!(host = GITHUB_HOST, "Checking GitHub CLI status");
    let version = match runner.run(["--version"]) {
        Ok(output) => Some(parse_gh_version(&output.stdout)),
        Err(GhCommandError::NotFound) => {
            tracing::warn!(host = GITHUB_HOST, "GitHub CLI binary not found");
            return Ok(GithubCliStatus::Unavailable {
                host: GITHUB_HOST.to_string(),
                message: "GitHub CLI is not installed on this machine.".to_string(),
            });
        }
        Err(GhCommandError::Failed {
            stdout,
            stderr,
            code,
        }) => {
            let detail = command_error_detail(&stdout, &stderr, code);
            tracing::warn!(
                host = GITHUB_HOST,
                code = ?code,
                detail = %detail,
                "GitHub CLI version probe exited non-zero"
            );
            return Ok(GithubCliStatus::Error {
                host: GITHUB_HOST.to_string(),
                version: None,
                message: format!("Unable to read GitHub CLI version: {detail}"),
            });
        }
        Err(GhCommandError::Other(message)) => {
            tracing::warn!(
                host = GITHUB_HOST,
                error = %message,
                "GitHub CLI version probe failed (IO error)"
            );
            return Ok(GithubCliStatus::Error {
                host: GITHUB_HOST.to_string(),
                version: None,
                message: format!("Unable to read GitHub CLI version: {message}"),
            });
        }
    };

    let auth_output = match runner.run([
        "auth",
        "status",
        "--hostname",
        GITHUB_HOST,
        "--json",
        "hosts",
    ]) {
        Ok(output) => output,
        Err(GhCommandError::NotFound) => {
            tracing::warn!(
                host = GITHUB_HOST,
                "GitHub CLI binary disappeared between probes"
            );
            return Ok(GithubCliStatus::Unavailable {
                host: GITHUB_HOST.to_string(),
                message: "GitHub CLI is not installed on this machine.".to_string(),
            });
        }
        Err(GhCommandError::Failed {
            stdout,
            stderr,
            code,
        }) => {
            let detail = command_error_detail(&stdout, &stderr, code);
            if looks_like_unauthenticated(&detail) {
                tracing::warn!(
                    host = GITHUB_HOST,
                    code = ?code,
                    detail = %detail,
                    "GitHub CLI unauthenticated"
                );
                return Ok(GithubCliStatus::Unauthenticated {
                    host: GITHUB_HOST.to_string(),
                    version,
                    message: "Run `gh auth login` to connect GitHub CLI.".to_string(),
                });
            }

            tracing::warn!(
                host = GITHUB_HOST,
                code = ?code,
                detail = %detail,
                "GitHub CLI auth check failed (transient or unknown)"
            );
            return Ok(GithubCliStatus::Error {
                host: GITHUB_HOST.to_string(),
                version,
                message: format!("GitHub CLI auth check failed: {detail}"),
            });
        }
        Err(GhCommandError::Other(message)) => {
            tracing::warn!(
                host = GITHUB_HOST,
                error = %message,
                "GitHub CLI auth check failed (IO error)"
            );
            return Ok(GithubCliStatus::Error {
                host: GITHUB_HOST.to_string(),
                version,
                message: format!("GitHub CLI auth check failed: {message}"),
            });
        }
    };

    let parsed =
        serde_json::from_str::<GhAuthStatusResponse>(&auth_output.stdout).map_err(|err| {
            tracing::error!(
                stdout = %auth_output.stdout,
                "Failed to decode `gh auth status --json hosts` output"
            );
            anyhow!("Failed to decode GitHub CLI auth status: {err}")
        })?;
    let host_entry = parsed
        .hosts
        .get(GITHUB_HOST)
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.active.unwrap_or(false))
                .or_else(|| entries.first())
        })
        .cloned()
        .context("GitHub CLI did not return auth status for github.com")?;

    let host = host_entry.host.unwrap_or_else(|| GITHUB_HOST.to_string());
    let login = host_entry.login.unwrap_or_default();

    if host_entry.state.as_deref() != Some("success") || login.trim().is_empty() {
        tracing::warn!(
            host = %host,
            state = ?host_entry.state,
            login_blank = login.trim().is_empty(),
            "GitHub CLI auth status JSON missing success/login"
        );
        return Ok(GithubCliStatus::Unauthenticated {
            host,
            version,
            message: "Run `gh auth login` to connect GitHub CLI.".to_string(),
        });
    }

    tracing::debug!(host = %host, login = %login, "GitHub CLI authenticated");
    Ok(GithubCliStatus::Ready {
        host,
        login: login.clone(),
        version: version.unwrap_or_else(|| "unknown".to_string()),
        message: format!("GitHub CLI ready as {login}."),
    })
}

#[cfg(test)]
fn get_github_cli_user_with(runner: &impl GhCommandRunner) -> Result<Option<GithubCliUser>> {
    let status = get_github_cli_status_with(runner)?;
    get_github_cli_user_with_status(runner, &status)
}

fn get_github_cli_user_with_status(
    runner: &impl GhCommandRunner,
    status: &GithubCliStatus,
) -> Result<Option<GithubCliUser>> {
    if !github_cli_is_ready(status) {
        return Ok(None);
    }

    let output = match runner.run(["api", "/user"]) {
        Ok(output) => output,
        Err(GhCommandError::NotFound) => {
            tracing::warn!("gh binary missing during /user lookup");
            return Ok(None);
        }
        Err(GhCommandError::Failed {
            stdout,
            stderr,
            code,
        }) => {
            let detail = command_error_detail(&stdout, &stderr, code);
            if looks_like_unauthenticated(&detail) {
                tracing::warn!(code = ?code, detail = %detail, "gh /user unauthenticated");
                return Ok(None);
            }

            tracing::warn!(code = ?code, detail = %detail, "gh /user lookup failed");
            return Err(anyhow!("GitHub CLI user lookup failed: {detail}"));
        }
        Err(GhCommandError::Other(message)) => {
            tracing::warn!(error = %message, "gh /user lookup failed (IO error)");
            return Err(anyhow!("GitHub CLI user lookup failed: {message}"));
        }
    };

    let parsed = serde_json::from_str::<GithubApiUser>(&output.stdout)
        .context("Failed to decode GitHub CLI /user response")?;

    Ok(Some(GithubCliUser {
        login: parsed.login,
        id: parsed.id,
        name: parsed.name,
        avatar_url: parsed.avatar_url,
        email: parsed.email,
    }))
}

#[cfg(test)]
fn list_github_accessible_repositories_with(
    runner: &impl GhCommandRunner,
) -> Result<Vec<GithubRepositorySummary>> {
    let status = get_github_cli_status_with(runner)?;
    list_github_accessible_repositories_with_status(runner, &status)
}

fn list_github_accessible_repositories_with_status(
    runner: &impl GhCommandRunner,
    status: &GithubCliStatus,
) -> Result<Vec<GithubRepositorySummary>> {
    if !github_cli_is_ready(status) {
        return Ok(Vec::new());
    }

    let output = match runner.run(["api", GITHUB_REPOS_ENDPOINT]) {
        Ok(output) => output,
        Err(GhCommandError::NotFound) => {
            tracing::warn!("gh binary missing during /user/repos lookup");
            return Ok(Vec::new());
        }
        Err(GhCommandError::Failed {
            stdout,
            stderr,
            code,
        }) => {
            let detail = command_error_detail(&stdout, &stderr, code);
            if looks_like_unauthenticated(&detail) {
                tracing::warn!(code = ?code, detail = %detail, "gh /user/repos unauthenticated");
                return Ok(Vec::new());
            }

            tracing::warn!(code = ?code, detail = %detail, "gh /user/repos lookup failed");
            return Err(anyhow!("GitHub CLI repository lookup failed: {detail}"));
        }
        Err(GhCommandError::Other(message)) => {
            tracing::warn!(error = %message, "gh /user/repos lookup failed (IO error)");
            return Err(anyhow!("GitHub CLI repository lookup failed: {message}"));
        }
    };

    let parsed = serde_json::from_str::<Vec<GithubApiRepository>>(&output.stdout)
        .context("Failed to decode GitHub CLI /user/repos response")?;

    Ok(parsed
        .into_iter()
        .map(|repository| GithubRepositorySummary {
            id: repository.id,
            name: repository.name,
            full_name: repository.full_name,
            owner_login: repository.owner.login,
            private: repository.private,
            default_branch: repository.default_branch,
            html_url: repository.html_url,
            updated_at: repository.updated_at,
            pushed_at: repository.pushed_at,
        })
        .collect())
}

fn parse_gh_version(stdout: &str) -> String {
    stdout
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(2))
        .unwrap_or("unknown")
        .to_string()
}

fn command_error_detail(stdout: &str, stderr: &str, code: Option<i32>) -> String {
    let trimmed_stderr = stderr.trim();
    if !trimmed_stderr.is_empty() {
        return trimmed_stderr.to_string();
    }

    let trimmed_stdout = stdout.trim();
    if !trimmed_stdout.is_empty() {
        return trimmed_stdout.to_string();
    }

    match code {
        Some(code) => format!("gh exited with status {code}"),
        None => "gh exited unsuccessfully".to_string(),
    }
}

/// Match `gh` output that conclusively means "no valid auth on file".
/// Avoid bare `401` / `unauthorized` / `unauthenticated` — those leak into
/// transient network errors (e.g. `401 Service Unavailable`,
/// `unauthenticated upstream timeout`) and would flap the UI on a network
/// blip. Mirror the whitelist style used by `looks_like_glab_unauthenticated`.
fn looks_like_unauthenticated(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("401 unauthorized")
        || normalized.contains("bad credentials")
        || normalized.contains("not logged into")
        || normalized.contains("not logged in")
        || normalized.contains("not authenticated")
        || normalized.contains("authentication failed")
        || normalized.contains("gh auth login")
        || normalized.contains("no token found")
        || normalized.contains("has not been authenticated")
}

fn github_cli_is_ready(status: &GithubCliStatus) -> bool {
    matches!(status, GithubCliStatus::Ready { .. })
}

struct SystemGhRunner;

impl GhCommandRunner for SystemGhRunner {
    fn run<I, S>(&self, args: I) -> std::result::Result<GhCommandOutput, GhCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = run_command("gh", args).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GhCommandError::NotFound
            } else {
                GhCommandError::Other(error.to_string())
            }
        })?;

        if output.success {
            return Ok(GhCommandOutput {
                stdout: output.stdout,
            });
        }

        Err(GhCommandError::Failed {
            stdout: output.stdout,
            stderr: output.stderr,
            code: output.status,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GhAuthStatusResponse {
    hosts: HashMap<String, Vec<GhHostStatusEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhHostStatusEntry {
    state: Option<String>,
    active: Option<bool>,
    host: Option<String>,
    login: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubApiUser {
    login: String,
    id: i64,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubApiRepository {
    id: i64,
    name: String,
    full_name: String,
    private: bool,
    default_branch: Option<String>,
    html_url: String,
    updated_at: Option<String>,
    pushed_at: Option<String>,
    owner: GithubApiRepositoryOwner,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubApiRepositoryOwner {
    login: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[derive(Clone)]
    enum MockRunnerResponse {
        Success {
            stdout: String,
            stderr: String,
        },
        NotFound,
        Failed {
            stdout: String,
            stderr: String,
            code: Option<i32>,
        },
        Other(String),
    }

    struct MockGhRunner {
        responses: RefCell<VecDeque<MockRunnerResponse>>,
    }

    impl MockGhRunner {
        fn new(responses: impl IntoIterator<Item = MockRunnerResponse>) -> Self {
            Self {
                responses: RefCell::new(responses.into_iter().collect()),
            }
        }
    }

    impl GhCommandRunner for MockGhRunner {
        fn run<I, S>(&self, _args: I) -> std::result::Result<GhCommandOutput, GhCommandError>
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            match self
                .responses
                .borrow_mut()
                .pop_front()
                .expect("mock response should exist")
            {
                MockRunnerResponse::Success { stdout, stderr } => {
                    let _ = stderr;
                    Ok(GhCommandOutput { stdout })
                }
                MockRunnerResponse::NotFound => Err(GhCommandError::NotFound),
                MockRunnerResponse::Failed {
                    stdout,
                    stderr,
                    code,
                } => Err(GhCommandError::Failed {
                    stdout,
                    stderr,
                    code,
                }),
                MockRunnerResponse::Other(message) => Err(GhCommandError::Other(message)),
            }
        }
    }

    #[test]
    fn get_github_cli_status_parses_ready_state() {
        let runner = MockGhRunner::new([
            MockRunnerResponse::Success {
                stdout:
                    "gh version 2.88.1 (2026-03-12)\nhttps://github.com/cli/cli/releases/tag/v2.88.1\n"
                        .to_string(),
                stderr: String::new(),
            },
            MockRunnerResponse::Success {
                stdout: r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"octocat"}]}}"#
                    .to_string(),
                stderr: String::new(),
            },
        ]);

        let status = get_github_cli_status_with(&runner).unwrap();

        assert_eq!(
            status,
            GithubCliStatus::Ready {
                host: "github.com".to_string(),
                login: "octocat".to_string(),
                version: "2.88.1".to_string(),
                message: "GitHub CLI ready as octocat.".to_string(),
            }
        );
    }

    #[test]
    fn get_github_cli_status_returns_unavailable_when_gh_is_missing() {
        let runner = MockGhRunner::new([MockRunnerResponse::NotFound]);

        let status = get_github_cli_status_with(&runner).unwrap();

        assert_eq!(
            status,
            GithubCliStatus::Unavailable {
                host: "github.com".to_string(),
                message: "GitHub CLI is not installed on this machine.".to_string(),
            }
        );
    }

    #[test]
    fn get_github_cli_status_returns_unauthenticated_when_gh_is_not_logged_in() {
        let runner = MockGhRunner::new([
            MockRunnerResponse::Success {
                stdout: "gh version 2.88.1 (2026-03-12)\n".to_string(),
                stderr: String::new(),
            },
            MockRunnerResponse::Failed {
                stdout: String::new(),
                stderr: "You are not logged into any GitHub hosts. To log in, run: gh auth login"
                    .to_string(),
                code: Some(1),
            },
        ]);

        let status = get_github_cli_status_with(&runner).unwrap();

        assert_eq!(
            status,
            GithubCliStatus::Unauthenticated {
                host: "github.com".to_string(),
                version: Some("2.88.1".to_string()),
                message: "Run `gh auth login` to connect GitHub CLI.".to_string(),
            }
        );
    }

    #[test]
    fn get_github_cli_user_parses_user_profile() {
        let runner = MockGhRunner::new([
            MockRunnerResponse::Success {
                stdout: "gh version 2.88.1 (2026-03-12)\n".to_string(),
                stderr: String::new(),
            },
            MockRunnerResponse::Success {
                stdout: r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"octocat"}]}}"#
                    .to_string(),
                stderr: String::new(),
            },
            MockRunnerResponse::Success {
                stdout: r#"{"login":"octocat","id":0,"name":"Octocat","avatar_url":"https://avatars.githubusercontent.com/u/0?v=4","email":"test@example.com"}"#
                    .to_string(),
                stderr: String::new(),
            },
        ]);

        let user = get_github_cli_user_with(&runner).unwrap();

        assert_eq!(
            user,
            Some(GithubCliUser {
                login: "octocat".to_string(),
                id: 0,
                name: Some("Octocat".to_string()),
                avatar_url: Some("https://avatars.githubusercontent.com/u/0?v=4".to_string()),
                email: Some("test@example.com".to_string()),
            })
        );
    }

    #[test]
    fn list_github_accessible_repositories_parses_repository_list() {
        let runner = MockGhRunner::new([
            MockRunnerResponse::Success {
                stdout: "gh version 2.88.1 (2026-03-12)\n".to_string(),
                stderr: String::new(),
            },
            MockRunnerResponse::Success {
                stdout: r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"octocat"}]}}"#
                    .to_string(),
                stderr: String::new(),
            },
            MockRunnerResponse::Success {
                stdout: r#"[{"id":0,"name":"winthorpe","full_name":"dohooo/winthorpe","private":false,"default_branch":"main","html_url":"https://github.com/dohooo/winthorpe","updated_at":"2026-01-01T00:00:00Z","pushed_at":"2026-01-01T00:00:00Z","owner":{"login":"dohooo"}}]"#
                    .to_string(),
                stderr: String::new(),
            },
        ]);

        let repositories = list_github_accessible_repositories_with(&runner).unwrap();

        assert_eq!(
            repositories,
            vec![GithubRepositorySummary {
                id: 0,
                name: "winthorpe".to_string(),
                full_name: "dohooo/winthorpe".to_string(),
                owner_login: "dohooo".to_string(),
                private: false,
                default_branch: Some("main".to_string()),
                html_url: "https://github.com/dohooo/winthorpe".to_string(),
                updated_at: Some("2026-01-01T00:00:00Z".to_string()),
                pushed_at: Some("2026-01-01T00:00:00Z".to_string()),
            }]
        );
    }

    #[test]
    fn get_github_cli_user_returns_none_when_unauthenticated() {
        let runner = MockGhRunner::new([
            MockRunnerResponse::Success {
                stdout: "gh version 2.88.1 (2026-03-12)\n".to_string(),
                stderr: String::new(),
            },
            MockRunnerResponse::Failed {
                stdout: String::new(),
                stderr: "You are not logged into any GitHub hosts. To log in, run: gh auth login"
                    .to_string(),
                code: Some(1),
            },
        ]);

        let user = get_github_cli_user_with(&runner).unwrap();

        assert_eq!(user, None);
    }

    #[test]
    fn get_github_cli_status_surfaces_version_lookup_errors() {
        let runner =
            MockGhRunner::new([MockRunnerResponse::Other("permission denied".to_string())]);

        let status = get_github_cli_status_with(&runner).unwrap();

        assert_eq!(
            status,
            GithubCliStatus::Error {
                host: "github.com".to_string(),
                version: None,
                message: "Unable to read GitHub CLI version: permission denied".to_string(),
            }
        );
    }

    #[test]
    fn looks_like_unauthenticated_matches_canonical_phrases() {
        assert!(looks_like_unauthenticated(
            "You are not logged into any GitHub hosts. Run gh auth login"
        ));
        assert!(looks_like_unauthenticated("HTTP 401: Bad credentials"));
        assert!(looks_like_unauthenticated("authentication failed"));
        assert!(looks_like_unauthenticated("no token found"));
    }

    #[test]
    fn looks_like_unauthenticated_ignores_transient_network_errors() {
        // 401 codes returned for non-auth reasons (rate limit, service degraded).
        assert!(!looks_like_unauthenticated("401 Service Unavailable"));
        // Bare "unauthenticated" / "unauthorized" must not match — they leak
        // into transient upstream errors.
        assert!(!looks_like_unauthenticated(
            "unauthenticated upstream timeout"
        ));
        assert!(!looks_like_unauthenticated("unauthorized origin: EOF"));
        // DNS / connect failures.
        assert!(!looks_like_unauthenticated(
            "Get \"https://api.github.com\": dial tcp: lookup api.github.com: no such host"
        ));
        assert!(!looks_like_unauthenticated(
            "connection reset by peer while reading response"
        ));
    }
}
