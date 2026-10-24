use crate::{
    app::{avd_locked, App},
    domain::*,
};
use std::{io::Write, path::Path};

fn index_contents(data: &Path, target: &str) -> Vec<u8> {
    format!(
        "avd.ini.encoding=UTF-8\npath={}\ntarget={target}\n",
        data.display()
    )
    .into_bytes()
}
fn index_matches(bytes: &[u8], data: &Path, target: &str) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let p = crate::android::properties(text);
    p.get("avd.ini.encoding").is_some_and(|s| s == "UTF-8")
        && p.get("target").is_some_and(|s| s == target)
        && p.get("path").is_some_and(|path| {
            matches!((std::fs::canonicalize(path), std::fs::canonicalize(data)), (Ok(actual), Ok(expected)) if actual == expected)
        })
}
fn publish_new_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        // Use extended absolute paths and deliberately omit REPLACE_EXISTING.
        let source = std::fs::canonicalize(temporary)?;
        let target = std::fs::canonicalize(destination.parent().unwrap())?
            .join(destination.file_name().unwrap());
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            if windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::hard_link(temporary, destination)?;
        let _ = std::fs::remove_file(temporary);
        Ok(())
    }
}
pub fn create_avd_index(ini: &Path, data: &Path, target: &str) -> Result<()> {
    let temporary = ini.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&index_contents(data, target))?;
        file.sync_all()?;
        drop(file);
        publish_new_file(&temporary, ini)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
fn recover_avd_index(ini: &Path, data: &Path, target: &str) -> Result<()> {
    match std::fs::read(ini) {
        Ok(bytes) if index_matches(&bytes, data, target) => return Ok(()),
        Ok(bytes) => {
            // Only our own truncated serialization is safe to reconstruct.
            // Other content is an external conflict and must remain untouched.
            if !index_contents(data, target).starts_with(&bytes) || std::fs::read(ini)? != bytes {
                return Err(AppError::new(
                    "AVD_INDEX_CONFLICT",
                    "AVD 索引内容与提交计划冲突，已保留索引和提交日志",
                ));
            }
            let backup =
                ini.with_extension(format!("ini.devkit-interrupted-{}", uuid::Uuid::new_v4()));
            std::fs::rename(ini, backup)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    create_avd_index(ini, data, target)?;
    if !index_matches(&std::fs::read(ini)?, data, target) {
        return Err(AppError::new(
            "AVD_INDEX_CONFLICT",
            "提交后索引校验失败，保留提交日志供检查",
        ));
    }
    Ok(())
}

pub fn write_commit_journal(path: &Path, journal: &serde_json::Value) -> Result<()> {
    let bytes = serde_json::to_vec(journal)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub(crate) fn commit_avd(
    data: &Path,
    final_data: &Path,
    ini: &Path,
    journal: &Path,
    stage: &Path,
    task_id: &str,
    target: &str,
) -> Result<()> {
    if final_data.exists() || ini.exists() {
        return Err(AppError::new("AVD_EXISTS", "提交前发现同名目录，未覆盖"));
    }
    #[cfg(test)]
    crate::fault_tests::checkpoint("avd.before_journal")?;
    write_commit_journal(
        journal,
        &serde_json::json!({"data":final_data,"ini":ini,"stage":stage,"taskId":task_id}),
    )?;
    #[cfg(test)]
    crate::fault_tests::checkpoint("avd.after_journal")?;
    std::fs::rename(data, final_data)?;
    #[cfg(test)]
    crate::fault_tests::checkpoint("avd.after_data")?;
    create_avd_index(ini, final_data, target)?;
    #[cfg(test)]
    crate::fault_tests::checkpoint("avd.after_index")?;
    let _ = std::fs::remove_file(journal);
    let _ = std::fs::remove_dir_all(stage);
    Ok(())
}

impl App {
    pub(crate) fn pending_avd_commits(&self) -> Result<std::collections::HashSet<String>> {
        let mut pending = std::collections::HashSet::new();
        for entry in std::fs::read_dir(&self.data)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(id) = name
                .strip_prefix("avd-commit-")
                .and_then(|n| n.strip_suffix(".json"))
            else {
                continue;
            };
            // Production names contain the task ID, so even an unreadable or
            // corrupt journal cannot accidentally prove that task completed.
            pending.insert(id.to_string());
            if let Ok(bytes) = std::fs::read(entry.path()) {
                if let Ok(journal) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(id) = journal["taskId"].as_str() {
                        pending.insert(id.to_string());
                    }
                }
            }
        }
        Ok(pending)
    }
    pub fn recover_commits(&self) -> Result<()> {
        let settings = self.store.settings();
        for entry in std::fs::read_dir(&self.data)?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("avd-commit-") || !name.ends_with(".json") {
                continue;
            }
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if let Err(error) = self.recover_commit(&settings, &entry.path()) {
                self.report_recovery_failure(&entry.path(), &error, error.code == "FORMAT_ERROR");
            }
        }
        Ok(())
    }
    fn recover_commit(&self, settings: &Settings, path: &Path) -> Result<()> {
        let journal: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let (Some(data), Some(ini), Some(task_id)) = (
            journal["data"].as_str(),
            journal["ini"].as_str(),
            journal["taskId"].as_str(),
        ) else {
            return Err(AppError::new("FORMAT_ERROR", "提交日志缺少必需字段"));
        };
        let data = Path::new(data);
        let ini = Path::new(ini);
        let Ok(task) = self.store.task(task_id) else {
            return Ok(());
        };
        let avd_name = task.operation.optional("name");
        if task.operation.kind != "avd_create"
            || !valid_name(avd_name)
            || ini.file_name().and_then(|n| n.to_str()) != Some(&format!("{avd_name}.ini"))
        {
            return Ok(());
        }
        if std::fs::read_to_string(data.join(".devkit-owned"))
            .ok()
            .as_deref()
            != Some(task_id)
        {
            return Ok(());
        }
        let Some(parent) = ini.parent() else {
            return Ok(());
        };
        let owned_location = settings.avd_roots.iter().any(|root| {
            matches!(
                (std::fs::canonicalize(root), std::fs::canonicalize(parent),
                 std::fs::canonicalize(data),
                 std::fs::canonicalize(Path::new(root).join(format!("{avd_name}.avd")))),
                (Ok(r), Ok(p), Ok(d), Ok(expected)) if r == p && d == expected && d.starts_with(&r)
            )
        });
        if !owned_location {
            return Ok(());
        }
        let config = crate::android::read_properties(&data.join("config.ini"));
        let target = task
            .operation
            .optional("image")
            .split(';')
            .nth(1)
            .or_else(|| config.get("target").map(String::as_str))
            .unwrap_or("");
        recover_avd_index(ini, data, target)?;
        std::fs::remove_file(path)?;
        Ok(())
    }
    pub fn report_recovery_failure(&self, path: &Path, error: &AppError, quarantine: bool) {
        let root = self.data.join("recovery");
        if std::fs::create_dir_all(&root).is_ok() {
            let preserved = root.join(format!(
                "{}-{}.invalid",
                uuid::Uuid::new_v4(),
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
            let quarantined = quarantine && std::fs::rename(path, &preserved).is_ok();
            let diagnostic = serde_json::json!({"source":path,"preserved":if quarantined {&preserved} else {path},"error":error,"at":now()});
            // One diagnostic per source prevents repeated failed recovery from
            // growing the diagnostic directory on every restart.
            let id = digest(path.to_string_lossy().as_bytes());
            if let Ok(bytes) = serde_json::to_vec_pretty(&diagnostic) {
                let _ = std::fs::write(root.join(format!("{id}.error.json")), bytes);
            }
        }
        let _ = self
            .store
            .audit("", &format!("recovery_failed {}: {error}", path.display()));
    }
    pub fn reconcile(
        &self,
        packages: &[SdkPackage],
        avds: &[Avd],
        devices: &[Device],
    ) -> Result<()> {
        let settings = self.store.settings();
        let pending_commits = self.pending_avd_commits()?;
        for task in self
            .store
            .review_tasks()?
            .into_iter()
            .filter(|t| t.state == "needs_review")
        {
            let op = &task.operation;
            let Ok(_task_use) = self.use_task_record(&task.id, false) else {
                continue;
            };
            let expected: std::collections::BTreeMap<String, String> =
                serde_json::from_str(op.optional("expectedVersions")).unwrap_or_default();
            let proven = match op.kind.as_str() {
                "sdk_install" => op.optional("packages").lines().all(|id| {
                    packages.iter().any(|p| {
                        p.id == id
                            && p.installed
                            && expected.get(id).is_none_or(|v| {
                                v == if p.installed_version.is_empty() {
                                    &p.version
                                } else {
                                    &p.installed_version
                                }
                            })
                    })
                }),
                "sdk_remove" => op
                    .optional("packages")
                    .lines()
                    .all(|id| !packages.iter().any(|p| p.id == id && p.installed)),
                "avd_create" => !pending_commits.contains(&task.id) && avds.iter().any(|a| {
                    let data = Path::new(&a.data_path);
                    let ini = Path::new(&a.id);
                    let config = crate::android::read_properties(&data.join("config.ini"));
                    let target = op.optional("image").split(';').nth(1)
                        .or_else(|| config.get("target").map(String::as_str)).unwrap_or("");
                    a.error.is_empty()
                        && a.name == op.optional("name")
                        && settings.avd_roots.iter().any(|root| {
                            matches!((std::fs::canonicalize(ini), std::fs::canonicalize(Path::new(root).join(format!("{}.ini", a.name))),
                                std::fs::canonicalize(data), std::fs::canonicalize(Path::new(root).join(format!("{}.avd", a.name)))),
                                (Ok(i), Ok(expected_i), Ok(d), Ok(expected_d)) if i == expected_i && d == expected_d)
                        })
                        && std::fs::read(ini).is_ok_and(|bytes| index_matches(&bytes, data, target))
                        && std::fs::read_to_string(data.join(".devkit-owned"))
                            .ok()
                            .as_deref()
                            == Some(task.id.as_str())
                }),
                "avd_start" | "avd_cold" | "avd_wait" => avds
                    .iter()
                    .find(|a| a.id == op.optional("avdId"))
                    .is_some_and(|a| {
                        devices
                            .iter()
                            .any(|d| d.avd_name == a.name && d.state == "ready")
                    }),
                "avd_stop" => avds
                    .iter()
                    .find(|a| a.id == op.optional("avdId"))
                    .is_some_and(|a| {
                        !avd_locked(Path::new(&a.data_path))
                            && !devices.iter().any(|d| d.avd_name == a.name)
                    }),
                "avd_delete" => {
                    !Path::new(op.optional("avdId")).exists()
                        && !op.optional("dataPath").is_empty()
                        && !Path::new(op.optional("dataPath")).exists()
                }
                "capture" => !task.artifact.is_empty() && image::open(&task.artifact).is_ok(),
                _ => false,
            };
            if !proven && task.phase == "reconciled" {
                continue;
            }
            self.update(&task.id, |t| {
                t.phase = "reconciled".into();
                if proven {
                    t.state = "succeeded".into();
                    t.message = "重开后已复查实际状态，预期结果已存在；未重放命令".into();
                } else {
                    t.message = if pending_commits.contains(&task.id) {
                        "AVD 提交记录尚未恢复完成，保留待检查状态；请检查索引冲突及恢复诊断"
                    } else {
                        "已复查本地状态，结果仍无法完全确认；请检查原目标后主动重试"
                    }
                    .into();
                }
            })?;
        }
        Ok(())
    }
}
