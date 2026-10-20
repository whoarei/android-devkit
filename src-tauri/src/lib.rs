#[cfg(test)]
mod acceptance;
mod android;
mod apk;
mod app;
mod avd_settings;
mod bootstrap;
mod commands;
#[cfg(test)]
mod contracts;
mod dependencies;
mod domain;
mod engine;
mod exports;
mod lifecycle;
mod log_buffer;
mod observe;
mod plans;
mod platform;
mod privacy;
mod process;
mod recording;
mod recovery;
#[cfg(test)]
mod regression_tests;
mod repository;
mod storage;
mod updater;
use tauri::Manager;
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(commands::setup)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window
                    .state::<std::sync::Arc<app::App>>()
                    .request_close(false);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ui_locale_set,
            commands::workspace_snapshot,
            commands::task_list,
            commands::environment_scan,
            commands::settings_save,
            commands::catalog_refresh,
            commands::sdk_sites_status,
            commands::plan_create,
            commands::task_submit,
            commands::task_cancel,
            commands::task_retry,
            commands::file_choose,
            commands::artifact_save,
            commands::artifact_copy,
            commands::apk_prepare,
            commands::log_read,
            commands::window_action,
            commands::window_state,
            commands::window_regions,
            commands::close_resolve,
            commands::close_status,
            commands::artifacts_list,
            commands::artifacts_discard,
            commands::export_prepare,
            commands::export_page,
            commands::export_save,
            commands::recording_segments,
            commands::diagnostics,
            commands::license_respond,
            updater::check_app_update,
            updater::install_app_update,
            updater::open_app_download
        ])
        .run(tauri::generate_context!())
        .expect("Android DevKit failed to run");
}
