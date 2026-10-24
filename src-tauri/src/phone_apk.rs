//! Immutable, verified APK sets. Every member is submitted in one install session.
use crate::{
    app::App,
    domain::*,
    process::{self, Spec},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkMember {
    pub file_name: String,
    pub split: String,
    pub sha256: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    pub min_sdk: u32,
    pub abis: Vec<String>,
    pub requires: Vec<String>,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkSet {
    pub id: String,
    pub package: String,
    pub version: String,
    pub version_code: String,
    pub signers: Vec<String>,
    pub members: Vec<ApkMember>,
}
fn tool(settings: &Settings) -> Result<PathBuf> {
    let root = Path::new(&settings.sdk_root).join("build-tools");
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("lib/apksigner.jar"))
        .filter(|p| p.is_file())
        .collect();
    paths.sort_by_key(|path| {
        path.ancestors()
            .nth(2)
            .and_then(Path::file_name)
            .unwrap_or_default()
            .to_string_lossy()
            .split('.')
            .map(|part| part.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    });
    paths.reverse();
    paths.into_iter().next().ok_or_else(|| {
        AppError::new(
            "APK_TOOLS_MISSING",
            "分包校验需要 SDK Build-Tools；请先通过下方依赖计划准备校验工具",
        )
    })
}
pub fn tool_package(app: &App) -> Result<String> {
    if tool(&app.store.settings()).is_ok() {
        return Ok(String::new());
    }
    let packages = app
        .store
        .sdk_catalog()
        .ok_or_else(|| AppError::new("CATALOG_REQUIRED", "请先刷新 SDK 目录"))?
        .0;
    packages
        .iter()
        .filter(|p| p.id.starts_with("build-tools;") && !p.id.contains("rc"))
        .max_by_key(|p| {
            p.id.split(';')
                .nth(1)
                .unwrap_or("")
                .split('.')
                .map(|v| v.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        })
        .map(|p| p.id.clone())
        .ok_or_else(|| {
            AppError::new(
                "CATALOG_REQUIRED",
                "请先刷新 SDK 目录，以生成 Build-Tools 最小依赖计划",
            )
        })
}
pub fn set_dir(app: &App, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("APK_SET_ID", "APK 组标识无效"))?;
    Ok(app.data.join("apk-sets").join(id))
}
pub fn load(app: &App, id: &str) -> Result<ApkSet> {
    let set: ApkSet = serde_json::from_slice(&std::fs::read(set_dir(app, id)?.join("set.json"))?)?;
    if set.id != id {
        return Err(AppError::new("APK_SET_ID", "APK 组记录无效"));
    }
    validate_set(&set)?;
    Ok(set)
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._".contains(&b))
}
pub fn validate_set(set: &ApkSet) -> Result<()> {
    crate::app::validate_app_package(&set.package)?;
    if set.members.is_empty()
        || set.members.len() > 256
        || set.members.iter().filter(|m| m.split.is_empty()).count() != 1
    {
        return Err(AppError::new(
            "APK_BASE_REQUIRED",
            "必须包含且仅包含一个 base APK",
        ));
    }
    let names: BTreeSet<_> = set.members.iter().map(|m| m.split.as_str()).collect();
    let hashes: BTreeSet<_> = set.members.iter().map(|m| &m.sha256).collect();
    if names.len() != set.members.len() || hashes.len() != set.members.len() {
        return Err(AppError::new(
            "APK_DUPLICATE",
            "重复的 split 名称或 APK 内容",
        ));
    }
    for member in &set.members {
        if !member.split.is_empty() && !safe_name(&member.split) {
            return Err(AppError::new("APK_SPLIT_NAME", "split 名称无效"));
        }
        for required in &member.requires {
            if required != "base" && !names.contains(required.as_str()) {
                return Err(AppError::new(
                    "APK_SPLIT_MISSING",
                    format!("{} 缺少依赖 split：{required}", member.file_name),
                ));
            }
        }
    }
    fn visit<'a>(
        name: &'a str,
        members: &'a [ApkMember],
        visiting: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name) {
            return Err(AppError::new(
                "APK_SPLIT_CYCLE",
                "split 依赖存在循环，未生成安装计划",
            ));
        }
        if let Some(member) = members.iter().find(|member| member.split == name) {
            for dependency in &member.requires {
                if dependency != "base" {
                    visit(dependency, members, visiting, done)?;
                }
            }
        }
        visiting.remove(name);
        done.insert(name);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for member in &set.members {
        visit(&member.split, &set.members, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}
fn metadata(path: &Path) -> Result<(BTreeMap<String, String>, Vec<String>)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)
        .map_err(|_| AppError::new("INVALID_APK", "无效的 APK ZIP"))?;
    if zip.len() > 100_000 {
        return Err(AppError::new("INVALID_APK", "APK 条目超限"));
    }
    let mut abis = BTreeSet::new();
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|_| AppError::new("INVALID_APK", "无法读取 APK"))?;
        if let Some((abi, _)) = entry
            .name()
            .strip_prefix("lib/")
            .and_then(|s| s.split_once('/'))
        {
            abis.insert(abi.to_string());
        }
    }
    let entry = zip
        .by_name("AndroidManifest.xml")
        .map_err(|_| AppError::new("INVALID_APK", "缺少 Manifest"))?;
    let mut bytes = vec![];
    entry.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    Ok((
        crate::apk::parse_manifest(&bytes)?,
        abis.into_iter().collect(),
    ))
}
fn signer_digests(text: &str) -> Result<Vec<String>> {
    let values: BTreeSet<_> = text
        .lines()
        .filter_map(|l| l.trim().split_once(" certificate SHA-256 digest: "))
        .filter(|(prefix, _)| prefix.starts_with("Signer #"))
        .map(|(_, v)| v.trim().to_ascii_lowercase())
        .collect();
    if values.is_empty()
        || values
            .iter()
            .any(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(AppError::new("APK_SIGNATURE", "未能可靠识别 APK 签名证书"));
    }
    Ok(values.into_iter().collect())
}
impl App {
    pub async fn prepare_apk_set(&self, mut paths: Vec<PathBuf>) -> Result<ApkSet> {
        let settings = self.store.settings();
        let jar = tool(&settings)?;
        crate::java::check(&settings, &settings.java_path, "configured")
            .await
            .require()?;
        if paths.len() == 1 && paths[0].is_dir() {
            crate::phone_files::regular_local(&paths[0], true)?;
            let directory = paths.remove(0);
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("apk"))
                {
                    paths.push(path);
                    if paths.len() > 256 {
                        break;
                    }
                }
            }
        }
        if paths.is_empty() || paths.len() > 256 {
            return Err(AppError::new(
                "APK_SET_SIZE",
                "请选择 1～256 个 APK，或包含它们的目录",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dir = set_dir(self, &id)?;
        std::fs::create_dir_all(&dir)?;
        // Unpublished temporary sets are never loadable; only set.json commits the set.
        let result = async {
            let mut set = ApkSet {
                id: id.clone(),
                package: String::new(),
                version: String::new(),
                version_code: String::new(),
                signers: vec![],
                members: vec![],
            };
            let mut total = 0u64;
            for (i, path) in paths.iter().enumerate() {
                crate::phone_files::regular_local(path, false)?;
                let bytes = path.metadata()?.len();
                total = total.saturating_add(bytes);
                if !path
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("apk"))
                    || bytes > 1024 * 1024 * 1024
                    || total > 4 * 1024 * 1024 * 1024
                {
                    return Err(AppError::new(
                        "APK_SET_SIZE",
                        "仅支持 APK；单文件最多 1 GiB，整组最多 4 GiB",
                    ));
                }
                crate::capacity::budget(&[(dir.clone(), bytes, "APK 暂存")])?;
                let staged = dir.join(format!("{i}.apk"));
                let staged_hash = crate::phone_files::stage_file(path, &staged, bytes)?;
                let (attrs, abis) = metadata(&staged)?;
                let package = attrs.get("manifest:package").cloned().unwrap_or_default();
                let low = attrs
                    .get("manifest:versionCode")
                    .and_then(|s| s.parse::<u32>().ok())
                    .ok_or_else(|| AppError::new("APK_VERSION", "无法可靠读取 versionCode"))?;
                let high = attrs
                    .get("manifest:versionCodeMajor")
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                let version_code = (((high as u64) << 32) | low as u64).to_string();
                let mut spec = Spec::new(
                    &settings.java_path,
                    vec![
                        "-jar".into(),
                        jar.to_string_lossy().into(),
                        "verify".into(),
                        "--verbose".into(),
                        "--print-certs".into(),
                        staged.to_string_lossy().into(),
                    ],
                );
                spec.timeout = 60;
                let output = process::run(spec).await?.success().map_err(|e| {
                    AppError::new("APK_SIGNATURE", "APK 签名校验失败").detail(e.detail)
                })?;
                let signers = signer_digests(&output.text())?;
                if i == 0 {
                    set.package = package.clone();
                    set.version_code = version_code.clone();
                    set.version = attrs
                        .get("manifest:versionName")
                        .cloned()
                        .unwrap_or_default();
                    set.signers = signers.clone();
                }
                if attrs.get("manifest:split").is_none_or(String::is_empty) {
                    set.version = attrs
                        .get("manifest:versionName")
                        .cloned()
                        .unwrap_or_default();
                }
                if set.package != package
                    || set.version_code != version_code
                    || set.signers != signers
                {
                    return Err(AppError::new(
                        "APK_SET_MISMATCH",
                        "整组 APK 的包名、versionCode 或签名证书不一致",
                    ));
                }
                let mut requires: Vec<_> = attrs
                    .iter()
                    .filter(|(k, _)| k.starts_with("uses-split:name:"))
                    .map(|(_, v)| v.clone())
                    .collect();
                if let Some(v) = attrs
                    .get("manifest:configForSplit")
                    .filter(|s| !s.is_empty())
                {
                    requires.push(v.clone());
                }
                set.members.push(ApkMember {
                    file_name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    split: attrs.get("manifest:split").cloned().unwrap_or_default(),
                    sha256: staged_hash,
                    bytes,
                    min_sdk: attrs
                        .get("uses-sdk:minSdkVersion")
                        .map(|s| s.parse::<u32>())
                        .transpose()
                        .map_err(|_| AppError::new("APK_MIN_SDK", "无法识别 minSdkVersion"))?
                        .unwrap_or(1),
                    abis,
                    requires,
                });
            }
            validate_set(&set)?;
            std::fs::write(dir.join("set.json"), serde_json::to_vec(&set)?)?;
            Ok(set)
        }
        .await;
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        result
    }
    pub async fn install_apk_set(
        &self,
        id: &str,
        op: &Operation,
        settings: &Settings,
    ) -> Result<String> {
        if digest_file(&set_dir(self, op.arg("setId")?)?.join("set.json"))? != op.arg("setHash")? {
            return Err(AppError::new(
                "APK_CHANGED",
                "已确认的 APK 清单发生变化，请重新准备",
            ));
        }
        let set = load(self, op.arg("setId")?)?;
        let dir = set_dir(self, &set.id)?;
        let target = op
            .target
            .as_ref()
            .ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择设备"))?;
        let devices = crate::android::devices(settings, self.epoch.load(Ordering::SeqCst)).await;
        let device = checked_target(target, &devices, self.epoch.load(Ordering::SeqCst))?;
        let api = device
            .api
            .parse::<u32>()
            .map_err(|_| AppError::new("DEVICE_API", "无法读取设备 API"))?;
        let mut args = vec!["install-multiple".into(), "-r".into()];
        for (i, m) in set.members.iter().enumerate() {
            let path = dir.join(format!("{i}.apk"));
            if digest_file(&path)? != m.sha256 {
                return Err(AppError::new("APK_CHANGED", "APK 暂存内容发生变化"));
            }
            compatible_member(m, api, &device.abi)?;
            args.push(path.to_string_lossy().into());
        }
        if self.cancelled(id) {
            return Err(AppError::new("CANCELLED", "已在安装前取消"));
        }
        self.phase(
            id,
            "installing",
            "整组安装到冻结目标；不自动卸载、降级或转投设备",
        )?;
        let output = self.bound_adb(settings, target, args, 300).await?;
        let text = format!("{}\n{}", output.text(), output.stderr);
        if (output.code != 0 || !text.lines().any(|l| l.trim() == "Success"))
            && !text.contains("INSTALL_FAILED_")
            && !text.contains("INSTALL_PARSE_FAILED_")
        {
            return Err(AppError::new(
                "RESULT_UNKNOWN",
                "分包安装响应未能确认；请检查原设备，不会自动重复安装",
            )
            .detail(text));
        }
        crate::engine::apk_install_result(output.code, &text, &set.package)?;
        self.phase(id, "verifying", "核对已安装版本及 split 集合")?;
        let out = self
            .bound_adb(
                settings,
                target,
                vec![
                    "shell".into(),
                    "dumpsys".into(),
                    "package".into(),
                    set.package.clone(),
                ],
                30,
            )
            .await
            .map_err(|e| {
                AppError::new("RESULT_UNKNOWN", "安装已返回，版本与分包集合尚待检查")
                    .detail(format!("{}\n{}", e, e.detail))
            })?
            .success()
            .map_err(|e| {
                AppError::new("RESULT_UNKNOWN", "安装已返回，结果复查未完成；请核对原设备")
                    .detail(format!("{}\n{}", e, e.detail))
            })?;
        verify_installed(&set, &out.text())?;
        Ok(format!(
            "已安装 {} · {} 个 APK；versionCode={}，分包集合核对一致",
            set.package,
            set.members.len(),
            set.version_code
        ))
    }
}
pub(crate) fn compatible_member(member: &ApkMember, api: u32, abis: &str) -> Result<()> {
    if api < 21 || api < member.min_sdk {
        return Err(AppError::new("SDK_MISMATCH", "目标 API 不满足分包安装要求"));
    }
    if !member.abis.is_empty() && !member.abis.iter().any(|a| abis.split(',').any(|v| v == a)) {
        return Err(AppError::new(
            "ABI_MISMATCH",
            format!("{} 与设备 ABI 不兼容", member.file_name),
        ));
    }
    Ok(())
}
pub fn verify_installed(set: &ApkSet, text: &str) -> Result<()> {
    let version = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("versionCode="))
        .filter_map(|l| l.split_whitespace().next())
        .any(|v| v == set.version_code);
    let expected: BTreeSet<_> = std::iter::once("base")
        .chain(
            set.members
                .iter()
                .filter(|m| !m.split.is_empty())
                .map(|m| m.split.as_str()),
        )
        .collect();
    let splits = text
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("splits=[")
                .and_then(|s| s.strip_suffix(']'))
        })
        .any(|s| s.split(',').map(str::trim).collect::<BTreeSet<_>>() == expected);
    if !version || !splits {
        return Err(AppError::new(
            "RESULT_UNKNOWN",
            "设备版本或 split 集合未核对一致，请检查安装状态；不会自动重复安装",
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verification_tool_uses_numeric_build_tools_versions() {
        let temp = tempfile::tempdir().unwrap();
        for version in ["9.0.0", "35.0.0", "35.0.1"] {
            let dir = temp.path().join("build-tools").join(version).join("lib");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("apksigner.jar"), b"fixture").unwrap();
        }
        let settings = Settings {
            sdk_root: temp.path().to_string_lossy().into(),
            ..Default::default()
        };
        assert_eq!(
            tool(&settings).unwrap(),
            temp.path().join("build-tools/35.0.1/lib/apksigner.jar")
        );
    }
}
