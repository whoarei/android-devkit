use crate::{
    app::App,
    domain::*,
    process::{self, Spec},
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JavaCheck {
    pub path: String,
    pub source: String,
    pub status: String,
    pub version: String,
    pub vendor: String,
    pub architecture: String,
    pub message: String,
    pub tools_ready: bool,
    pub compatibility_checked: bool,
}
impl JavaCheck {
    pub fn ready(&self) -> bool {
        self.status == "ready"
    }
    pub fn require(&self) -> Result<()> {
        if self.ready() {
            Ok(())
        } else {
            Err(
                AppError::new("JAVA_CONFIGURATION_REQUIRED", self.message.clone())
                    .detail(self.status.clone()),
            )
        }
    }
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaEnvironment {
    pub current: JavaCheck,
    pub candidates: Vec<JavaCheck>,
    pub private_path: String,
    pub download: SdkPackage,
    pub license: License,
    pub busy: bool,
    pub preparation: Option<Task>,
}
pub fn private_path(data: &Path) -> PathBuf {
    data.join("runtime/java21/bin/java.exe")
}
pub fn package() -> Result<SdkPackage> {
    let manifest: serde_json::Value = serde_json::from_str(
        include_str!("../resources/bootstrap.json").trim_start_matches('\u{feff}'),
    )?;
    let j = &manifest["java"];
    Ok(SdkPackage {
        id: "private-java-21".into(),
        name: "Temurin JRE 21".into(),
        version: j["version"].as_str().unwrap_or_default().into(),
        url: j["url"].as_str().unwrap_or_default().into(),
        checksum: j["checksum"].as_str().unwrap_or_default().into(),
        checksum_type: "sha256".into(),
        size: j["size"].as_u64().unwrap_or_default(),
        ..Default::default()
    })
}
pub fn license() -> License {
    let text = include_str!("../resources/java-license.txt");
    License {
        id: "Temurin-JRE-21-GPLv2-Classpath".into(),
        text: text.into(),
        hash: digest(text.as_bytes()),
    }
}
pub fn needed(kind: &str) -> bool {
    matches!(kind, "sdk_install" | "sdk_remove" | "avd_create")
}
pub fn related(kind: &str) -> bool {
    needed(kind) || matches!(kind, "java_prepare" | "bootstrap")
}
pub fn normalize(path: &str) -> PathBuf {
    let p = PathBuf::from(path.trim().trim_matches('"'));
    if !path.trim().is_empty() && p.is_dir() {
        p.join("bin/java.exe")
    } else {
        p
    }
}
fn property<'a>(text: &'a str, name: &str) -> &'a str {
    text.lines()
        .filter_map(|line| line.trim().split_once('='))
        .find_map(|(key, value)| (key.trim() == name).then_some(value.trim()))
        .unwrap_or_default()
}
fn major(version: &str) -> u32 {
    version
        .strip_prefix("1.")
        .unwrap_or(version)
        .split(['.', '-', '+'])
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
pub async fn check(settings: &Settings, path: &str, source: &str) -> JavaCheck {
    let p = normalize(path);
    let mut result = JavaCheck {
        path: p.to_string_lossy().into(),
        source: source.into(),
        status: "missing".into(),
        message: "未找到可用的 Java，请配置运行环境".into(),
        ..Default::default()
    };
    if path.trim().is_empty() {
        result.path.clear();
        return result;
    }
    if !p.is_absolute() || !p.is_file() {
        result.message = "Java 路径不存在，请重新选择 java.exe".into();
        return result;
    }
    let mut spec = Spec::new(
        &p,
        vec!["-XshowSettings:properties".into(), "-version".into()],
    );
    spec.timeout = 5;
    let output = match process::run(spec).await {
        Ok(v) if v.code == 0 => format!("{}\n{}", v.text(), v.stderr),
        Ok(_) => {
            result.status = "failed".into();
            result.message = "Java 启动失败，请选择其他运行时".into();
            return result;
        }
        Err(e) => {
            result.status = if e.code == "TOOL_TIMEOUT" {
                "timeout"
            } else {
                "failed"
            }
            .into();
            result.message = if e.code == "TOOL_TIMEOUT" {
                "Java 检测超时，请重试或更换运行时"
            } else {
                "无法启动 Java，请检查文件与权限"
            }
            .into();
            return result;
        }
    };
    result.version = property(&output, "java.version").into();
    result.vendor = property(&output, "java.vendor").into();
    result.architecture = property(&output, "os.arch").into();
    if major(&result.version) == 0 {
        result.status = "failed".into();
        result.message = "无法读取 Java 版本，请选择完整运行时".into();
        return result;
    }
    if !matches!(result.architecture.as_str(), "amd64" | "x86_64") {
        result.status = "architecture".into();
        result.message = "请选择 Windows x64 Java 运行时".into();
        return result;
    }
    let mut candidate = settings.clone();
    candidate.java_path = result.path.clone();
    match process::java_tool(&candidate, "sdkmanager", vec!["--version".into()]) {
        Ok(mut spec) => {
            result.tools_ready = true;
            spec.timeout = 10;
            match process::run(spec).await {
                Ok(v) if v.code == 0 => {
                    result.compatibility_checked = true;
                }
                Ok(v) => {
                    result.status = "incompatible".into();
                    result.message = if format!("{}{}", v.text(), v.stderr)
                        .contains("UnsupportedClassVersionError")
                    {
                        "Java 版本与当前 SDK 工具不兼容，请更换运行时"
                    } else {
                        "SDK 工具启动验证失败，请检查 Java 与 Command-line Tools"
                    }
                    .into();
                    return result;
                }
                Err(e) => {
                    result.status = if e.code == "TOOL_TIMEOUT" {
                        "timeout"
                    } else {
                        "failed"
                    }
                    .into();
                    result.message = "SDK 工具启动检测失败或超时，请重试".into();
                    return result;
                }
            }
        }
        Err(e) if e.code == "MISSING_COMMAND_TOOLS" => {
            // Until the pinned tools are available, use the application's verified runtime baseline.
            if major(&result.version) < 21 {
                result.status = "incompatible".into();
                result.message =
                    "SDK 工具尚未准备，请使用推荐的 Java 21，或准备工具后重新检测兼容性".into();
                return result;
            }
        }
        Err(e) => {
            result.status = "tools_error".into();
            result.message = e.message;
            return result;
        }
    }
    result.status = "ready".into();
    result.message = if result.compatibility_checked {
        "Java 与 SDK 工具兼容性检查通过"
    } else {
        "Java 启动验证通过；Command-line Tools 尚未准备"
    }
    .into();
    result
}
fn candidate_paths(settings: &Settings, data: &Path) -> Vec<(String, String)> {
    let mut paths = vec![
        (settings.java_path.clone(), "configured".into()),
        (
            private_path(data).to_string_lossy().into(),
            "private".into(),
        ),
    ];
    for key in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
        if let Ok(root) = std::env::var(key) {
            paths.push((
                Path::new(&root)
                    .join("Android/Android Studio/jbr/bin/java.exe")
                    .to_string_lossy()
                    .into(),
                "studio".into(),
            ));
        }
    }
    if let Ok(home) = std::env::var("JAVA_HOME") {
        paths.push((
            Path::new(&home)
                .join("bin/java.exe")
                .to_string_lossy()
                .into(),
            "JAVA_HOME".into(),
        ));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.is_absolute() {
                paths.push((dir.join("java.exe").to_string_lossy().into(), "PATH".into()));
            }
        }
    }
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|(path, _)| !path.is_empty() && Path::new(path).is_file())
        .filter(|(path, _)| {
            seen.insert(
                std::fs::canonicalize(path)
                    .unwrap_or_else(|_| PathBuf::from(path))
                    .to_string_lossy()
                    .to_lowercase(),
            )
        })
        .take(12)
        .collect()
}
impl App {
    pub fn java_busy(&self, except: Option<&str>) -> Result<bool> {
        Ok(self
            .store
            .active_tasks()?
            .iter()
            .any(|task| Some(task.id.as_str()) != except && related(&task.operation.kind)))
    }
    pub async fn java_environment(&self) -> Result<JavaEnvironment> {
        let settings = self.store.settings();
        let current = check(&settings, &settings.java_path, "configured").await;
        let mut candidates = Vec::new();
        for (path, source) in candidate_paths(&settings, &self.data) {
            if path == settings.java_path {
                candidates.push(JavaCheck {
                    source,
                    ..current.clone()
                });
            } else {
                candidates.push(check(&settings, &path, &source).await);
            }
        }
        Ok(JavaEnvironment {
            current,
            candidates,
            private_path: private_path(&self.data).to_string_lossy().into(),
            download: package()?,
            license: license(),
            busy: self.java_busy(None)?,
            preparation: self
                .store
                .active_tasks()?
                .into_iter()
                .find(|task| task.operation.kind == "java_prepare"),
        })
    }
    pub async fn apply_java(&self, path: &str) -> Result<JavaCheck> {
        let settings = self.store.settings();
        let checked = check(&settings, path, "configured").await;
        checked.require()?;
        let _guard = self.configuration_gate.lock().unwrap();
        if self.java_busy(None)? {
            return Err(AppError::new(
                "TASKS_ACTIVE",
                "相关任务进行中，结束后可切换 Java",
            ));
        }
        let mut current = self.store.settings();
        if current.sdk_root != settings.sdk_root || current.java_path != settings.java_path {
            return Err(AppError::new(
                "JAVA_CONTEXT_CHANGED",
                "检测期间工具配置已变化，请重新检测",
            ));
        }
        current.java_path = checked.path.clone();
        self.store.save_settings(&current)?;
        self.notify();
        Ok(checked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_and_properties() {
        assert_eq!(major("1.8.0_442"), 8);
        assert_eq!(major("21.0.12+1-LTS"), 21);
        assert_eq!(
            property("    os.arch = amd64\n    java.version = 21.0.1", "os.arch"),
            "amd64"
        );
        assert_eq!(major("invalid"), 0);
    }
    #[tokio::test]
    async fn missing_java_is_not_usable() {
        let dir = tempfile::tempdir().unwrap();
        let check = check(
            &Settings::default(),
            &dir.path().join("missing.exe").to_string_lossy(),
            "test",
        )
        .await;
        assert!(!check.ready());
        assert!(check.require().is_err());
    }
    #[test]
    fn java_plan_is_independent_of_sdk_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let plan = app
            .prepare_plan(Operation {
                kind: "java_prepare".into(),
                args: Default::default(),
                target: None,
            })
            .unwrap();
        assert_eq!(plan.licenses[0].hash, license().hash);
        assert!(!plan.operation.args.contains_key("downloadPackages"));
        assert!(package()
            .unwrap()
            .url
            .starts_with("https://github.com/adoptium/"));
    }
    #[test]
    fn java_preparation_requires_consent_and_reuses_the_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let plan = app
            .prepare_plan(Operation {
                kind: "java_prepare".into(),
                args: Default::default(),
                target: None,
            })
            .unwrap();
        assert_eq!(
            app.submit(
                &plan.id,
                &plan.hash,
                "no-consent",
                &plan.confirmation_token,
                &[]
            )
            .unwrap_err()
            .code,
            "LICENSE_REQUIRED"
        );
        let mut active = crate::regression_tests::task("existing-java", "running");
        active.operation.kind = "java_prepare".into();
        app.store.save_task(&active).unwrap();
        assert_eq!(
            app.submit(
                &plan.id,
                &plan.hash,
                "deduplicated",
                &plan.confirmation_token,
                &[license().hash]
            )
            .unwrap()
            .id,
            active.id
        );
        active.operation.kind = "sdk_install".into();
        app.store.save_task(&active).unwrap();
        assert_eq!(
            app.submit(
                &plan.id,
                &plan.hash,
                "busy",
                &plan.confirmation_token,
                &[license().hash]
            )
            .unwrap_err()
            .code,
            "TASKS_ACTIVE"
        );
    }
    #[tokio::test(flavor = "multi_thread")]
    async fn cancelling_a_pending_java_download_preserves_settings() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let (accepted, connection) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            let _ = accepted.send(());
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let settings = Settings {
            java_path: String::new(),
            sdk_root: dir.path().join("sdk").to_string_lossy().into(),
            proxy,
            ..Default::default()
        };
        app.store.save_settings(&settings).unwrap();
        let plan = app
            .prepare_plan(Operation {
                kind: "java_prepare".into(),
                args: Default::default(),
                target: None,
            })
            .unwrap();
        let task = app
            .submit(
                &plan.id,
                &plan.hash,
                "cancel-download",
                &plan.confirmation_token,
                &[license().hash],
            )
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), connection)
            .await
            .unwrap()
            .unwrap();
        app.cancel_task(&task.id).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let current = app.store.task(&task.id).unwrap();
                if !current.active() {
                    assert_eq!(current.state, "cancelled", "{}", current.message);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        server.abort();
        assert_eq!(
            serde_json::to_value(app.store.settings()).unwrap(),
            serde_json::to_value(settings).unwrap()
        );
        assert!(!private_path(dir.path()).exists());
    }
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "downloads the pinned Java runtime into an isolated temporary app; optional ANDROID_DEVKIT_JAVA_TEST_SDK is only probed with --version"]
    async fn private_java_real_install_repair_and_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let path = private_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"broken runtime fixture").unwrap();
        let settings = Settings {
            java_path: path.to_string_lossy().into(),
            sdk_root: std::env::var("ANDROID_DEVKIT_JAVA_TEST_SDK")
                .unwrap_or_else(|_| dir.path().join("sdk").to_string_lossy().into()),
            ..Default::default()
        };
        app.store.save_settings(&settings).unwrap();
        let original_home = std::env::var_os("JAVA_HOME");
        let original_path = std::env::var_os("PATH");
        let plan = app
            .prepare_plan(Operation {
                kind: "java_prepare".into(),
                args: Default::default(),
                target: None,
            })
            .unwrap();
        let task = app
            .submit(
                &plan.id,
                &plan.hash,
                "install",
                &plan.confirmation_token,
                &[license().hash],
            )
            .unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            let current = app.store.task(&task.id).unwrap();
            if !current.active() {
                assert_eq!(
                    current.state,
                    "succeeded",
                    "{}\n{}",
                    current.message,
                    current.log.join("\n")
                );
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Java preparation timed out: {}",
                current.message
            );
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        let checked = app
            .apply_java(path.parent().unwrap().parent().unwrap().to_str().unwrap())
            .await
            .unwrap();
        assert!(checked.ready());
        assert_eq!(major(&checked.version), 21);
        assert_eq!(app.store.settings().sdk_root, settings.sdk_root);
        assert_eq!(std::env::var_os("JAVA_HOME"), original_home);
        assert_eq!(std::env::var_os("PATH"), original_path);
        let mut active = crate::regression_tests::task("busy-sdk", "running");
        active.operation.kind = "sdk_install".into();
        app.store.save_task(&active).unwrap();
        assert_eq!(
            app.apply_java(&checked.path).await.unwrap_err().code,
            "TASKS_ACTIVE"
        );
        assert_eq!(app.store.settings().java_path, checked.path);
        println!(
            "Private runtime repaired, installed, validated and reused: {} ({})",
            checked.version, checked.message
        );
    }
}
