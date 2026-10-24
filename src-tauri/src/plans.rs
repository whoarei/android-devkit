use crate::{
    android,
    app::{
        assert_avd_stopped, assert_package_unused, validate_app_package, validate_avd_values, App,
    },
    domain::*,
};
use std::{collections::BTreeMap, path::Path};

pub fn fingerprint(app: &App, settings: &Settings) -> Result<String> {
    let mut bytes = serde_json::to_vec(settings)?;
    if let Ok(meta) = std::fs::metadata(&settings.java_path) {
        bytes.extend(meta.len().to_le_bytes());
        if let Ok(modified) = meta.modified().and_then(|time| {
            time.duration_since(std::time::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        }) {
            bytes.extend(modified.as_nanos().to_le_bytes());
        }
    }
    for relative in [
        "cmdline-tools/latest/source.properties",
        "platform-tools/source.properties",
        "emulator/source.properties",
    ] {
        if let Ok(raw) = std::fs::read(Path::new(&settings.sdk_root).join(relative)) {
            bytes.extend(raw);
        }
    }
    bytes.extend(serde_json::to_vec(&*app.catalog.lock().unwrap())?);
    Ok(digest(&bytes))
}
fn license(pkg: &SdkPackage, list: &mut Vec<License>) {
    if !pkg.license.is_empty() && !list.iter().any(|l| l.id == pkg.license_id) {
        list.push(License {
            id: pkg.license_id.clone(),
            text: pkg.license.clone(),
            hash: digest(pkg.license.as_bytes()),
        });
    }
}
pub fn build(app: &App, mut op: Operation) -> Result<Plan> {
    let settings = app.store.settings();
    let avds = android::avds(&settings);
    let local = android::installed(&settings);
    let catalog: Vec<_> = app
        .catalog
        .lock()
        .unwrap()
        .iter()
        .filter(|package| crate::repository::package_enabled(package, &settings))
        .cloned()
        .collect();
    let mut effects = Vec::new();
    let mut warnings = Vec::new();
    let mut licenses = Vec::new();
    let title = match op.kind.as_str() {
        "java_prepare" => {
            let package = crate::java::package()?;
            licenses.push(crate::java::license());
            effects.push(format!("准备应用私有 Temurin JRE 21 · {:.1} MiB", package.size as f64 / 1048576.0));
            effects.push(format!("Java 安装位置：{}", crate::java::private_path(&app.data).parent().unwrap().parent().unwrap().display()));
            effects.push(format!("Java 下载地址：{}", package.url));
            effects.push("校验运行时并验证 SDK 工具；仅更新本应用 Java 路径".into());
            "准备 Java 运行时"
        }
        "adb_start" => "启动设备服务",
        "adb_restart" => {
            warnings.push("将中断本机其他工具及设备连接，旧会话全部失效。".into());
            "重启共享 ADB"
        }
        "sdk_install" | "sdk_remove" => {
            let ids: Vec<String> = op.arg("packages")?.lines().map(String::from).collect();
            if ids.iter().any(|id| !valid_package(id)) {
                return Err(AppError::new("INVALID_PACKAGE", "无效组件 ID"));
            }
            let mut expanded = ids.clone();
            let mut expected = BTreeMap::new();
            if op.kind == "sdk_install" {
                expanded = crate::dependencies::resolve(&ids, &local, &catalog)?;
            }
            for id in &expanded {
                let changes_platform_tools = id == "platform-tools"
                    && (op.kind == "sdk_remove"
                        || catalog.iter().find(|p| p.id == *id).is_some_and(|desired| {
                            !local.iter().any(|p| p.id == *id && p.version == desired.version)
                        }));
                if changes_platform_tools {
                    app.terminals.check_sdk_unused(&settings.sdk_root)?;
                }
                effects.push(format!(
                    "{} {id}",
                    if op.kind == "sdk_remove" {
                        "卸载"
                    } else {
                        "安装 / 更新"
                    }
                ));
                if op.kind == "sdk_remove" {
                    assert_package_unused(id, &settings, &avds)?;
                } else if let Some(p) = catalog.iter().find(|p| p.id == *id) {
                    license(p, &mut licenses);
                    expected.insert(id.clone(), p.version.clone());
                    if let Some(old) = local.iter().find(|p| p.id == *id) {
                        if old.version != p.version {
                            warnings.push(format!(
                                "{id} 将从 {} 替换为 {}，不保证可回滚",
                                old.version, p.version
                            ));
                            assert_package_unused(id, &settings, &avds)?;
                        }
                    }
                    if p.size > 0 {
                        effects.push(format!(
                            "版本 {} · 下载约 {:.1} MiB",
                            p.version,
                            p.size as f64 / 1048576.0
                        ));
                    }
                } else if !local.iter().any(|p| p.id == *id) {
                    return Err(AppError::new(
                        "PACKAGE_UNKNOWN",
                        format!("{id} 不在已验证目录中，请刷新仓库"),
                    ));
                }
            }
            op.args.insert("packages".into(), expanded.join("\n"));
            op.args
                .insert("expectedVersions".into(), serde_json::to_string(&expected)?);
            if op.kind == "sdk_remove" {
                warnings.push("无法检测全部外部项目依赖；已知 AVD 引用会阻止卸载。".into());
            }
            "应用 SDK 变更"
        }
        "bootstrap" => {
            let purpose = op.optional("purpose").to_owned();
            if !matches!(purpose.as_str(), "usb" | "emulator") {
                return Err(AppError::new("INVALID_PURPOSE", "请选择真机或模拟器环境"));
            }
            effects.push(format!(
                "准备 {purpose} 最小环境；SDK {}",
                settings.sdk_root
            ));
            let mut expected = BTreeMap::new();
            let mut missing = Vec::new();
            for id in if purpose == "usb" {
                vec!["platform-tools"]
            } else {
                vec!["platform-tools", "cmdline-tools;22.0", "emulator"]
            } {
                let pkg = catalog.iter().find(|p| p.id == id).ok_or_else(|| {
                    AppError::new("LICENSE_UNAVAILABLE", "先刷新官方仓库，取得组件和许可")
                })?;
                license(pkg, &mut licenses);
                expected.insert(id.to_string(), pkg.version.clone());
                if !local.iter().any(|p| p.id == id) && id != "cmdline-tools;22.0" {
                    missing.push(id);
                }
                effects.push(format!(
                    "{id} · 缺失时下载 {:.1} MiB",
                    pkg.size as f64 / 1048576.0
                ));
            }
            if purpose != "usb" && (op.optional("prepareJava") == "true" || !Path::new(&settings.java_path).is_file()) {
                let text = include_str!("../resources/java-license.txt");
                licenses.push(License {
                    id: "Temurin-JRE-21-GPLv2-Classpath".into(),
                    text: text.into(),
                    hash: digest(text.as_bytes()),
                });
                effects.push("按需准备应用私有 Temurin JRE 21，不修改系统 Java".into());
            }
            op.args.insert("packages".into(), missing.join("\n"));
            op.args
                .insert("expectedVersions".into(), serde_json::to_string(&expected)?);
            "准备最小环境"
        }
        "avd_create" => {
            let name = op.arg("name")?;
            if !valid_name(name) {
                return Err(AppError::new(
                    "INVALID_NAME",
                    "内部名称限 1～64 个英文字母、数字、下划线或连字符",
                ));
            }
            if avds.iter().any(|a| a.name.eq_ignore_ascii_case(name)) {
                return Err(AppError::new("AVD_EXISTS", "同名 AVD 已存在，不会覆盖"));
            }
            if !matches!(op.optional("profile"), "pixel_2" | "Nexus 9") {
                return Err(AppError::new(
                    "UNSUPPORTED_PROFILE",
                    "仅支持已验证的手机 Pixel 2 与平板 Nexus 9 profile",
                ));
            }
            validate_avd_values(&op)?;
            for key in ["memory", "gpu"] {
                op.arg(key)?;
            }
            if op.optional("storageMiB").is_empty() && op.optional("storage").is_empty() {
                return Err(AppError::new("INVALID_STORAGE", "请填写数据分区目标容量"));
            }
            if op.optional("displayName").contains(['\n', '\r'])
                || op.optional("displayName").len() > 200
            {
                return Err(AppError::new(
                    "INVALID_DISPLAY_NAME",
                    "显示名称不能含换行且不得超过 200 字节",
                ));
            }
            let image = op.arg("image")?.to_owned();
            if !valid_package(&image)
                || !image.starts_with("system-images;")
                || !image.ends_with(";x86_64")
            {
                return Err(AppError::new(
                    "INVALID_IMAGE",
                    "Windows x64 请选择受支持的 x86_64 系统镜像",
                ));
            }
            if image.contains("google_apis_playstore") && op.optional("profile") == "Nexus 9" {
                return Err(AppError::new(
                    "PROFILE_IMAGE_MISMATCH",
                    "该平板 profile 不支持 Google Play 镜像，请选择 AOSP 或 Google APIs",
                ));
            }
            effects.push(format!("新建 {}；镜像 {image}", name));
            effects.extend(crate::avd_settings::summary(&op)?);
            // An installed shared image stays at its current revision during AVD creation.
            let installed_image = local.iter().any(|p| p.id == image);
            let available: Vec<_> = catalog
                .iter()
                .filter(|p| !installed_image || p.id != image)
                .cloned()
                .collect();
            let mut prerequisites =
                crate::dependencies::resolve(std::slice::from_ref(&image), &local, &available)?;
            prerequisites.retain(|id| !installed_image || *id != image);
            let mut expected = BTreeMap::new();
            for id in &prerequisites {
                let p = catalog
                    .iter()
                    .find(|p| p.id == *id)
                    .ok_or_else(|| AppError::new("IMAGE_UNKNOWN", "请刷新官方镜像及依赖目录"))?;
                license(p, &mut licenses);
                expected.insert(id.clone(), p.version.clone());
                effects.push(format!(
                    "先准备 {id} · {}，约 {:.1} MiB",
                    p.version,
                    p.size as f64 / 1048576.0
                ));
                if let Some(old) = local.iter().find(|p| p.id == *id) {
                    if old.version != p.version {
                        assert_package_unused(id, &settings, &avds)?;
                        warnings.push(format!(
                            "{id} 将从 {} 替换为 {}，不保证可回滚",
                            old.version, p.version
                        ));
                    }
                }
            }
            op.args
                .insert("prerequisitePackages".into(), prerequisites.join("\n"));
            op.args
                .insert("expectedVersions".into(), serde_json::to_string(&expected)?);
            "创建模拟器"
        }
        "avd_start" | "avd_cold" | "avd_wait" | "avd_stop" | "avd_force_stop" | "avd_wipe" | "avd_delete"
        | "avd_edit" | "avd_focus" => {
            let a = avds
                .iter()
                .find(|a| a.id == op.optional("avdId"))
                .ok_or_else(|| AppError::new("AVD_NOT_FOUND", "AVD 已不存在"))?;
            effects.push(format!("{} · {}", a.display_name, a.data_path));
            op.args.insert("dataPath".into(), a.data_path.clone());
            op.args
                .insert("configurationHash".into(), a.fingerprint.clone());
            op.args
                .insert("registryHash".into(), digest_file(Path::new(&a.ini_path))?);
            if matches!(op.kind.as_str(), "avd_delete" | "avd_wipe") {
                android::safe_avd_path(a, &settings)?;
                assert_avd_stopped(a)?;
                warnings.push(
                    if op.kind == "avd_wipe" {
                        "清除应用、用户数据、快照和独立 SD 卡；配置和共享镜像保留。"
                    } else {
                        "永久删除配置与用户数据；共享镜像保留。"
                    }
                    .into(),
                );
                if op.kind == "avd_wipe" && a.config.get("devkit.sdcard.managed").is_some_and(|value| value == "no") {
                    warnings.push("外部扩展存储镜像保留，不会被格式化或删除。".into());
                }
            }
            if op.kind == "avd_delete" {
                let usage = crate::capacity::usage(Path::new(&a.data_path));
                effects.push(format!("预计可释放 {:.1} MiB{}；不包括共享镜像或外部存储", usage.bytes as f64 / 1048576.0, if usage.complete { "" } else { "（已统计部分，估算）" }));
            }
            if op.kind == "avd_edit" {
                if op.arg("fingerprint")? != a.fingerprint {
                    return Err(AppError::new("EXTERNAL_CHANGE", "配置在编辑期间已变化，请重新打开编辑面板"));
                }
                validate_avd_values(&op)?;
                if !op.optional("storageMiB").is_empty() {
                    warnings.push("只修改数据分区目标容量，不自动清除用户数据；已有分区是否扩容取决于模拟器。需要重建分区时请另行使用清除数据。".into());
                }
                if op.optional("sdCardMode") == "size" {
                    warnings.push("将创建并挂载新的扩展存储镜像，原扩展存储镜像保留，不复制其中数据。".into());
                }
                effects.extend(crate::avd_settings::resolved_changes(&op, &a.config)?.into_iter()
                    .filter(|(key,value)| a.config.get(key) != Some(value)).map(|(key,value)| format!("{key} = {value}")));
            }
            match op.kind.as_str() {
                "avd_start" => "启动模拟器",
                "avd_cold" => "冷启动模拟器",
                "avd_wait" => "继续等待系统就绪",
                "avd_stop" => "停止模拟器",
                "avd_force_stop" => "强制停止模拟器",
                "avd_wipe" => "清除模拟器数据",
                "avd_delete" => "删除模拟器",
                "avd_edit" => "保存模拟器配置",
                _ => "打开模拟器窗口",
            }
        }
        "wireless_pair" => {
            let address = android::normalize_endpoint(op.arg("address")?)?;
            op.args.insert("address".into(), address);
            let code = op.arg("code")?;
            if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
                return Err(AppError::new("INVALID_PAIR_CODE", "配对码应为 6 位数字"));
            }
            effects.push(format!(
                "配对 {}；配对成功后仍需验证连接",
                op.arg("address")?
            ));
            "无线设备配对"
        }
        "wireless_connect" | "wireless_disconnect" | "network_connect" | "network_disconnect" => {
            let address = android::normalize_endpoint(op.arg("address")?)?;
            op.args.insert("address".into(), address);
            effects.push(op.arg("address")?.into());
            match op.kind.as_str() {
                "network_connect" => {
                    effects.push("直接连接设备已开启的 ADB TCP/IP 端口，并验证设备实际可用".into());
                    "连接网络 ADB 设备"
                }
                "network_disconnect" => "断开网络 ADB 连接",
                "wireless_connect" => "连接无线设备",
                _ => "断开无线连接",
            }
        }
        "apk_install" | "apk_install_wait" | "app_reinstall" => {
            let apk = app
                .apks
                .lock()
                .unwrap()
                .get(op.arg("apkId")?)
                .cloned()
                .ok_or_else(|| AppError::new("APK_NOT_FOUND", "请重新选择 APK"))?;
            if op.kind == "apk_install_wait" {
                let avd = avds.iter().find(|avd| avd.id == op.optional("avdId"))
                    .ok_or_else(|| AppError::new("AVD_NOT_FOUND", "请选择等待启动的模拟器"))?;
                if avds.iter().filter(|other| other.name == avd.name).count() != 1 {
                    return Err(AppError::new("AMBIGUOUS_AVD", "模拟器名称不唯一，无法安全等待"));
                }
                op.args.insert("dataPath".into(), avd.data_path.clone());
                op.args.insert("configurationHash".into(), avd.fingerprint.clone());
                op.args.insert("registryHash".into(), digest_file(Path::new(&avd.ini_path))?);
                op.args.insert("expectedServerEpoch".into(), app.epoch.load(std::sync::atomic::Ordering::SeqCst).to_string());
                effects.push(format!("等待 {} 系统就绪后安装；最多 {} 秒，可取消，不自动启动或切换连接", avd.display_name, settings.boot_timeout_secs));
            } else if op.target.is_none() {
                return Err(AppError::new("TARGET_REQUIRED", "必须明确选择设备"));
            }
            effects.push(format!(
                "{} · {:.1} MiB · {} {}",
                apk.file_name,
                apk.size as f64 / 1048576.0,
                apk.package,
                apk.version
            ));
            op.args.insert("package".into(), apk.package.clone());
            op.args.insert("version".into(), apk.version.clone());
            if op.kind == "app_reinstall" {
                validate_app_package(&apk.package)?;
                warnings.push("将卸载原应用及全部应用数据，再安装所选 APK。".into());
            } else {
                warnings.push("覆盖安装并保留数据；冲突时不会自动卸载或降级。".into());
            }
            if op.kind == "apk_install_wait" { "就绪后安装 APK" } else { "安装 APK" }
        }
        "apk_set_install" => {
            if op.target.is_none() { return Err(AppError::new("TARGET_REQUIRED", "请选择安装设备")); }
            let set = crate::phone_apk::load(app, op.arg("setId")?)?;
            op.args.insert("setHash".into(), digest_file(&crate::phone_apk::set_dir(app, &set.id)?.join("set.json"))?);
            op.args.insert("package".into(), set.package.clone());
            op.args.insert("version".into(), set.version.clone());
            effects.push(format!("{} · versionCode={} · {} 个 APK · {:.1} MiB", set.package,set.version_code,set.members.len(),set.members.iter().map(|m|m.bytes).sum::<u64>() as f64/1048576.0));
            effects.extend(set.members.iter().map(|m|format!("{} · {} · SHA-256 {}",m.file_name,if m.split.is_empty(){"base"}else{&m.split},m.sha256)));
            warnings.push("所有 APK 在一次安装会话中提交；覆盖安装保留数据，冲突时不自动卸载或降级。安装后核对 versionCode 与 split 集合。".into());
            "安装分包 APK"
        }
        "screen_record" | "screen_recover" => {
            crate::phone_record::plan(app, &mut op, &mut effects, &mut warnings)?;
            if op.kind == "screen_recover" { "取回已有录屏" } else { "录制屏幕" }
        }
        "file_upload" | "file_download" | "file_cleanup" | "file_restore" => {
            crate::phone_files::plan(app, &mut op, &mut effects, &mut warnings)?;
            match op.kind.as_str() { "file_upload" => "上传文件", "file_download" => "下载文件", "file_restore"=>"恢复被替换的旧目标", _ => "清理传输暂存" }
        }
        "log_record" => {
            if op.target.is_none() {
                return Err(AppError::new("TARGET_REQUIRED", "请选择采集设备"));
            }
            let f: LogFilter = serde_json::from_str(op.arg("filter")?)?;
            if !f.package.is_empty() {
                validate_app_package(&f.package)?;
            }
            let quota = op.arg("quotaMiB")?.parse::<u64>().unwrap_or(0);
            let seconds = op.arg("seconds")?.parse::<u64>().unwrap_or(0);
            if !(8..=4096).contains(&quota) || !(1..=86400).contains(&seconds) {
                return Err(AppError::new(
                    "INVALID_RECORDING_LIMIT",
                    "总配额限 8～4096 MiB，时限 1～86400 秒",
                ));
            }
            if app
                .store
                .active_tasks()?
                .iter()
                .any(|t| t.operation.kind == "log_record" && t.operation.target == op.target)
            {
                return Err(AppError::new("RECORDING_ACTIVE", "该设备已有采集任务"));
            }
            effects.push(format!(
                "流式采集，最多 {quota} MiB / {seconds} 秒，每段最多 100 MiB；关闭视图继续采集"
            ));
            effects.push(format!(
                "过滤应用 {} · 级别 {} · tag {}",
                f.package, f.level, f.tag
            ));
            warnings.push(
                "原始日志仅存本地，可能含应用数据。导出时默认脱敏并预览；停止采集会封口文件。"
                    .into(),
            );
            "开始文件采集"
        }
        "app_launch" | "capture" => {
            if op.target.is_none() {
                return Err(AppError::new("TARGET_REQUIRED", "必须明确选择设备"));
            }
            if op.kind == "app_launch" {
                validate_app_package(op.arg("package")?)?;
                "启动应用"
            } else {
                "截图"
            }
        }
        _ => return Err(AppError::new("UNSUPPORTED_OPERATION", "未支持的操作")),
    }
    .to_owned();
    if matches!(op.kind.as_str(), "sdk_install" | "bootstrap" | "avd_create") {
        let ids = if op.kind == "avd_create" {
            op.optional("prerequisitePackages")
        } else {
            op.optional("packages")
        };
        let mut downloads: Vec<_> = catalog
            .iter()
            .filter(|p| ids.lines().any(|id| id == p.id))
            .cloned()
            .collect();
        if op.kind == "bootstrap" {
            let bootstrap_id = if op.optional("purpose") == "usb" {
                "platform-tools"
            } else {
                "cmdline-tools;22.0"
            };
            if !downloads.iter().any(|p| p.id == bootstrap_id) {
                if let Some(p) = catalog.iter().find(|p| p.id == bootstrap_id) {
                    downloads.push(p.clone());
                }
            }
        }
        for package in &downloads {
            if !local
                .iter()
                .any(|p| p.id == package.id && p.version == package.version)
            {
                crate::sdk_source::verify_package(&settings, package)?;
            }
        }
        op.args.insert(
            "downloadPackages".into(),
            serde_json::to_string(&downloads)?,
        );
        op.args.insert(
            "sdkDownloadSource".into(),
            crate::sdk_source::name(&settings),
        );
        op.args
            .insert("sdkDownloadUrl".into(), crate::sdk_source::base(&settings)?);
        op.args.insert(
            "sdkOfficialVerification".into(),
            settings.sdk_download.verify_official.to_string(),
        );
        effects.push(format!(
            "下载源：{} · {}",
            crate::sdk_source::name(&settings),
            crate::sdk_source::base(&settings)?
        ));
        effects.push(
            if settings.sdk_download.verify_official {
                "官方校验已开启：仅安装与官方记录匹配的组件"
            } else {
                "官方校验已关闭：仍按所选下载源的校验值验证完整性"
            }
            .into(),
        );
    }
    if let Some(t) = &op.target {
        effects.push(format!(
            "目标 {} · transport {} · 会话 {}",
            t.serial, t.transport_id, t.server_epoch
        ));
    }
    if op.kind == "avd_create" {
        effects.push(format!(
            "硬件加速检查：{}",
            if op.optional("accelerationCheck").is_empty() {
                "待检查：启动前检测"
            } else {
                op.optional("accelerationCheck")
            }
        ));
    }
    effects.extend(crate::capacity::operation_budget(
        &app.data, &settings, &op,
    )?);
    let environment_fingerprint = fingerprint(app, &settings)?;
    op.args.insert(
        "environmentFingerprint".into(),
        environment_fingerprint.clone(),
    );
    let mut audited_operation = op.clone();
    audited_operation.args.remove("code");
    let plan = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        hash: digest(&serde_json::to_vec(&audited_operation)?),
        operation: op,
        title,
        effects,
        warnings,
        licenses,
        created_at: now(),
        settings_hash: environment_fingerprint,
        confirmation_token: uuid::Uuid::new_v4().to_string(),
    };
    app.plans
        .lock()
        .unwrap()
        .insert(plan.id.clone(), plan.clone());
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_connection_plans_require_an_explicit_valid_endpoint_without_pairing() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().join("state")).unwrap();
        app.store
            .save_settings(&Settings {
                sdk_root: dir.path().join("sdk").to_string_lossy().into(),
                avd_roots: vec![dir.path().join("avd").to_string_lossy().into()],
                ..Default::default()
            })
            .unwrap();
        for kind in ["network_connect", "network_disconnect"] {
            let operation = |address: &str| Operation {
                kind: kind.into(),
                args: BTreeMap::from([("address".into(), address.into())]),
                target: None,
            };
            let plan = app.prepare_plan(operation("[2001:0db8::1]:5555")).unwrap();
            assert_eq!(plan.operation.optional("address"), "[2001:db8::1]:5555");
            assert!(!plan.operation.args.contains_key("code"));
            assert!(plan
                .effects
                .iter()
                .any(|effect| effect.contains("[2001:db8::1]:5555")));
            for address in [
                "",
                "192.168.1.2",
                "192.168.1.2:0",
                "192.168.1.2:65536",
                "--help",
            ] {
                assert!(
                    app.prepare_plan(operation(address)).is_err(),
                    "{kind}: {address}"
                );
            }
        }
        assert!(app.store.tasks().unwrap().is_empty());
    }
    #[test]
    fn consent_and_catalog_revision_are_enforced_before_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().join("state")).unwrap();
        let settings = Settings {
            sdk_root: dir.path().join("sdk").to_string_lossy().into(),
            avd_roots: vec![dir.path().join("avd").to_string_lossy().into()],
            ..Default::default()
        };
        app.store.save_settings(&settings).unwrap();
        app.catalog.lock().unwrap().push(SdkPackage {
            id: "platform-tools".into(),
            version: "36.0.0".into(),
            license_id: "fixture-license".into(),
            license: "Fixture terms".into(),
            ..Default::default()
        });
        let p = app
            .prepare_plan(Operation {
                kind: "sdk_install".into(),
                args: BTreeMap::from([("packages".into(), "platform-tools".into())]),
                target: None,
            })
            .unwrap();
        assert_eq!(
            app.submit(&p.id, &p.hash, "r", &p.confirmation_token, &[])
                .unwrap_err()
                .code,
            "LICENSE_REQUIRED"
        );
        assert_eq!(
            app.submit(&p.id, &p.hash, "r", "wrong", &[p.licenses[0].hash.clone()])
                .unwrap_err()
                .code,
            "PLAN_CHANGED"
        );
        app.catalog.lock().unwrap()[0].version = "37.0.0".into();
        assert_eq!(
            app.submit(
                &p.id,
                &p.hash,
                "r",
                &p.confirmation_token,
                &[p.licenses[0].hash.clone()]
            )
            .unwrap_err()
            .code,
            "PLAN_CHANGED"
        );
        assert!(app.store.tasks().unwrap().is_empty());
        assert!(!Path::new(&settings.sdk_root).exists());
    }
    #[test]
    fn dead_or_reused_pid_lock_does_not_make_stopped_avd_permanently_busy() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("hardware-qemu.ini.lock");
        std::fs::create_dir(&lock).unwrap();
        std::fs::write(lock.join("pid"), "4294967294").unwrap();
        assert!(!crate::app::avd_locked(dir.path()));
        std::fs::write(lock.join("pid"), std::process::id().to_string()).unwrap();
        assert!(!crate::app::avd_locked(dir.path()));
        assert!(lock.join("pid").exists());
    }
}
