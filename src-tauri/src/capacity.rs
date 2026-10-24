use crate::domain::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    pub files: u32,
    pub complete: bool,
}

fn linked(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// Bounded, read-only, and never follows junctions or external storage images.
pub fn usage(root: &Path) -> Usage {
    let mut result = Usage {
        complete: true,
        ..Default::default()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut pending = vec![root.to_path_buf()];
    let mut visited = 0;
    while let Some(path) = pending.pop() {
        visited += 1;
        if visited > 50_000 || std::time::Instant::now() > deadline {
            result.complete = false;
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            result.complete = false;
            continue;
        };
        if linked(&meta) {
            result.complete = false;
            continue;
        }
        if meta.is_file() {
            result.bytes = result.bytes.saturating_add(allocated(&path, &meta));
            result.files += 1;
        } else if meta.is_dir() {
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => pending.push(entry.path()),
                            Err(_) => result.complete = false,
                        }
                        if pending.len() > 50_000 {
                            result.complete = false;
                            break;
                        }
                    }
                }
                Err(_) => result.complete = false,
            }
        }
    }
    result
}

fn allocated(path: &Path, meta: &std::fs::Metadata) -> u64 {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetCompressedFileSizeW;
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut high = 0;
        let low = unsafe { GetCompressedFileSizeW(name.as_ptr(), &mut high) };
        if low != u32::MAX {
            return ((high as u64) << 32) | low as u64;
        }
    }
    let _ = path;
    meta.len()
}

fn volume(path: &Path) -> PathBuf {
    let existing = path
        .ancestors()
        .find(|parent| parent.is_dir())
        .unwrap_or(path);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW;
        let name: Vec<u16> = existing.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut buffer = vec![0u16; 32768];
        if unsafe { GetVolumePathNameW(name.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) }
            != 0
        {
            let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
            return PathBuf::from(
                std::ffi::OsString::from_wide(&buffer[..len])
                    .to_string_lossy()
                    .to_lowercase(),
            );
        }
    }
    existing
        .ancestors()
        .last()
        .unwrap_or(existing)
        .to_path_buf()
}

/// Estimates are explicit; sum requirements sharing a volume before checking it.
pub fn budget(entries: &[(PathBuf, u64, &str)]) -> Result<Vec<String>> {
    let mut volumes = std::collections::BTreeMap::<PathBuf, (u64, u64)>::new();
    let mut effects =
        vec!["空间为保守估算；SDK 解压占用依上游内容变化，稀疏数据分区可能逐步增长。".into()];
    for (path, bytes, label) in entries {
        let available = crate::bootstrap::free_space(path);
        effects.push(format!(
            "{label}：{} · 预计需要 {:.1} MiB · 可用 {:.1} MiB",
            path.display(),
            *bytes as f64 / 1048576.0,
            available as f64 / 1048576.0
        ));
        let item = volumes.entry(volume(path)).or_insert((0, available));
        item.0 = item.0.saturating_add(*bytes);
        item.1 = item.1.min(available);
    }
    for (path, (needed, available)) in volumes {
        if needed > available {
            return Err(AppError::new(
                "DISK_SPACE",
                format!(
                    "卷 {} 空间不足：预计需要 {:.1} MiB，可用 {:.1} MiB",
                    path.display(),
                    needed as f64 / 1048576.0,
                    available as f64 / 1048576.0
                ),
            ));
        }
    }
    Ok(effects)
}

pub fn operation_budget(data: &Path, settings: &Settings, op: &Operation) -> Result<Vec<String>> {
    if !matches!(op.kind.as_str(), "sdk_install" | "bootstrap" | "avd_create") {
        return Ok(vec![]);
    }
    let packages: Vec<SdkPackage> =
        serde_json::from_str(op.optional("downloadPackages")).unwrap_or_default();
    let installed = crate::android::installed(settings);
    let needed: Vec<_> = packages
        .iter()
        .filter(|package| {
            !installed
                .iter()
                .any(|local| local.id == package.id && local.version == package.version)
        })
        .collect();
    let download = needed
        .iter()
        .fold(0u64, |total, package| total.saturating_add(package.size));
    let mut entries = vec![];
    if download > 0 {
        entries.push((data.join("cache/downloads"), download, "下载缓存"));
        entries.push((
            PathBuf::from(&settings.sdk_root),
            download.saturating_mul(4),
            "SDK 安装与暂存",
        ));
    }
    if op.kind == "avd_create" {
        let storage = op
            .optional("storageMiB")
            .parse::<u64>()
            .unwrap_or_else(|_| {
                op.optional("storage")
                    .parse::<u64>()
                    .unwrap_or(2)
                    .saturating_mul(1024)
            });
        let sd = if op.optional("sdCardMode") == "size" {
            op.optional("sdCardMiB").parse::<u64>().unwrap_or(0)
        } else {
            0
        };
        let root = settings
            .avd_roots
            .last()
            .ok_or_else(|| AppError::new("INVALID_PATH", "请选择 AVD 目录"))?;
        entries.push((
            PathBuf::from(root),
            storage
                .saturating_add(sd)
                .saturating_add(512)
                .saturating_mul(1048576),
            "AVD 数据与暂存",
        ));
    }
    let mut effects = if entries.is_empty() {
        vec![]
    } else {
        budget(&entries)?
    };
    if needed.iter().any(|package| package.size == 0) {
        effects.push("部分组件下载量与解压占用待获取；执行时将再次检查空间。".into());
    }
    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usage_and_shared_volume_budget_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("child")).unwrap();
        std::fs::write(temp.path().join("child/file"), b"hello").unwrap();
        let result = usage(temp.path());
        assert!(result.complete);
        assert_eq!(result.files, 1);
        assert!(result.bytes >= 5);
        let free = crate::bootstrap::free_space(temp.path());
        assert!(budget(&[
            (temp.path().join("a"), free / 2 + 1, "a"),
            (temp.path().join("b"), free / 2 + 1, "b")
        ])
        .is_err());
        assert!(budget(&[(temp.path().join("a"), 1, "a")]).is_ok());
    }
    #[cfg(windows)]
    #[test]
    fn usage_never_follows_junctions() {
        use std::os::windows::process::CommandExt;
        let temp = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::fs::write(external.path().join("secret"), b"preserve").unwrap();
        let link = temp.path().join("external");
        let output = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(external.path())
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(output.status.success());
        let result = usage(temp.path());
        assert!(!result.complete);
        assert_eq!(result.files, 0);
        std::fs::remove_dir(&link).unwrap();
        assert_eq!(
            std::fs::read(external.path().join("secret")).unwrap(),
            b"preserve"
        );
    }
}
