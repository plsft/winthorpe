//! Windows deep link (URL handler) registration.
//!
//! Registers `winthorpe://` and `winthorpe-dev://` under
//! `HKCU\Software\Classes\<scheme>` so the OS routes
//! `start winthorpe://...` / browser link clicks to our app.
//!
//! Per-user registration → no admin rights, no UAC prompt. macOS handles
//! this automatically via the Tauri deep-link plugin and LaunchServices;
//! Windows requires explicit registry entries.
//!
//! Should be called once at first launch (idempotent — overwriting existing
//! values to keep them in sync with the current install path).

#![cfg(windows)]

use std::path::Path;

use anyhow::{Context, Result};

/// Register all Winthorpe URL schemes for the current user.
///
/// `app_exe` is the path to the running Winthorpe.exe (typically
/// `std::env::current_exe()`). The registration uses `"<exe>" "%1"` as the
/// command line so the launched URL is passed as the first arg.
pub fn register_url_schemes(app_exe: &Path) -> Result<()> {
    register_scheme(app_exe, "winthorpe")?;
    if cfg!(debug_assertions) {
        register_scheme(app_exe, "winthorpe-dev")?;
    }
    Ok(())
}

fn register_scheme(app_exe: &Path, scheme: &str) -> Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let exe = app_exe.to_string_lossy().to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // HKCU\Software\Classes\<scheme>
    let scheme_path = format!(r"Software\Classes\{scheme}");
    let (scheme_key, _) = hkcu
        .create_subkey(&scheme_path)
        .with_context(|| format!("create_subkey {scheme_path}"))?;
    scheme_key
        .set_value("", &format!("URL:{scheme} Protocol"))
        .context("set scheme description")?;
    scheme_key
        .set_value("URL Protocol", &"")
        .context("set URL Protocol marker")?;

    // DefaultIcon
    let (icon_key, _) = hkcu
        .create_subkey(format!(r"{scheme_path}\DefaultIcon"))
        .context("create_subkey DefaultIcon")?;
    icon_key
        .set_value("", &format!("\"{exe}\",0"))
        .context("set DefaultIcon")?;

    // shell\open\command
    let (cmd_key, _) = hkcu
        .create_subkey(format!(r"{scheme_path}\shell\open\command"))
        .context("create_subkey shell\\open\\command")?;
    cmd_key
        .set_value("", &format!("\"{exe}\" \"%1\""))
        .context("set shell open command")?;

    tracing::info!(scheme, exe = %exe, "Registered Windows URL scheme");
    Ok(())
}

/// Best-effort cleanup. Used by uninstall flows; safe to call when the
/// scheme isn't registered.
pub fn unregister_url_schemes() -> Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for scheme in ["winthorpe", "winthorpe-dev"] {
        let scheme_path = format!(r"Software\Classes\{scheme}");
        let _ = hkcu.delete_subkey_all(&scheme_path);
    }
    Ok(())
}
