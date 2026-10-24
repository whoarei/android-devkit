#[cfg(test)]
mod acceptance;
mod android;
mod apk;
mod app;
mod avd_settings;
mod bootstrap;
mod capacity;
mod commands;
#[cfg(test)]
mod contracts;
mod dependencies;
mod domain;
mod engine;
mod exports;
#[cfg(test)]
mod fault_tests;
mod java;
mod lifecycle;
mod log_buffer;
mod observe;
mod phone_apk;
mod phone_files;
mod phone_publish;
mod phone_record;
#[cfg(test)]
mod phone_tests;
mod plans;
mod platform;
mod privacy;
mod process;
mod readiness;
mod recording;
mod recovery;
#[cfg(test)]
mod regression_tests;
mod repository;
mod sdk_installer;
mod sdk_source;
mod storage;
mod task_records;
mod terminal;
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
            terminal::terminal_create,
            terminal::terminal_list,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_ack,
            terminal::terminal_close,
            commands::ui_locale_set,
            commands::workspace_snapshot,
            commands::task_list,
            commands::task_delete,
            commands::recording_discard,
            commands::environment_scan,
            commands::avd_usage,
            commands::text_copy,
            commands::environment_summary,
            commands::java_environment,
            commands::java_check,
            commands::java_apply,
            commands::sdk_open_directory,
            commands::settings_save,
            commands::catalog_refresh,
            commands::sdk_sites_status,
            commands::sdk_source_check,
            commands::sdk_source_apply,
            commands::plan_create,
            commands::task_submit,
            commands::task_cancel,
            commands::task_retry,
            commands::file_choose,
            commands::artifact_save,
            commands::artifact_copy,
            commands::apk_prepare,
            commands::apk_set_prepare,
            commands::apk_set_tools,
            commands::phone_choose,
            commands::device_files,
            commands::transfer_prepare,
            commands::video_preview,
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
