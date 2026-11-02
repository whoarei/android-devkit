//! Signing tool adapters and public metadata. Secrets never implement Serialize/Debug.
use crate::{
    app::App,
    domain::*,
    process::{self, Spec},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_KEYSTORE: u64 = 8 * 1024 * 1024;
const BRIDGE: &[u8] = include_bytes!("../resources/signing/DevkitSigningBridge.class");
pub(crate) struct Pending {
    pub plan: Plan,
    pub request: SigningRequest,
    pub secrets: Passwords,
    pub expires_at: tokio::time::Instant,
}
pub struct SigningState {
    pub(crate) pending: Mutex<HashMap<String, Pending>>,
    pub slots: tokio::sync::Semaphore,
}
impl Default for SigningState {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            slots: tokio::sync::Semaphore::new(2),
        }
    }
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Passwords {
    pub store_password: String,
    #[serde(default)]
    pub key_password: String,
}
impl Drop for Passwords {
    fn drop(&mut self) {
        self.store_password.zeroize();
        self.key_password.zeroize();
    }
}
impl Passwords {
    pub fn validate(&self, creating: bool) -> Result<()> {
        for value in [&self.store_password, &self.key_password] {
            if value.len() > 4096 || value.contains(['\0', '\r', '\n']) {
                return Err(AppError::new(
                    "SIGNING_PASSWORD",
                    "密码不能包含换行或 NUL，长度不能超过 4096 字节",
                ));
            }
        }
        if self.store_password.is_empty()
            || (creating
                && (self.store_password.chars().count() < 12 || self.key().chars().count() < 12))
        {
            return Err(AppError::new(
                "SIGNING_PASSWORD",
                "请输入容器密码；新建密码至少 12 个字符",
            ));
        }
        Ok(())
    }
    pub fn key(&self) -> &str {
        if self.key_password.is_empty() {
            &self.store_password
        } else {
            &self.key_password
        }
    }
    pub(crate) fn bridge_input(&self, key: bool) -> Zeroizing<Vec<u8>> {
        let mut input = Zeroizing::new(Vec::new());
        for value in std::iter::once(self.store_password.as_str()).chain(key.then(|| self.key())) {
            input.extend_from_slice(&(value.len() as u32).to_be_bytes());
            input.extend_from_slice(value.as_bytes());
        }
        input
    }
    pub fn lines(&self, creating: bool, format: &str) -> Zeroizing<Vec<u8>> {
        let mut input = Zeroizing::new(Vec::new());
        let values: Vec<&str> = if creating {
            if format == "JKS" {
                vec![
                    &self.store_password,
                    &self.store_password,
                    self.key(),
                    self.key(),
                ]
            } else {
                vec![&self.store_password, &self.store_password]
            }
        } else {
            vec![&self.store_password, self.key()]
        };
        for value in values {
            input.extend_from_slice(value.as_bytes());
            input.push(b'\n');
        }
        input
    }
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateInfo {
    pub subject: String,
    pub issuer: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub not_before: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub not_after: i64,
    pub algorithm: String,
    pub bits: u32,
    pub sha1: String,
    pub sha256: String,
    pub der: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyEntryInfo {
    pub alias: String,
    pub private_key: bool,
    pub certificate: CertificateInfo,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeystoreInfo {
    pub format: String,
    pub entries: Vec<KeyEntryInfo>,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeystoreRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub role: String,
    pub package: String,
    pub notes: String,
    pub sha256: String,
    pub format: String,
    pub entries: Vec<KeyEntryInfo>,
    #[cfg_attr(test, ts(type = "number"))]
    pub checked_at: u64,
    pub status: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkSignatureReport {
    pub unsigned: bool,
    pub path: String,
    pub sha256: String,
    pub package: String,
    pub version: String,
    pub verified: bool,
    pub signers: Vec<CertificateInfo>,
    pub schemes: Vec<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub lineage: bool,
    pub standalone: bool,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningEnvironment {
    pub java: String,
    pub keytool: String,
    pub apksigner: String,
    pub zipalign: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SigningRequest {
    pub mode: String,
    pub input: String,
    pub keystore_id: String,
    pub alias: String,
    pub output: String,
    pub name: String,
    pub format: String,
    pub subject: String,
    pub replace_signature: bool,
    pub overwrite_source: bool,
}

pub fn bounded_file(path: &Path, limit: u64) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(AppError::new("SIGNING_PATH", "请选择绝对文件路径"));
    }
    let path = std::fs::canonicalize(path)?;
    let meta = std::fs::metadata(&path)?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > limit {
        return Err(AppError::new(
            "SIGNING_SIZE",
            "文件为空、不是普通文件或超过大小限制",
        ));
    }
    Ok(path)
}
pub fn output_path(path: &str) -> Result<PathBuf> {
    let file = std::path::absolute(Path::new(path))?;
    if path.trim().is_empty() || file.file_name().is_none() || file.is_dir() {
        return Err(AppError::new("SIGNING_OUTPUT", "请选择有效的输出文件路径"));
    }
    let parent = std::fs::canonicalize(
        file.parent()
            .ok_or_else(|| AppError::new("SIGNING_OUTPUT", "请选择输出目录"))?,
    )?;
    let output = parent.join(file.file_name().unwrap());
    Ok(if output.try_exists()? {
        std::fs::canonicalize(output)?
    } else {
        output
    })
}
pub(crate) fn owned_root(app: &App, name: &str) -> Result<PathBuf> {
    let root = app.data.join(name);
    std::fs::create_dir_all(&root)?;
    let metadata = std::fs::symlink_metadata(&root)?;
    let mut linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        linked |= metadata.file_attributes() & 0x400 != 0;
    }
    if linked || !std::fs::canonicalize(&root)?.starts_with(std::fs::canonicalize(&app.data)?) {
        return Err(AppError::new("SIGNING_PATH", "应用签名目录归属无效"));
    }
    if matches!(name, "signing-jobs" | "artifacts") {
        protect_directory(&root)?;
    }
    Ok(root)
}
fn protect_directory(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                },
                SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
            },
        };
        let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)\0"
            .encode_utf16()
            .collect();
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor = std::ptr::null_mut();
        unsafe {
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let result = SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor,
            );
            LocalFree(descriptor);
            if result == 0 {
                return Err(AppError::new(
                    "SIGNING_PERMISSIONS",
                    "无法保护签名暂存目录权限，未继续处理密钥",
                ));
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn environment(app: &App) -> SigningEnvironment {
    let java = crate::java::normalize(&app.store.settings().java_path);
    let keytool = java.parent().unwrap_or(Path::new("")).join("keytool.exe");
    let jar = crate::phone_apk::tool(&app.store.settings()).unwrap_or_default();
    let zipalign = jar
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new(""))
        .join("zipalign.exe");
    let existing = |p: &Path| {
        if p.is_file() {
            p.to_string_lossy().into_owned()
        } else {
            String::new()
        }
    };
    SigningEnvironment {
        java: existing(&java),
        keytool: existing(&keytool),
        apksigner: existing(&jar),
        zipalign: existing(&zipalign),
    }
}
fn bridge_spec(app: &App, mode: &str, file: &Path, alias: &str) -> Result<Spec> {
    let env = environment(app);
    if env.java.is_empty() {
        return Err(AppError::new("SIGNING_JAVA", "请先配置 Java 17 或更高版本"));
    }
    let directory = owned_root(app, "runtime")?.join(format!("signing-{}", digest(BRIDGE)));
    std::fs::create_dir_all(&directory)?;
    if !std::fs::canonicalize(&directory)?
        .starts_with(std::fs::canonicalize(app.data.join("runtime"))?)
    {
        return Err(AppError::new("SIGNING_PATH", "应用签名目录归属无效"));
    }
    let class = directory.join("DevkitSigningBridge.class");
    if !class.exists() {
        let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut out = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        out.write_all(BRIDGE)?;
        out.sync_all()?;
        drop(out);
        let published = crate::recovery::publish_new_file(&temporary, &class);
        let _ = std::fs::remove_file(&temporary);
        if let Err(error) = published {
            if !class.exists() {
                return Err(error.into());
            }
        }
    }
    let mut actual = Vec::new();
    std::fs::File::open(&class)?
        .take(BRIDGE.len() as u64 + 1)
        .read_to_end(&mut actual)?;
    if actual != BRIDGE {
        return Err(AppError::new(
            "SIGNING_BRIDGE",
            "签名桥接程序内容不匹配，请检查应用运行时",
        ));
    }
    let classpath = std::env::join_paths(
        std::iter::once(directory)
            .chain((!env.apksigner.is_empty()).then(|| PathBuf::from(&env.apksigner))),
    )
    .map_err(|_| AppError::new("SIGNING_PATH", "工具路径无法用于 Java classpath"))?;
    let mut spec = Spec::new(
        env.java,
        vec![
            "-Xmx192m".into(),
            "-Dfile.encoding=UTF-8".into(),
            "-cp".into(),
            classpath.to_string_lossy().into(),
            "DevkitSigningBridge".into(),
            mode.into(),
            file.to_string_lossy().into(),
            alias.into(),
        ],
    );
    spec.timeout = 90;
    Ok(spec)
}
pub(crate) async fn sensitive_tool(
    app: &Arc<App>,
    task: Option<&str>,
    spec: Spec,
    input: Zeroizing<Vec<u8>>,
) -> Result<process::Output> {
    let cancelled = async {
        loop {
            if task.is_some_and(|id| app.cancelled(id)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    };
    tokio::select! {
        value = process::run_sensitive(spec, input) => value.map_err(|_| AppError::new("SIGNING_TOOL", "签名工具启动失败或超时；已生成文件会保留供检查")),
        _ = cancelled => Err(AppError::new("CANCELLED", "已取消")),
    }
}
pub(crate) async fn bridge<T: serde::de::DeserializeOwned>(
    app: &Arc<App>,
    task: Option<&str>,
    mode: &str,
    file: &Path,
    alias: &str,
    input: Zeroizing<Vec<u8>>,
) -> Result<T> {
    let output = sensitive_tool(app, task, bridge_spec(app, mode, file, alias)?, input).await?;
    if output.code != 0 {
        let kind = serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .ok()
            .and_then(|v| {
                v.get("bridgeError")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
            .filter(|value| value.len() < 80 && value.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or_default();
        if matches!(
            kind.as_str(),
            "CertificateExpiredException" | "CertificateNotYetValidException"
        ) {
            return Err(AppError::new(
                "SIGNING_CERTIFICATE",
                "证书已过期或尚未生效，不能用于签名",
            ));
        }
        return Err(AppError::new(
            "SIGNING_UNLOCK",
            "无法读取：请检查密码、文件、格式及工具版本",
        )
        .detail(kind));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| AppError::new("SIGNING_RESPONSE", "签名工具结果无法可靠识别"))
}
pub async fn inspect(
    app: &Arc<App>,
    path: &Path,
    secrets: &Passwords,
    alias: Option<&str>,
) -> Result<KeystoreInfo> {
    secrets.validate(false)?;
    let path = bounded_file(path, MAX_KEYSTORE)?;
    let result: KeystoreInfo = bridge(
        app,
        None,
        if alias.is_some() {
            "keycheck"
        } else {
            "inspect"
        },
        &path,
        alias.unwrap_or(""),
        secrets.bridge_input(alias.is_some()),
    )
    .await?;
    if !matches!(result.format.as_str(), "JKS" | "PKCS12") || result.entries.len() > 256 {
        return Err(AppError::new(
            "SIGNING_FORMAT",
            "首轮支持 JKS 和 PKCS12 格式",
        ));
    }
    Ok(result)
}
pub(crate) async fn sign_apk_with_key(
    app: &Arc<App>,
    task: &str,
    keystore: &Path,
    alias: &str,
    input: &Path,
    output: &Path,
    secrets: &Passwords,
) -> Result<()> {
    let mut spec = bridge_spec(app, "sign", keystore, alias)?;
    spec.args.extend([
        input.to_string_lossy().into_owned(),
        output.to_string_lossy().into_owned(),
    ]);
    spec.timeout = 120;
    let result = sensitive_tool(app, Some(task), spec, secrets.bridge_input(true)).await?;
    if result.code != 0 {
        return Err(AppError::new(
            "SIGNING_FAILED",
            "APK 签名失败，请检查所选条目、密码及工具版本",
        ));
    }
    Ok(())
}
pub(crate) async fn normalize_generated_key(
    app: &Arc<App>,
    task: Option<&str>,
    file: &Path,
    alias: &str,
    secrets: &Passwords,
) -> Result<()> {
    let before = digest_file(file)?;
    let temporary = file.with_extension("normalizing");
    // A validated original exists. This secondary file is owned by the same job.
    if temporary.try_exists()? {
        std::fs::remove_file(&temporary)?;
    }
    let mut spec = bridge_spec(app, "normalize", file, alias)?;
    spec.args.push(temporary.to_string_lossy().into());
    let output = sensitive_tool(app, task, spec, secrets.bridge_input(true)).await?;
    if output.code != 0 {
        return Err(AppError::new(
            "SIGNING_PASSWORD_ENCODING",
            "无法规范化新密钥密码，已保留原始生成结果",
        ));
    }
    let _: KeystoreInfo = bridge(
        app,
        task,
        "strict-keycheck",
        &temporary,
        alias,
        secrets.bridge_input(true),
    )
    .await?;
    if digest_file(file)? != before {
        return Err(AppError::new("SIGNING_CHANGED", "密钥文件在检查期间变化"));
    }
    std::fs::rename(temporary, file)?;
    Ok(())
}
pub async fn inspect_apk(app: &Arc<App>, file: &Path) -> Result<ApkSignatureReport> {
    let file = bounded_file(file, 1024 * 1024 * 1024)?;
    if file
        .extension()
        .and_then(|v| v.to_str())
        .is_none_or(|v| !v.eq_ignore_ascii_case("apk"))
    {
        return Err(AppError::new("APK_ONLY", "请选择完整 APK 文件"));
    }
    if environment(app).apksigner.is_empty() {
        return Err(AppError::new(
            "APK_TOOLS_MISSING",
            "请先准备 SDK Build-Tools",
        ));
    }
    let sha = digest_file(&file)?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&file)?)
        .map_err(|_| AppError::new("INVALID_APK", "文件不是有效 APK"))?;
    if archive.len() > 100_000 {
        return Err(AppError::new("INVALID_APK", "APK 条目超限"));
    }
    let mut bytes = Vec::new();
    archive
        .by_name("AndroidManifest.xml")
        .map_err(|_| AppError::new("INVALID_APK", "缺少 AndroidManifest.xml"))?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(AppError::new("INVALID_APK", "Manifest 体积超限"));
    }
    let attrs = crate::apk::parse_manifest(&bytes)?;
    drop(archive);
    #[derive(Deserialize)]
    struct Verified {
        unsigned: bool,
        verified: bool,
        signers: Vec<CertificateInfo>,
        schemes: Vec<String>,
        errors: Vec<String>,
        warnings: Vec<String>,
        lineage: bool,
    }
    let value: Verified = bridge(app, None, "verify", &file, "", Zeroizing::new(vec![])).await?;
    if digest_file(&file)? != sha {
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "APK 在检查期间发生变化，请重新选择",
        ));
    }
    Ok(ApkSignatureReport {
        unsigned: value.unsigned,
        path: file.to_string_lossy().into(),
        sha256: sha,
        package: attrs.get("manifest:package").cloned().unwrap_or_default(),
        version: attrs
            .get("manifest:versionName")
            .cloned()
            .unwrap_or_default(),
        standalone: !attrs.contains_key("manifest:split")
            && !attrs.keys().any(|key| key.starts_with("uses-split:"))
            && attrs
                .get("application:isSplitRequired")
                .is_none_or(|v| v != "true" && v != "1"),
        verified: value.verified,
        signers: value.signers,
        schemes: value.schemes,
        errors: value.errors,
        warnings: value.warnings,
        lineage: value.lineage,
    })
}
pub fn records(app: &App) -> Result<Vec<KeystoreRecord>> {
    let mut records = app.store.signing_records()?;
    for record in &mut records {
        record.status = match bounded_file(Path::new(&record.path), MAX_KEYSTORE)
            .and_then(|path| digest_file(&path))
        {
            Ok(hash) if hash == record.sha256 && !hash.is_empty() => "ready",
            Ok(_) => "needs_review",
            Err(_) => "missing_dependency",
        }
        .into();
    }
    Ok(records)
}
pub async fn register(
    app: &Arc<App>,
    mut record: KeystoreRecord,
    secrets: Option<Passwords>,
) -> Result<KeystoreRecord> {
    if record.name.trim().is_empty()
        || record.name.len() > 200
        || record.notes.len() > 2000
        || record.package.len() > 200
        || !matches!(record.role.as_str(), "unknown" | "upload" | "app" | "debug")
    {
        return Err(AppError::new("SIGNING_INPUT", "请检查名称、用途和备注长度"));
    }
    let existing = app.store.signing_records()?;
    if !record.id.is_empty() && !existing.iter().any(|r| r.id == record.id) {
        return Err(AppError::new("SIGNING_RECORD", "登记记录不存在"));
    }
    if record.id.is_empty() && existing.len() >= 256 {
        return Err(AppError::new("SIGNING_LIMIT", "最多登记 256 个 Keystore"));
    }
    let path = Path::new(&record.path);
    if !path.is_absolute() {
        return Err(AppError::new("SIGNING_PATH", "请选择绝对文件路径"));
    }
    record.path = std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into();
    if let Some(duplicate) = existing
        .iter()
        .find(|r| r.path.eq_ignore_ascii_case(&record.path) && r.id != record.id)
    {
        return Ok(duplicate.clone());
    }
    record.sha256.clear();
    record.entries.clear();
    record.format.clear();
    record.checked_at = 0;
    record.status = "needs_review".into();
    if secrets.is_none() {
        if let Some(previous) = existing
            .iter()
            .find(|r| r.id == record.id && r.path == record.path)
        {
            if !previous.sha256.is_empty()
                && bounded_file(Path::new(&record.path), MAX_KEYSTORE)
                    .and_then(|p| digest_file(&p))
                    .ok()
                    .as_deref()
                    == Some(&previous.sha256)
            {
                record.sha256 = previous.sha256.clone();
                record.entries = previous.entries.clone();
                record.format = previous.format.clone();
                record.checked_at = previous.checked_at;
                record.status = "ready".into();
            }
        }
    }
    if let Some(secret) = secrets {
        let path = bounded_file(Path::new(&record.path), MAX_KEYSTORE)?;
        let hash = digest_file(&path)?;
        let info = inspect(app, &path, &secret, None).await?;
        if digest_file(&path)? != hash {
            return Err(AppError::new(
                "SIGNING_CHANGED",
                "Keystore 在检查期间发生变化",
            ));
        }
        record.sha256 = hash;
        record.entries = info.entries;
        record.format = info.format;
        record.checked_at = now();
        record.status = "ready".into();
    }
    if record.id.is_empty() {
        record.id = uuid::Uuid::new_v4().to_string();
    }
    app.store.save_signing_record(&record)?;
    app.notify();
    Ok(record)
}
pub(crate) fn select_record(app: &App, id: &str) -> Result<KeystoreRecord> {
    app.store
        .signing_records()?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| AppError::new("SIGNING_RECORD", "请选择已登记的 Keystore"))
}
pub(crate) fn new_copy(source: &Path, destination: &Path, expected: &str) -> Result<()> {
    let source = bounded_file(source, 1024 * 1024 * 1024)?;
    let mut input = std::fs::File::open(source)?;
    let mut out = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    let result = (|| -> Result<()> {
        let count = std::io::copy(
            &mut Read::by_ref(&mut input).take(1024 * 1024 * 1024 + 1),
            &mut out,
        )?;
        out.sync_all()?;
        if count > 1024 * 1024 * 1024 || digest_file(destination)? != expected {
            return Err(AppError::new(
                "SIGNING_CHANGED",
                "复制期间文件内容变化，已阻止执行",
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        drop(out);
        let _ = std::fs::remove_file(destination);
    }
    result
}
pub(crate) fn publish_copy(source: &Path, destination: &Path, expected: &str) -> Result<()> {
    let destination = output_path(&destination.to_string_lossy())?;
    let temporary = destination
        .parent()
        .unwrap()
        .join(format!(".devkit-signing-{}.tmp", uuid::Uuid::new_v4()));
    let result = new_copy(source, &temporary, expected).and_then(|_| {
        crate::recovery::publish_new_file(&temporary, &destination).map_err(Into::into)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_matches_reviewable_source_and_bytecode() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../resources/signing/bridge.json")).unwrap();
        assert_eq!(
            manifest["sourceSha256"],
            digest(
                include_str!("../resources/signing/DevkitSigningBridge.java")
                    .replace("\r\n", "\n")
                    .as_bytes()
            )
        );
        assert_eq!(manifest["classSha256"], digest(BRIDGE));
    }
    #[test]
    fn publication_never_replaces_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source");
        let dest = dir.path().join("target");
        std::fs::write(&src, b"new").unwrap();
        std::fs::write(&dest, b"old").unwrap();
        assert!(publish_copy(&src, &dest, &digest(b"new")).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");
        assert!(publish_copy(&src, &dir.path().join("changed"), &digest(b"other")).is_err());
        assert!(!dir.path().join("changed").exists());
    }
    #[test]
    fn output_selection_accepts_existing_and_relative_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing.apk");
        std::fs::write(&target, b"old").unwrap();
        assert_eq!(
            output_path(&target.to_string_lossy()).unwrap(),
            std::fs::canonicalize(&target).unwrap()
        );
        assert!(output_path("relative-output.apk").unwrap().is_absolute());
        assert!(output_path("").is_err());
        assert!(output_path(&dir.path().to_string_lossy()).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }
}
