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
    tauri::async_runtime::spawn_blocking(move || app.task_records_page(&query))
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
pub async fn avd_usage(app: Service<'_>, avd_id: String) -> Result<crate::capacity::Usage> {
    let settings = app.store.settings();
    tokio::task::spawn_blocking(move || {
        let avd = android::avds(&settings)
            .into_iter()
            .find(|avd| avd.id == avd_id)
            .ok_or_else(|| AppError::new("AVD_NOT_FOUND", "AVD 已不存在"))?;
        Ok(crate::capacity::usage(Path::new(&avd.data_path)))
    })
    .await
    .map_err(|error| AppError::new("USAGE_FAILED", error.to_string()))?
}
#[tauri::command]
pub fn text_copy(text: String) -> Result<()> {
    if text.len() > 256 * 1024 {
        return Err(AppError::new(
            "CLIPBOARD_LIMIT",
            "复制内容超过 256 KiB，请改用文件导出",
        ));
    }
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|error| {
            AppError::new("CLIPBOARD_FAILED", "复制失败，请重试").detail(error.to_string())
        })
}
#[tauri::command]
pub async fn environment_summary(app: Service<'_>, redacted: Option<bool>) -> Result<String> {
    let snapshot = app.snapshot().await?;
    let tasks: Vec<_> = snapshot
        .tasks
        .iter()
        .take(20)
        .map(|task| {
            serde_json::json!({
                "kind": task.operation.kind, "state": task.state, "message": task.message
            })
        })
        .collect();
    let summary = serde_json::json!({"product":"Android DevKit", "version":env!("CARGO_PKG_VERSION"),
        "os":std::env::consts::OS, "arch":std::env::consts::ARCH, "sdk":snapshot.environment,
        "server":snapshot.server, "devices":snapshot.devices,
        "installed":snapshot.packages.iter().filter(|p|p.installed).map(|p|(&p.id,&p.installed_version)).collect::<Vec<_>>(),
        "recentTasks":tasks});
    let content = serde_json::to_string_pretty(&summary)?;
    if !redacted.unwrap_or(true) {
        return Ok(content);
    }
    let ids = snapshot
        .devices
        .iter()
        .map(|device| device.serial.clone())
        .chain(snapshot.tasks.iter().filter_map(|task| {
            task.operation
                .target
                .as_ref()
                .map(|target| target.serial.clone())
        }))
        .collect();
    Ok(crate::privacy::Redactor::new(ids).redact(&content))
}
#[tauri::command]
pub async fn task_delete(app: Service<'_>, request: TaskDeleteRequest) -> Result<TaskDeleteResult> {
    let app = app.inner().clone();
    tokio::task::spawn_blocking(move || app.delete_task_records(request))
        .await
        .map_err(|e| {
            AppError::new("TASK_DELETE_FAILED", "删除任务记录失败").detail(e.to_string())
        })?
}
#[tauri::command]
pub async fn recording_discard(app: Service<'_>, task_id: String) -> Result<()> {
    let app = app.inner().clone();
    tokio::task::spawn_blocking(move || app.discard_recording(&task_id))
        .await
        .map_err(|e| {
            AppError::new("RECORDING_CLEANUP_FAILED", "清理原始采集失败").detail(e.to_string())
        })?
}
#[tauri::command]
pub async fn java_environment(app: Service<'_>) -> Result<crate::java::JavaEnvironment> {
    app.java_environment().await
}
#[tauri::command]
pub async fn java_check(app: Service<'_>, path: Option<String>) -> Result<crate::java::JavaCheck> {
    let settings = app.store.settings();
    Ok(crate::java::check(
        &settings,
        path.as_deref().unwrap_or(&settings.java_path),
        "configured",
    )
    .await)
}
#[tauri::command]
pub async fn java_apply(app: Service<'_>, path: String) -> Result<crate::java::JavaCheck> {
    app.apply_java(&path).await
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
    if settings.java_path != app.store.settings().java_path {
        return Err(AppError::new(
            "JAVA_CONTEXT_CHANGED",
            "Java 配置已更新，请刷新设置后保存；切换运行时请使用配置 Java",
        ));
    }
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
    app.store.save_settings(&settings)?;
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
pub async fn plan_create(app: Service<'_>, mut operation: Operation) -> Result<Plan> {
    if matches!(operation.kind.as_str(), "file_upload" | "file_download") {
        app.prepare_transfer_destinations(&mut operation).await?;
    }
    let settings = app.store.settings();
    if crate::java::needed(&operation.kind) {
        if app
            .store
            .active_tasks()?
            .iter()
            .any(|task| task.operation.kind == "java_prepare")
        {
            return Err(AppError::new(
                "JAVA_CONFIGURATION_REQUIRED",
                "Java 准备进行中，请等待任务完成后返回安装计划",
            ));
        }
        crate::java::check(&settings, &settings.java_path, "configured")
            .await
            .require()?;
    }
    if operation.kind == "bootstrap" && operation.optional("purpose") == "emulator" {
        let needs_java = !crate::java::check(&settings, &settings.java_path, "configured")
            .await
            .ready();
        operation
            .args
            .insert("prepareJava".into(), needs_java.to_string());
    }
    if operation.kind == "avd_create" {
        let emulator = Path::new(&settings.sdk_root).join("emulator/emulator.exe");
        let check = if emulator.is_file() {
            let mut spec = crate::process::Spec::new(emulator, vec!["-accel-check".into()]);
            spec.timeout = 10;
            match crate::process::run(spec).await {
                Ok(output) => format!(
                    "{}：{}{}",
                    if output.code == 0 {
                        "可用"
                    } else {
                        "不可用"
                    },
                    output.text(),
                    output.stderr
                ),
                Err(error) => error.to_string(),
            }
        } else {
            "待检查：安装 Emulator 后在启动前检测，不自动修改系统虚拟化功能。".into()
        };
        operation.args.insert("accelerationCheck".into(), check);
    }
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
    app.retry_plan(&task_id)
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
pub async fn apk_set_prepare(
    app: Service<'_>,
    paths: Vec<String>,
) -> Result<crate::phone_apk::ApkSet> {
    app.prepare_apk_set(paths.into_iter().map(std::path::PathBuf::from).collect())
        .await
}
#[tauri::command]
pub fn apk_set_tools(app: Service<'_>) -> Result<String> {
    crate::phone_apk::tool_package(&app)
}
#[tauri::command]
pub async fn phone_selection_release(app: Service<'_>, kind: String, id: String) -> Result<bool> {
    let app = app.inner().clone();
    tokio::task::spawn_blocking(move || app.release_phone_selection(&kind, &id))
        .await
        .map_err(|e| AppError::new("STAGING_RELEASE", e.to_string()))?
}
#[tauri::command]
pub async fn phone_choose(kind: String) -> Result<Vec<String>> {
    tokio::task::spawn_blocking(move || {
        let dialog = rfd::FileDialog::new();
        let paths = match kind.as_str() {
            "apk_directory" | "download_directory" => dialog.pick_folder().map(|p| vec![p]),
            "upload_directory" => dialog.pick_folders(),
            "apk_files" => dialog.add_filter("Android APK", &["apk"]).pick_files(),
            "upload_files" => dialog.pick_files(),
            _ => return Err(AppError::new("DIALOG_KIND", "选择类型无效")),
        };
        Ok(paths
            .unwrap_or_default()
            .iter()
            .map(|p| p.to_string_lossy().into())
            .collect())
    })
    .await
    .map_err(|e| AppError::new("DIALOG", e.to_string()))?
}
#[tauri::command]
pub async fn device_files(
    app: Service<'_>,
    target: Target,
    path: String,
) -> Result<Vec<crate::phone_files::RemoteEntry>> {
    app.remote_entries(&target, &path).await
}
#[tauri::command]
pub async fn transfer_prepare(
    app: Service<'_>,
    direction: String,
    paths: Vec<String>,
    target: Target,
    directory: String,
) -> Result<crate::phone_files::TransferSet> {
    app.prepare_transfer(&direction, paths, target, directory)
        .await
}
#[tauri::command]
pub fn video_preview(app: Service<'_>, artifact_id: String) -> Result<String> {
    let (artifact, path) = app.checked_artifact(&artifact_id)?;
    let _task_use = app.use_task_record(&artifact.task_id, false)?;
    if artifact.kind != "video" {
        return Err(AppError::new("ARTIFACT_TYPE", "请选择 MP4 录屏"));
    }
    if artifact.bytes > 64 * 1024 * 1024 {
        return Err(AppError::new(
            "PREVIEW_SIZE",
            "超过 64 MiB，请保存后使用本机播放器查看",
        ));
    }
    crate::phone_record::validate_mp4(&path)?;
    Ok(format!(
        "data:video/mp4;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?)
    ))
}
#[tauri::command]
pub async fn artifact_save(app: Service<'_>, task_id: String) -> Result<Option<String>> {
    let _task_use = app.use_task_record(&task_id, false)?;
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
    let _task_use = app.use_task_record(&task_id, false)?;
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
    serde_json::json!({"maximized":window.is_maximized().unwrap_or(false),"minimized":window.is_minimized().unwrap_or(false),"focused":window.is_focused().unwrap_or(false),"visible":window.is_visible().unwrap_or(false),"scale":window.scale_factor().unwrap_or(1.0)})
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
    } else if artifact.kind == "video" {
        "android-screenrecord.mp4"
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
    let _task_use = app.use_task_record(&task_id, false)?;
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
    state.recover_committed_phone_recordings()?;
    // Cache cleanup is best effort; it must never prevent opening recovery UI.
    if let Err(error) = state.prune_phone_selections() {
        eprintln!("Unsubmitted phone cache cleanup: {error}");
    }
    *state.handle.lock().unwrap() = Some(app.handle().clone());
    state.monitor();
    app.manage(state);
    if let Err(error) = crate::lifecycle::create_tray(app.handle()) {
        // A missing system tray must not prevent startup or leave the app unreachable.
        eprintln!("System tray: {error}");
    }
    if let Some(window) = app.get_webview_window("main") {
        platform::install(&window);
        let preferences = app.state::<Arc<App>>().store.preferences()?;
        window.set_theme(Some(if preferences.theme == "dark" {
            tauri::Theme::Dark
        } else {
            tauri::Theme::Light
        }))?;
        #[cfg(windows)]
        if let Err(error) = crate::window_state::install(&window, app.state::<Arc<App>>().inner()) {
            eprintln!("Window preferences: {error}");
        }
        #[cfg(windows)]
        if let Err(error) = crate::ui_preferences::install(&window, app.state::<Arc<App>>().inner())
        {
            eprintln!("UI zoom preference: {error}");
        }
        window.show()?;
    }
    Ok(())
}
