use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const GITHUB_CLIENT_ID_KEY: &str = "WINTHORPE_GITHUB_CLIENT_ID";
const UPDATER_ENDPOINTS_KEY: &str = "WINTHORPE_UPDATER_ENDPOINTS";
const UPDATER_PUBKEY_KEY: &str = "WINTHORPE_UPDATER_PUBKEY";

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
    let mut emitted: HashSet<&'static str> = HashSet::new();
    for env_path in candidate_env_paths() {
        if env_path.exists() {
            println!("cargo:rerun-if-changed={}", env_path.display());
        }
        for key in [
            GITHUB_CLIENT_ID_KEY,
            UPDATER_ENDPOINTS_KEY,
            UPDATER_PUBKEY_KEY,
        ] {
            if emitted.contains(key) {
                continue;
            }
            if load_env_var(&env_path, key) {
                emitted.insert(key);
            }
        }
    }

    tauri_build::build();
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

/// Returns true when a value for `key` was emitted from `path`.
fn load_env_var(path: &Path, key: &str) -> bool {
    if env::var_os(key).is_some() {
        // The build process itself already has the var; option_env! will pick
        // it up directly. Treat as "already emitted" so lower-priority files
        // can't overwrite it.
        return true;
    }
    if !path.exists() {
        return false;
    }

    let Ok(iter) = dotenvy::from_path_iter(path) else {
        return false;
    };

    for item in iter.flatten() {
        if item.0 == key {
            println!("cargo:rustc-env={}={}", item.0, item.1);
            return true;
        }
    }
    false
}
