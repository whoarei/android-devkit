use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub detail: String,
}
impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: String::new(),
        }
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AppError {}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::new("IO_ERROR", e.to_string())
    }
}
impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("DATABASE_ERROR", e.to_string())
    }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("FORMAT_ERROR", e.to_string())
    }
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub sdk_root: String,
    pub avd_roots: Vec<String>,
    pub java_path: String,
    pub proxy: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub boot_timeout_secs: u64,
    #[serde(default)]
    pub tray_enabled: bool,
    #[serde(default)]
    pub sdk_sites: Vec<SdkSite>,
    #[serde(default)]
    pub sdk_channel: u8,
    #[serde(default)]
    pub sdk_download: SdkDownloadSource,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SdkDownloadSource {
    pub kind: String,
    pub name: String,
    pub url: String,
    pub verify_official: bool,
}
impl Default for SdkDownloadSource {
    fn default() -> Self {
        Self {
            kind: "google".into(),
            name: String::new(),
            url: String::new(),
            verify_official: true,
        }
    }
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkSourceCheck {
    pub sites: Vec<SdkSiteStatus>,
    pub archive_url: String,
    pub archive_error: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub checked_at: u64,
}
impl Default for Settings {
    fn default() -> Self {
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        let java = [
            std::env::var("JAVA_HOME")
                .ok()
                .map(|p| format!("{p}\\bin\\java.exe")),
            Some("C:\\Program Files\\Android\\Android Studio\\jbr\\bin\\java.exe".into()),
        ]
        .into_iter()
        .flatten()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap_or_default();
        Self {
            sdk_root: format!("{local}\\Android\\Sdk"),
            avd_roots: vec![
                format!("{home}\\.android\\avd"),
                format!("{local}\\AndroidDevKit\\avd"),
            ],
            java_path: java,
            proxy: String::new(),
            boot_timeout_secs: 180,
            tray_enabled: false,
            sdk_sites: Vec::new(),
            sdk_channel: 0,
            sdk_download: SdkDownloadSource::default(),
        }
    }
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SdkSite {
    pub name: String,
    pub url: String,
    pub enabled: bool,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SdkSiteStatus {
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub builtin: bool,
    #[cfg_attr(test, ts(type = "number"))]
    pub checked_at: u64,
    pub package_count: u32,
    pub error: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub path: String,
    pub source: String,
    pub adb: String,
    pub emulator: String,
    pub command_tools: String,
    pub java: String,
    pub writable: bool,
    pub warnings: Vec<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub free_bytes: u64,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SdkPackage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub installed: bool,
    #[serde(default)]
    pub installed_version: String,
    pub api: String,
    pub abi: String,
    pub tag: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub size: u64,
    pub url: String,
    pub checksum: String,
    pub checksum_type: String,
    pub license_id: String,
    pub license: String,
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub dependency_min_versions: BTreeMap<String, String>,
    pub obsolete: bool,
    #[serde(default)]
    pub channel: u8,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub metadata_key: String,
    #[serde(default)]
    pub official_checksum: String,
    #[serde(default)]
    pub official_status: String,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub official_checked_at: u64,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Avd {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub ini_path: String,
    pub data_path: String,
    pub image: String,
    pub api: String,
    pub abi: String,
    pub memory: String,
    pub disk: String,
    pub gpu: String,
    pub fingerprint: String,
    pub status: String,
    pub error: String,
    pub serial: String,
    pub owned: bool,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial: String,
    pub transport_id: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub server_epoch: u64,
    pub model: String,
    pub state: String,
    pub connection: String,
    pub avd_name: String,
    pub api: String,
    pub abi: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub serial: String,
    pub transport_id: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub server_epoch: u64,
}
impl From<&Device> for Target {
    fn from(d: &Device) -> Self {
        Self {
            serial: d.serial.clone(),
            transport_id: d.transport_id.clone(),
            server_epoch: d.server_epoch,
        }
    }
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub kind: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    pub target: Option<Target>,
}
impl Operation {
    pub fn arg(&self, name: &str) -> Result<&str> {
        self.args
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| AppError::new("INVALID_INPUT", format!("缺少参数 {name}")))
    }
    pub fn optional(&self, name: &str) -> &str {
        self.args.get(name).map(String::as_str).unwrap_or("")
    }
    pub fn destructive(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "sdk_remove"
                | "avd_delete"
                | "avd_wipe"
                | "avd_force_stop"
                | "app_reinstall"
                | "adb_restart"
                | "screen_cleanup"
        ) || (matches!(self.kind.as_str(), "file_upload" | "file_download")
            && self.optional("conflicts") == "replace")
    }
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub hash: String,
    pub operation: Operation,
    pub title: String,
    pub effects: Vec<String>,
    pub warnings: Vec<String>,
    pub licenses: Vec<License>,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: u64,
    pub settings_hash: String,
    pub confirmation_token: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct License {
    pub id: String,
    pub text: String,
    pub hash: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub request_id: String,
    pub title: String,
    pub operation: Operation,
    pub state: String,
    pub phase: String,
    pub progress: Option<f64>,
    pub message: String,
    pub log: Vec<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub updated_at: u64,
    pub cancel_requested: bool,
    pub cancellable: bool,
    pub artifact: String,
    pub license_prompt: String,
    pub confirmed_licenses: Vec<String>,
}
impl Task {
    pub fn active(&self) -> bool {
        matches!(
            self.state.as_str(),
            "queued" | "running" | "waiting_confirmation"
        )
    }
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskQuery {
    pub keyword: String,
    pub state: String,
    pub offset: u32,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPage {
    pub tasks: Vec<Task>,
    pub total: u32,
    pub counts: std::collections::BTreeMap<String, u32>,
    pub offset: u32,
    pub page_size: u32,
    pub deletion: TaskDeletionSummary,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDeletionSummary {
    pub eligible: u32,
    pub active: u32,
    pub files: u32,
    pub review: u32,
    pub busy: u32,
    pub blocked: BTreeMap<String, String>,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskDeleteRequest {
    pub task_ids: Vec<String>,
    pub query: Option<TaskQuery>,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDeleteIssue {
    pub task_id: String,
    pub title: String,
    pub reason: String,
    pub retryable: bool,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDeleteResult {
    pub deleted: u32,
    pub already_deleted: u32,
    pub issues: Vec<TaskDeleteIssue>,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApkInfo {
    pub id: String,
    pub path: String,
    pub file_name: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub size: u64,
    pub sha256: String,
    pub package: String,
    pub version: String,
    pub min_sdk: String,
    pub abis: Vec<String>,
    pub warning: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub setup_required: bool,
    pub settings: Settings,
    pub environment: Environment,
    pub packages: Vec<SdkPackage>,
    pub avds: Vec<Avd>,
    pub devices: Vec<Device>,
    pub tasks: Vec<Task>,
    pub server: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub server_epoch: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub free_bytes: u64,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LogFilter {
    pub level: String,
    pub tag: String,
    pub keyword: String,
    pub package: String,
    pub pids: Vec<u32>,
    #[cfg_attr(test, ts(type = "number"))]
    pub after: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub before: u64,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    #[cfg_attr(test, ts(type = "number"))]
    pub seq: u64,
    pub timestamp: String,
    pub pid: u32,
    pub tid: u32,
    pub level: String,
    pub tag: String,
    pub text: String,
    pub raw: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSnapshot {
    pub rows: Vec<LogEntry>,
    #[cfg_attr(test, ts(type = "number"))]
    pub latest: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub oldest: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub dropped: u64,
    pub buffered: usize,
    pub active: bool,
    pub status: String,
    pub process_status: String,
    pub pids: Vec<u32>,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub task_id: String,
    pub bytes: usize,
    pub sha256: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: u64,
    pub acknowledged: bool,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreview {
    pub artifact: Artifact,
    pub text: String,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub rows: usize,
    pub redacted: bool,
    pub truncated: bool,
    pub warning: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseStatus {
    pub tasks: Vec<Task>,
    pub unsaved: Vec<Artifact>,
    pub draining: bool,
    pub tray_enabled: bool,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}
pub fn digest_file(path: &std::path::Path) -> Result<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = sha2::Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !name.starts_with('-')
}
pub fn valid_package(id: &str) -> bool {
    !id.is_empty()
        && id.len() < 200
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_;.-".contains(c))
}
pub fn checked_target<'a>(
    target: &Target,
    devices: &'a [Device],
    epoch: u64,
) -> Result<&'a Device> {
    if target.server_epoch != epoch {
        return Err(AppError::new(
            "STALE_SESSION",
            "ADB 服务会话已经变化，请重新选择设备",
        ));
    }
    devices
        .iter()
        .find(|d| {
            d.serial == target.serial
                && d.transport_id == target.transport_id
                && d.server_epoch == epoch
                && d.state == "ready"
        })
        .ok_or_else(|| {
            AppError::new(
                "DEVICE_UNAVAILABLE",
                "原目标已断开或不可用，任务不会转移到其他设备",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_falls_back_to_another_device() {
        let a = Target {
            serial: "A".into(),
            transport_id: "1".into(),
            server_epoch: 1,
        };
        let b = Device {
            serial: "B".into(),
            transport_id: "2".into(),
            server_epoch: 1,
            state: "ready".into(),
            ..Default::default()
        };
        assert!(checked_target(&a, &[b], 1).is_err());
    }
    #[test]
    fn rejects_reused_transport_after_server_restart() {
        let a = Target {
            serial: "A".into(),
            transport_id: "1".into(),
            server_epoch: 1,
        };
        assert!(checked_target(&a, &[], 2).is_err());
    }
    #[test]
    fn validates_tool_identifiers() {
        for n in ["../x", "a & calc", "-force", ""] {
            assert!(!valid_name(n));
        }
        assert!(valid_name("DevKit_M3_QA"));
        assert!(valid_package("system-images;android-29;google_apis;x86_64"));
        assert!(!valid_package("--update"));
    }
}
