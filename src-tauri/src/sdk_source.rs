use crate::{android, bootstrap, domain::*};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

pub const GOOGLE: &str = "https://dl.google.com/android/repository/";
pub const TENCENT: &str = "https://mirrors.cloud.tencent.com/AndroidSDK/";

pub fn normalize(source: &SdkDownloadSource) -> Result<SdkDownloadSource> {
    let mut value = source.clone();
    match value.kind.as_str() {
        "google" | "tencent" => {}
        "custom" => {
            value.name = value.name.trim().to_owned();
            if value.name.is_empty() || value.name.len() > 200 || value.name.contains(['\r', '\n'])
            {
                return Err(AppError::new(
                    "INVALID_SDK_SOURCE",
                    "请输入不超过 200 字节的下载源名称",
                ));
            }
            let mut url = reqwest::Url::parse(value.url.trim()).map_err(|_| {
                AppError::new("INVALID_SDK_SOURCE", "请输入有效的 HTTPS 仓库根地址")
            })?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url
                    .path()
                    .trim_end_matches('/')
                    .to_ascii_lowercase()
                    .ends_with(".xml")
                || value.url.chars().any(char::is_whitespace)
                || value.url.contains('%')
                || url.path().contains('%')
            {
                return Err(AppError::new(
                    "INVALID_SDK_SOURCE",
                    "下载源需为无凭据、查询参数或片段的 HTTPS 根目录，不能填写 XML 文件",
                ));
            }
            if !url.path().ends_with('/') {
                url.set_path(&format!("{}/", url.path()));
            }
            value.url = url.into();
        }
        _ => {
            return Err(AppError::new(
                "INVALID_SDK_SOURCE",
                "请选择 Google 官方、腾讯云镜像或自定义源",
            ))
        }
    }
    Ok(value)
}
pub fn base(settings: &Settings) -> Result<String> {
    let source = normalize(&settings.sdk_download)?;
    Ok(match source.kind.as_str() {
        "google" => GOOGLE.into(),
        "tencent" => TENCENT.into(),
        _ => source.url,
    })
}
pub fn name(settings: &Settings) -> String {
    match settings.sdk_download.kind.as_str() {
        "google" => "Google 官方".into(),
        "tencent" => "腾讯云镜像".into(),
        _ => settings.sdk_download.name.clone(),
    }
}
pub fn sites(settings: &Settings) -> Result<Vec<SdkSite>> {
    let base = base(settings)?;
    Ok(crate::repository::builtins()
        .into_iter()
        .map(|mut site| {
            site.url = site.url.replacen(GOOGLE, &base, 1);
            site
        })
        .collect())
}
pub fn canonical_url(settings: &Settings, url: &str) -> Option<String> {
    let base = base(settings).ok()?;
    url.strip_prefix(&base)
        .map(|path| format!("{GOOGLE}{path}"))
        .or_else(|| url.starts_with(GOOGLE).then(|| url.to_owned()))
}
pub fn builtin(package: &SdkPackage, settings: &Settings) -> bool {
    crate::repository::builtins()
        .iter()
        .any(|site| site.url == package.source_url)
        || sites(settings)
            .is_ok_and(|sites| sites.iter().any(|site| site.url == package.source_url))
}
pub fn map_archive(settings: &Settings, url: &str) -> Result<String> {
    let base = base(settings)?;
    let mapped = url
        .strip_prefix(GOOGLE)
        .map(|path| format!("{base}{path}"))
        .unwrap_or_else(|| url.into());
    let parsed = reqwest::Url::parse(&mapped)
        .map_err(|_| AppError::new("UNTRUSTED_SOURCE", "无效的安装包地址"))?;
    if !parsed.as_str().starts_with(&base)
        || parsed.scheme() != "https"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path().contains('%')
    {
        return Err(AppError::new(
            "UNTRUSTED_SOURCE",
            "安装包地址不在选定下载源的仓库目录内",
        ));
    }
    Ok(mapped)
}
pub fn metadata_path(data: &Path, key: &str) -> Result<PathBuf> {
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::new(
            "CATALOG_UNAVAILABLE",
            "请刷新目录以取得可安装的仓库元数据",
        ));
    }
    Ok(data.join("repository-metadata").join(format!("{key}.xml")))
}
pub async fn fetch_xml(settings: &Settings, url: &str, seconds: u64) -> Result<String> {
    let mut response = bootstrap::client(settings)?
        .get(url)
        .timeout(Duration::from_secs(seconds))
        .send()
        .await
        .map_err(|e| AppError::new("NETWORK", e.to_string()))?
        .error_for_status()
        .map_err(|e| AppError::new("NETWORK", e.to_string()))?;
    if url.starts_with(GOOGLE) && response.url().host_str() != Some("dl.google.com") {
        return Err(AppError::new(
            "UNTRUSTED_SOURCE",
            "官方校验记录请求重定向至其他站点",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::new("NETWORK", e.to_string()))?
    {
        if bytes.len() + chunk.len() > 24 * 1024 * 1024 {
            return Err(AppError::new(
                "CATALOG_TOO_LARGE",
                "仓库目录超出 24 MiB 上限",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| AppError::new("CATALOG_FORMAT", "仓库 XML 必须使用 UTF-8"))
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct OfficialRecord {
    checksum: String,
    checksum_type: String,
    size: u64,
    checked_at: u64,
}
fn record_key(package: &SdkPackage, canonical: &str) -> String {
    digest(format!("{}\n{}\n{canonical}", package.id, package.version).as_bytes())
}
fn records(data: &Path) -> BTreeMap<String, OfficialRecord> {
    std::fs::read(data.join("official-checksums.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}
fn add_records(records: &mut BTreeMap<String, OfficialRecord>, packages: &[SdkPackage]) {
    for package in packages {
        if package.url.starts_with(GOOGLE) && !package.checksum.is_empty() {
            records.insert(
                record_key(package, &package.url),
                OfficialRecord {
                    checksum: package.checksum.clone(),
                    checksum_type: package.checksum_type.clone(),
                    size: package.size,
                    checked_at: now(),
                },
            );
        }
    }
}
pub async fn official_records(
    data: &Path,
    settings: &Settings,
    packages: &mut [SdkPackage],
) -> Result<()> {
    let mut saved = records(data);
    if settings.sdk_download.kind == "google" {
        let official: Vec<_> = packages
            .iter()
            .filter(|p| builtin(p, settings))
            .cloned()
            .collect();
        add_records(&mut saved, &official);
    } else if settings.sdk_download.verify_official {
        let mut jobs = tokio::task::JoinSet::new();
        for site in crate::repository::builtins() {
            let settings = settings.clone();
            jobs.spawn(async move {
                let xml = fetch_xml(&settings, &site.url, 8).await?;
                let base = reqwest::Url::parse(&site.url).unwrap().join(".").unwrap();
                android::parse_catalog(&xml, base.as_str(), false)
            });
        }
        while let Some(Ok(result)) = jobs.join_next().await {
            if let Ok(found) = result {
                add_records(&mut saved, &found);
            }
        }
    }
    std::fs::create_dir_all(data)?;
    let temp = data.join(format!(".official-checksums-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temp, serde_json::to_vec(&saved)?)?;
    // The catalog gate serializes writers. Rename replaces the complete snapshot.
    std::fs::rename(&temp, data.join("official-checksums.json"))?;
    for package in packages {
        if !builtin(package, settings) {
            continue;
        }
        if let Some(record) = canonical_url(settings, &package.url)
            .and_then(|url| saved.get(&record_key(package, &url)))
        {
            package.official_checked_at = record.checked_at;
            package.official_checksum = record.checksum.clone();
            package.official_status = if record.checksum.eq_ignore_ascii_case(&package.checksum)
                && record.checksum_type == package.checksum_type
                && record.size == package.size
            {
                "ready"
            } else {
                "mismatch"
            }
            .into();
        } else {
            package.official_status = "missing".into();
        }
    }
    Ok(())
}
pub fn verify_package(settings: &Settings, package: &SdkPackage) -> Result<()> {
    if settings.sdk_download.verify_official
        && builtin(package, settings)
        && (package.official_status != "ready"
            || !package
                .official_checksum
                .eq_ignore_ascii_case(&package.checksum))
    {
        return Err(AppError::new(
            "OFFICIAL_CHECKSUM_REQUIRED",
            format!(
                "{} {} 缺少匹配的官方校验记录；请刷新目录，或在下载源设置中关闭官方校验",
                package.id, package.version
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn active_sdk_tasks_block_source_changes_before_any_network_request() {
        let root = tempfile::tempdir().unwrap();
        let app = crate::app::App::new(root.path().to_path_buf()).unwrap();
        let mut task = crate::regression_tests::task("sdk-source-guard", "queued");
        task.operation.kind = "sdk_install".into();
        app.store.save_task(&task).unwrap();
        let source = SdkDownloadSource {
            kind: "tencent".into(),
            ..Default::default()
        };
        assert_eq!(
            app.apply_sdk_source(source).await.unwrap_err().code,
            "TASKS_ACTIVE"
        );
        assert_eq!(app.store.settings().sdk_download.kind, "google");
    }
    #[test]
    fn sources_map_every_index_and_archive_and_preserve_custom_boundaries() {
        let mut settings = Settings::default();
        settings.sdk_download.kind = "tencent".into();
        assert!(sites(&settings)
            .unwrap()
            .iter()
            .all(|site| site.url.starts_with(TENCENT)));
        assert_eq!(
            map_archive(&settings, &format!("{GOOGLE}sys-img/google_apis/x.zip")).unwrap(),
            format!("{TENCENT}sys-img/google_apis/x.zip")
        );
        assert!(map_archive(&settings, "https://evil.example/x.zip").is_err());
        settings.sdk_download = SdkDownloadSource {
            kind: "custom".into(),
            name: "团队".into(),
            url: "https://sdk.example/a".into(),
            verify_official: true,
        };
        assert_eq!(base(&settings).unwrap(), "https://sdk.example/a/");
        for url in [
            "http://sdk.example/",
            "https://u:p@sdk.example/",
            "https://sdk.example/x.xml",
            "https://sdk.example/?x=1",
            "https://sdk.example/#x",
            "https://sdk.example/%2e%2e/",
        ] {
            settings.sdk_download.url = url.into();
            assert!(normalize(&settings.sdk_download).is_err(), "{url}");
        }
    }
    #[test]
    fn official_verification_can_be_disabled_but_never_mislabeled() {
        let mut settings = Settings::default();
        let package = SdkPackage {
            source_url: crate::repository::builtins()[0].url.clone(),
            official_status: "missing".into(),
            ..Default::default()
        };
        assert!(verify_package(&settings, &package).is_err());
        settings.sdk_download.verify_official = false;
        assert!(verify_package(&settings, &package).is_ok());
        assert_eq!(package.official_status, "missing");
    }
    #[tokio::test]
    async fn mirror_records_never_become_official_and_cached_records_keep_their_identity() {
        let data = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let mut package = SdkPackage {
            id: "platform-tools".into(),
            version: "1.0.0".into(),
            url: format!("{GOOGLE}fixture.zip"),
            source_url: crate::repository::builtins()[0].url.clone(),
            checksum: "a".repeat(40),
            checksum_type: "sha1".into(),
            size: 42,
            ..Default::default()
        };
        official_records(data.path(), &settings, std::slice::from_mut(&mut package))
            .await
            .unwrap();
        assert_eq!(package.official_status, "ready");
        settings.sdk_download.kind = "tencent".into();
        settings.sdk_download.verify_official = false;
        package.url = map_archive(&settings, &package.url).unwrap();
        package.source_url = sites(&settings).unwrap()[0].url.clone();
        official_records(data.path(), &settings, std::slice::from_mut(&mut package))
            .await
            .unwrap();
        assert_eq!(package.official_status, "ready");
        package.checksum = "b".repeat(40);
        official_records(data.path(), &settings, std::slice::from_mut(&mut package))
            .await
            .unwrap();
        assert_eq!(package.official_status, "mismatch");
        assert_eq!(package.official_checksum, "a".repeat(40));
        package.version = "2.0.0".into();
        package.official_checksum.clear();
        package.official_checked_at = 0;
        official_records(data.path(), &settings, std::slice::from_mut(&mut package))
            .await
            .unwrap();
        assert_eq!(package.official_status, "missing");
        assert!(package.official_checksum.is_empty());
        settings.sdk_download.verify_official = true;
        assert!(verify_package(&settings, &package).is_err());
    }
}
