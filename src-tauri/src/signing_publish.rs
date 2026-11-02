//! Verified output replacement. The result stays in the app cache until acknowledged.
use crate::{domain::*, signing::new_copy};
use std::path::{Path, PathBuf};

fn paths(destination: &Path, id: &str) -> Result<(PathBuf, PathBuf)> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("SIGNING_ID", "签名任务标识无效"))?;
    let parent = std::fs::canonicalize(
        destination
            .parent()
            .ok_or_else(|| AppError::new("SIGNING_PATH", "目标文件目录无效"))?,
    )?;
    Ok((
        parent.join(format!(".devkit-signing-{id}.tmp")),
        parent.join(format!(".devkit-signing-{id}.backup")),
    ))
}
fn fingerprint(path: &Path) -> Option<String> {
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 * 1024 {
        return None;
    }
    digest_file(path).ok()
}
fn replace(temporary: &Path, destination: &Path, backup: Option<&Path>) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide = |p: &Path| {
            p.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<u16>>()
        };
        let source = wide(temporary);
        let target = wide(destination);
        let saved = backup.map(wide);
        unsafe {
            if windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
                target.as_ptr(),
                source.as_ptr(),
                saved.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
                0,
                std::ptr::null(),
                std::ptr::null(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if let Some(backup) = backup {
            std::fs::rename(destination, backup)?;
        }
        std::fs::rename(temporary, destination)?;
        Ok(())
    }
}
fn remove_matching(path: &Path, expected: &str) -> Result<()> {
    if path.try_exists()? {
        if fingerprint(path).as_deref() != Some(expected) {
            return Err(AppError::new(
                "SIGNING_CHANGED",
                "覆盖恢复材料已变化，保留供检查",
            ));
        }
        std::fs::remove_file(path)?;
    }
    Ok(())
}
pub fn replace_verified(
    source: &Path,
    destination: &Path,
    old: &str,
    new: &str,
    id: &str,
) -> Result<()> {
    let (temporary, backup) = paths(destination, id)?;
    if fingerprint(destination).as_deref() != Some(old) {
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "目标文件已变化，未覆盖；签名结果已保留供另存",
        ));
    }
    if temporary.try_exists()? || backup.try_exists()? {
        return Err(AppError::new(
            "SIGNING_RECOVERY",
            "覆盖恢复材料尚未处理，请先检查已有结果",
        ));
    }
    new_copy(source, &temporary, new)?;
    if fingerprint(destination).as_deref() != Some(old) {
        remove_matching(&temporary, new)?;
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "目标文件已变化，未覆盖；签名结果已保留供另存",
        ));
    }
    let result = replace(&temporary, destination, Some(&backup));
    if result.is_err() {
        recover(destination, old, new, id)?;
        return result;
    }
    if fingerprint(&backup).as_deref() != Some(old) {
        // Preserve a concurrent edit captured by ReplaceFile; never silently discard it.
        if fingerprint(destination).as_deref() == Some(new) && backup.is_file() {
            replace(&backup, destination, None)?;
        }
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "覆盖时目标文件发生变化，已保留文件供检查",
        ));
    }
    if fingerprint(destination).as_deref() != Some(new) {
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "覆盖后目标文件发生变化，原件备份已保留供检查",
        ));
    }
    Ok(())
}
pub fn recover(destination: &Path, old: &str, new: &str, id: &str) -> Result<()> {
    let (temporary, backup) = paths(destination, id)?;
    if !destination.try_exists()? && fingerprint(&backup).as_deref() == Some(old) {
        crate::recovery::publish_new_file(&backup, destination)?;
    }
    let actual = fingerprint(destination);
    if backup.try_exists()? && fingerprint(&backup).as_deref() != Some(old) {
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "覆盖恢复材料与确认内容不符，已保留文件供检查",
        ));
    }
    if actual.as_deref() == Some(new) || actual.as_deref() == Some(old) {
        remove_matching(&temporary, new)?;
        remove_matching(&backup, old)?;
    } else if backup.try_exists()? {
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "目标文件已由外部修改，原件备份已保留供检查",
        ));
    }
    Ok(())
}
pub fn cleanup(destination: &Path, old: &str, new: &str, id: &str) -> Result<()> {
    recover(destination, old, new, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_replace_preserves_conflicts_and_recovers_interruption() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("input.apk");
        let result = root.path().join("verified.apk");
        std::fs::write(&target, b"original").unwrap();
        std::fs::write(&result, b"signed").unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let old = digest(b"original");
        let new = digest(b"signed");
        assert!(replace_verified(&result, &target, &digest(b"changed"), &new, &id).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        replace_verified(&result, &target, &old, &new, &id).unwrap();
        let (_, backup) = paths(&target, &id).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), b"original");
        recover(&target, &old, &new, &id).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"signed");
        assert!(!backup.exists());
        // Simulate an interruption after preserving the old file, before publishing the new file.
        std::fs::remove_file(&target).unwrap();
        std::fs::write(&backup, b"original").unwrap();
        recover(&target, &old, &new, &id).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(std::fs::read(&result).unwrap(), b"signed");
        // Recovery must not use externally changed backup contents to overwrite a result.
        std::fs::write(&target, b"signed").unwrap();
        std::fs::write(&backup, b"external backup edit").unwrap();
        assert!(recover(&target, &old, &new, &id).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"signed");
        assert_eq!(std::fs::read(&backup).unwrap(), b"external backup edit");
        std::fs::remove_file(&backup).unwrap();
        std::fs::write(&target, b"original").unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let guard = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
                .open(&target)
                .unwrap();
            let locked_id = uuid::Uuid::new_v4().to_string();
            assert!(replace_verified(&result, &target, &old, &new, &locked_id).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"original");
            drop(guard);
        }
    }
}
