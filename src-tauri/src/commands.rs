use crate::{android, app::App, domain::*, platform};
use base64::Engine;
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tauri::{Manager, State};
type Service<'a> = State<'a, Arc<App>>;

#[tauri::command]
pub fn ui_locale_set(app: Service<'_>, locale: String) -> Result<()> {
    let english = match locale.as_str() {
        "zh-CN" => false,
        "en-US" => true,
        _ => return Err(AppError::new("INVALID_LOCALE", "不支持的界面语言")),
    };
    app.english.store(english, Ordering::Relaxed);
    if let Some(handle) = app.handle.lock().unwrap().clone() {
        crate::lifecycle::update_tray(&handle, &app);
    }
    Ok(())
}

#[tauri::command]
pub async fn workspace_snapshot(app: Service<'_>) -> Result<Workspace> {
    app.snapshot().await
}
#[tauri::command]
pub async fn task_list(app: Service<'_>, query: TaskQuery) -> Result<TaskPage> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || app.store.task_page(&query))
        .await
        .map_err(|error| {
            AppError::new("TASK_QUERY_FAILED", "读取任务失败").detail(error.to_string())
        })?
}
#[tauri::command]
pub fn environment_scan(app: Service<'_>) -> Vec<Environment> {
    android::scan(&app.store.settings())
}
#[tauri::command]
pub fn sdk_open_directory(app: Service<'_>) -> Result<()> {
    let settings = app.store.settings();
    let path = Path::new(&settings.sdk_root);
    if !path.is_absolute() || !path.is_dir() {
        return Err(AppError::new(
            "SDK_DIRECTORY_MISSING",
            "SDK 目录不存在，请在设置中检查当前 SDK 根目录",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let verb: Vec<u16> = "explore\0".encode_utf16().collect();
        let directory: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                directory.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize > 32 {
            return Ok(());
        }
    }
    Err(AppError::new(
        "OPEN_SDK_DIRECTORY_FAILED",
        "无法打开 SDK 目录，请检查目录访问权限",
    ))
}
#[tauri::command]
pub fn settings_save(app: Service<'_>, settings: Settings) -> Result<()> {
    let _configuration_guard = app.configuration_gate.lock().unwrap();
    crate::repository::validate(&settings)?;
    if settings.sdk_download != app.store.settings().sdk_download {
        return Err(AppError::new(
            "SDK_SOURCE_APPLY_REQUIRED",
            "请在 SDK 管理中检查并应用下载源",
        ));
    }
    if !app.store.active_tasks()?.is_empty() {
        return Err(AppError::new("TASKS_ACTIVE", "任务执行中不能切换工具环境"));
    }
    if !Path::new(&settings.sdk_root).is_absolute()
        || settings.avd_roots.is_empty()
        || settings
            .avd_roots
            .iter()
            .any(|p| !Path::new(p).is_absolute())
    {
        return Err(AppError::new(
            "INVALID_PATH",
            "SDK 与 AVD 目录必须是绝对路径",
        ));
    }
    if !(30..=900).contains(&settings.boot_timeout_secs) {
        return Err(AppError::new(
            "INVALID_TIMEOUT",
            "启动提醒阈值限 30～900 秒",
        ));
    }
    crate::bootstrap::client(&settings)?;
    let previous = app.store.settings();
    let handle = app.handle.lock().unwrap().clone();
    if let Some(h) = &handle {
        if let Some(tray) = h.tray_by_id("devkit") {
            tray.set_visible(settings.tray_enabled)
                .map_err(|e| AppError::new("TRAY", e.to_string()))?;
        }
    }
    if let Err(error) = app.store.save_settings(&settings) {
        if let Some(h) = &handle {
            if let Some(tray) = h.tray_by_id("devkit") {
                let _ = tray.set_visible(previous.tray_enabled);
            }
        }
        return Err(error);
    }
    app.notify();
    Ok(())
}
#[tauri::command]
pub async fn catalog_refresh(app: Service<'_>) -> Result<Vec<String>> {
    app.refresh_catalog().await
}
#[tauri::command]
pub fn sdk_sites_status(app: Service<'_>) -> Vec<SdkSiteStatus> {
    app.store
        .sdk_catalog()
        .map(|(_, sites)| sites)
        .unwrap_or_else(|| crate::repository::statuses(&app.data, &app.store.settings()))
}
#[tauri::command]
pub async fn sdk_source_check(
    app: Service<'_>,
    source: SdkDownloadSource,
) -> Result<SdkSourceCheck> {
    app.check_sdk_source(source).await
}
#[tauri::command]
pub async fn sdk_source_apply(app: Service<'_>, source: SdkDownloadSource) -> Result<Vec<String>> {
    app.apply_sdk_source(source).await
}
#[tauri::command]
pub fn plan_create(app: Service<'_>, operation: Operation) -> Result<Plan> {
    app.prepare_plan(operation)
}
#[tauri::command]
pub fn task_submit(
    app: Service<'_>,
    plan_id: String,
    plan_hash: String,
    request_id: String,
    confirmation_token: String,
    accepted_licenses: Vec<String>,
) -> Result<Task> {
    app.inner().submit(
        &plan_id,
        &plan_hash,
        &request_id,
        &confirmation_token,
        &accepted_licenses,
    )
}
#[tauri::command]
pub fn task_cancel(app: Service<'_>, task_id: String) -> Result<()> {
    app.cancel_task(&task_id)
}
#[tauri::command]
pub fn task_retry(app: Service<'_>, task_id: String) -> Result<Plan> {
    let t = app.store.task(&task_id)?;
    if t.active() {
        return Err(AppError::new("TASK_ACTIVE", "任务仍在运行"));
    }
    if t.operation.kind == "wireless_pair" {
        return Err(AppError::new("PAIR_CODE_REQUIRED", "请重新获取配对码"));
    }
    app.prepare_plan(t.operation)
}
#[tauri::command]
pub async fn file_choose(kind: String) -> Result<Option<String>> {
    tokio::task::spawn_blocking(move || {
        let dialog = rfd::FileDialog::new();
        let path = match kind.as_str() {
            "sdk" | "avd" | "skin" => dialog.pick_folder(),
            "sdcard" => dialog.add_filter("SD card image", &["img"]).pick_file(),
            "java" => dialog.add_filter("Java", &["exe"]).pick_file(),
            _ => dialog.add_filter("Android APK", &["apk"]).pick_file(),
        };
        Ok(path.map(|p| p.to_string_lossy().into()))
    })
    .await
    .map_err(|e| AppError::new("DIALOG", e.to_string()))?
}
#[tauri::command]
pub fn apk_prepare(app: Service<'_>, path: String) -> Result<ApkInfo> {
    app.prepare_apk(Path::new(&path))
}
#[tauri::command]
pub async fn artifact_save(app: Service<'_>, task_id: String) -> Result<Option<String>> {
    let task = app.store.task(&task_id)?;
    let source = task.artifact;
    if source.is_empty() {
        return Err(AppError::new("ARTIFACT_NOT_FOUND", "没有可保存的产物"));
    }
    let result = tokio::task::spawn_blocking(move || -> Result<Option<String>> {
        let Some(dest) = rfd::FileDialog::new()
            .set_file_name("android-screenshot.png")
            .save_file()
        else {
            return Ok(None);
        };
        crate::exports::atomic_copy(Path::new(&source), &dest)?;
        Ok(Some(dest.to_string_lossy().into()))
    })
    .await
    .map_err(|e| AppError::new("SAVE_FAILED", e.to_string()))??;
    if result.is_some() {
        app.store.acknowledge(&task_id)?;
        app.notify();
    }
    Ok(result)
}
#[tauri::command]
pub fn artifact_copy(app: Service<'_>, task_id: String, preview: bool) -> Result<String> {
    let task = app.store.task(&task_id)?;
    if task.artifact.is_empty() {
        return Err(AppError::new("ARTIFACT_NOT_FOUND", "没有截图产物"));
    }
    let bytes = std::fs::read(&task.artifact)?;
    if preview {
        return Ok(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ));
    }
    let image = image::load_from_memory(&bytes)
        .map_err(|e| AppError::new("IMAGE_INVALID", e.to_string()))?
        .to_rgba8();
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| AppError::new("CLIPBOARD", e.to_string()))?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Owned(image.into_raw()),
        })
        .map_err(|e| AppError::new("CLIPBOARD", e.to_string()))?;
    Ok("截图已复制".into())
}
#[tauri::command]
pub async fn log_read(
    app: Service<'_>,
    target: Target,
    action: String,
    filter: Option<LogFilter>,
) -> Result<LogSnapshot> {
    let key = crate::observe::key(&target);
    if action == "start" {
        app.inner().log_start(target.clone()).await?;
        app.log_views.lock().unwrap().insert(key.clone());
    }
    if action == "stop" {
        app.log_release(&target);
    }
    if !matches!(action.as_str(), "start" | "stop" | "read" | "clear") {
        return Err(AppError::new("INVALID_ACTION", "日志动作无效"));
    }
    app.log_snapshot(&target, &filter.unwrap_or_default(), 500, true)
        .await
}
#[tauri::command]
pub fn window_state(window: tauri::WebviewWindow) -> serde_json::Value {
    serde_json::json!({"maximized":window.is_maximized().unwrap_or(false),"focused":window.is_focused().unwrap_or(false),"visible":window.is_visible().unwrap_or(false),"scale":window.scale_factor().unwrap_or(1.0)})
}
#[tauri::command]
pub fn window_regions(window: tauri::WebviewWindow, regions: platform::Regions) {
    if (1..=100).contains(&regions.width)
        && (1..=80).contains(&regions.height)
        && (0.5..=4.0).contains(&regions.scale)
        && regions.left >= 0
        && (-2..=4).contains(&regions.top)
    {
        platform::set_regions(&window, regions);
    }
}
#[tauri::command]
pub fn window_action(window: tauri::WebviewWindow, action: String) -> Result<()> {
    let result = match action.as_str() {
        "minimize" => window.minimize(),
        "maximize" => {
            if window.is_maximized().unwrap_or(false) {
                window.unmaximize()
            } else {
                window.maximize()
            }
        }
        "drag" => window.start_dragging(),
        "theme_light" => window.set_theme(Some(tauri::Theme::Light)),
        "theme_dark" => window.set_theme(Some(tauri::Theme::Dark)),
        "close" => window.close(),
        _ => return Err(AppError::new("INVALID_ACTION", "窗口动作无效")),
    };
    result.map_err(|e| AppError::new("WINDOW", e.to_string()))
}
#[tauri::command]
pub fn close_status(app: Service<'_>) -> Result<CloseStatus> {
    app.close_status()
}
#[tauri::command]
pub fn close_resolve(
    app: Service<'_>,
    mode: String,
    discard: Option<bool>,
    stop_recordings: Option<bool>,
    discard_ids: Option<Vec<String>>,
) -> Result<()> {
    app.inner().resolve_close(
        &mode,
        discard.unwrap_or(false),
        stop_recordings.unwrap_or(false),
        &discard_ids.unwrap_or_default(),
    )
}
#[tauri::command]
pub fn artifacts_list(app: Service<'_>) -> Result<Vec<Artifact>> {
    app.store.artifacts()
}
#[tauri::command]
pub async fn artifacts_discard(app: Service<'_>, artifact_ids: Vec<String>) -> Result<()> {
    let app = app.inner().clone();
    tokio::task::spawn_blocking(move || app.discard_artifacts(&artifact_ids))
        .await
        .map_err(|e| {
            AppError::new("CACHE_CLEANUP_FAILED", "清理缓存失败，请重试").detail(e.to_string())
        })?
}
#[tauri::command]
pub async fn export_prepare(
    app: Service<'_>,
    kind: String,
    target: Option<Target>,
    filter: Option<LogFilter>,
    task_id: Option<String>,
    segments: Option<Vec<usize>>,
    redacted: Option<bool>,
) -> Result<ExportPreview> {
    let _update_guard = app.allow_update_sensitive_work()?;
    if app.draining.load(Ordering::SeqCst) {
        return Err(AppError::new("APP_CLOSING", "正在退出，不能生成新导出"));
    }
    let redacted = redacted.unwrap_or(true);
    if kind == "view" {
        let target = target.ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择日志目标"))?;
        let snapshot = app
            .log_snapshot(&target, &filter.unwrap_or_default(), 10000, true)
            .await?;
        let mut lines = vec![format!(
            "Logcat 当前视图 · 导出时间 {} · 缓冲 {} · 淘汰 {} · {}",
            now(),
            snapshot.buffered,
            snapshot.dropped,
            snapshot.status
        )];
        lines.extend(snapshot.rows.into_iter().map(|r| r.raw));
        return app.prepare_text_export("当前日志视图", lines, redacted, vec![target.serial]);
    }
    if kind == "recording" {
        return app.export_recording(
            task_id
                .as_deref()
                .ok_or_else(|| AppError::new("TASK_REQUIRED", "请选择采集任务"))?,
            &segments.unwrap_or_default(),
            redacted,
        );
    }
    if kind != "diagnostics" {
        return Err(AppError::new("INVALID_EXPORT", "导出范围无效"));
    }
    let snapshot = app.snapshot().await?;
    let tasks:Vec<_>=snapshot.tasks.iter().take(20).map(|t|serde_json::json!({"id":t.id,"kind":t.operation.kind,"state":t.state,"phase":t.phase,"message":t.message,"diagnosticTail":t.log.iter().rev().take(5).collect::<Vec<_>>()})).collect();
    let summary = serde_json::json!({"product":"Android DevKit","version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"sdk":snapshot.environment,"server":snapshot.server,"devices":snapshot.devices,"installed":snapshot.packages.iter().filter(|p|p.installed).map(|p|(&p.id,&p.installed_version)).collect::<Vec<_>>(),"recentTasks":tasks});
    let ids = snapshot
        .devices
        .iter()
        .map(|d| d.serial.clone())
        .chain(
            snapshot
                .tasks
                .iter()
                .filter_map(|t| t.operation.target.as_ref().map(|t| t.serial.clone())),
        )
        .collect();
    app.prepare_text_export(
        "环境诊断",
        serde_json::to_string_pretty(&summary)?
            .lines()
            .map(String::from)
            .collect(),
        redacted,
        ids,
    )
}
#[tauri::command]
pub fn export_page(app: Service<'_>, artifact_id: String, offset: usize) -> Result<ExportPreview> {
    app.export_page(&artifact_id, offset)
}
#[tauri::command]
pub async fn export_save(app: Service<'_>, artifact_id: String) -> Result<Option<String>> {
    let (artifact, source) = app.checked_artifact(&artifact_id)?;
    let file_name = if artifact.kind == "screenshot" {
        "android-screenshot.png"
    } else {
        "android-devkit-export.txt"
    };
    let result = tokio::task::spawn_blocking(move || -> Result<Option<String>> {
        let Some(dest) = rfd::FileDialog::new().set_file_name(file_name).save_file() else {
            return Ok(None);
        };
        crate::exports::atomic_copy_verified(&source, &dest, &artifact.sha256)?;
        Ok(Some(dest.to_string_lossy().into()))
    })
    .await
    .map_err(|e| AppError::new("SAVE_FAILED", e.to_string()))??;
    if result.is_some() {
        app.store.acknowledge(&artifact_id)?;
        app.notify();
    }
    Ok(result)
}
#[tauri::command]
pub fn recording_segments(app: Service<'_>, task_id: String) -> Result<serde_json::Value> {
    let task = app.store.task(&task_id)?;
    if task.operation.kind != "log_record" {
        return Err(AppError::new("INVALID_TASK", "不是文件采集任务"));
    }
    let manifest: crate::recording::Manifest = serde_json::from_slice(&std::fs::read(
        app.data
            .join("recordings")
            .join(&task_id)
            .join("manifest.json"),
    )?)?;
    Ok(serde_json::to_value(manifest)?)
}
#[tauri::command]
pub async fn diagnostics(app: Service<'_>) -> Result<serde_json::Value> {
    let s = app.store.settings();
    let accel = crate::process::run(crate::process::Spec::new(
        Path::new(&s.sdk_root).join("emulator/emulator.exe"),
        vec!["-accel-check".into()],
    ))
    .await
    .map(|o| o.text() + &o.stderr)
    .unwrap_or_else(|e| e.to_string());
    let java = if s.java_path.is_empty() {
        "未配置".into()
    } else {
        crate::process::run(crate::process::Spec::new(
            &s.java_path,
            vec!["-version".into()],
        ))
        .await
        .map(|o| o.text() + &o.stderr)
        .unwrap_or_else(|e| e.to_string())
    };
    let mdns = if android::host_request("host:version").is_ok() {
        android::adb(&s, None, vec!["mdns".into(), "services".into()], 5)
            .await
            .map(|o| o.text())
            .unwrap_or_else(|e| e.to_string())
    } else {
        "ADB 未运行".into()
    };
    Ok(
        serde_json::json!({"sdkRoot":s.sdk_root,"environment":android::environment(&s),"acceleration":accel,"java":java,"mdns":mdns,"serverEpoch":app.epoch.load(Ordering::SeqCst)}),
    )
}
#[tauri::command]
pub fn license_respond(app: Service<'_>, task_id: String, accept: bool) -> Result<()> {
    let task = app.store.task(&task_id)?;
    if task.license_prompt.is_empty() {
        return Err(AppError::new("NO_LICENSE_PROMPT", "当前无待确认许可"));
    }
    let answers = app.answers.lock().unwrap();
    let tx = answers
        .get(&task_id)
        .ok_or_else(|| AppError::new("TASK_NOT_WAITING", "任务不再等待输入"))?;
    let permit = tx
        .try_reserve()
        .map_err(|_| AppError::new("TASK_NOT_WAITING", "任务不再等待输入"))?;
    app.store.audit(
        &task_id,
        if accept {
            "license_accepted_by_user"
        } else {
            "license_declined_by_user"
        },
    )?;
    app.update(&task_id, |t| {
        t.license_prompt.clear();
        if !accept {
            t.cancel_requested = true;
        }
    })?;
    permit.send(if accept { "y\n" } else { "n\n" }.into());
    Ok(())
}

pub fn setup(app: &mut tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = std::env::var("ANDROID_DEVKIT_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("LOCALAPPDATA")
                .map(|p| Path::new(&p).join("AndroidDevKit"))
                .unwrap_or_else(|_| app.path().app_local_data_dir().unwrap())
        });
    let state = App::new(root)?;
    state.recover_commits()?;
    state.recover_recordings()?;
    *state.handle.lock().unwrap() = Some(app.handle().clone());
    state.monitor();
    app.manage(state);
    crate::lifecycle::create_tray(app.handle())?;
    if let Some(window) = app.get_webview_window("main") {
        platform::install(&window);
    }
    Ok(())
}
