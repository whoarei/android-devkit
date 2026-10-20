use crate::{
    android,
    app::{
        assert_avd_stopped, assert_package_unused, validate_app_package, validate_avd_values, App,
    },
    bootstrap,
    domain::*,
    process::{self, Spec},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{atomic::Ordering, Arc},
};
use tauri::Emitter;

impl App {
    async fn bound_adb(
        &self,
        settings: &Settings,
        target: &Target,
        args: Vec<String>,
        timeout: u64,
    ) -> Result<process::Output> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        let raw = android::host_request("host:devices-l")?;
        let current = android::parse_devices(&String::from_utf8_lossy(&raw), epoch);
        checked_target(target, &current, epoch)?;
        android::adb(settings, Some(target), args, timeout).await
    }
    pub fn submit(
        self: &Arc<Self>,
        plan_id: &str,
        plan_hash: &str,
        request_id: &str,
        token: &str,
        accepted: &[String],
    ) -> Result<Task> {
        if let Some(t) = self.store.request(request_id) {
            return Ok(t);
        }
        if self.draining.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "APP_CLOSING",
                "应用正在等待退出，不接收新写任务",
            ));
        }
        let plan = self
            .plans
            .lock()
            .unwrap()
            .get(plan_id)
            .cloned()
            .ok_or_else(|| AppError::new("PLAN_EXPIRED", "计划已过期，请重新确认"))?;
        if now().saturating_sub(plan.created_at) > 600
            || plan.hash != plan_hash
            || plan.confirmation_token != token
            || plan.settings_hash != crate::plans::fingerprint(self, &self.store.settings())?
        {
            return Err(AppError::new(
                "PLAN_CHANGED",
                "环境或计划已经变化，请重新确认",
            ));
        }
        if plan.licenses.iter().any(|l| !accepted.contains(&l.hash)) {
            return Err(AppError::new(
                "LICENSE_REQUIRED",
                "需逐项查看并接受所需许可",
            ));
        }
        let mut op = plan.operation;
        let code = op.args.remove("code").unwrap_or_default();
        let task = Task {
            id: uuid::Uuid::new_v4().to_string(),
            request_id: request_id.into(),
            title: plan.title,
            operation: op,
            state: "queued".into(),
            phase: "queued".into(),
            progress: None,
            message: "等待资源".into(),
            log: Vec::new(),
            created_at: now(),
            updated_at: now(),
            cancel_requested: false,
            cancellable: true,
            artifact: String::new(),
            license_prompt: String::new(),
            confirmed_licenses: accepted.into(),
        };
        self.store.save_task(&task)?;
        self.plans.lock().unwrap().remove(plan_id);
        self.store.audit(
            &task.id,
            &format!(
                "confirmed:{}:{}:high-impact={}",
                task.operation.kind,
                plan.hash,
                task.operation.destructive()
            ),
        )?;
        let app = self.clone();
        let id = task.id.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = app.clone().perform(&id, code).await {
                app.report_recovery_failure(&app.data.join("state/devkit.db"), &error, false);
                if let Some(handle) = app.handle.lock().unwrap().clone() {
                    let notice = AppError::new(
                        "TASK_PERSISTENCE_FAILED",
                        "任务状态无法保存，请检查磁盘空间和应用数据目录；结果需重新检查",
                    )
                    .detail(format!("task={id}: {error}"));
                    let _ = handle.emit_to("main", "task-error", notice);
                }
                app.notify();
            }
        });
        self.notify();
        Ok(task)
    }
    pub fn cancel_task(&self, id: &str) -> Result<()> {
        let t = self.store.task(id)?;
        if !t.active() {
            return Ok(());
        }
        self.update(id, |t| {
            if !t.active() {
                return;
            }
            t.cancel_requested = true;
            t.message = if t.cancellable {
                "正在取消"
            } else {
                "当前写入步骤结束后取消"
            }
            .into();
        })?;
        if !t.license_prompt.is_empty() {
            if let Some(tx) = self.answers.lock().unwrap().get(id) {
                let _ = tx.try_send("n\n".into());
            }
        }
        Ok(())
    }
    async fn perform(self: Arc<Self>, id: &str, secret: String) -> Result<()> {
        let task = self.store.task(id)?;
        let op = task.operation.clone();
        let settings = self.store.settings();
        let key = if op.kind == "log_record" {
            format!(
                "record:{}",
                op.target
                    .as_ref()
                    .map(crate::observe::key)
                    .unwrap_or_default()
            )
        } else if let Some(t) = &op.target {
            format!("device:{}:{}", t.server_epoch, t.transport_id)
        } else if op.kind.starts_with("avd_") {
            format!("avd:{}", op.optional("avdId"))
        } else if op.kind.starts_with("sdk_") || op.kind == "bootstrap" {
            format!("sdk:{}", settings.sdk_root)
        } else {
            "adb-global".into()
        };
        let lock = self.lock(&key);
        let _guard = lock.lock().await;
        let _slot = if op.target.is_some() && op.kind != "log_record" {
            Some(self.device_slots.acquire().await.unwrap())
        } else {
            None
        };
        let _sdk_write = if matches!(
            op.kind.as_str(),
            "sdk_install" | "sdk_remove" | "bootstrap" | "avd_create"
        ) {
            Some(self.sdk_gate.write().await)
        } else {
            None
        };
        let _sdk_read = if op.kind.starts_with("avd_") && op.kind != "avd_create" {
            Some(self.sdk_gate.read().await)
        } else {
            None
        };
        if op.optional("environmentFingerprint")
            != crate::plans::fingerprint(&self, &self.store.settings()).unwrap_or_default()
        {
            self.update(id, |t| {
                t.state = "failed".into();
                t.message = "执行前工具环境或仓库已变化，请重新确认计划".into();
            })?;
            return Ok(());
        }
        if self.cancelled(id) {
            self.update(id, |t| {
                t.state = "cancelled".into();
                t.message = "已在执行前取消".into();
            })?;
            return Ok(());
        }
        self.update(id, |t| {
            t.state = "running".into();
            t.phase = "preparing".into();
            t.message = "检查当前状态".into();
            t.cancellable = false;
        })?;
        // The operation dispatcher contains large SDK/AVD futures. Keep it out
        // of the task runner's future to avoid overflowing Windows debug stacks.
        let result = Box::pin(self.execute(id, &op, &settings, &secret)).await;
        let persisted = match result {
            Ok(message) => self.update(id, |t| {
                t.state = if t.cancel_requested && message == "已取消" {
                    "cancelled"
                } else {
                    "succeeded"
                }
                .into();
                t.phase = "complete".into();
                t.message = message;
                t.progress = Some(100.0);
                t.cancellable = false;
                t.license_prompt.clear();
            }),
            Err(error) => self.update(id, |t| {
                t.state = if (op.kind == "log_record" && !t.artifact.is_empty())
                    || matches!(
                        error.code.as_str(),
                        "TOOL_TIMEOUT" | "RESULT_UNKNOWN" | "PARTIAL_COMPLETE"
                    ) {
                    "needs_review"
                } else if error.code == "CANCELLED" {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                t.message = format!("{}：{}", error.code, error.message);
                if !error.detail.is_empty() {
                    t.log.push(error.detail.chars().take(12000).collect());
                }
                t.cancellable = false;
                t.license_prompt.clear();
            }),
        };
        if op.kind == "log_record" {
            let _ = self.recover_recordings();
            if let Some(target) = &op.target {
                if !self
                    .log_views
                    .lock()
                    .unwrap()
                    .contains(&crate::observe::key(target))
                {
                    self.log_stop(target);
                }
            }
            self.prune_logs();
        }
        let _ = self.store.audit(
            id,
            &format!(
                "result:{}",
                self.store.task(id).map(|t| t.state).unwrap_or_default()
            ),
        );
        self.answers.lock().unwrap().remove(id);
        self.notify();
        persisted
    }
    async fn execute(
        self: &Arc<Self>,
        id: &str,
        op: &Operation,
        settings: &Settings,
        secret: &str,
    ) -> Result<String> {
        if let Some(target) = &op.target {
            let current = android::devices(settings, self.epoch.load(Ordering::SeqCst)).await;
            checked_target(target, &current, self.epoch.load(Ordering::SeqCst))?;
        }
        let device_resource = if matches!(op.kind.as_str(), "avd_stop" | "avd_force_stop") {
            let avd = find_avd(settings, op)?;
            let devices = android::devices(settings, self.epoch.load(Ordering::SeqCst)).await;
            if devices
                .iter()
                .any(|d| d.avd_name == avd.name && self.recording_target(&Target::from(d)))
            {
                return Err(AppError::new(
                    "RECORDING_ACTIVE",
                    "请先在任务中心停止并封口该设备的文件采集",
                ));
            }
            devices
                .iter()
                .find(|d| d.avd_name == avd.name)
                .map(|d| self.lock(&format!("device:{}:{}", d.server_epoch, d.transport_id)))
        } else {
            None
        };
        let _device_guard = if let Some(lock) = &device_resource {
            Some(lock.lock().await)
        } else {
            None
        };
        match op.kind.as_str() {
            "adb_start" => {
                let adb = android::compatible_adb(settings).await?;
                process::run(Spec::new(adb, vec!["start-server".into()]))
                    .await?
                    .success()?;
                Ok("设备服务已启动；正在刷新设备".into())
            }
            "adb_restart" => {
                if self.store.active_tasks()?.iter().any(|t| t.id != id) {
                    return Err(AppError::new(
                        "TASKS_ACTIVE",
                        "先完成其他任务再重启共享服务",
                    ));
                }
                let path = Path::new(&settings.sdk_root).join("platform-tools/adb.exe");
                if android::host_request("host:version").is_ok() {
                    let _ = android::host_request("host:kill");
                }
                self.epoch.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                process::run(Spec::new(path, vec!["start-server".into()]))
                    .await?
                    .success()?;
                Ok("共享服务已重新启动，旧目标全部失效".into())
            }
            "sdk_install" | "sdk_remove" => {
                self.sdk_apply(id, settings, op.arg("packages")?, op.kind == "sdk_remove")
                    .await?;
                Ok("组件计划已执行并复查".into())
            }
            "bootstrap" => {
                self.bootstrap_environment(id, settings, op.optional("purpose"))
                    .await?;
                Ok("所需工具已准备，可进入设备工作台".into())
            }
            "avd_create" => {
                self.create_avd(id, settings, op).await?;
                Ok("模拟器已创建；可从设备列表启动".into())
            }
            "avd_start" | "avd_cold" => {
                let a = find_avd(settings, op)?;
                self.start_avd(id, settings, &a, op.kind == "avd_cold", false)
                    .await?;
                Ok("模拟器系统已就绪".into())
            }
            "avd_stop" => {
                let a = find_avd(settings, op)?;
                self.stop_avd(id, settings, &a).await?;
                Ok("模拟器已停止".into())
            }
            "avd_force_stop" => {
                let a = find_avd(settings, op)?;
                let saved = self
                    .data
                    .join(format!("process-{}.json", digest(a.id.as_bytes())));
                let record: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&saved).map_err(|_| {
                        AppError::new("UNKNOWN_PROCESS", "不能证明外部模拟器进程归属，请手动停止")
                    })?)?;
                crate::platform::terminate_verified(
                    record["pid"].as_u64().unwrap_or(0) as u32,
                    record["created"].as_u64().unwrap_or(0),
                    record["path"].as_str().unwrap_or(""),
                )?;
                for _ in 0..40 {
                    if !crate::app::avd_locked(Path::new(&a.data_path)) {
                        return Ok("已强制停止确认归属的进程，并复查文件占用已释放".into());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err(AppError::new(
                    "RESULT_UNKNOWN",
                    "已请求强制停止，但仍检测到文件占用，请检查实例状态",
                ))
            }
            "avd_focus" => {
                let a = find_avd(settings, op)?;
                let saved = self
                    .data
                    .join(format!("process-{}.json", digest(a.id.as_bytes())));
                let record: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(saved).map_err(|_| {
                        AppError::new("NO_WINDOW", "外部实例未记录窗口；请从系统任务栏打开")
                    })?)?;
                let pid = record["pid"].as_u64().unwrap_or(0) as u32;
                let actual = crate::platform::process_identity(pid)?;
                if actual.0 != record["created"].as_u64().unwrap_or(0)
                    || !actual
                        .1
                        .eq_ignore_ascii_case(record["path"].as_str().unwrap_or(""))
                {
                    return Err(AppError::new(
                        "PROCESS_IDENTITY_CHANGED",
                        "进程身份已变化，请刷新设备列表",
                    ));
                }
                crate::platform::focus_process(pid)?;
                Ok("已请求打开模拟器窗口".into())
            }
            "avd_edit" => {
                let a = find_avd(settings, op)?;
                assert_avd_stopped(&a)?;
                if a.fingerprint != op.arg("fingerprint")? {
                    return Err(AppError::new("EXTERNAL_CHANGE", "配置在编辑期间已变化"));
                }
                validate_avd_values(op)?;
                let mut changes = crate::avd_settings::resolved_changes(op, &a.config)?;
                let new_sd =
                    crate::avd_settings::create_sd_card(settings, Path::new(&a.data_path), op, id)
                        .await?;
                if let Some(path) = &new_sd {
                    changes.insert("sdcard.path".into(), path.to_string_lossy().into());
                }
                let result = android::write_properties_preserving(
                    &Path::new(&a.data_path).join("config.ini"),
                    &changes,
                    &a.fingerprint,
                );
                if let Err(error) = result {
                    return Err(if let Some(path) = new_sd {
                        AppError::new(
                            "PARTIAL_COMPLETE",
                            "扩展存储镜像已创建，但配置未保存；请检查原配置后重试",
                        )
                        .detail(format!("{}\n{error}", path.display()))
                    } else {
                        error
                    });
                }
                Ok("配置已保存，未知字段保留".into())
            }
            "avd_delete" => {
                let a = find_avd(settings, op)?;
                assert_avd_stopped(&a)?;
                let path = android::safe_avd_path(&a, settings)?;
                reject_reparse_tree(&path)?;
                let mut spec = process::java_tool(
                    settings,
                    "avdmanager",
                    vec!["delete".into(), "avd".into(), "-n".into(), a.name.clone()],
                )?;
                spec.env.insert(
                    "ANDROID_AVD_HOME".into(),
                    Path::new(&a.ini_path)
                        .parent()
                        .unwrap()
                        .to_string_lossy()
                        .into(),
                );
                process::run(spec).await?.success()?;
                if path.exists() || Path::new(&a.ini_path).exists() {
                    return Err(AppError::new(
                        "PARTIAL_COMPLETE",
                        "删除后仍有残留，请查看数据目录",
                    ));
                }
                Ok("AVD 已删除，共享镜像保留".into())
            }
            "avd_wipe" => {
                let a = find_avd(settings, op)?;
                assert_avd_stopped(&a)?;
                let path = android::safe_avd_path(&a, settings)?;
                reject_reparse_tree(&path)?;
                crate::avd_settings::reset_managed_sd(settings, &a).await?;
                for name in [
                    "sdcard.img",
                    "sdcard.img.qcow2",
                    "snapshots.img",
                    "snapshots",
                ] {
                    if name.starts_with("sdcard")
                        && a.config
                            .get("devkit.sdcard.managed")
                            .is_some_and(|value| value == "no")
                    {
                        continue;
                    }
                    let p = path.join(name);
                    if p.is_dir() {
                        std::fs::remove_dir_all(&p)?;
                    } else if p.is_file() {
                        std::fs::remove_file(&p)?;
                    }
                }
                self.start_avd(id, settings, &a, true, true)
                    .await
                    .map_err(|e| {
                        AppError::new("PARTIAL_COMPLETE", "数据清理已开始，但重置未完全确认")
                            .detail(e.to_string())
                    })?;
                self.stop_avd(id, settings, &a).await?;
                Ok("用户数据、快照与应用管理的扩展存储已重置；外部镜像保留，设备已停止".into())
            }
            "wireless_pair" => {
                let path = android::compatible_adb(settings).await?;
                let mut s = Spec::new(path, vec!["pair".into(), op.arg("address")?.into()]);
                s.stdin = Some(format!("{secret}\n"));
                s.timeout = 60;
                let out = process::run(s).await?;
                if out.code != 0 {
                    return Err(AppError::new(
                        "PAIR_FAILED",
                        "配对失败，请重新获取配对码，并核对配对地址、端口和网络",
                    )
                    .detail(out.text() + &out.stderr));
                }
                if !out.text().to_lowercase().contains("successfully paired") {
                    return Err(AppError::new("PAIR_NOT_CONFIRMED", "配对结果未确认"));
                }
                Ok("配对成功，尚未确认连接。请输入手机无线调试页的连接端口继续。".into())
            }
            "wireless_connect" | "network_connect" => {
                android::adb(
                    settings,
                    None,
                    vec!["connect".into(), op.arg("address")?.into()],
                    20,
                )
                .await?
                .success()?;
                for _ in 0..6 {
                    let d = android::devices(settings, self.epoch.load(Ordering::SeqCst)).await;
                    if d.iter()
                        .any(|d| d.serial == op.optional("address") && d.state == "ready")
                    {
                        return Ok(if op.kind == "network_connect" {
                            "网络 ADB 连接已验证，设备可用"
                        } else {
                            "无线连接已验证，设备可用"
                        }
                        .into());
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                Err(AppError::new(
                    "CONNECT_NOT_READY",
                    if op.kind == "network_connect" {
                        "未验证可用连接；请检查 IP、端口、设备 TCP/IP 调试是否开启及调试授权"
                    } else {
                        "未验证可用连接；检查连接端口、网络隔离或重新配对"
                    },
                ))
            }
            "wireless_disconnect" | "network_disconnect" => {
                android::adb(
                    settings,
                    None,
                    vec!["disconnect".into(), op.arg("address")?.into()],
                    10,
                )
                .await?
                .success()?;
                Ok("已断开该网络连接；设备端 ADB 监听与调试授权仍保留".into())
            }
            "apk_install" | "app_reinstall" => {
                let apk = self
                    .apks
                    .lock()
                    .unwrap()
                    .get(op.arg("apkId")?)
                    .cloned()
                    .ok_or_else(|| AppError::new("APK_NOT_FOUND", "APK 暂存记录已过期"))?;
                if digest_file(Path::new(&apk.path))? != apk.sha256 {
                    return Err(AppError::new("APK_CHANGED", "APK 暂存内容发生变化"));
                }
                let target = op.target.as_ref().unwrap();
                let devices = android::devices(settings, self.epoch.load(Ordering::SeqCst)).await;
                let device = checked_target(target, &devices, self.epoch.load(Ordering::SeqCst))?;
                if let (Ok(required), Ok(actual)) =
                    (apk.min_sdk.parse::<u32>(), device.api.parse::<u32>())
                {
                    if actual < required {
                        return Err(AppError::new(
                            "SDK_MISMATCH",
                            format!("APK 至少需要 API {required}，目标为 API {actual}"),
                        ));
                    }
                }
                if !apk.abis.is_empty()
                    && !device.abi.is_empty()
                    && !apk
                        .abis
                        .iter()
                        .any(|a| device.abi.split(',').any(|d| d == a))
                {
                    return Err(AppError::new(
                        "ABI_MISMATCH",
                        "APK native ABI 与所选设备不兼容",
                    ));
                }
                if op.kind == "app_reinstall" {
                    validate_app_package(&apk.package)?;
                    self.bound_adb(
                        settings,
                        target,
                        vec!["uninstall".into(), apk.package.clone()],
                        60,
                    )
                    .await?
                    .success()?;
                }
                self.phase(id, "installing", "正在安装到冻结目标；断线不会转投其他设备")?;
                let output = self
                    .bound_adb(
                        settings,
                        target,
                        vec!["install".into(), "-r".into(), apk.path.clone()],
                        180,
                    )
                    .await?;
                let text = format!("{}\n{}", output.text(), output.stderr);
                if output.code != 0 || !text.lines().any(|l| l.trim() == "Success") {
                    return Err(install_error(&text));
                }
                Ok(format!("已安装 {}；运行情况需另行观察", apk.package))
            }
            "app_launch" => {
                let package = op.arg("package")?;
                validate_app_package(package)?;
                let target = op.target.as_ref().unwrap();
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
                            "-a".into(),
                            "android.intent.action.MAIN".into(),
                            "-c".into(),
                            "android.intent.category.LAUNCHER".into(),
                            package.into(),
                        ],
                        10,
                    )
                    .await?
                    .success()?;
                let component = out
                    .text()
                    .lines()
                    .find(|l| {
                        l.starts_with(&format!("{package}/"))
                            && l.chars()
                                .all(|c| c.is_ascii_alphanumeric() || "._/$".contains(c))
                    })
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        AppError::new(
                            "NO_LAUNCH_ACTIVITY",
                            "没有可靠识别到启动 Activity，安装结果保持不变",
                        )
                    })?;
                self.bound_adb(
                    settings,
                    target,
                    vec![
                        "shell".into(),
                        "am".into(),
                        "start".into(),
                        "-W".into(),
                        "-n".into(),
                        component,
                    ],
                    30,
                )
                .await?
                .success()?;
                Ok("系统已接受应用启动请求".into())
            }
            "log_record" => self.record_logs(id, op).await,
            "capture" => {
                self.phase(id, "capturing", "正在读取 PNG 二进制数据")?;
                let output = self
                    .bound_adb(
                        settings,
                        op.target.as_ref().unwrap(),
                        vec!["exec-out".into(), "screencap".into(), "-p".into()],
                        30,
                    )
                    .await?
                    .success()?;
                image::load_from_memory_with_format(&output.stdout, image::ImageFormat::Png)
                    .map_err(|_| AppError::new("CAPTURE_INVALID", "截图输出不是有效 PNG"))?;
                let path = self.data.join("artifacts").join(format!("{id}.png"));
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(&path, &output.stdout)?;
                self.store
                    .register_artifact(id, &path, "screenshot", "设备截图", id)?;
                self.update(id, |t| t.artifact = path.to_string_lossy().into())?;
                Ok("截图已生成，可预览、保存或复制".into())
            }
            _ => Err(AppError::new("UNSUPPORTED_OPERATION", "未知任务类型")),
        }
    }
    async fn sdk_apply(
        self: &Arc<Self>,
        id: &str,
        settings: &Settings,
        ids: &str,
        remove: bool,
    ) -> Result<()> {
        bootstrap::assert_writable(Path::new(&settings.sdk_root))?;
        let packages: Vec<_> = ids.lines().filter(|p| !p.is_empty()).collect();
        let expected: BTreeMap<String, String> = self
            .store
            .task(id)?
            .operation
            .args
            .get("expectedVersions")
            .map(|v| serde_json::from_str(v))
            .transpose()?
            .unwrap_or_default();
        for (index, pkg) in packages.iter().enumerate() {
            if self.cancelled(id) {
                return Err(AppError::new(
                    if index == 0 {
                        "CANCELLED"
                    } else {
                        "PARTIAL_COMPLETE"
                    },
                    "已在组件边界取消；已完成项保留",
                ));
            }
            if remove {
                assert_package_unused(pkg, settings, &android::avds(settings))?;
            }
            let local = android::installed(settings);
            let desired = expected.get(*pkg).cloned();
            if !remove
                && local
                    .iter()
                    .any(|p| p.id == *pkg && desired.as_ref().is_none_or(|v| *v == p.version))
            {
                self.line(id, format!("已存在，保留：{pkg}"))?;
                continue;
            }
            if !remove && local.iter().any(|p| p.id == *pkg) {
                assert_package_unused(pkg, settings, &android::avds(settings))?;
            }
            self.phase(
                id,
                "installing",
                &format!(
                    "{} {pkg}（{}/{})",
                    if remove { "卸载" } else { "安装" },
                    index + 1,
                    packages.len()
                ),
            )?;
            let mut args = vec![
                format!("--sdk_root={}", settings.sdk_root),
                if remove { "--uninstall" } else { "--install" }.into(),
                pkg.to_string(),
            ];
            if !settings.proxy.is_empty() {
                let u = reqwest::Url::parse(&settings.proxy)
                    .map_err(|_| AppError::new("INVALID_PROXY", "无效代理"))?;
                args.extend([
                    format!(
                        "--proxy={}",
                        if u.scheme().starts_with("socks") {
                            "socks"
                        } else {
                            "http"
                        }
                    ),
                    format!("--proxy_host={}", u.host_str().unwrap_or("")),
                    format!("--proxy_port={}", u.port().unwrap_or(8080)),
                ]);
            }
            let mut spec = process::java_tool(settings, "sdkmanager", args)?;
            let package = self
                .catalog
                .lock()
                .unwrap()
                .iter()
                .find(|package| package.id == *pkg)
                .cloned();
            crate::repository::configure_sdkmanager(
                &self.data,
                settings,
                package.as_ref(),
                &mut spec,
            )?;
            let (tx, rx) = tokio::sync::mpsc::channel(8);
            self.answers.lock().unwrap().insert(id.into(), tx);
            let app = self.clone();
            let task_id = id.to_string();
            let license_output = std::sync::Mutex::new(String::new());
            let callback = Arc::new(move |line: String| {
                let mut full = license_output.lock().unwrap();
                if full.len() + line.len() < 256 * 1024 {
                    full.push_str(&line);
                    full.push('\n');
                }
                if line.contains("(y/N)") || line.contains("(y/n)") {
                    let _ = app.update(&task_id, |t| {
                        t.license_prompt = full.clone();
                        t.message = "官方工具要求许可确认，请在任务中心查看并选择".into();
                    });
                }
                drop(full);
                // The external write is already in flight; finish that step.
                // Critical updates before the next step still propagate errors.
                let _ = app.line(&task_id, line);
            });
            process::streaming(spec, callback, Arc::new(tokio::sync::Mutex::new(rx)))
                .await?
                .success()?;
            let after = android::installed(settings);
            let actual = after.iter().find(|p| p.id == *pkg);
            let exists = actual.is_some();
            if exists == remove {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    format!("{pkg} 的工具结果与安装清单不一致"),
                ));
            }
            if !remove
                && desired
                    .as_ref()
                    .is_some_and(|v| actual.is_none_or(|p| p.version != *v))
            {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    format!("{pkg} 实际版本与确认计划不同，请刷新清单复核"),
                ));
            }
            self.line(id, format!("已复查：{pkg}"))?;
        }
        Ok(())
    }
    async fn bootstrap_environment(
        self: &Arc<Self>,
        id: &str,
        settings: &Settings,
        purpose: &str,
    ) -> Result<()> {
        let mut settings = settings.clone();
        bootstrap::assert_writable(Path::new(&settings.sdk_root))?;
        let manifest: serde_json::Value = serde_json::from_str(
            include_str!("../resources/bootstrap.json").trim_start_matches('\u{feff}'),
        )?;
        let tool = if purpose == "usb" {
            "platform-tools"
        } else {
            "cmdline-tools;22.0"
        };
        let pkg = self
            .catalog
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == tool)
            .cloned()
            .ok_or_else(|| AppError::new("CATALOG_UNAVAILABLE", "请刷新官方仓库目录"))?;
        let dest = Path::new(&settings.sdk_root).join(if purpose == "usb" {
            "platform-tools"
        } else {
            "cmdline-tools/latest"
        });
        if !dest.exists() {
            let mut pkg = pkg;
            if purpose != "usb" {
                let fixed = &manifest["commandTools"];
                pkg.url = fixed["url"].as_str().unwrap().into();
                pkg.checksum = fixed["checksum"].as_str().unwrap().into();
                pkg.checksum_type = "sha1".into();
            }
            self.install_archive(
                id,
                &settings,
                &pkg,
                &dest,
                if purpose == "usb" {
                    "platform-tools"
                } else {
                    "cmdline-tools"
                },
            )
            .await?;
        }
        if purpose != "usb" {
            if !Path::new(&settings.java_path).is_file() {
                let j = &manifest["java"];
                let pkg = SdkPackage {
                    id: "private-java-21".into(),
                    url: j["url"].as_str().unwrap().into(),
                    checksum: j["checksum"].as_str().unwrap().into(),
                    checksum_type: "sha256".into(),
                    size: j["size"].as_u64().unwrap(),
                    ..Default::default()
                };
                let dest = self.data.join("runtime/java21");
                if !dest.exists() {
                    self.install_archive(id, &settings, &pkg, &dest, "").await?;
                }
                settings.java_path = dest.join("bin/java.exe").to_string_lossy().into();
                self.store.save_settings(&settings)?;
            }
            let task = self.store.task(id)?;
            self.sdk_apply(id, &settings, task.operation.optional("packages"), false)
                .await?;
        }
        Ok(())
    }
    async fn install_archive(
        self: &Arc<Self>,
        id: &str,
        settings: &Settings,
        pkg: &SdkPackage,
        dest: &Path,
        inner: &str,
    ) -> Result<()> {
        self.phase(id, "downloading", &format!("下载 {}", pkg.id))?;
        let app = self.clone();
        let task = id.to_string();
        let archive = bootstrap::download(
            settings,
            pkg,
            &self.data.join("cache/downloads"),
            move |n, total| {
                let _ = app.update(&task, |t| {
                    t.progress = if total > 0 {
                        Some(n as f64 / total as f64 * 100.0)
                    } else {
                        None
                    };
                    t.message = format!(
                        "已下载 {:.1} / {:.1} MiB",
                        n as f64 / 1048576.0,
                        total as f64 / 1048576.0
                    );
                });
            },
        )
        .await?;
        if self.cancelled(id) {
            return Err(AppError::new("CANCELLED", "下载完成后取消，未提交安装"));
        }
        self.phase(id, "verifying", "校验完成，安全解压并检查结构")?;
        let parent = dest.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let stage = parent.join(format!(".devkit-stage-{id}"));
        std::fs::create_dir(&stage)?;
        bootstrap::extract_zip(&archive, &stage)?;
        let source = if inner.is_empty() {
            std::fs::read_dir(&stage)?
                .flatten()
                .find(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .ok_or_else(|| AppError::new("ARCHIVE_STRUCTURE", "运行时目录结构无效"))?
        } else {
            stage.join(inner)
        };
        if !source.is_dir() {
            return Err(AppError::new("ARCHIVE_STRUCTURE", "官方包结构不符合预期"));
        }
        if dest.exists() {
            return Err(AppError::new("DEST_EXISTS", "目标目录已出现，不会覆盖"));
        }
        std::fs::rename(source, dest)?;
        let _ = std::fs::remove_dir(&stage);
        Ok(())
    }
    async fn create_avd(
        self: &Arc<Self>,
        id: &str,
        settings: &Settings,
        op: &Operation,
    ) -> Result<()> {
        let image = op.arg("image")?;
        let prerequisites = op.optional("prerequisitePackages");
        if !prerequisites.is_empty() {
            self.sdk_apply(id, settings, prerequisites, false).await?;
        }
        if !android::installed(settings).iter().any(|p| p.id == image) {
            return Err(AppError::new(
                "IMAGE_MISSING",
                "镜像状态已变化，请重新生成创建计划",
            ));
        }
        if self.cancelled(id) {
            return Err(AppError::new("CANCELLED", "创建前已取消；镜像保留"));
        }
        let name = op.arg("name")?;
        if android::avds(settings)
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case(name))
        {
            return Err(AppError::new("AVD_EXISTS", "同名 AVD 已存在"));
        }
        let root = PathBuf::from(
            settings
                .avd_roots
                .last()
                .ok_or_else(|| AppError::new("AVD_HOME_MISSING", "请设置 AVD 根目录"))?,
        );
        bootstrap::assert_writable(&root)?;
        let stage = root.join(format!(".devkit-create-{id}"));
        std::fs::create_dir(&stage)?;
        let home = stage.join("index");
        std::fs::create_dir(&home)?;
        let data = stage.join(format!("{name}.avd"));
        let mut spec = process::java_tool(
            settings,
            "avdmanager",
            vec![
                "create".into(),
                "avd".into(),
                "-n".into(),
                name.into(),
                "-k".into(),
                image.into(),
                "-d".into(),
                op.optional("profile").into(),
                "-p".into(),
                data.to_string_lossy().into(),
            ],
        )?;
        spec.env
            .insert("ANDROID_AVD_HOME".into(), home.to_string_lossy().into());
        spec.stdin = Some("no\n".into());
        self.phase(id, "creating", "在独立暂存目录创建，不覆盖既有 AVD")?;
        let result = process::run(spec).await?;
        if result.code != 0 {
            return Err(AppError::new(
                "AVD_CREATE_FAILED",
                "官方创建工具返回失败；保留诊断暂存目录",
            )
            .detail(result.text() + &result.stderr));
        }
        let config = data.join("config.ini");
        let hash = digest(&std::fs::read(&config)?);
        let current = android::read_properties(&config);
        let mut changes = crate::avd_settings::resolved_changes(op, &current)?;
        if let Some(path) = crate::avd_settings::create_sd_card(settings, &data, op, id).await? {
            // Bind the final location, because the staging directory is moved at commit.
            changes.insert(
                "sdcard.path".into(),
                root.join(format!("{name}.avd"))
                    .join(path.file_name().unwrap())
                    .to_string_lossy()
                    .into(),
            );
        }
        android::write_properties_preserving(&config, &changes, &hash)?;
        std::fs::write(data.join(".devkit-owned"), id)?;
        let final_data = root.join(format!("{name}.avd"));
        let final_ini = root.join(format!("{name}.ini"));
        if final_data.exists() || final_ini.exists() {
            return Err(AppError::new("AVD_EXISTS", "提交前发现同名目录，未覆盖"));
        }
        let journal = self.data.join(format!("avd-commit-{id}.json"));
        crate::recovery::write_commit_journal(
            &journal,
            &serde_json::json!({"data":final_data,"ini":final_ini,"stage":stage,"taskId":id}),
        )?;
        std::fs::rename(&data, &final_data)?;
        crate::recovery::create_avd_index(
            &final_ini,
            &final_data,
            image.split(';').nth(1).unwrap_or(""),
        )?;
        let _ = std::fs::remove_file(journal);
        let _ = std::fs::remove_dir_all(&stage);
        Ok(())
    }
    async fn start_avd(
        self: &Arc<Self>,
        id: &str,
        settings: &Settings,
        avd: &Avd,
        cold: bool,
        wipe: bool,
    ) -> Result<()> {
        assert_avd_stopped(avd)?;
        if !avd.error.is_empty() {
            return Err(AppError::new("AVD_DEPENDENCY", &avd.error));
        }
        let devices = android::devices(settings, self.epoch.load(Ordering::SeqCst)).await;
        if devices.iter().any(|d| d.avd_name == avd.name) {
            return Err(AppError::new("AVD_RUNNING", "该模拟器已存在运行实例"));
        }
        let emulator = Path::new(&settings.sdk_root).join("emulator/emulator.exe");
        let acceleration = process::run(Spec::new(&emulator, vec!["-accel-check".into()]))
            .await?
            .success()?;
        self.line(id, acceleration.text())?;
        let adb = android::compatible_adb(settings).await?;
        process::run(Spec::new(adb, vec!["start-server".into()]))
            .await?
            .success()?;
        let log_path = self.data.join(format!("emulator-{id}.log"));
        let log = std::fs::File::create(&log_path)?;
        let mut command = std::process::Command::new(&emulator);
        command
            .args(["-avd", &avd.name])
            .env("ANDROID_HOME", &settings.sdk_root)
            .env(
                "ANDROID_AVD_HOME",
                Path::new(&avd.ini_path).parent().unwrap(),
            )
            .env_remove("ANDROID_SERIAL")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        let config = android::read_properties(&Path::new(&avd.data_path).join("config.ini"));
        command.args(crate::avd_settings::launch_args(&config)?);
        if cold
            || config
                .get("fastboot.forceColdBoot")
                .is_some_and(|value| value == "yes")
        {
            command.arg("-no-snapshot-load");
        }
        if wipe {
            command.arg("-wipe-data");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x00000200 | 0x08000000);
        }
        let child = command.spawn()?;
        let (created, path) = crate::platform::process_identity(child.id())?;
        std::fs::write(
            self.data
                .join(format!("process-{}.json", digest(avd.id.as_bytes()))),
            serde_json::to_vec(
                &serde_json::json!({"pid":child.id(),"created":created,"path":path}),
            )?,
        )?;
        drop(child);
        self.phase(id, "waiting_adb", "模拟器进程已创建，等待 ADB 与系统启动")?;
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(settings.boot_timeout_secs);
        loop {
            if self.cancelled(id) {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    "已停止等待，模拟器可能继续运行；可从列表明确停止",
                ));
            }
            let devices = android::devices(settings, self.epoch.load(Ordering::SeqCst)).await;
            if let Some(device) = devices.iter().find(|d| d.avd_name == avd.name) {
                if device.state == "ready" {
                    let out = android::adb(
                        settings,
                        Some(&Target::from(device)),
                        vec!["shell".into(), "pm".into(), "path".into(), "android".into()],
                        10,
                    )
                    .await?;
                    if out.code == 0 && out.text().contains("package:") {
                        if let Ok(raw) = std::fs::read_to_string(
                            Path::new(&avd.data_path).join("hardware-qemu.ini.lock/pid"),
                        ) {
                            if let Ok(pid) = raw.trim().parse::<u32>() {
                                if let Ok((created, path)) = crate::platform::process_identity(pid)
                                {
                                    if Path::new(&path)
                                        .starts_with(Path::new(&settings.sdk_root).join("emulator"))
                                    {
                                        std::fs::write(
                                            self.data.join(format!(
                                                "process-{}.json",
                                                digest(avd.id.as_bytes())
                                            )),
                                            serde_json::to_vec(
                                                &serde_json::json!({"pid":pid,"created":created,"path":path}),
                                            )?,
                                        )?;
                                    }
                                }
                            }
                        }
                        return Ok(());
                    }
                }
                self.phase(id, "waiting_boot", "ADB 已出现，等待系统和包管理服务就绪")?;
            }
            if tokio::time::Instant::now() > deadline {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    format!(
                        "启动超过 {} 秒；模拟器未被自动终止，可继续观察或停止。日志 {}",
                        settings.boot_timeout_secs,
                        log_path.display()
                    ),
                ));
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }
    async fn stop_avd(&self, id: &str, settings: &Settings, avd: &Avd) -> Result<()> {
        if android::avds(settings)
            .iter()
            .filter(|a| a.name == avd.name)
            .count()
            != 1
        {
            return Err(AppError::new(
                "AMBIGUOUS_AVD_SESSION",
                "多个 AVD 同名，无法证明运行会话归属，未发送停止命令",
            ));
        }
        let devices = android::devices(settings, self.epoch.load(Ordering::SeqCst)).await;
        let device = devices
            .iter()
            .find(|d| d.avd_name == avd.name)
            .ok_or_else(|| {
                AppError::new(
                    "AVD_SESSION_UNKNOWN",
                    "无法可靠关联运行会话，未发送停止命令",
                )
            })?;
        let target = Target::from(device);
        self.phase(id, "stopping", "已请求正常退出，等待数据保存")?;
        self.bound_adb(settings, &target, vec!["emu".into(), "kill".into()], 15)
            .await?
            .success()?;
        for _ in 0..60 {
            if !crate::app::avd_locked(Path::new(&avd.data_path)) {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        Err(AppError::new(
            "RESULT_UNKNOWN",
            "正常停止超时，可单独确认强制停止",
        ))
    }
}
fn find_avd(settings: &Settings, op: &Operation) -> Result<Avd> {
    let avd = android::avds(settings)
        .into_iter()
        .find(|a| a.id == op.optional("avdId"))
        .ok_or_else(|| AppError::new("AVD_NOT_FOUND", "AVD 不存在"))?;
    if avd.data_path != op.optional("dataPath")
        || avd.fingerprint != op.optional("configurationHash")
        || digest_file(Path::new(&avd.ini_path))? != op.optional("registryHash")
    {
        return Err(AppError::new(
            "EXTERNAL_CHANGE",
            "AVD 路径或配置在确认后已变化，请重新确认操作范围",
        ));
    }
    Ok(avd)
}
fn reject_reparse_tree(path: &Path) -> Result<()> {
    for e in std::fs::read_dir(path)? {
        let e = e?;
        let metadata = std::fs::symlink_metadata(e.path())?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(AppError::new(
                    "UNSAFE_PATH",
                    "数据目录中包含重解析点，拒绝递归清理",
                ));
            }
        }
        if metadata.is_dir() {
            reject_reparse_tree(&e.path())?;
        }
    }
    Ok(())
}
pub fn install_error(text: &str) -> AppError {
    let code = for_install_code(text);
    AppError::new(
        code,
        match code {
            "SIGNATURE_CONFLICT" => "签名冲突；原应用和数据已保留",
            "VERSION_DOWNGRADE" => "版本降级被拒绝；请选择新版本",
            "ABI_MISMATCH" => "APK 架构与设备不匹配",
            "SDK_MISMATCH" => "设备系统版本低于 APK 要求",
            "STORAGE_FULL" => "设备存储不足",
            "MISSING_SPLIT" => "需要分包；P0 仅支持完整单 APK",
            _ => "安装未确认成功，请检查原设备与错误详情",
        },
    )
    .detail(text)
}
fn for_install_code(text: &str) -> &'static str {
    for (needle, code) in [
        ("UPDATE_INCOMPATIBLE", "SIGNATURE_CONFLICT"),
        ("VERSION_DOWNGRADE", "VERSION_DOWNGRADE"),
        ("NO_MATCHING_ABIS", "ABI_MISMATCH"),
        ("OLDER_SDK", "SDK_MISMATCH"),
        ("INSUFFICIENT_STORAGE", "STORAGE_FULL"),
        ("MISSING_SPLIT", "MISSING_SPLIT"),
    ] {
        if text.contains(needle) {
            return code;
        }
    }
    "INSTALL_FAILED"
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specific_install_failures_preserve_data() {
        assert_eq!(
            install_error("Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE]").code,
            "SIGNATURE_CONFLICT"
        );
        assert_eq!(
            install_error("INSTALL_FAILED_VERSION_DOWNGRADE").code,
            "VERSION_DOWNGRADE"
        );
    }
}
