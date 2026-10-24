//! Signed, stable Windows NSIS updates; installation is exclusive with new work.
use crate::{app::App, domain::*};
use serde::Serialize;
#[cfg(any(all(windows, target_arch = "x86_64", not(debug_assertions)), test))]
use std::path::Path;
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::{Mutex, RwLockReadGuard, RwLockWriteGuard};

const RELEASES_URL: &str = "https://github.com/whoarei/android-devkit/releases/latest";

#[derive(Default)]
pub struct UpdateState {
    checked: Mutex<Option<(String, Update)>>,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub update_id: String,
    pub current_version: String,
    pub latest_version: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub can_auto_install: bool,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateProgress {
    pub phase: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub downloaded: u64,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub total: Option<u64>,
}

pub struct InstallGuard<'a> {
    app: &'a App,
    _gate: RwLockWriteGuard<'a, ()>,
}
impl Drop for InstallGuard<'_> {
    fn drop(&mut self) {
        self.app.updating.store(false, Ordering::SeqCst);
    }
}

impl App {
    pub fn allow_update_sensitive_work(&self) -> Result<RwLockReadGuard<'_, ()>> {
        self.update_gate
            .try_read()
            .map_err(|_| AppError::new("APP_UPDATING", "软件正在更新，请等待更新完成后再操作。"))
    }

    fn begin_app_update(&self) -> Result<InstallGuard<'_>> {
        let gate = self
            .update_gate
            .try_write()
            .map_err(|_| AppError::new("UPDATE_BUSY", "有操作正在处理，请稍后重试软件更新。"))?;
        let status = self.close_status()?;
        if self.terminals.active_count() > 0 {
            return Err(AppError::new(
                "UPDATE_TERMINALS_ACTIVE",
                "请先关闭终端会话，再安装更新。",
            ));
        }
        if status.draining || !status.tasks.is_empty() {
            return Err(AppError::new(
                "UPDATE_TASKS_ACTIVE",
                "请先完成或停止所有任务和日志采集，再安装更新。",
            ));
        }
        if !status.unsaved.is_empty() {
            return Err(AppError::new(
                "UPDATE_UNSAVED_FILES",
                "请先保存或清理待保存文件，再安装更新。",
            ));
        }
        self.updating.store(true, Ordering::SeqCst);
        Ok(InstallGuard {
            app: self,
            _gate: gate,
        })
    }
}

fn newer_stable(current: &semver::Version, candidate: &semver::Version) -> bool {
    candidate.pre.is_empty() && candidate.cmp_precedence(current).is_gt()
}

fn validate_candidate(version: &str, url: &str, signature: &str) -> Result<()> {
    let version = semver::Version::parse(version)
        .map_err(|_| AppError::new("INVALID_UPDATE", "更新清单无效，请前往下载页面更新。"))?;
    let expected = format!(
        "https://github.com/whoarei/android-devkit/releases/download/v{version}/AndroidDevKit-{version}-setup.exe"
    );
    if !version.pre.is_empty()
        || url != expected
        || signature.trim().is_empty()
        || signature.len() > 16384
    {
        return Err(AppError::new(
            "INVALID_UPDATE",
            "更新清单无效，请前往下载页面更新。",
        ));
    }
    Ok(())
}

fn require_checked_id(checked: Option<&str>, expected: &str) -> Result<()> {
    if checked != Some(expected) {
        return Err(AppError::new(
            "UPDATE_EXPIRED",
            "更新信息已失效，请重新检查更新。",
        ));
    }
    Ok(())
}

#[cfg(any(all(windows, target_arch = "x86_64", not(debug_assertions)), test))]
fn matches_installation(exe: &Path, location: &str, uninstaller: &str) -> bool {
    let root = Path::new(location.trim_matches('"'));
    let uninstall = Path::new(uninstaller.trim_matches('"'));
    if !root.is_absolute() || !uninstall.is_absolute() || !root.join("uninstall.exe").is_file() {
        return false;
    }
    match (
        exe.canonicalize(),
        root.join("android-devkit.exe").canonicalize(),
        uninstall.canonicalize(),
        root.join("uninstall.exe").canonicalize(),
    ) {
        (Ok(exe), Ok(installed), Ok(uninstall), Ok(expected)) => {
            exe == installed && uninstall == expected
        }
        _ => false,
    }
}

fn current_exe_is_installed() -> bool {
    #[cfg(all(windows, target_arch = "x86_64", not(debug_assertions)))]
    {
        use winreg::{enums::*, RegKey};
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let root = RegKey::predef(HKEY_CURRENT_USER);
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Ok(key) = root.open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Android DevKit",
                KEY_READ | view,
            ) else {
                continue;
            };
            if let (Ok(location), Ok(uninstall)) = (
                key.get_value::<String, _>("InstallLocation"),
                key.get_value::<String, _>("UninstallString"),
            ) {
                if matches_installation(&exe, &location, &uninstall) {
                    return true;
                }
            }
        }
    }
    false
}

#[tauri::command]
pub async fn check_app_update(
    handle: AppHandle,
    app: State<'_, Arc<App>>,
) -> Result<Option<AppUpdateInfo>> {
    let mut checked = app
        .app_update
        .checked
        .try_lock()
        .map_err(|_| AppError::new("UPDATE_BUSY", "正在检查或安装更新，请稍后重试。"))?;
    *checked = None;
    let mut builder = handle
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .version_comparator(|current, release| newer_stable(&current, &release.version));
    let proxy = app.store.settings().proxy;
    if !proxy.is_empty() {
        builder = builder.proxy(
            proxy
                .parse()
                .map_err(|_| AppError::new("INVALID_PROXY", "代理地址无效。"))?,
        );
    } else {
        builder = builder.no_proxy();
    }
    let update = builder
        .build()
        .map_err(|e| {
            AppError::new(
                "UPDATE_CHECK_FAILED",
                "无法检查更新，请检查网络或稍后重试。",
            )
            .detail(e.to_string())
        })?
        .check()
        .await
        .map_err(|e| {
            AppError::new(
                "UPDATE_CHECK_FAILED",
                "无法检查更新，请检查网络或稍后重试。",
            )
            .detail(e.to_string())
        })?;
    let Some(mut update) = update else {
        return Ok(None);
    };
    validate_candidate(
        &update.version,
        update.download_url.as_str(),
        &update.signature,
    )?;
    update.timeout = Some(Duration::from_secs(600));
    let id = uuid::Uuid::new_v4().to_string();
    let info = AppUpdateInfo {
        update_id: id.clone(),
        current_version: handle.package_info().version.to_string(),
        latest_version: update.version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        published_at: update.date.map(|d| d.to_string()),
        can_auto_install: current_exe_is_installed(),
    };
    *checked = Some((id, update));
    Ok(Some(info))
}

#[tauri::command]
pub async fn install_app_update(
    handle: AppHandle,
    app: State<'_, Arc<App>>,
    update_id: String,
) -> Result<()> {
    if !current_exe_is_installed() {
        return Err(AppError::new(
            "UPDATE_MANUAL_ONLY",
            "当前为便携版或开发版，请前往下载页面更新。",
        ));
    }
    let checked = app
        .app_update
        .checked
        .try_lock()
        .map_err(|_| AppError::new("UPDATE_BUSY", "正在检查或安装更新，请稍后重试。"))?;
    require_checked_id(checked.as_ref().map(|(id, _)| id.as_str()), &update_id)?;
    let update = &checked.as_ref().unwrap().1;
    let _install_guard = app.begin_app_update()?;
    let mut downloaded = 0u64;
    let mut last_emit = Instant::now();
    let emit = |phase: &str, downloaded, total| {
        let _ = handle.emit_to(
            "main",
            "app-update-progress",
            AppUpdateProgress {
                phase: phase.into(),
                downloaded,
                total,
            },
        );
    };
    emit("downloading", 0, None);
    let bytes = update
        .download(
            |chunk, total| {
                downloaded = downloaded.saturating_add(chunk as u64);
                if last_emit.elapsed() >= Duration::from_millis(100) || total == Some(downloaded) {
                    emit("downloading", downloaded, total);
                    last_emit = Instant::now();
                }
            },
            || emit("verifying", 0, None),
        )
        .await
        .map_err(|e| {
            AppError::new(
                "UPDATE_DOWNLOAD_FAILED",
                "更新下载或签名校验失败，请重试或前往下载页面。",
            )
            .detail(e.to_string())
        })?;
    emit("installing", downloaded, Some(downloaded));
    app.stop_observers();
    update.install(bytes).map_err(|e| {
        AppError::new(
            "UPDATE_INSTALL_FAILED",
            "更新安装未完成，请重试或前往下载页面。",
        )
        .detail(e.to_string())
    })?;
    Err(AppError::new(
        "UPDATE_INSTALL_FAILED",
        "更新安装未完成，请重试或前往下载页面。",
    ))
}

#[tauri::command]
pub fn open_app_download() -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let url: Vec<u16> = format!("{RELEASES_URL}\0").encode_utf16().collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                url.as_ptr(),
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
        "OPEN_DOWNLOAD_FAILED",
        "无法打开下载页面，请在浏览器中访问 GitHub Releases。",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the signed CI release directory in ANDROID_DEVKIT_UPDATE_VERIFY_DIR"]
    fn signed_release_verifies_with_embedded_public_key() {
        use base64::Engine;
        let directory = std::path::PathBuf::from(
            std::env::var("ANDROID_DEVKIT_UPDATE_VERIFY_DIR").expect("signed release directory"),
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("latest.json")).unwrap()).unwrap();
        let version = manifest["version"].as_str().unwrap();
        let target = &manifest["platforms"]["windows-x86_64"];
        let signature = target["signature"].as_str().unwrap();
        validate_candidate(version, target["url"].as_str().unwrap(), signature).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let decode = |value: &str| {
            String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(value)
                    .unwrap(),
            )
            .unwrap()
        };
        let key = minisign_verify::PublicKey::decode(&decode(
            config["plugins"]["updater"]["pubkey"].as_str().unwrap(),
        ))
        .unwrap();
        let signature = minisign_verify::Signature::decode(&decode(signature)).unwrap();
        let mut bytes =
            std::fs::read(directory.join(format!("AndroidDevKit-{version}-setup.exe"))).unwrap();
        key.verify(&bytes, &signature, true)
            .expect("release must verify with the exact public key embedded in the app");
        bytes[0] ^= 1;
        assert!(
            key.verify(&bytes, &signature, true).is_err(),
            "modified installer bytes must be rejected"
        );
    }
    #[test]
    fn stable_versions_ignore_previews_downgrades_and_build_metadata() {
        for (current, candidate, expected) in [
            ("0.1.0", "0.1.1", true),
            ("0.2.0", "0.10.0", true),
            ("0.2.0", "0.2.0", false),
            ("0.2.0", "0.1.9", false),
            ("0.2.0", "0.3.0-rc.1", false),
            ("0.2.0", "0.2.0+build2", false),
        ] {
            assert_eq!(
                newer_stable(&current.parse().unwrap(), &candidate.parse().unwrap()),
                expected
            );
        }
    }
    #[test]
    fn only_accepts_this_products_versioned_installer() {
        let good = "https://github.com/whoarei/android-devkit/releases/download/v0.2.0/AndroidDevKit-0.2.0-setup.exe";
        assert!(validate_candidate("0.2.0", good, "signature").is_ok());
        for url in [
            good.replace("https:", "http:"),
            good.replace("android-devkit", "remote-runner"),
            good.replace("v0.2.0/", "v0.3.0/"),
            format!("{good}?token=test"),
            good.replace("-setup.exe", ".zip"),
        ] {
            assert!(validate_candidate("0.2.0", &url, "signature").is_err());
        }
        assert!(validate_candidate("0.2.0", good, " ").is_err());
        assert!(validate_candidate("0.2.0-rc.1", good, "signature").is_err());
        assert!(require_checked_id(None, "id").is_err());
        assert!(require_checked_id(Some("new"), "old").is_err());
        assert!(require_checked_id(Some("id"), "id").is_ok());
    }
    #[test]
    fn installation_identity_uses_exact_files() {
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("installed");
        let portable = dir.path().join("portable");
        for root in [&installed, &portable] {
            std::fs::create_dir(root).unwrap();
            std::fs::write(root.join("android-devkit.exe"), b"exe").unwrap();
        }
        std::fs::write(installed.join("uninstall.exe"), b"uninstaller").unwrap();
        let location = format!("\"{}\"", installed.display());
        let uninstall = format!("\"{}\"", installed.join("uninstall.exe").display());
        assert!(matches_installation(
            &installed.join("android-devkit.exe"),
            &location,
            &uninstall
        ));
        assert!(!matches_installation(
            &portable.join("android-devkit.exe"),
            &location,
            &uninstall
        ));
        assert!(!matches_installation(
            &installed.join("android-devkit.exe"),
            "relative",
            &uninstall
        ));
        std::fs::remove_file(installed.join("uninstall.exe")).unwrap();
        std::fs::create_dir(installed.join("uninstall.exe")).unwrap();
        assert!(!matches_installation(
            &installed.join("android-devkit.exe"),
            &location,
            &uninstall
        ));
    }
    #[test]
    fn installation_gate_preserves_pending_files_and_serializes_new_work() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let read = app.allow_update_sensitive_work().unwrap();
        assert!(app.begin_app_update().is_err());
        drop(read);
        let install = app.begin_app_update().unwrap();
        assert!(app.updating.load(Ordering::SeqCst));
        assert!(app.allow_update_sensitive_work().is_err());
        assert_eq!(
            app.submit("", "", "new-task", "", &[]).unwrap_err().code,
            "APP_UPDATING"
        );
        drop(install);
        assert!(!app.updating.load(Ordering::SeqCst));
        app.prepare_text_export("test", vec!["data".into()], false, vec![])
            .unwrap();
        assert_eq!(
            app.begin_app_update().err().unwrap().code,
            "UPDATE_UNSAVED_FILES"
        );
        app.draining.store(true, Ordering::SeqCst);
        assert_eq!(
            app.begin_app_update().err().unwrap().code,
            "UPDATE_TASKS_ACTIVE"
        );
    }

    #[test]
    fn running_tasks_block_updates_until_they_finish() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let mut task: Task = serde_json::from_value(serde_json::json!({
            "id": "recording", "requestId": "recording", "title": "recording",
            "operation": {"kind": "log_record", "args": {}, "target": null},
            "state": "running", "phase": "recording", "progress": null,
            "message": "", "log": [], "createdAt": 1, "updatedAt": 1,
            "cancelRequested": false, "cancellable": true, "artifact": "",
            "licensePrompt": "", "confirmedLicenses": []
        }))
        .unwrap();
        app.store.save_task(&task).unwrap();
        assert_eq!(
            app.begin_app_update().err().unwrap().code,
            "UPDATE_TASKS_ACTIVE"
        );
        task.state = "succeeded".into();
        app.store.save_task(&task).unwrap();
        assert!(app.begin_app_update().is_ok());
    }
}
