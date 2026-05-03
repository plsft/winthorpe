//! `gh` / `glab` status probing + Connect terminal flow.

#[cfg(target_os = "macos")]
use anyhow::Context;
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crate::github_cli;

use super::bundled;
#[cfg(target_os = "macos")]
use super::command::run_command_with_timeout;
use super::command::{command_detail, run_command};
use super::status_cache::{self, CacheableStatus, CachedEntry};
use super::types::{ForgeCliStatus, ForgeLabels, ForgeProvider};

const OPEN_TERMINAL_TIMEOUT: Duration = Duration::from_secs(10);
const GITLAB_CLI_STATUS_CACHE_TTL: Duration = Duration::from_secs(2);
const GITLAB_CLI_READY_DOWNGRADE_GRACE: Duration = Duration::from_secs(600);

type GitlabStatusCache = Mutex<HashMap<String, CachedEntry<ForgeCliStatus>>>;
static GITLAB_STATUS_CACHE: LazyLock<GitlabStatusCache> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl CacheableStatus for ForgeCliStatus {
    fn is_ready(&self) -> bool {
        matches!(self, ForgeCliStatus::Ready { .. })
    }
    fn should_debounce_ready_downgrade(&self) -> bool {
        // Only `Error` (network blip / glab wedged) is genuinely transient.
        // `Unauthenticated` reads from glab's local config — surface it
        // immediately on `glab auth logout`.
        matches!(self, ForgeCliStatus::Error { .. })
    }
}

pub fn get_forge_cli_status(provider: ForgeProvider, host: Option<&str>) -> Result<ForgeCliStatus> {
    match provider {
        ForgeProvider::Github => github_status(),
        ForgeProvider::Gitlab => gitlab_status(host.unwrap_or("gitlab.com")),
        ForgeProvider::Unknown => Ok(ForgeCliStatus::Error {
            provider,
            host: host.unwrap_or("unknown").to_string(),
            cli_name: String::new(),
            version: None,
            message: "Unknown forge provider.".to_string(),
        }),
    }
}

pub fn open_forge_cli_auth_terminal(provider: ForgeProvider, host: Option<&str>) -> Result<()> {
    let command = forge_cli_auth_command(provider, host)?;
    open_terminal_with_command(&command)
}

pub(crate) fn forge_cli_auth_command(
    provider: ForgeProvider,
    host: Option<&str>,
) -> Result<String> {
    Ok(match provider {
        ForgeProvider::Github => format!("{} auth login", bundled_program_token("gh")?),
        ForgeProvider::Gitlab => {
            let host = host.unwrap_or("gitlab.com");
            // Reject obviously broken hostnames before they reach AppleScript:
            // a newline would let the user inject extra `do script` commands.
            if host.contains(['\n', '\r']) {
                bail!("Invalid hostname (contains newline): {host:?}");
            }
            format!(
                "{} auth login --hostname {host}",
                bundled_program_token("glab")?
            )
        }
        ForgeProvider::Unknown => bail!("Unknown forge provider."),
    })
}

/// Absolute bundled path (shell-quoted). In release builds, missing the
/// bundled binary means the .app payload is broken — fail loudly rather
/// than spawning a Terminal session that immediately dies on
/// `command not found`. In dev (`debug_assertions`), fall back to PATH so
/// `bun run dev` keeps working without a full bundle.
fn bundled_program_token(program: &str) -> Result<String> {
    if let Some(path) = bundled::bundled_path_for(program) {
        return Ok(shell_single_quote(&path.display().to_string()));
    }
    if cfg!(debug_assertions) {
        return Ok(program.to_string());
    }
    bail!("Bundled `{program}` is missing; reinstall Winthorpe to recover")
}

/// `'foo'\''bar'`-style single quoting safe for /bin/sh.
fn shell_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

pub(crate) fn labels_for(provider: ForgeProvider) -> ForgeLabels {
    match provider {
        ForgeProvider::Github => ForgeLabels {
            provider_name: "GitHub".to_string(),
            cli_name: "gh".to_string(),
            change_request_name: "PR".to_string(),
            change_request_full_name: "pull request".to_string(),
            connect_action: "Connect GitHub".to_string(),
        },
        ForgeProvider::Gitlab => ForgeLabels {
            provider_name: "GitLab".to_string(),
            cli_name: "glab".to_string(),
            change_request_name: "MR".to_string(),
            change_request_full_name: "merge request".to_string(),
            connect_action: "Connect GitLab".to_string(),
        },
        ForgeProvider::Unknown => ForgeLabels {
            provider_name: "Git".to_string(),
            cli_name: String::new(),
            change_request_name: "change request".to_string(),
            change_request_full_name: "change request".to_string(),
            connect_action: String::new(),
        },
    }
}

pub(crate) fn github_status() -> Result<ForgeCliStatus> {
    github_status_from(github_cli::get_github_cli_status()?)
}

fn github_status_from(status: github_cli::GithubCliStatus) -> Result<ForgeCliStatus> {
    Ok(match status {
        github_cli::GithubCliStatus::Ready {
            host,
            login,
            version,
            message,
        } => ForgeCliStatus::Ready {
            provider: ForgeProvider::Github,
            host,
            cli_name: "gh".to_string(),
            login,
            version,
            message,
        },
        github_cli::GithubCliStatus::Unauthenticated {
            host,
            version,
            message,
        } => ForgeCliStatus::Unauthenticated {
            provider: ForgeProvider::Github,
            host,
            cli_name: "gh".to_string(),
            version,
            message,
            login_command: "gh auth login".to_string(),
        },
        github_cli::GithubCliStatus::Unavailable { host, message } => ForgeCliStatus::Error {
            provider: ForgeProvider::Github,
            host,
            cli_name: "gh".to_string(),
            version: None,
            message: format!(
                "Bundled GitHub CLI was not found. Reinstall Winthorpe to recover. ({message})"
            ),
        },
        github_cli::GithubCliStatus::Error {
            host,
            version,
            message,
        } => ForgeCliStatus::Error {
            provider: ForgeProvider::Github,
            host,
            cli_name: "gh".to_string(),
            version,
            message,
        },
    })
}

pub(crate) fn gitlab_status(host: &str) -> Result<ForgeCliStatus> {
    status_cache::load_cached(
        &GITLAB_STATUS_CACHE,
        host.to_string(),
        GITLAB_CLI_STATUS_CACHE_TTL,
        GITLAB_CLI_READY_DOWNGRADE_GRACE,
        || gitlab_status_raw(host),
    )
}

fn gitlab_status_raw(host: &str) -> Result<ForgeCliStatus> {
    tracing::debug!(host, "Checking GitLab CLI status");
    let version = match run_command("glab", ["--version"]) {
        Ok(output) => Some(parse_glab_version(&output.stdout)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(host, "Bundled GitLab CLI not found");
            return Ok(ForgeCliStatus::Error {
                provider: ForgeProvider::Gitlab,
                host: host.to_string(),
                cli_name: "glab".to_string(),
                version: None,
                message: "Bundled GitLab CLI was not found. Reinstall Winthorpe to recover."
                    .to_string(),
            });
        }
        Err(error) => {
            tracing::warn!(host, error = %error, "Unable to read GitLab CLI version");
            return Ok(ForgeCliStatus::Error {
                provider: ForgeProvider::Gitlab,
                host: host.to_string(),
                cli_name: "glab".to_string(),
                version: None,
                message: format!("Unable to read GitLab CLI version: {error}"),
            });
        }
    };

    match run_command("glab", ["auth", "status", "--hostname", host]) {
        Ok(output) if output.success => {
            let login = parse_glab_login(&output.stderr)
                .or_else(|| parse_glab_login(&output.stdout))
                .unwrap_or_else(|| "authenticated".to_string());
            tracing::debug!(host, login, "GitLab CLI authenticated");
            Ok(ForgeCliStatus::Ready {
                provider: ForgeProvider::Gitlab,
                host: host.to_string(),
                cli_name: "glab".to_string(),
                login: login.clone(),
                version: version.unwrap_or_else(|| "unknown".to_string()),
                message: format!("GitLab CLI ready as {login}."),
            })
        }
        Ok(output) => {
            let detail = command_detail(&output);
            if looks_like_glab_unauthenticated(&detail) {
                tracing::warn!(host, detail = %detail, "GitLab CLI unauthenticated");
                Ok(ForgeCliStatus::Unauthenticated {
                    provider: ForgeProvider::Gitlab,
                    host: host.to_string(),
                    cli_name: "glab".to_string(),
                    version,
                    message: format!(
                        "Run `glab auth login --hostname {host}` to connect GitLab CLI."
                    ),
                    login_command: format!("glab auth login --hostname {host}"),
                })
            } else {
                tracing::warn!(host, detail = %detail, "GitLab CLI auth check failed");
                Ok(ForgeCliStatus::Error {
                    provider: ForgeProvider::Gitlab,
                    host: host.to_string(),
                    cli_name: "glab".to_string(),
                    version,
                    message: format!("GitLab CLI auth check failed: {detail}"),
                })
            }
        }
        Err(error) => {
            tracing::warn!(host, error = %error, "Failed to run GitLab CLI auth check");
            Ok(ForgeCliStatus::Error {
                provider: ForgeProvider::Gitlab,
                host: host.to_string(),
                cli_name: "glab".to_string(),
                version,
                message: format!("GitLab CLI auth check failed: {error}"),
            })
        }
    }
}

#[cfg(target_os = "macos")]
fn open_terminal_with_command(command: &str) -> Result<()> {
    let script = terminal_auth_script(command);
    let output = run_command_with_timeout(
        "osascript",
        ["-e".to_string(), script],
        OPEN_TERMINAL_TIMEOUT,
    )
    .context("Failed to open Terminal")?;
    if !output.success {
        bail!("Failed to open Terminal: {}", output.stderr.trim());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn terminal_auth_script(command: &str) -> String {
    format!(
        r#"set terminalWasRunning to application "Terminal" is running
tell application "Terminal"
    if terminalWasRunning then
        do script "{}"
    else
        activate
        delay 0.2
        if (count of windows) = 0 then
            do script ""
            delay 0.1
        end if
        do script "{}" in selected tab of front window
    end if
    activate
end tell"#,
        applescript_string(command),
        applescript_string(command)
    )
}

#[cfg(not(target_os = "macos"))]
fn open_terminal_with_command(_command: &str) -> Result<()> {
    bail!("Opening a terminal for forge CLI auth is only supported on macOS right now.")
}

fn applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn parse_glab_version(stdout: &str) -> String {
    stdout
        .lines()
        .next()
        .and_then(|line| {
            line.split_whitespace()
                .find(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        })
        .unwrap_or("unknown")
        .to_string()
}

/// `glab auth status` decorates each detail line with leading whitespace
/// and a status glyph (`✓`, `✗`, `*`, etc.) before the human prose. Strip
/// the decoration, locate `Logged in to <host> as <user>`, and stop at the
/// first separator after the username so trailing config paths or punctuation
/// don't leak into the parsed login.
fn parse_glab_login(text: &str) -> Option<String> {
    for raw in text.lines() {
        let body = raw.trim_start_matches(|c: char| {
            c.is_whitespace() || c == '✓' || c == '✗' || c == '*' || c == '-' || c == '•'
        });
        let Some(after_to) = body.strip_prefix("Logged in to ") else {
            continue;
        };
        let Some((_, after_as)) = after_to.split_once(" as ") else {
            continue;
        };
        let login = after_as
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
            .next()
            .unwrap_or("")
            .trim_end_matches(['.', ',', ';', ':']);
        if !login.is_empty() {
            return Some(login.to_string());
        }
    }
    None
}

/// Match `glab` stderr that conclusively indicates "no valid auth on file"
/// (token absent / revoked / expired). Avoid bare substrings like
/// `authentication` that would also catch transient errors like
/// "authentication endpoint unreachable".
fn looks_like_glab_unauthenticated(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("401 unauthorized")
        || normalized.contains("no token found")
        || normalized.contains("not logged in")
        || normalized.contains("not authenticated")
        || normalized.contains("authentication failed")
        || normalized.contains("glab auth login")
        || normalized.contains("has not been authenticated")
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn terminal_auth_script_reuses_initial_window_when_terminal_was_not_running() {
        let script = terminal_auth_script("glab auth login --hostname gitlab.com");

        assert!(script.contains("set terminalWasRunning to application \"Terminal\" is running"));
        assert!(script.contains("if terminalWasRunning then"));
        assert!(script.contains("do script \"glab auth login --hostname gitlab.com\""));
        assert!(
            script.contains(
                "do script \"glab auth login --hostname gitlab.com\" in selected tab of front window"
            ),
            "cold-start path must not create a second command window"
        );
    }

    #[test]
    fn shell_single_quote_handles_embedded_single_quotes() {
        assert_eq!(shell_single_quote("/usr/bin/gh"), "'/usr/bin/gh'");
        assert_eq!(
            shell_single_quote("/Apps/Tom's Stuff/Winthorpe.app/Contents/Resources/vendor/gh/gh"),
            "'/Apps/Tom'\\''s Stuff/Winthorpe.app/Contents/Resources/vendor/gh/gh'"
        );
        assert_eq!(shell_single_quote("a'b'c"), "'a'\\''b'\\''c'");
    }

    #[test]
    fn parse_glab_login_extracts_username_from_decorated_line() {
        let stderr = concat!(
            "example.cn\n",
            "  ✓ Logged in to example.cn as liangeqiang (/Users/liangeqiang/.config/glab-cli/config.yml)\n",
            "  ✓ Git operations for example.cn configured to use https protocol.\n",
            "  ✓ Token found: ********\n",
        );
        assert_eq!(parse_glab_login(stderr), Some("liangeqiang".to_string()),);
    }

    #[test]
    fn parse_glab_login_handles_plain_legacy_format() {
        assert_eq!(
            parse_glab_login("Logged in to gitlab.com as octo"),
            Some("octo".to_string()),
        );
        assert_eq!(
            parse_glab_login("Logged in to gitlab.com as octo."),
            Some("octo".to_string()),
        );
    }

    #[test]
    fn parse_glab_login_returns_none_when_no_match() {
        assert_eq!(parse_glab_login(""), None);
        assert_eq!(parse_glab_login("✗ Not logged in"), None);
        assert_eq!(parse_glab_login("Some unrelated diagnostic output"), None);
    }

    #[test]
    fn parse_glab_login_skips_unrelated_lines_until_match() {
        let stderr = concat!(
            "Warning: Multiple config files found.\n",
            "  ✗ Some other check\n",
            "  ✓ Logged in to gitlab.example.com as ada-lovelace (/path/to/config)\n",
        );
        assert_eq!(parse_glab_login(stderr), Some("ada-lovelace".to_string()),);
    }

    #[test]
    fn looks_like_glab_unauthenticated_recognises_canonical_messages() {
        let real_world = "X example.cn has not been authenticated with glab. Run `glab auth login --hostname example.cn` to authenticate.";
        assert!(looks_like_glab_unauthenticated(real_world));
        assert!(looks_like_glab_unauthenticated("401 Unauthorized"));
        assert!(looks_like_glab_unauthenticated("No token found"));
        assert!(looks_like_glab_unauthenticated("you are not logged in"));
        assert!(looks_like_glab_unauthenticated(
            "Please run: glab auth login"
        ));
        assert!(looks_like_glab_unauthenticated("authentication failed"));
    }

    #[test]
    fn looks_like_glab_unauthenticated_rejects_transient_errors() {
        // These contain auth-related substrings but are NOT proof that the
        // user has no valid token — they're transient infra problems.
        assert!(!looks_like_glab_unauthenticated(
            "authentication endpoint unreachable"
        ));
        assert!(!looks_like_glab_unauthenticated(
            "two-factor authentication setup required"
        ));
        assert!(!looks_like_glab_unauthenticated("connection reset by peer"));
        assert!(!looks_like_glab_unauthenticated("internal server error"));
    }
}
