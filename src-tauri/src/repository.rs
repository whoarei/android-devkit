use crate::{android, bootstrap, domain::*, process::Spec};
use std::{collections::BTreeMap, path::Path};

pub fn builtins() -> Vec<SdkSite> {
    [
        (
            "Android SDK",
            "https://dl.google.com/android/repository/repository2-3.xml",
        ),
        (
            "AOSP 系统镜像",
            "https://dl.google.com/android/repository/sys-img/android/sys-img2-3.xml",
        ),
        (
            "Google APIs 系统镜像",
            "https://dl.google.com/android/repository/sys-img/google_apis/sys-img2-3.xml",
        ),
        (
            "Google Play 系统镜像",
            "https://dl.google.com/android/repository/sys-img/google_apis_playstore/sys-img2-3.xml",
        ),
    ]
    .into_iter()
    .map(|(name, url)| SdkSite {
        name: name.into(),
        url: url.into(),
        enabled: true,
    })
    .collect()
}
pub fn validate(settings: &Settings) -> Result<()> {
    if settings.sdk_channel > 3 || settings.sdk_sites.len() > 32 {
        return Err(AppError::new(
            "INVALID_SDK_SITES",
            "更新通道须为 0～3，自定义站点最多 32 个",
        ));
    }
    let mut seen: Vec<_> = builtins().into_iter().map(|site| site.url).collect();
    for site in &settings.sdk_sites {
        let url = reqwest::Url::parse(&site.url)
            .map_err(|_| AppError::new("INVALID_SDK_SITE", "请输入有效的 HTTPS 仓库 XML 地址"))?;
        if site.name.trim().is_empty()
            || site.name.len() > 200
            || site.name.contains(['\r', '\n'])
            || site.url.contains(['\r', '\n'])
            || url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !url.path().ends_with(".xml")
        {
            return Err(AppError::new(
                "INVALID_SDK_SITE",
                "站点需包含名称和无凭据的 HTTPS 仓库 XML 地址",
            ));
        }
        if seen.iter().any(|existing| existing == url.as_str()) {
            return Err(AppError::new("DUPLICATE_SDK_SITE", "更新站点地址重复"));
        }
        seen.push(url.to_string());
    }
    Ok(())
}
pub fn package_enabled(package: &SdkPackage, settings: &Settings) -> bool {
    let channel = if package.channel == 0
        && (package.id.to_ascii_lowercase().contains("canary") || package.version.contains("-rc"))
    {
        3
    } else {
        package.channel
    };
    channel <= settings.sdk_channel
        && (package.source_url.is_empty()
            || builtins().iter().any(|site| site.url == package.source_url)
            || settings
                .sdk_sites
                .iter()
                .any(|site| site.enabled && site.url == package.source_url))
}
pub fn statuses(data: &Path, settings: &Settings) -> Vec<SdkSiteStatus> {
    let previous: Vec<SdkSiteStatus> = std::fs::read(data.join("sdk-site-status.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    builtins()
        .into_iter()
        .map(|site| (site, true))
        .chain(settings.sdk_sites.iter().cloned().map(|site| (site, false)))
        .map(|(site, builtin)| {
            let mut status = previous
                .iter()
                .find(|status| status.url == site.url)
                .cloned()
                .unwrap_or_default();
            status.name = site.name;
            status.url = site.url;
            status.enabled = site.enabled;
            status.builtin = builtin;
            status
        })
        .collect()
}
fn revision_key(value: &str) -> ([u32; 3], bool, u32) {
    let (release, preview) = value.split_once("-rc").unwrap_or((value, "0"));
    let mut parts = [0; 3];
    for (index, part) in release.split('.').take(3).enumerate() {
        parts[index] = part.parse().unwrap_or(0);
    }
    let preview = preview.parse::<u32>().unwrap_or(0);
    (parts, preview == 0, preview)
}
fn merge_site(
    packages: &mut BTreeMap<String, SdkPackage>,
    parsed: Vec<SdkPackage>,
    source: &str,
    builtin: bool,
) -> Result<u32> {
    if !builtin
        && parsed
            .iter()
            .any(|package| packages.contains_key(&package.id))
    {
        return Err(AppError::new(
            "SDK_SITE_CONFLICT",
            "该站点含与其他更新源重复的组件 ID，未加载以避免来源混淆",
        ));
    }
    let count = parsed.len() as u32;
    for mut package in parsed {
        package.source_url = source.into();
        let replace = packages.get(&package.id).is_none_or(|old| {
            revision_key(&package.version) > revision_key(&old.version)
                || (package.version == old.version && package.channel < old.channel)
        });
        if replace {
            packages.insert(package.id.clone(), package);
        }
    }
    Ok(count)
}
pub async fn fetch(settings: &Settings) -> Result<(Vec<SdkPackage>, Vec<SdkSiteStatus>)> {
    validate(settings)?;
    let client = bootstrap::client(settings)?;
    let mut packages = BTreeMap::<String, SdkPackage>::new();
    let mut statuses = Vec::new();
    for (site, builtin) in builtins()
        .into_iter()
        .map(|site| (site, true))
        .chain(settings.sdk_sites.iter().cloned().map(|site| (site, false)))
    {
        let mut status = SdkSiteStatus {
            name: site.name,
            url: site.url.clone(),
            enabled: site.enabled,
            builtin,
            ..Default::default()
        };
        if site.enabled {
            status.checked_at = now();
            let result = async {
                let url = reqwest::Url::parse(&site.url)
                    .map_err(|e| AppError::new("INVALID_SDK_SITE", e.to_string()))?;
                let mut response = client
                    .get(url.clone())
                    .timeout(std::time::Duration::from_secs(40))
                    .send()
                    .await
                    .map_err(|e| AppError::new("NETWORK", e.to_string()))?
                    .error_for_status()
                    .map_err(|e| AppError::new("NETWORK", e.to_string()))?;
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
                let xml = std::str::from_utf8(&bytes)
                    .map_err(|_| AppError::new("CATALOG_FORMAT", "仓库 XML 必须使用 UTF-8"))?;
                let base = url
                    .join(".")
                    .map_err(|e| AppError::new("CATALOG_FORMAT", e.to_string()))?;
                let mut parsed = android::parse_catalog(xml, base.as_str(), false)?;
                parsed.retain(|package| package.channel <= settings.sdk_channel);
                Ok(parsed)
            }
            .await;
            match result {
                Ok(parsed) => match merge_site(&mut packages, parsed, &site.url, builtin) {
                    Ok(count) => status.package_count = count,
                    Err(error) => status.error = error.message,
                },
                Err(error) => status.error = error.message,
            }
        }
        statuses.push(status);
    }
    Ok((packages.into_values().collect(), statuses))
}
fn property(value: &str) -> String {
    let mut result = String::new();
    for unit in value.encode_utf16() {
        match unit {
            0x20..=0x7e => {
                let ch = char::from_u32(unit as u32).unwrap();
                if matches!(ch, '\\' | ':' | '=') {
                    result.push('\\');
                }
                result.push(ch);
            }
            _ => result.push_str(&format!("\\u{unit:04x}")),
        }
    }
    result
}
pub fn configure_sdkmanager(
    data: &Path,
    settings: &Settings,
    package: Option<&SdkPackage>,
    spec: &mut Spec,
) -> Result<()> {
    let home = data.join("sdk-manager-home");
    std::fs::create_dir_all(&home)?;
    let sites: Vec<_> = settings
        .sdk_sites
        .iter()
        .filter(|site| {
            site.enabled && package.is_some_and(|package| package.source_url == site.url)
        })
        .collect();
    let mut text = format!(
        "# Android DevKit private SDK sources\ncount={}\n",
        sites.len()
    );
    for (index, site) in sites.iter().enumerate() {
        text.push_str(&format!(
            "src{index:02}={}\ndisp{index:02}={}\nenabled{index:02}=true\n",
            property(&site.url),
            property(&site.name)
        ));
    }
    std::fs::write(home.join("repositories.cfg"), text)?;
    spec.env
        .insert("ANDROID_USER_HOME".into(), home.to_string_lossy().into());
    if settings.sdk_channel > 0 {
        spec.args
            .push(format!("--channel={}", settings.sdk_channel));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn site_configuration_is_validated_and_older_settings_still_load() {
        let settings: Settings = serde_json::from_value(serde_json::json!({"sdkRoot":"E:\\sdk","avdRoots":[],"javaPath":"","proxy":"","bootTimeoutSecs":180})).unwrap();
        assert!(settings.sdk_sites.is_empty());
        assert_eq!(settings.sdk_channel, 0);
        for url in [
            "http://example.com/repo.xml",
            "https://user:secret@example.com/repo.xml",
            "https://example.com/",
            "https://example.com/repo.xml#fragment",
        ] {
            let mut settings = settings.clone();
            settings.sdk_sites.push(SdkSite {
                name: "Vendor".into(),
                url: url.into(),
                enabled: true,
            });
            assert!(validate(&settings).is_err(), "{url}");
        }
        let mut duplicate = settings.clone();
        duplicate.sdk_sites.push(builtins()[0].clone());
        assert_eq!(validate(&duplicate).unwrap_err().code, "DUPLICATE_SDK_SITE");
    }
    #[test]
    fn private_sources_select_only_the_confirmed_package_feed() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            sdk_channel: 3,
            sdk_sites: vec![
                SdkSite {
                    name: "设备厂商".into(),
                    url: "https://example.com/addon.xml".into(),
                    enabled: true,
                },
                SdkSite {
                    name: "Other".into(),
                    url: "https://other.example.com/addon.xml".into(),
                    enabled: true,
                },
            ],
            ..Default::default()
        };
        let package = SdkPackage {
            source_url: settings.sdk_sites[0].url.clone(),
            ..Default::default()
        };
        let mut spec = Spec::new("java", vec![]);
        configure_sdkmanager(dir.path(), &settings, Some(&package), &mut spec).unwrap();
        let home = std::path::PathBuf::from(&spec.env["ANDROID_USER_HOME"]);
        assert!(home.starts_with(dir.path()));
        let text = std::fs::read_to_string(home.join("repositories.cfg")).unwrap();
        assert!(text.contains("count=1"));
        assert!(text.contains("\\u8bbe\\u5907"));
        assert!(!text.contains("other.example.com"));
        assert!(spec.args.contains(&"--channel=3".into()));
        settings.sdk_sites[0].enabled = false;
        assert!(!package_enabled(&package, &settings));
        configure_sdkmanager(dir.path(), &settings, Some(&package), &mut spec).unwrap();
        assert!(std::fs::read_to_string(home.join("repositories.cfg"))
            .unwrap()
            .contains("count=0"));
    }
    #[test]
    fn source_collisions_cannot_override_reviewed_components() {
        let mut packages = BTreeMap::new();
        let package = |version: &str, channel: u8| SdkPackage {
            id: "emulator".into(),
            version: version.into(),
            channel,
            ..Default::default()
        };
        merge_site(
            &mut packages,
            vec![package("35.0.0", 0), package("35.0.0-rc2", 3)],
            "official",
            true,
        )
        .unwrap();
        assert_eq!(packages["emulator"].version, "35.0.0");
        assert_eq!(
            merge_site(&mut packages, vec![package("99.0.0", 0)], "vendor", false)
                .unwrap_err()
                .code,
            "SDK_SITE_CONFLICT"
        );
        assert_eq!(packages["emulator"].source_url, "official");
        let preview = package("36.0.0-rc1", 3);
        assert!(!package_enabled(&preview, &Settings::default()));
    }
}
