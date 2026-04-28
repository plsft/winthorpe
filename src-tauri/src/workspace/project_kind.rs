//! Detect the primary project type in a workspace directory.
//!
//! Used by the UI to surface platform-aware action suggestions (e.g. show
//! "dotnet build" / "dotnet test" buttons for .NET workspaces, "bun install"
//! / "bun test" for Bun workspaces) without making the user wire up
//! `winthorpe.json` for every repo.
//!
//! Heuristic: enumerate the root once, look for marker files. If multiple
//! kinds match, return `Mixed` and let the UI offer all sets.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceProjectKind {
    /// `*.sln` or `*.csproj` / `*.fsproj` / `*.vbproj` present.
    DotNet,
    /// `bun.lock`, `bunfig.toml`, or `package.json` with a `"packageManager":
    /// "bun@..."` field present.
    Bun,
    /// `package.json` without Bun marker (npm/pnpm/yarn).
    Node,
    /// `Cargo.toml` present.
    Cargo,
    /// `pyproject.toml`, `requirements.txt`, or `setup.py` present.
    Python,
    /// `go.mod` present.
    Go,
    /// More than one of the above present at the root.
    Mixed,
    /// None matched. UI falls back to the user-provided scripts (if any).
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectKindDetection {
    pub kind: WorkspaceProjectKind,
    /// All marker files that triggered the detection. Helps the UI decide
    /// which sub-action set to show — e.g. "dotnet build <foo.sln>" with
    /// the actual filename rather than a generic "dotnet build".
    pub markers: Vec<String>,
    /// Suggested setup/build/test commands tailored to the detected kind.
    /// Empty when `kind == Unknown`. The UI shows these as quick-action
    /// chips that the user can run with one click.
    pub suggested_actions: Vec<SuggestedAction>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedAction {
    /// Short label shown in the UI: "Restore", "Build", "Test", "Run".
    pub label: String,
    /// Command line to execute via the workspace script runner.
    pub command: String,
    /// Lifecycle category: maps to existing setup/run/archive slots.
    pub category: ActionCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionCategory {
    Setup,
    Build,
    Test,
    Run,
    Other,
}

pub fn detect(workspace_root: &Path) -> ProjectKindDetection {
    let mut kinds: Vec<WorkspaceProjectKind> = Vec::new();
    let mut markers: Vec<String> = Vec::new();

    // Direct file probes — cheaper than read_dir for the common case.
    let probes: &[(&str, WorkspaceProjectKind)] = &[
        ("Cargo.toml", WorkspaceProjectKind::Cargo),
        ("go.mod", WorkspaceProjectKind::Go),
        ("pyproject.toml", WorkspaceProjectKind::Python),
        ("requirements.txt", WorkspaceProjectKind::Python),
        ("setup.py", WorkspaceProjectKind::Python),
        ("bun.lock", WorkspaceProjectKind::Bun),
        ("bunfig.toml", WorkspaceProjectKind::Bun),
    ];
    for (name, kind) in probes {
        if workspace_root.join(name).is_file() {
            push_kind(&mut kinds, *kind);
            markers.push((*name).to_string());
        }
    }

    // .NET solution / project files — single-pass dir read filtering by
    // extension. Don't recurse; the root is the source of truth.
    if let Ok(entries) = std::fs::read_dir(workspace_root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else {
                continue;
            };
            if name_str.ends_with(".sln")
                || name_str.ends_with(".csproj")
                || name_str.ends_with(".fsproj")
                || name_str.ends_with(".vbproj")
            {
                push_kind(&mut kinds, WorkspaceProjectKind::DotNet);
                markers.push(name_str.to_string());
            }
        }
    }

    // package.json: distinguish Bun from Node by the packageManager field.
    let pkg_json = workspace_root.join("package.json");
    if pkg_json.is_file() {
        let is_bun = std::fs::read_to_string(&pkg_json)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("packageManager")
                    .and_then(|pm| pm.as_str())
                    .map(|pm| pm.starts_with("bun@"))
            })
            .unwrap_or(false);
        if is_bun {
            push_kind(&mut kinds, WorkspaceProjectKind::Bun);
        } else if !kinds.contains(&WorkspaceProjectKind::Bun) {
            push_kind(&mut kinds, WorkspaceProjectKind::Node);
        }
        markers.push("package.json".to_string());
    }

    let kind = match kinds.len() {
        0 => WorkspaceProjectKind::Unknown,
        1 => kinds[0],
        _ => WorkspaceProjectKind::Mixed,
    };

    let suggested_actions = suggested_actions_for(kind, &markers);

    ProjectKindDetection {
        kind,
        markers,
        suggested_actions,
    }
}

fn push_kind(kinds: &mut Vec<WorkspaceProjectKind>, kind: WorkspaceProjectKind) {
    if !kinds.contains(&kind) {
        kinds.push(kind);
    }
}

fn suggested_actions_for(kind: WorkspaceProjectKind, markers: &[String]) -> Vec<SuggestedAction> {
    use ActionCategory::*;
    let mut out = Vec::new();

    match kind {
        WorkspaceProjectKind::DotNet => {
            // Pick the primary solution if present, else the first project.
            let solution = markers.iter().find(|m| m.ends_with(".sln")).cloned();
            let project = markers
                .iter()
                .find(|m| {
                    m.ends_with(".csproj") || m.ends_with(".fsproj") || m.ends_with(".vbproj")
                })
                .cloned();
            let target = solution.or(project).unwrap_or_default();
            let target_arg = if target.is_empty() {
                String::new()
            } else {
                format!(" \"{target}\"")
            };
            out.push(SuggestedAction {
                label: "Restore".into(),
                command: format!("dotnet restore{target_arg}"),
                category: Setup,
            });
            out.push(SuggestedAction {
                label: "Build".into(),
                command: format!("dotnet build{target_arg} -c Release"),
                category: Build,
            });
            out.push(SuggestedAction {
                label: "Test".into(),
                command: format!("dotnet test{target_arg}"),
                category: Test,
            });
            out.push(SuggestedAction {
                label: "Run".into(),
                command: format!("dotnet run --project{target_arg}"),
                category: Run,
            });
        }
        WorkspaceProjectKind::Bun => {
            out.push(SuggestedAction {
                label: "Install".into(),
                command: "bun install".into(),
                category: Setup,
            });
            out.push(SuggestedAction {
                label: "Build".into(),
                command: "bun run build".into(),
                category: Build,
            });
            out.push(SuggestedAction {
                label: "Test".into(),
                command: "bun test".into(),
                category: Test,
            });
            out.push(SuggestedAction {
                label: "Dev".into(),
                command: "bun run dev".into(),
                category: Run,
            });
        }
        WorkspaceProjectKind::Node => {
            out.push(SuggestedAction {
                label: "Install".into(),
                command: "npm install".into(),
                category: Setup,
            });
            out.push(SuggestedAction {
                label: "Test".into(),
                command: "npm test".into(),
                category: Test,
            });
            out.push(SuggestedAction {
                label: "Run".into(),
                command: "npm run dev".into(),
                category: Run,
            });
        }
        WorkspaceProjectKind::Cargo => {
            out.push(SuggestedAction {
                label: "Build".into(),
                command: "cargo build".into(),
                category: Build,
            });
            out.push(SuggestedAction {
                label: "Test".into(),
                command: "cargo test".into(),
                category: Test,
            });
            out.push(SuggestedAction {
                label: "Run".into(),
                command: "cargo run".into(),
                category: Run,
            });
        }
        WorkspaceProjectKind::Python => {
            out.push(SuggestedAction {
                label: "Install".into(),
                command: "pip install -e .".into(),
                category: Setup,
            });
            out.push(SuggestedAction {
                label: "Test".into(),
                command: "pytest".into(),
                category: Test,
            });
        }
        WorkspaceProjectKind::Go => {
            out.push(SuggestedAction {
                label: "Build".into(),
                command: "go build ./...".into(),
                category: Build,
            });
            out.push(SuggestedAction {
                label: "Test".into(),
                command: "go test ./...".into(),
                category: Test,
            });
            out.push(SuggestedAction {
                label: "Run".into(),
                command: "go run .".into(),
                category: Run,
            });
        }
        WorkspaceProjectKind::Mixed | WorkspaceProjectKind::Unknown => {}
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_dotnet_solution() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("MyApp.sln"), "").unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, WorkspaceProjectKind::DotNet);
        assert!(d.markers.contains(&"MyApp.sln".to_string()));
        assert!(d.suggested_actions.iter().any(|a| a.label == "Build"));
    }

    #[test]
    fn detects_csproj_when_no_sln() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cli.csproj"), "").unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, WorkspaceProjectKind::DotNet);
    }

    #[test]
    fn detects_bun_via_lockfile() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("bun.lock"), "").unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, WorkspaceProjectKind::Bun);
        assert!(d
            .suggested_actions
            .iter()
            .any(|a| a.command == "bun install"));
    }

    #[test]
    fn detects_node_when_package_json_lacks_bun_pm_field() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, WorkspaceProjectKind::Node);
    }

    #[test]
    fn detects_bun_via_packagejson_package_manager_field() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"packageManager":"bun@1.2.0"}"#,
        )
        .unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, WorkspaceProjectKind::Bun);
    }

    #[test]
    fn detects_cargo() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, WorkspaceProjectKind::Cargo);
    }

    #[test]
    fn detects_mixed_when_dotnet_and_bun_coexist() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Server.csproj"), "").unwrap();
        fs::write(dir.path().join("bun.lock"), "").unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, WorkspaceProjectKind::Mixed);
    }

    #[test]
    fn unknown_when_no_markers() {
        let dir = tempdir().unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, WorkspaceProjectKind::Unknown);
        assert!(d.suggested_actions.is_empty());
    }
}
