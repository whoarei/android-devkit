//! Import APK archives into the existing immutable, signed APK-set workflow.
use crate::{
    app::App,
    domain::*,
    phone_apk::{self, ApkSet},
    process::{self, Spec},
};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
};

const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_APK: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 10000;

fn safe_archive_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 4096
        && !name.starts_with('/')
        && !name.contains(['\\', ':', '\0', '\r', '\n', '<', '>', '"', '|', '?', '*'])
        && name.trim_end_matches('/').split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || stem
                    .strip_prefix("COM")
                    .or_else(|| stem.strip_prefix("LPT"))
                    .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'));
            !part.is_empty()
                && !matches!(part, "." | "..")
                && !reserved
                && !part.ends_with(['.', ' '])
                && !part.chars().any(|c| c < ' ')
        })
}

#[derive(Debug)]
struct Inventory {
    apk_indices: Vec<usize>,
    bundletool: bool,
    unpacked: u64,
}

fn inventory(archive: &Path) -> Result<Inventory> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)
        .map_err(|_| AppError::new("APK_CONTAINER_INVALID", "安装包不是可读取的 ZIP 容器"))?;
    if zip.is_empty() || zip.len() > MAX_ENTRIES {
        return Err(AppError::new("APK_CONTAINER_LIMIT", "安装包条目数量超限"));
    }
    let mut result = Inventory {
        apk_indices: vec![],
        bundletool: false,
        unpacked: 0,
    };
    let mut names = BTreeSet::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| {
            AppError::new("APK_CONTAINER_INVALID", "安装包包含加密或无法读取的条目")
        })?;
        let name = entry.name().to_string();
        if !safe_archive_name(&name)
            || !names.insert(name.to_lowercase())
            || entry.unix_mode().unwrap_or(0) & 0o170000 == 0o120000
        {
            return Err(AppError::new(
                "APK_CONTAINER_PATH",
                "安装包包含越界、重复或链接路径",
            ));
        }
        result.unpacked = result
            .unpacked
            .checked_add(entry.size())
            .ok_or_else(|| AppError::new("APK_CONTAINER_LIMIT", "安装包解压体积超限"))?;
        if result.unpacked > MAX_BYTES || (entry.size() > MAX_APK && !entry.is_dir()) {
            return Err(AppError::new(
                "APK_CONTAINER_LIMIT",
                "安装包解压总量最多 4 GiB，单文件最多 1 GiB",
            ));
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".obb") || lower.starts_with("android/obb/") {
            return Err(AppError::new(
                "APK_CONTAINER_OBB",
                "安装包包含 OBB 扩展数据，当前仅支持 APK 集合，请勿忽略扩展数据安装",
            ));
        }
        let expected = entry.size();
        if lower == "manifest.json" && expected <= 1024 * 1024 {
            let mut bytes = Vec::new();
            (&mut entry).take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 != expected {
                return Err(AppError::new(
                    "APK_CONTAINER_INVALID",
                    "安装包条目长度与清单不一致",
                ));
            }
            if let Ok(meta) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if meta
                    .get("expansions")
                    .and_then(|v| v.as_array())
                    .is_some_and(|v| !v.is_empty())
                {
                    return Err(AppError::new(
                        "APK_CONTAINER_OBB",
                        "安装包声明 OBB 扩展数据，当前仅支持 APK 集合",
                    ));
                }
            }
        } else if !entry.is_dir() {
            // Validate actual inflation and CRC before Java sees the archive.
            // Central-directory sizes alone do not bound a malicious deflate stream.
            let actual = std::io::copy(
                &mut (&mut entry).take(expected.saturating_add(1)),
                &mut std::io::sink(),
            )?;
            if actual != expected {
                return Err(AppError::new(
                    "APK_CONTAINER_INVALID",
                    "安装包条目长度与清单不一致",
                ));
            }
        }
        if lower == "toc.pb" {
            result.bundletool = true;
        }
        if lower.ends_with(".apk") && !entry.is_dir() {
            result.apk_indices.push(index);
        }
    }
    if result.apk_indices.is_empty() {
        return Err(AppError::new(
            "APK_CONTAINER_EMPTY",
            "安装包中没有可安装的 APK",
        ));
    }
    Ok(result)
}

fn extract_apks(archive: &Path, output: &Path, indices: &[usize]) -> Result<Vec<PathBuf>> {
    if indices.len() > 256 {
        return Err(AppError::new(
            "APK_CONTAINER_LIMIT",
            "普通 APK 集合最多包含 256 个 APK",
        ));
    }
    std::fs::create_dir_all(output)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)
        .map_err(|_| AppError::new("APK_CONTAINER_INVALID", "安装包已变化"))?;
    let mut paths = Vec::new();
    for (number, index) in indices.iter().enumerate() {
        let mut entry = zip
            .by_index(*index)
            .map_err(|_| AppError::new("APK_CONTAINER_INVALID", "无法读取 APK 条目"))?;
        let expected = entry.size();
        let path = output.join(format!("{number}.apk"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let written = std::io::copy(
            &mut (&mut entry).take(expected.saturating_add(1)),
            &mut file,
        )?;
        if written != expected {
            return Err(AppError::new(
                "APK_CONTAINER_INVALID",
                "APK 条目长度与清单不一致",
            ));
        }
        file.flush()?;
        file.sync_all()?;
        paths.push(path);
    }
    Ok(paths)
}

fn extracted_apks(root: &Path) -> Result<Vec<PathBuf>> {
    fn walk(
        root: &Path,
        dir: &Path,
        paths: &mut Vec<PathBuf>,
        bytes: &mut u64,
        entries: &mut usize,
    ) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            *entries += 1;
            if *entries > MAX_ENTRIES {
                return Err(AppError::new("APK_CONTAINER_LIMIT", "选包结果数量超限"));
            }
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_symlink() || !path.canonicalize()?.starts_with(root.canonicalize()?) {
                return Err(AppError::new(
                    "APK_CONTAINER_PATH",
                    "选包结果包含链接或越界路径",
                ));
            }
            if kind.is_dir() {
                walk(root, &path, paths, bytes, entries)?;
            } else {
                crate::phone_files::regular_local(&path, false)?;
                if !path
                    .extension()
                    .is_some_and(|v| v.eq_ignore_ascii_case("apk"))
                {
                    continue;
                }
                let size = path.metadata()?.len();
                *bytes = bytes.saturating_add(size);
                if size > MAX_APK || *bytes > MAX_BYTES || paths.len() >= 256 {
                    return Err(AppError::new(
                        "APK_CONTAINER_LIMIT",
                        "选包结果体积或数量超限",
                    ));
                }
                paths.push(path);
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    walk(root, root, &mut paths, &mut 0, &mut 0)?;
    paths.sort();
    if paths.is_empty() {
        return Err(AppError::new(
            "APK_CONTAINER_EMPTY",
            "没有与目标设备匹配的 APK",
        ));
    }
    Ok(paths)
}

fn native_selection(parsed: &[(bool, Vec<String>)], supported: &str) -> Result<BTreeSet<usize>> {
    let preferred = supported
        .split(',')
        .find(|abi| parsed.iter().any(|(_, abis)| abis.iter().any(|v| v == abi)));
    if preferred.is_none() && parsed.iter().any(|(_, abis)| !abis.is_empty()) {
        return Err(AppError::new(
            "ABI_MISMATCH",
            "安装包没有与目标设备 ABI 匹配的 APK",
        ));
    }
    let mut result = BTreeSet::new();
    for (index, (configuration, abis)) in parsed.iter().enumerate() {
        if abis.is_empty() || preferred.is_some_and(|abi| abis.iter().any(|v| v == abi)) {
            result.insert(index);
        } else if !*configuration {
            return Err(AppError::new(
                "ABI_MISMATCH",
                "安装包中的应用或功能模块与目标设备 ABI 不兼容",
            ));
        }
    }
    if result.is_empty() {
        return Err(AppError::new(
            "ABI_MISMATCH",
            "安装包没有与目标设备 ABI 匹配的 APK",
        ));
    }
    Ok(result)
}

fn select_native_abi(paths: Vec<PathBuf>, supported: &str) -> Result<Vec<PathBuf>> {
    let mut parsed = Vec::new();
    for path in &paths {
        let (attrs, abis) = phone_apk::metadata(path)?;
        let split = attrs.get("manifest:split").cloned().unwrap_or_default();
        let configuration =
            split.starts_with("config.") || attrs.contains_key("manifest:configForSplit");
        parsed.push((configuration, abis));
    }
    let selected = native_selection(&parsed, supported)?;
    Ok(paths
        .into_iter()
        .enumerate()
        .filter_map(|(index, path)| selected.contains(&index).then_some(path))
        .collect())
}

impl App {
    async fn container_device_spec(
        &self,
        settings: &Settings,
        target: &Target,
    ) -> Result<serde_json::Value> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        let devices = crate::android::devices(settings, epoch).await;
        let device = checked_target(target, &devices, epoch)?;
        let sdk = device
            .api
            .parse::<u32>()
            .map_err(|_| AppError::new("DEVICE_API", "无法读取目标设备 API"))?;
        let output = self.remote_shell(settings, target,
            "getprop ro.product.cpu.abilist; printf '\\nDEVKIT_LOCALES\\n'; getprop persist.sys.locales; getprop persist.sys.locale; getprop ro.product.locale; printf '\\nDEVKIT_DENSITY\\n'; wm density".into(), 15).await?.text().replace("\r\n", "\n");
        let (abis, rest) = output
            .split_once("DEVKIT_LOCALES\n")
            .ok_or_else(|| AppError::new("DEVICE_SPEC", "无法读取设备配置"))?;
        let (locales, density) = rest
            .split_once("DEVKIT_DENSITY\n")
            .ok_or_else(|| AppError::new("DEVICE_SPEC", "无法读取设备配置"))?;
        let supported_abis: Vec<_> = abis
            .trim()
            .split(',')
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect();
        let locales: Vec<_> = locales
            .lines()
            .find(|v| !v.trim().is_empty())
            .unwrap_or("en-US")
            .split(',')
            .map(|s| s.trim().replace('_', "-"))
            .collect();
        let density = density
            .lines()
            .filter_map(|l| {
                l.rsplit_once(':')
                    .and_then(|(_, v)| v.trim().parse::<u32>().ok())
            })
            .next_back()
            .filter(|v| *v > 0)
            .ok_or_else(|| AppError::new("DEVICE_SPEC", "无法读取设备屏幕密度"))?;
        if supported_abis.is_empty() {
            return Err(AppError::new("DEVICE_SPEC", "无法读取设备 ABI"));
        }
        Ok(
            serde_json::json!({"supportedAbis": supported_abis, "supportedLocales": locales, "screenDensity": density, "sdkVersion": sdk}),
        )
    }

    pub async fn prepare_apk_container(&self, source: &Path, target: &Target) -> Result<ApkSet> {
        let extension = source
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "apks" | "xapk" | "apkm") {
            return Err(AppError::new(
                "APK_CONTAINER_TYPE",
                "请选择 APKS、XAPK 或 APKM 安装包；AAB 需先生成 APK 集合",
            ));
        }
        crate::phone_files::regular_local(source, false)?;
        let size = source.metadata()?.len();
        if size == 0 || size > MAX_BYTES {
            return Err(AppError::new(
                "APK_CONTAINER_LIMIT",
                "安装包应为不超过 4 GiB 的普通文件",
            ));
        }
        let settings = self.store.settings();
        let staging = self
            .data
            .join("container-imports")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&staging)?;
        let result = async {
            crate::capacity::budget(&[(staging.clone(), size.saturating_mul(3), "安装包导入")])?;
            let archive = staging.join("source.apks");
            crate::phone_files::stage_file(source, &archive, size)?;
            let inventory = inventory(&archive)?;
            crate::capacity::budget(&[(
                staging.clone(),
                inventory.unpacked.saturating_mul(2),
                "安装包解压与校验",
            )])?;
            let spec = self.container_device_spec(&settings, target).await?;
            let output = staging.join("selected");
            let paths = if inventory.bundletool {
                let jar = crate::device_tools::path(self, "bundletool")?;
                crate::java::check(&settings, &settings.java_path, "configured")
                    .await
                    .require()?;
                let spec_file = staging.join("device.json");
                std::fs::write(&spec_file, serde_json::to_vec(&spec)?)?;
                let mut command = Spec::new(
                    &settings.java_path,
                    vec![
                        "-jar".into(),
                        jar.to_string_lossy().into(),
                        "extract-apks".into(),
                        format!("--apks={}", archive.display()),
                        format!("--output-dir={}", output.display()),
                        format!("--device-spec={}", spec_file.display()),
                    ],
                );
                command.timeout = 180;
                process::run(command).await?.success().map_err(|error| {
                    AppError::new("APK_CONTAINER_SELECTION", "官方选包工具未能选出可安装集合")
                        .detail(error.detail)
                })?;
                extracted_apks(&output)?
            } else {
                let extracted = extract_apks(&archive, &output, &inventory.apk_indices)?;
                let abis = spec["supportedAbis"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                select_native_abi(extracted, &abis)?
            };
            let mut set = self.prepare_apk_set(paths).await?;
            let directory = phone_apk::set_dir(self, &set.id)?;
            let compatibility = set.members.iter().try_for_each(|member| {
                let abis = spec["supportedAbis"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                phone_apk::compatible_member(
                    member,
                    spec["sdkVersion"].as_u64().unwrap_or(0) as u32,
                    &abis,
                )
            });
            if let Err(error) = compatibility {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(error);
            }
            // A selected set must never be reused on a different transport, even if compatible.
            set.target = Some(target.clone());
            let epoch = self.epoch.load(Ordering::SeqCst);
            let devices = crate::android::devices(&settings, epoch).await;
            if let Err(error) = checked_target(target, &devices, epoch) {
                let _ = std::fs::remove_dir_all(phone_apk::set_dir(self, &set.id)?);
                return Err(error);
            }
            if let Err(error) =
                std::fs::write(directory.join("set.json"), serde_json::to_vec(&set)?)
            {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(error.into());
            }
            Ok(set)
        }
        .await;
        let _ = std::fs::remove_dir_all(staging);
        result
    }
}

#[tauri::command]
pub async fn apk_container_prepare(
    app: tauri::State<'_, Arc<App>>,
    path: String,
    target: Target,
) -> Result<ApkSet> {
    let _guard = app.allow_update_sensitive_work()?;
    if app.draining.load(Ordering::SeqCst) {
        return Err(AppError::new(
            "APP_CLOSING",
            "应用正在退出，不能导入新安装包",
        ));
    }
    app.prepare_apk_container(Path::new(&path), &target).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    #[test]
    fn container_paths_and_obb_are_not_silently_accepted() {
        for name in [
            "../base.apk",
            "/base.apk",
            "a/../b.apk",
            "a\\base.apk",
            "C:/base.apk",
            "a:ads.apk",
            "a//b.apk",
            "a./b.apk",
            "CON.apk",
            "a/LPT1.apk",
            "a/<bad>.apk",
        ] {
            assert!(!safe_archive_name(name), "{name}");
        }
        for name in ["base.apk", "splits/config.arm64_v8a.apk", "assets/中文.txt"] {
            assert!(safe_archive_name(name), "{name}");
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("input.xapk");
        for entries in [
            vec![
                ("base.apk", b"apk".as_slice()),
                ("Android/obb/main.obb", b"data"),
            ],
            vec![
                ("base.apk", b"apk".as_slice()),
                ("manifest.json", br#"{"expansions":[{"file":"main.obb"}]}"#),
            ],
        ] {
            archive(&path, &entries);
            assert_eq!(inventory(&path).unwrap_err().code, "APK_CONTAINER_OBB");
        }
        archive(&path, &[("base.apk", b"a"), ("BASE.APK", b"b")]);
        assert_eq!(inventory(&path).unwrap_err().code, "APK_CONTAINER_PATH");
        archive(&path, &[("../base.apk", b"a")]);
        assert_eq!(inventory(&path).unwrap_err().code, "APK_CONTAINER_PATH");
    }
    #[test]
    fn raw_collections_extract_to_owned_flat_names_and_detect_bundletool() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("input.apks");
        archive(
            &source,
            &[
                ("nested/base.apk", b"base"),
                ("nested/config.zh.apk", b"split"),
                ("info.json", b"{}"),
            ],
        );
        let inventory = inventory(&source).unwrap();
        assert!(!inventory.bundletool);
        let paths =
            extract_apks(&source, &temp.path().join("out"), &inventory.apk_indices).unwrap();
        assert_eq!(std::fs::read(&paths[0]).unwrap(), b"base");
        assert_eq!(std::fs::read(&paths[1]).unwrap(), b"split");
        archive(
            &source,
            &[("toc.pb", b"protobuf"), ("splits/base.apk", b"base")],
        );
        assert!(super::inventory(&source).unwrap().bundletool);
        archive(&source, &[("info.json", b"{}")]);
        assert_eq!(
            super::inventory(&source).unwrap_err().code,
            "APK_CONTAINER_EMPTY"
        );
    }
    #[test]
    fn target_bound_sets_reject_reconnection_and_other_devices() {
        let target = Target {
            serial: "one".into(),
            transport_id: "1".into(),
            server_epoch: 2,
        };
        let mut set = ApkSet {
            id: uuid::Uuid::new_v4().to_string(),
            package: "dev.example.app".into(),
            version: "1".into(),
            version_code: "1".into(),
            signers: vec![],
            members: vec![],
            target: Some(target.clone()),
        };
        phone_apk::check_target(&set, &target).unwrap();
        let mut other = target.clone();
        other.transport_id = "2".into();
        assert_eq!(
            phone_apk::check_target(&set, &other).unwrap_err().code,
            "APK_CONTAINER_TARGET_CHANGED"
        );
        other = target.clone();
        other.server_epoch += 1;
        assert!(phone_apk::check_target(&set, &other).is_err());
        set.target = None;
        phone_apk::check_target(&set, &other).unwrap();
    }
    #[test]
    fn native_architecture_uses_device_preference_and_never_drops_feature_code() {
        let members = vec![
            (false, vec![]),
            (true, vec!["armeabi-v7a".into()]),
            (true, vec!["arm64-v8a".into()]),
            (true, vec![]),
        ];
        assert_eq!(
            native_selection(&members, "arm64-v8a,armeabi-v7a").unwrap(),
            BTreeSet::from([0, 2, 3])
        );
        assert_eq!(
            native_selection(&members, "armeabi-v7a").unwrap(),
            BTreeSet::from([0, 1, 3])
        );
        assert!(native_selection(&members, "x86_64").is_err());
        let feature = vec![(false, vec![]), (false, vec!["arm64-v8a".into()])];
        assert_eq!(
            native_selection(&feature, "x86_64").unwrap_err().code,
            "ABI_MISMATCH"
        );
        let base = vec![(false, vec!["arm64-v8a".into()])];
        assert!(native_selection(&base, "x86_64").is_err());
    }
}
