//! Windows CLI install + uninstall helpers.
//!
//! What "install" means on Windows:
//!   1. Create `%LOCALAPPDATA%\Programs\Winthorpe\bin\` if missing.
//!   2. Copy the bundled `winthorpe-cli.exe` into it.
//!   3. Drop a `winthorpe.cmd` shim that forwards to the exe (so users can
//!      type `winthorpe foo` without the `.exe` and get tab completion in
//!      pwsh).
//!   4. Append the bin dir to the user's `PATH` env var via
//!      `HKCU\Environment` if not already present.
//!   5. Broadcast `WM_SETTINGCHANGE("Environment")` so any new shell process
//!      sees the updated PATH without requiring a logoff/logon.
//!
//! No admin rights needed for any of this — it's all per-user.

#![cfg(windows)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Result of an install operation. Used by the Tauri command to update the
/// "CLI installed" indicator in the Settings UI.
#[derive(Debug)]
pub struct InstallOutcome {
    pub install_path: PathBuf,
    pub bin_dir: PathBuf,
    pub path_updated: bool,
}

/// Install the bundled CLI for the current user. `bundled_cli` is the
/// absolute path to the exe shipped in the Winthorpe bundle resources.
pub fn install_cli(bundled_cli: &Path, install_path: &Path) -> Result<InstallOutcome> {
    let bin_dir = install_path
        .parent()
        .context("install_path has no parent directory")?
        .to_path_buf();

    std::fs::create_dir_all(&bin_dir)
        .with_context(|| format!("Failed to create {}", bin_dir.display()))?;

    // Copy the exe (replacing any prior version atomically: copy to .new then rename).
    let staging = install_path.with_extension("new");
    if staging.exists() {
        let _ = std::fs::remove_file(&staging);
    }
    std::fs::copy(bundled_cli, &staging)
        .with_context(|| format!("Failed to copy CLI to {}", staging.display()))?;
    if install_path.exists() {
        let _ = std::fs::remove_file(install_path);
    }
    std::fs::rename(&staging, install_path).with_context(|| {
        format!(
            "Failed to rename {} → {}",
            staging.display(),
            install_path.display()
        )
    })?;

    // Drop the .cmd shim. Users can call `winthorpe foo` without typing .exe.
    let shim_path = bin_dir.join(shim_name(install_path));
    let shim_body = format!(
        "@echo off\r\n\"%~dp0{}\" %*\r\n",
        install_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    std::fs::write(&shim_path, shim_body)
        .with_context(|| format!("Failed to write shim at {}", shim_path.display()))?;

    // Append bin_dir to user PATH if not already present.
    let path_updated = ensure_user_path_contains(&bin_dir)?;

    Ok(InstallOutcome {
        install_path: install_path.to_path_buf(),
        bin_dir,
        path_updated,
    })
}

/// Uninstall the CLI for the current user. Removes the exe + .cmd shim;
/// does NOT remove the bin dir from PATH (other tools may live there too).
pub fn uninstall_cli(install_path: &Path) -> Result<()> {
    if install_path.exists() {
        std::fs::remove_file(install_path)
            .with_context(|| format!("Failed to remove {}", install_path.display()))?;
    }
    if let Some(bin_dir) = install_path.parent() {
        let shim_path = bin_dir.join(shim_name(install_path));
        if shim_path.exists() {
            let _ = std::fs::remove_file(shim_path);
        }
    }
    Ok(())
}

fn shim_name(install_path: &Path) -> String {
    let stem = install_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "winthorpe".to_string());
    format!("{stem}.cmd")
}

/// Ensure `bin_dir` is in the user's PATH. Returns true if we modified it.
fn ensure_user_path_contains(bin_dir: &Path) -> Result<bool> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_EXPAND_SZ};
    use winreg::RegKey;

    let target = bin_dir.to_string_lossy().to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu
        .open_subkey_with_flags("Environment", KEY_ALL_ACCESS)
        .context("Failed to open HKCU\\Environment")?;

    let current: String = env.get_value("Path").unwrap_or_default();
    let already_present = current
        .split(';')
        .any(|seg| seg.eq_ignore_ascii_case(&target));
    if already_present {
        return Ok(false);
    }

    let mut new_path = current.clone();
    if !new_path.is_empty() && !new_path.ends_with(';') {
        new_path.push(';');
    }
    new_path.push_str(&target);

    env.set_raw_value(
        "Path",
        &winreg::RegValue {
            bytes: encode_utf16_z(&new_path),
            vtype: REG_EXPAND_SZ,
        },
    )
    .context("Failed to write HKCU\\Environment\\Path")?;

    broadcast_environment_change();
    Ok(true)
}

/// Broadcast WM_SETTINGCHANGE so already-running Explorer + new shells pick
/// up the new PATH without requiring a logoff/logon. Best-effort; failure is
/// not surfaced to the user (the install still succeeded).
fn broadcast_environment_change() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    let env_str: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result = 0usize;
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(env_str.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            Some(&mut result),
        );
    }
}

/// UTF-16 with NUL terminator for REG_EXPAND_SZ.
fn encode_utf16_z(s: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::with_capacity((s.len() + 1) * 2);
    for c in s.encode_utf16() {
        bytes.extend_from_slice(&c.to_le_bytes());
    }
    bytes.push(0);
    bytes.push(0);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn install_drops_cmd_shim_alongside_exe() {
        let dir = tempdir().unwrap();
        let bundled = dir.path().join("source").join("winthorpe-cli.exe");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"MZ").unwrap();

        let install = dir.path().join("bin").join("winthorpe-cli.exe");
        let outcome = install_cli(&bundled, &install).unwrap();
        assert!(outcome.install_path.exists());
        let shim = outcome.bin_dir.join("winthorpe-cli.cmd");
        assert!(shim.exists());
        let body = std::fs::read_to_string(&shim).unwrap();
        assert!(body.contains("winthorpe-cli.exe"));
    }

    #[test]
    fn uninstall_removes_exe_and_shim() {
        let dir = tempdir().unwrap();
        let bundled = dir.path().join("source").join("winthorpe-cli.exe");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"MZ").unwrap();
        let install = dir.path().join("bin").join("winthorpe-cli.exe");
        install_cli(&bundled, &install).unwrap();

        uninstall_cli(&install).unwrap();
        assert!(!install.exists());
        assert!(!install.with_extension("cmd").exists());
    }
}
