//! Baseline serde-shape tests for `commands/system_commands.rs`.
//!
//! These establish the published contract (JSON shape) the frontend depends on.
//! Editor-catalog tests live next to the catalog in `commands::editors`.

use crate::commands::system_commands::{CliInstallState, CliStatus, DataInfo};

#[test]
fn cli_status_serializes_camel_case() {
    let status = CliStatus {
        installed: true,
        install_path: Some("/usr/local/bin/winthorpe-dev".into()),
        build_mode: "development".into(),
        install_state: CliInstallState::Managed,
    };
    let value = serde_json::to_value(&status).unwrap();
    assert!(value.get("installed").is_some());
    assert_eq!(value["installPath"], "/usr/local/bin/winthorpe-dev");
    assert_eq!(value["buildMode"], "development");
    assert_eq!(value["installState"], "managed");
    assert!(value.get("install_path").is_none());
}

#[test]
fn cli_status_missing_install_path_is_null() {
    let status = CliStatus {
        installed: false,
        install_path: None,
        build_mode: "development".into(),
        install_state: CliInstallState::Missing,
    };
    let value = serde_json::to_value(&status).unwrap();
    assert!(value["installPath"].is_null());
    assert_eq!(value["installed"], false);
    assert_eq!(value["installState"], "missing");
}

#[test]
fn data_info_serializes_camel_case() {
    let info = DataInfo {
        data_mode: "development".into(),
        data_dir: "/tmp/winthorpe".into(),
        db_path: "/tmp/winthorpe/winthorpe.db".into(),
    };
    let value = serde_json::to_value(&info).unwrap();
    assert_eq!(value["dataMode"], "development");
    assert_eq!(value["dataDir"], "/tmp/winthorpe");
    assert_eq!(value["dbPath"], "/tmp/winthorpe/winthorpe.db");
    assert!(value.get("data_mode").is_none());
}
