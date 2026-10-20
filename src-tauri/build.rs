fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }
    const COMMANDS: &[&str] = &[
        "ui_locale_set",
        "workspace_snapshot",
        "task_list",
        "environment_scan",
        "settings_save",
        "catalog_refresh",
        "sdk_sites_status",
        "plan_create",
        "task_submit",
        "task_cancel",
        "task_retry",
        "file_choose",
        "artifact_save",
        "artifact_copy",
        "apk_prepare",
        "log_read",
        "window_action",
        "window_state",
        "window_regions",
        "close_resolve",
        "close_status",
        "artifacts_list",
        "artifacts_discard",
        "export_prepare",
        "export_page",
        "export_save",
        "recording_segments",
        "diagnostics",
        "license_respond",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest())
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("build Tauri application");
}
