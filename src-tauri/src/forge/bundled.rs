//! Paths to bundled `gh` / `glab` inside `Resources/vendor/`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const GH_PATH_ENV: &str = "WINTHORPE_GH_BIN_PATH";
pub const GLAB_PATH_ENV: &str = "WINTHORPE_GLAB_BIN_PATH";

#[derive(Debug, Default, Clone)]
pub struct BundledForgeCliPaths {
    pub gh: Option<PathBuf>,
    pub glab: Option<PathBuf>,
}

static BUNDLED_PATHS: OnceLock<BundledForgeCliPaths> = OnceLock::new();

/// Call once from the Tauri setup hook; later calls are a no-op in release
/// and a debug assertion failure in dev (catches accidental re-init).
pub fn init() {
    let result = BUNDLED_PATHS.set(resolve_from_running_exe());
    debug_assert!(result.is_ok(), "forge::bundled::init called more than once");
    let paths = BUNDLED_PATHS.get();
    tracing::info!(
        gh = ?paths.and_then(|p| p.gh.as_deref()),
        glab = ?paths.and_then(|p| p.glab.as_deref()),
        "Resolved bundled forge CLI paths"
    );
}

/// Env var override > startup-resolved path > `None` (caller falls back to PATH).
pub fn bundled_path_for(program: &str) -> Option<PathBuf> {
    if let Some(env_key) = env_key_for(program) {
        if let Ok(raw) = std::env::var(env_key) {
            let path = PathBuf::from(raw);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    let cached = BUNDLED_PATHS.get()?;
    match program {
        "gh" => cached.gh.clone(),
        "glab" => cached.glab.clone(),
        _ => None,
    }
}

fn env_key_for(program: &str) -> Option<&'static str> {
    match program {
        "gh" => Some(GH_PATH_ENV),
        "glab" => Some(GLAB_PATH_ENV),
        _ => None,
    }
}

fn resolve_from_running_exe() -> BundledForgeCliPaths {
    let paths = std::env::current_exe()
        .ok()
        .and_then(|exe| resolve_for_exe(&exe))
        .unwrap_or_default();

    #[cfg(debug_assertions)]
    {
        paths.with_fallback(resolve_for_dev_workspace(&dev_workspace_root()))
    }

    #[cfg(not(debug_assertions))]
    {
        paths
    }
}

fn resolve_for_exe(exe: &Path) -> Option<BundledForgeCliPaths> {
    let exe_dir = exe.parent()?;

    let gh_name = if cfg!(windows) { "gh.exe" } else { "gh" };
    let glab_name = if cfg!(windows) { "glab.exe" } else { "glab" };

    // Resource roots vary by installer:
    //   - macOS .app:    Contents/MacOS/<exe>           → ../Resources
    //   - Windows NSIS:  <install>\Winthorpe.exe        → ./resources
    //   - Linux AppImage AppDir layout puts resources beside the binary.
    // Try every plausible location; the first hit wins. Any missing root
    // is silently skipped (parent() may return None on the filesystem root).
    let mut candidate_roots: Vec<PathBuf> = Vec::new();
    if let Some(contents_dir) = exe_dir.parent() {
        candidate_roots.push(contents_dir.join("Resources"));
    }
    candidate_roots.push(exe_dir.join("resources"));
    candidate_roots.push(exe_dir.to_path_buf());

    let mut gh: Option<PathBuf> = None;
    let mut glab: Option<PathBuf> = None;
    for root in &candidate_roots {
        if gh.is_none() {
            let candidate = root.join(format!("vendor/gh/{gh_name}"));
            if candidate.is_file() {
                gh = Some(candidate);
            }
        }
        if glab.is_none() {
            let candidate = root.join(format!("vendor/glab/{glab_name}"));
            if candidate.is_file() {
                glab = Some(candidate);
            }
        }
        if gh.is_some() && glab.is_some() {
            break;
        }
    }

    Some(BundledForgeCliPaths { gh, glab })
}

#[cfg(debug_assertions)]
impl BundledForgeCliPaths {
    fn with_fallback(self, fallback: BundledForgeCliPaths) -> BundledForgeCliPaths {
        BundledForgeCliPaths {
            gh: self.gh.or(fallback.gh),
            glab: self.glab.or(fallback.glab),
        }
    }
}

#[cfg(debug_assertions)]
fn dev_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

#[cfg(debug_assertions)]
fn resolve_for_dev_workspace(workspace_root: &Path) -> BundledForgeCliPaths {
    let vendor = workspace_root.join("sidecar/dist/vendor");
    let gh_name = if cfg!(windows) { "gh.exe" } else { "gh" };
    let glab_name = if cfg!(windows) { "glab.exe" } else { "glab" };

    let gh = vendor.join(format!("gh/{gh_name}"));
    let glab = vendor.join(format!("glab/{glab_name}"));

    BundledForgeCliPaths {
        gh: gh.is_file().then_some(gh),
        glab: glab.is_file().then_some(glab),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_finds_binaries_under_resources_vendor() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("Winthorpe.app/Contents/MacOS/Winthorpe");
        let vendor = root.path().join("Winthorpe.app/Contents/Resources/vendor");
        std::fs::create_dir_all(vendor.join("gh")).unwrap();
        std::fs::create_dir_all(vendor.join("glab")).unwrap();
        std::fs::write(vendor.join("gh/gh"), "").unwrap();
        std::fs::write(vendor.join("glab/glab"), "").unwrap();

        let paths = resolve_for_exe(&exe).unwrap();

        assert_eq!(
            paths.gh.unwrap(),
            root.path()
                .join("Winthorpe.app/Contents/Resources/vendor/gh/gh")
        );
        assert_eq!(
            paths.glab.unwrap(),
            root.path()
                .join("Winthorpe.app/Contents/Resources/vendor/glab/glab")
        );
    }

    #[test]
    fn resolve_finds_binaries_under_windows_install_resources() {
        let root = tempfile::tempdir().unwrap();
        let install_dir = root.path().join("Programs/Winthorpe");
        let vendor = install_dir.join("resources/vendor");
        std::fs::create_dir_all(vendor.join("gh")).unwrap();
        std::fs::create_dir_all(vendor.join("glab")).unwrap();
        let gh_name = if cfg!(windows) { "gh.exe" } else { "gh" };
        let glab_name = if cfg!(windows) { "glab.exe" } else { "glab" };
        std::fs::write(vendor.join(format!("gh/{gh_name}")), "").unwrap();
        std::fs::write(vendor.join(format!("glab/{glab_name}")), "").unwrap();

        let exe = install_dir.join("Winthorpe.exe");
        std::fs::write(&exe, "").unwrap();

        let paths = resolve_for_exe(&exe).unwrap();
        assert_eq!(paths.gh.unwrap(), vendor.join(format!("gh/{gh_name}")));
        assert_eq!(
            paths.glab.unwrap(),
            vendor.join(format!("glab/{glab_name}"))
        );
    }

    #[test]
    fn resolve_returns_none_when_binaries_missing() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("Winthorpe.app/Contents/MacOS/Winthorpe");
        let paths = resolve_for_exe(&exe).unwrap();
        assert!(paths.gh.is_none());
        assert!(paths.glab.is_none());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn resolve_finds_debug_vendor_under_workspace_root() {
        let root = tempfile::tempdir().unwrap();
        let vendor = root.path().join("sidecar/dist/vendor");
        std::fs::create_dir_all(vendor.join("gh")).unwrap();
        std::fs::create_dir_all(vendor.join("glab")).unwrap();
        std::fs::write(vendor.join("gh/gh"), "").unwrap();
        std::fs::write(vendor.join("glab/glab"), "").unwrap();

        let paths = resolve_for_dev_workspace(root.path());

        assert_eq!(paths.gh.unwrap(), vendor.join("gh/gh"));
        assert_eq!(paths.glab.unwrap(), vendor.join("glab/glab"));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn app_bundle_paths_win_over_debug_vendor() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("Winthorpe.app/Contents/MacOS/Winthorpe");
        let app_vendor = root.path().join("Winthorpe.app/Contents/Resources/vendor");
        let dev_vendor = root.path().join("sidecar/dist/vendor");
        std::fs::create_dir_all(app_vendor.join("gh")).unwrap();
        std::fs::create_dir_all(app_vendor.join("glab")).unwrap();
        std::fs::create_dir_all(dev_vendor.join("gh")).unwrap();
        std::fs::create_dir_all(dev_vendor.join("glab")).unwrap();
        std::fs::write(app_vendor.join("gh/gh"), "").unwrap();
        std::fs::write(app_vendor.join("glab/glab"), "").unwrap();
        std::fs::write(dev_vendor.join("gh/gh"), "").unwrap();
        std::fs::write(dev_vendor.join("glab/glab"), "").unwrap();

        let paths = resolve_for_exe(&exe)
            .unwrap()
            .with_fallback(resolve_for_dev_workspace(root.path()));

        assert_eq!(paths.gh.unwrap(), app_vendor.join("gh/gh"));
        assert_eq!(paths.glab.unwrap(), app_vendor.join("glab/glab"));
    }
}
