use tauri::AppHandle;

use crate::{db, git_watcher, repos, settings};

use super::common::{run_blocking, CmdResult};

#[tauri::command]
pub async fn list_repositories() -> CmdResult<Vec<repos::RepositoryCreateOption>> {
    run_blocking(repos::list_repositories).await
}

#[tauri::command]
pub async fn get_add_repository_defaults() -> CmdResult<repos::AddRepositoryDefaults> {
    run_blocking(|| {
        // Resolve the default clone location with this priority:
        //   1. The user's last-used clone directory (persisted in settings).
        //   2. The directory Winthorpe was launched from, IF it looks like a
        //      project workspace (i.e. not a system path like
        //      C:\Windows\System32 or the user-profile root).
        //   3. The Documents directory as a sensible fallback.
        // Without this, users who launch Winthorpe from CLI in their project
        // folder would still see the home dir suggested for clone targets.
        let last_clone_directory = settings::load_setting_value("last_clone_directory")?;
        let resolved = last_clone_directory
            .or_else(launch_directory_for_clone_default)
            .or_else(documents_directory);
        Ok(repos::AddRepositoryDefaults {
            last_clone_directory: resolved,
        })
    })
    .await
}

/// Returns the process CWD if it's a sensible default for cloning into —
/// i.e. NOT a system directory (Windows/System32, Program Files), the user
/// profile root itself, or anywhere obviously not a code folder. Returns
/// None when the CWD is unsuitable (then we fall through to Documents).
fn launch_directory_for_clone_default() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let cwd_str = cwd.display().to_string();
    let lower = cwd_str.to_ascii_lowercase().replace('/', "\\");

    // Windows-specific reject list: system paths and shortcut "Start in"
    // targets that are never user project folders.
    let bad_prefixes = [
        "c:\\windows",
        "c:\\program files",
        "c:\\program files (x86)",
    ];
    for bad in bad_prefixes {
        if lower.starts_with(bad) {
            return None;
        }
    }

    // Reject the user-profile root exactly (e.g. C:\Users\georg) — the
    // user said this is their complaint. Subdirectories of it (Code,
    // projects, etc.) are fine.
    if let Ok(home) = std::env::var("USERPROFILE") {
        if cwd_str.eq_ignore_ascii_case(&home)
            || cwd_str.eq_ignore_ascii_case(home.trim_end_matches('\\'))
        {
            return None;
        }
    }

    Some(cwd_str)
}

fn documents_directory() -> Option<String> {
    // %USERPROFILE%\Documents is the conventional "where users put project
    // folders" path on Windows. Fall through to home if it's missing.
    let home = std::env::var("USERPROFILE").ok()?;
    let docs = std::path::PathBuf::from(&home).join("Documents");
    if docs.is_dir() {
        return Some(docs.display().to_string());
    }
    Some(home)
}

#[tauri::command]
pub async fn add_repository_from_local_path(
    folder_path: String,
) -> CmdResult<repos::AddRepositoryResponse> {
    let _lock = db::WORKSPACE_FS_MUTATION_LOCK.lock().await;
    run_blocking(move || repos::add_repository_from_local_path(&folder_path)).await
}

#[tauri::command]
pub async fn clone_repository_from_url(
    git_url: String,
    clone_directory: String,
) -> CmdResult<repos::AddRepositoryResponse> {
    let _lock = db::WORKSPACE_FS_MUTATION_LOCK.lock().await;
    run_blocking(move || repos::clone_repository_from_url(&git_url, &clone_directory)).await
}

#[tauri::command]
pub async fn update_repository_default_branch(
    app: AppHandle,
    repo_id: String,
    default_branch: String,
) -> CmdResult<()> {
    run_blocking(move || repos::update_repository_default_branch(&repo_id, &default_branch))
        .await?;
    git_watcher::notify_workspace_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn update_repository_branch_prefix(
    repo_id: String,
    branch_prefix_custom: Option<String>,
) -> CmdResult<()> {
    run_blocking(move || {
        repos::update_repository_branch_prefix(&repo_id, branch_prefix_custom.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn update_repository_remote(
    app: AppHandle,
    repo_id: String,
    remote: String,
) -> CmdResult<repos::UpdateRepositoryRemoteResponse> {
    let result = run_blocking(move || repos::update_repository_remote(&repo_id, &remote)).await?;
    git_watcher::notify_workspace_changed(&app);
    Ok(result)
}

#[tauri::command]
pub async fn list_repo_remotes(repo_id: String) -> CmdResult<Vec<String>> {
    run_blocking(move || repos::list_repo_remotes(&repo_id)).await
}

#[tauri::command]
pub async fn load_repo_scripts(
    repo_id: String,
    workspace_id: Option<String>,
) -> CmdResult<repos::RepoScripts> {
    run_blocking(move || repos::load_repo_scripts(&repo_id, workspace_id.as_deref())).await
}

#[tauri::command]
pub async fn load_repo_preferences(repo_id: String) -> CmdResult<repos::RepoPreferences> {
    run_blocking(move || repos::load_repo_preferences(&repo_id)).await
}

#[tauri::command]
pub async fn update_repo_scripts(
    repo_id: String,
    setup_script: Option<String>,
    run_script: Option<String>,
    archive_script: Option<String>,
) -> CmdResult<()> {
    run_blocking(move || {
        repos::update_repo_scripts(
            &repo_id,
            setup_script.as_deref(),
            run_script.as_deref(),
            archive_script.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn update_repo_auto_run_setup(repo_id: String, enabled: bool) -> CmdResult<()> {
    run_blocking(move || repos::update_repo_auto_run_setup(&repo_id, enabled)).await
}

#[tauri::command]
pub async fn update_repo_preferences(
    repo_id: String,
    preferences: repos::RepoPreferences,
) -> CmdResult<()> {
    run_blocking(move || repos::update_repo_preferences(&repo_id, &preferences)).await
}

#[tauri::command]
pub async fn delete_repository(repo_id: String) -> CmdResult<()> {
    let _lock = db::WORKSPACE_FS_MUTATION_LOCK.lock().await;
    run_blocking(move || repos::delete_repository_cascade(&repo_id)).await
}
