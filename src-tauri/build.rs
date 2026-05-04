use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const GITHUB_CLIENT_ID_KEY: &str = "WINTHORPE_GITHUB_CLIENT_ID";
const UPDATER_ENDPOINTS_KEY: &str = "WINTHORPE_UPDATER_ENDPOINTS";
const UPDATER_PUBKEY_KEY: &str = "WINTHORPE_UPDATER_PUBKEY";

// Must match auth.rs::PLACEHOLDER_CLIENT_ID. The committed .env.example uses
// this value so contributors get a clear error instead of silently
// authenticating against someone else's OAuth app.
const GITHUB_CLIENT_ID_PLACEHOLDER: &str = "REPLACE_WITH_YOUR_GITHUB_OAUTH_CLIENT_ID";

fn main() {
    ensure_external_bin_placeholders();

    println!("cargo:rerun-if-changed=build.rs");
    for key in [
        GITHUB_CLIENT_ID_KEY,
        UPDATER_ENDPOINTS_KEY,
        UPDATER_PUBKEY_KEY,
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }

    // Walk env files highest-priority → lowest. The first file that defines a
    // given key wins; later files cannot overwrite it. Without this, the
    // committed `.env.example` placeholder would clobber the real value from
    // `.env.local`, since `cargo:rustc-env=` lines emitted later override
    // earlier ones for the compiled binary.
    let mut resolved: HashMap<&'static str, String> = HashMap::new();
    for env_path in candidate_env_paths() {
        if env_path.exists() {
            println!("cargo:rerun-if-changed={}", env_path.display());
        }
        for key in [
            GITHUB_CLIENT_ID_KEY,
            UPDATER_ENDPOINTS_KEY,
            UPDATER_PUBKEY_KEY,
        ] {
            if resolved.contains_key(key) {
                continue;
            }
            if let Some(value) = load_env_var(&env_path, key) {
                resolved.insert(key, value);
            }
        }
    }

    enforce_github_client_id(resolved.get(GITHUB_CLIENT_ID_KEY).map(String::as_str));

    tauri_build::build();
}

/// Production builds MUST bake in a real GitHub OAuth client ID. v0.6.4
/// shipped without one and the in-app "Connect GitHub" button surfaced a
/// runtime error. Hard-fail release builds so an unconfigured CI run never
/// produces a binary again. Debug builds emit a warning instead so
/// contributors without `.env.local` can still run `cargo check`/`cargo test`.
fn enforce_github_client_id(value: Option<&str>) {
    let problem = match value {
        None => Some("WINTHORPE_GITHUB_CLIENT_ID is not set".to_string()),
        Some(v) if v.trim().is_empty() => Some("WINTHORPE_GITHUB_CLIENT_ID is empty".to_string()),
        Some(v) if v == GITHUB_CLIENT_ID_PLACEHOLDER => Some(format!(
            "WINTHORPE_GITHUB_CLIENT_ID is still the placeholder ({GITHUB_CLIENT_ID_PLACEHOLDER})"
        )),
        Some(_) => None,
    };

    let Some(reason) = problem else { return };

    let message = format!(
        "{reason}. Set it in .env.local (see .env.local.example) or as an \
         environment variable; CI must inject it via the \
         WINTHORPE_GITHUB_CLIENT_ID repo secret. See docs/github-oauth-setup.md."
    );

    let is_release = env::var("PROFILE").as_deref() == Ok("release");
    if is_release {
        panic!("{message}");
    }
    println!("cargo:warning={message}");
}

fn ensure_external_bin_placeholders() {
    let Ok(target) = env::var("TARGET") else {
        return;
    };

    // Tauri's bundler appends `.exe` to externalBin entries when targeting
    // Windows. Mirror that here so `cargo check`/`cargo build` finds the
    // placeholder before the real artifacts have been staged.
    let exe_suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"));
    ensure_executable_placeholder(
        manifest_dir
            .join("target")
            .join("bundled")
            .join(format!("winthorpe-cli-{target}{exe_suffix}")),
    );

    if let Some(repo_root) = manifest_dir.parent() {
        ensure_executable_placeholder(
            repo_root
                .join("sidecar")
                .join("dist")
                .join(format!("winthorpe-sidecar-{target}{exe_suffix}")),
        );

        // tauri.conf.json's `bundle.resources` references `../sidecar/dist/vendor/`.
        // Tauri checks this exists at build time. Create an empty directory so
        // `cargo check` succeeds before the sidecar's vendor staging script
        // has run (Phase 3 fills it on Windows).
        let vendor_dir = repo_root.join("sidecar").join("dist").join("vendor");
        let _ = fs::create_dir_all(&vendor_dir);
    }
}

fn ensure_executable_placeholder(path: PathBuf) {
    if path.exists() {
        return;
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // The body is irrelevant — Tauri only checks for existence at build time.
    // We use a shebang on Unix and a tiny no-op MZ stub on Windows so the file
    // doesn't trip antivirus scanners that flag zero-byte executables.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::write(&path, "#!/bin/sh\nexit 0\n");
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    {
        let _ = fs::write(&path, "MZ");
    }
}

fn candidate_env_paths() -> Vec<PathBuf> {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"));
    let mut paths = vec![manifest_dir.join(".env.local")];

    if let Some(repo_root) = manifest_dir.parent() {
        paths.push(repo_root.join(".env.local"));
        // Lowest-priority fallback: committed `.env.example` provides defaults
        // for public values (e.g. GitHub Device Flow client ID) so a fresh
        // `cargo build` works without any manual `cp .env.example .env.local`.
        paths.push(repo_root.join(".env.example"));
    }

    paths
}

/// Returns the resolved value for `key`, in priority order: process env first
/// (rustc inherits it directly, no `cargo:rustc-env=` needed), then the .env
/// file at `path`. Returns None if the key isn't found at this layer so the
/// caller can try the next candidate path.
fn load_env_var(path: &Path, key: &str) -> Option<String> {
    if let Ok(value) = env::var(key) {
        return Some(value);
    }
    if !path.exists() {
        return None;
    }

    let iter = dotenvy::from_path_iter(path).ok()?;
    for item in iter.flatten() {
        if item.0 == key {
            println!("cargo:rustc-env={}={}", item.0, item.1);
            return Some(item.1);
        }
    }
    None
}
