//! App management is scoped to an explicit transport and Android user.
use crate::{app::App, domain::*};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub fn validate_package(package: &str) -> Result<()> {
    if package == "android" {
        Ok(())
    } else {
        crate::app::validate_app_package(package)
    }
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceApp {
    pub package: String,
    pub version_code: String,
    pub uid: String,
    pub system: bool,
    pub disabled: bool,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAppList {
    pub user_id: u32,
    pub apps: Vec<DeviceApp>,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAppDetail {
    pub app: DeviceApp,
    pub user_id: u32,
    pub version_name: String,
    pub min_sdk: String,
    pub target_sdk: String,
    pub paths: Vec<String>,
    pub fingerprint: String,
}
pub fn parse_packages(text: &str) -> Result<Vec<DeviceApp>> {
    let mut apps = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let package = parts
            .next()
            .and_then(|p| p.strip_prefix("package:"))
            .ok_or_else(|| AppError::new("APP_LIST", "设备返回的应用列表无效"))?;
        validate_package(package)?;
        let fields: BTreeMap<_, _> = parts.filter_map(|p| p.split_once(':')).collect();
        apps.insert(
            package.to_string(),
            DeviceApp {
                package: package.into(),
                version_code: fields.get("versionCode").unwrap_or(&"").to_string(),
                uid: fields.get("uid").unwrap_or(&"").to_string(),
                system: false,
                disabled: false,
            },
        );
        if apps.len() > 10000 {
            return Err(AppError::new("APP_LIST_LIMIT", "应用列表超过上限"));
        }
    }
    Ok(apps.into_values().collect())
}
fn apk_paths(text: &str) -> Result<Vec<String>> {
    let mut paths = BTreeSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let p = line.strip_prefix("package:").unwrap_or("").trim();
        if !p.starts_with('/')
            || !p.ends_with(".apk")
            || p.len() > 4096
            || p.contains(['\\', '\r', '\n', '\0'])
            || p.split('/').any(|c| c == ".." || c == ".")
        {
            return Err(AppError::new("APP_PATH", "设备返回的 APK 路径无效"));
        }
        paths.insert(p.to_string());
    }
    if paths.is_empty() || paths.len() > 256 {
        return Err(AppError::new("APP_NOT_FOUND", "应用未安装或 APK 集合无效"));
    }
    Ok(paths.into_iter().collect())
}
fn field(text: &str, name: &str) -> String {
    let pattern = format!(r"(?:^|\s){}=([^\s]+)", regex::escape(name));
    regex::Regex::new(&pattern)
        .unwrap()
        .captures(text)
        .map(|c| c[1].into())
        .unwrap_or_default()
}
pub fn check_action(action: &str, detail: &DeviceAppDetail) -> Result<()> {
    if !matches!(action, "launch" | "stop" | "clear" | "uninstall" | "export") {
        return Err(AppError::new("APP_ACTION", "应用操作无效"));
    }
    if detail.app.system && matches!(action, "stop" | "clear" | "uninstall") {
        return Err(AppError::new(
            "SYSTEM_APP_PROTECTED",
            "系统应用仅提供查看、启动与 APK 导出",
        ));
    }
    Ok(())
}
impl App {
    async fn android_user(&self, settings: &Settings, target: &Target) -> Result<u32> {
        self.bound_adb(
            settings,
            target,
            vec!["shell".into(), "am".into(), "get-current-user".into()],
            10,
        )
        .await?
        .success()?
        .text()
        .trim()
        .parse::<u32>()
        .map_err(|_| AppError::new("ANDROID_USER", "无法确认当前 Android 用户"))
    }
    pub async fn list_device_apps(&self, target: &Target) -> Result<DeviceAppList> {
        let settings = self.store.settings();
        let user_id = self.android_user(&settings, target).await?;
        let query = |flag: &str| {
            vec![
                "shell".into(),
                "pm".into(),
                "list".into(),
                "packages".into(),
                "--user".into(),
                user_id.to_string(),
                flag.into(),
            ]
        };
        let mut all = query("--show-versioncode");
        all.push("-U".into());
        let mut apps = parse_packages(
            &self
                .bound_adb(&settings, target, all, 30)
                .await?
                .success()?
                .text(),
        )?;
        let system: BTreeSet<_> = parse_packages(
            &self
                .bound_adb(&settings, target, query("-s"), 30)
                .await?
                .success()?
                .text(),
        )?
        .into_iter()
        .map(|a| a.package)
        .collect();
        let disabled: BTreeSet<_> = parse_packages(
            &self
                .bound_adb(&settings, target, query("-d"), 30)
                .await?
                .success()?
                .text(),
        )?
        .into_iter()
        .map(|a| a.package)
        .collect();
        for app in &mut apps {
            app.system = system.contains(&app.package);
            app.disabled = disabled.contains(&app.package);
        }
        if self.android_user(&settings, target).await? != user_id {
            return Err(AppError::new(
                "ANDROID_USER_CHANGED",
                "Android 用户已切换，请刷新应用列表",
            ));
        }
        Ok(DeviceAppList { user_id, apps })
    }
    pub async fn device_app_detail(
        &self,
        target: &Target,
        package: &str,
    ) -> Result<DeviceAppDetail> {
        validate_package(package)?;
        let list = self.list_device_apps(target).await?;
        let app = list
            .apps
            .into_iter()
            .find(|a| a.package == package)
            .ok_or_else(|| AppError::new("APP_NOT_FOUND", "应用已移除，请刷新列表"))?;
        let settings = self.store.settings();
        let paths = apk_paths(
            &self
                .bound_adb(
                    &settings,
                    target,
                    vec![
                        "shell".into(),
                        "pm".into(),
                        "path".into(),
                        "--user".into(),
                        list.user_id.to_string(),
                        package.into(),
                    ],
                    15,
                )
                .await?
                .success()?
                .text(),
        )?;
        let dump = self
            .bound_adb(
                &settings,
                target,
                vec![
                    "shell".into(),
                    "dumpsys".into(),
                    "package".into(),
                    package.into(),
                ],
                20,
            )
            .await?
            .success()?
            .text();
        let version = field(&dump, "versionCode");
        if version.is_empty() || (!app.version_code.is_empty() && app.version_code != version) {
            return Err(AppError::new(
                "APP_CHANGED",
                "读取期间应用发生变化，请刷新重试",
            ));
        }
        let timestamp = |name: &str| {
            dump.lines()
                .find_map(|line| {
                    line.trim()
                        .strip_prefix(&format!("{name}="))
                        .map(str::to_owned)
                })
                .unwrap_or_default()
        };
        if self.android_user(&settings, target).await? != list.user_id {
            return Err(AppError::new(
                "ANDROID_USER_CHANGED",
                "Android 用户已切换，请刷新应用列表",
            ));
        }
        let fingerprint = digest(&serde_json::to_vec(&(
            list.user_id,
            package,
            &version,
            &paths,
            timestamp("firstInstallTime"),
            timestamp("lastUpdateTime"),
            app.system,
        ))?);
        Ok(DeviceAppDetail {
            app,
            user_id: list.user_id,
            version_name: field(&dump, "versionName"),
            min_sdk: field(&dump, "minSdk"),
            target_sdk: field(&dump, "targetSdk"),
            paths,
            fingerprint,
        })
    }
    pub async fn execute_device_app(
        &self,
        id: &str,
        op: &Operation,
        settings: &Settings,
    ) -> Result<String> {
        let target = op
            .target
            .as_ref()
            .ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择设备"))?;
        let package = op.arg("package")?;
        let detail = self.device_app_detail(target, package).await?;
        let action = op.kind.strip_prefix("device_app_").unwrap_or("");
        check_action(action, &detail)?;
        if detail.fingerprint != op.arg("appFingerprint")?
            || detail.user_id.to_string() != op.arg("userId")?
        {
            return Err(AppError::new(
                "APP_CHANGED",
                "应用版本、安装位置或 Android 用户已变化，请重新确认",
            ));
        }
        if action == "export" {
            return self.export_device_apks(id, target, &detail, settings).await;
        }
        let user = detail.user_id.to_string();
        let args: Vec<String> = match action {
            "stop" => vec!["shell", "am", "force-stop", "--user", &user, package],
            "clear" => vec!["shell", "pm", "clear", "--user", &user, package],
            "uninstall" => vec!["shell", "pm", "uninstall", "--user", &user, package],
            "launch" => {
                let out = self
                    .bound_adb(
                        settings,
                        target,
                        vec![
                            "shell".into(),
                            "cmd".into(),
                            "package".into(),
                            "resolve-activity".into(),
                            "--brief".into(),
                            "--user".into(),
                            user.clone(),
                            "-a".into(),
                            "android.intent.action.MAIN".into(),
                            "-c".into(),
                            "android.intent.category.LAUNCHER".into(),
                            package.into(),
                        ],
                        15,
                    )
                    .await?
                    .success()?;
                let component = out
                    .text()
                    .lines()
                    .find(|l| l.starts_with(&format!("{package}/")))
                    .map(str::to_owned)
                    .ok_or_else(|| AppError::new("NO_LAUNCHER", "该应用没有可启动的桌面入口"))?;
                if !component
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._/$".contains(&b))
                {
                    return Err(AppError::new("NO_LAUNCHER", "应用入口无效"));
                }
                let output = self
                    .bound_adb(
                        settings,
                        target,
                        vec![
                            "shell".into(),
                            "am".into(),
                            "start".into(),
                            "-W".into(),
                            "--user".into(),
                            user,
                            "-n".into(),
                            component,
                        ],
                        30,
                    )
                    .await?
                    .success()?;
                if output.text().contains("Error:") || !output.text().contains("Status: ok") {
                    return Err(
                        AppError::new("APP_LAUNCH_FAILED", "应用启动失败").detail(output.text())
                    );
                }
                return Ok("应用已启动".into());
            }
            _ => return Err(AppError::new("APP_ACTION", "应用操作无效")),
        }
        .into_iter()
        .map(str::to_owned)
        .collect();
        let out = self
            .bound_adb(settings, target, args, 60)
            .await?
            .success()?;
        if matches!(action, "clear" | "uninstall") && out.text().trim() != "Success" {
            return Err(AppError::new("APP_ACTION_FAILED", "设备拒绝应用操作").detail(out.text()));
        }
        if action == "stop" && (!out.text().trim().is_empty() || !out.stderr.trim().is_empty()) {
            return Err(
                AppError::new("APP_ACTION_FAILED", "设备未确认应用停止").detail(format!(
                    "{}\n{}",
                    out.text(),
                    out.stderr
                )),
            );
        }
        if action == "uninstall"
            && self
                .list_device_apps(target)
                .await?
                .apps
                .iter()
                .any(|a| a.package == package)
        {
            return Err(AppError::new(
                "RESULT_UNKNOWN",
                "卸载后仍发现应用，请刷新核对",
            ));
        }
        Ok(match action {
            "clear" => "应用数据已清除",
            "uninstall" => "应用已卸载",
            _ => "应用已停止",
        }
        .into())
    }
    async fn export_device_apks(
        &self,
        id: &str,
        target: &Target,
        detail: &DeviceAppDetail,
        settings: &Settings,
    ) -> Result<String> {
        let stage = self.data.join("app-exports").join(id);
        std::fs::create_dir_all(&stage)?;
        let result = async {
            self.phase(id, "pulling", "读取并校验已安装 APK")?;
            self.update(id, |t| t.cancellable = true)?;
            let mut total = 0u64;
            for (index, path) in detail.paths.iter().enumerate() {
                if self.cancelled(id) {
                    return Err(AppError::new("CANCELLED", "已取消 APK 导出"));
                }
                let hash = self.remote_hash(settings, target, path).await?;
                if hash.is_empty() {
                    return Err(AppError::new(
                        "APK_EXPORT_VERIFY",
                        "设备不支持 SHA-256，无法校验 APK 导出",
                    ));
                }
                let bytes = self
                    .remote_shell(
                        settings,
                        target,
                        format!("stat -c %s {}", crate::phone_files::quote(path)),
                        15,
                    )
                    .await?
                    .text()
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| AppError::new("APK_EXPORT_SIZE", "无法读取 APK 大小"))?;
                total = total.saturating_add(bytes);
                if bytes > 1024 * 1024 * 1024 || total > 4 * 1024 * 1024 * 1024 {
                    return Err(AppError::new(
                        "APK_EXPORT_SIZE",
                        "单个 APK 最多 1 GiB，整组最多 4 GiB",
                    ));
                }
                crate::capacity::budget(&[(stage.clone(), bytes * 2, "APK 导出")])?;
                let local = stage.join(format!("{index}.apk"));
                self.bound_adb(
                    settings,
                    target,
                    vec!["pull".into(), path.clone(), local.to_string_lossy().into()],
                    180,
                )
                .await?
                .success()?;
                if local.metadata()?.len() != bytes
                    || digest_file(&local)? != hash
                    || self.remote_hash(settings, target, path).await? != hash
                {
                    return Err(AppError::new(
                        "APK_EXPORT_VERIFY",
                        "导出期间 APK 内容变化，未发布结果",
                    ));
                }
                self.update(id, |t| {
                    t.progress = Some((index + 1) as f64 * 100.0 / detail.paths.len() as f64)
                })?;
            }
            if self
                .device_app_detail(target, &detail.app.package)
                .await?
                .fingerprint
                != detail.fingerprint
            {
                return Err(AppError::new(
                    "APP_CHANGED",
                    "导出期间应用已变化，未发布结果",
                ));
            }
            if self.cancelled(id) {
                return Err(AppError::new("CANCELLED", "已取消 APK 导出"));
            }
            let artifacts = self.data.join("artifacts");
            std::fs::create_dir_all(&artifacts)?;
            let extension = if detail.paths.len() == 1 {
                "apk"
            } else {
                "apks"
            };
            let destination = artifacts.join(format!("{id}.{extension}"));
            let temporary = stage.join("result.part");
            crate::capacity::budget(&[(stage.clone(), total + 1024 * 1024, "APK 集合打包")])?;
            if detail.paths.len() == 1 {
                std::fs::copy(stage.join("0.apk"), &temporary)?;
            } else {
                let mut zip = zip::ZipWriter::new(std::fs::File::create(&temporary)?);
                for (index, remote) in detail.paths.iter().enumerate() {
                    let name = if remote.ends_with("/base.apk") {
                        "base.apk".into()
                    } else {
                        format!("split-{index}.apk")
                    };
                    zip.start_file(
                        name,
                        zip::write::SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Stored),
                    )
                    .map_err(|e| AppError::new("APK_EXPORT_ZIP", e.to_string()))?;
                    std::io::copy(
                        &mut std::fs::File::open(stage.join(format!("{index}.apk")))?,
                        &mut zip,
                    )?;
                }
                zip.finish()
                    .map_err(|e| AppError::new("APK_EXPORT_ZIP", e.to_string()))?
                    .sync_all()?;
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(&temporary)?
                .sync_all()?;
            self.phase(id, "publishing", "发布已校验的 APK 导出")?;
            self.update(id, |t| {
                t.cancellable = false;
                t.operation
                    .args
                    .insert("exportExtension".into(), extension.into());
                t.operation.args.insert(
                    "exportTitle".into(),
                    format!("{}.{extension}", detail.app.package),
                );
            })?;
            let expected = digest_file(&temporary)?;
            self.update(id, |t| {
                t.operation.args.insert("exportSha256".into(), expected);
            })?;
            std::fs::rename(temporary, &destination)?;
            self.store.register_artifact(
                id,
                &destination,
                if extension == "apk" { "apk" } else { "apk_set" },
                &format!("{}.{extension}", detail.app.package),
                id,
            )?;
            self.update(id, |t| t.artifact = destination.to_string_lossy().into())?;
            Ok("APK 已校验并加入待保存文件".into())
        }
        .await;
        let _ = std::fs::remove_dir_all(stage);
        result
    }
    pub fn recover_device_app_exports(&self) -> Result<()> {
        for task in self.store.review_tasks()? {
            if task.operation.kind != "device_app_export"
                || !task.artifact.is_empty()
                || uuid::Uuid::parse_str(&task.id).is_err()
            {
                continue;
            }
            let ext = task.operation.optional("exportExtension");
            if !matches!(ext, "apk" | "apks") {
                continue;
            }
            let path = self
                .data
                .join("artifacts")
                .join(format!("{}.{ext}", task.id));
            let expected = task.operation.optional("exportSha256");
            if expected.len() != 64
                || crate::phone_files::regular_local(&path, false).is_err()
                || digest_file(&path)? != expected
            {
                continue;
            }
            self.store.register_artifact(
                &task.id,
                &path,
                if ext == "apk" { "apk" } else { "apk_set" },
                task.operation.optional("exportTitle"),
                &task.id,
            )?;
            self.update(&task.id, |t| {
                t.artifact = path.to_string_lossy().into();
                t.state = "succeeded".into();
                t.phase = "complete".into();
                t.progress = Some(100.0);
                t.cancellable = false;
                t.message = "已恢复本机完整 APK 导出，无需连接设备或重新导出".into();
            })?;
        }
        Ok(())
    }
}
type Service<'a> = tauri::State<'a, std::sync::Arc<App>>;
#[tauri::command]
pub async fn device_app_list(app: Service<'_>, target: Target) -> Result<DeviceAppList> {
    app.list_device_apps(&target).await
}
#[tauri::command]
pub async fn device_app_detail(
    app: Service<'_>,
    target: Target,
    package: String,
) -> Result<DeviceAppDetail> {
    app.device_app_detail(&target, &package).await
}
#[tauri::command]
pub async fn device_app_prepare(
    app: Service<'_>,
    target: Target,
    package: String,
    action: String,
) -> Result<BTreeMap<String, String>> {
    let detail = app.device_app_detail(&target, &package).await?;
    check_action(&action, &detail)?;
    Ok(BTreeMap::from([
        ("package".into(), package),
        ("userId".into(), detail.user_id.to_string()),
        ("appFingerprint".into(), detail.fingerprint),
        ("version".into(), detail.version_name),
        ("system".into(), detail.app.system.to_string()),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exported_apk_recovers_only_the_committed_hash_and_stays_clearable() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(temp.path().into()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let mut task = crate::regression_tests::task(&id, "needs_review");
        task.operation.kind = "device_app_export".into();
        task.operation.args = BTreeMap::from([
            ("exportExtension".into(), "apks".into()),
            ("exportTitle".into(), "dev.example.app.apks".into()),
            ("exportSha256".into(), digest(b"verified APKs")),
        ]);
        app.store.save_task(&task).unwrap();
        let path = app.data.join("artifacts").join(format!("{id}.apks"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        app.recover_device_app_exports().unwrap();
        assert!(app.store.artifacts().unwrap().is_empty());
        std::fs::write(&path, b"verified APKs").unwrap();
        app.recover_device_app_exports().unwrap();
        app.recover_device_app_exports().unwrap();
        assert_eq!(app.store.artifacts().unwrap().len(), 1);
        assert_eq!(app.store.task(&id).unwrap().state, "succeeded");
        app.discard_artifacts(&[id]).unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn package_listing_and_paths_reject_untrusted_output() {
        let list=parse_packages("package:dev.example.app versionCode:23 uid:10123\npackage:com.android.settings uid:1000").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].version_code, "23");
        for value in ["package:bad;reboot", "truncated output", "package:../app"] {
            assert!(parse_packages(value).is_err());
        }
        assert_eq!(
            apk_paths("package:/data/app/a/base.apk\npackage:/data/app/a/split.apk")
                .unwrap()
                .len(),
            2
        );
        for path in [
            "package:/data/app/../secret.apk",
            "package:relative.apk",
            "Error: denied",
        ] {
            assert!(apk_paths(path).is_err());
        }
    }
    #[test]
    fn system_app_mutations_are_blocked_and_metadata_fields_are_precise() {
        let detail = DeviceAppDetail {
            app: DeviceApp {
                package: "android".into(),
                version_code: "1".into(),
                uid: "1000".into(),
                system: true,
                disabled: false,
            },
            user_id: 0,
            version_name: String::new(),
            min_sdk: String::new(),
            target_sdk: String::new(),
            paths: vec![],
            fingerprint: String::new(),
        };
        for action in ["stop", "clear", "uninstall", "injected"] {
            assert!(check_action(action, &detail).is_err());
        }
        for action in ["launch", "export"] {
            check_action(action, &detail).unwrap();
        }
        assert_eq!(
            field(
                "versionCode=123 minSdk=23 targetSdk=29\n versionName=1.2",
                "versionCode"
            ),
            "123"
        );
    }
}
