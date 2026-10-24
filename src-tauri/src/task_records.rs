use crate::{app::App, domain::*};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub struct TaskRecordUse<'a> {
    app: &'a App,
    id: String,
}
impl Drop for TaskRecordUse<'_> {
    fn drop(&mut self) {
        let mut uses = self.app.task_uses.lock().unwrap();
        if let Some(count) = uses.get_mut(&self.id) {
            if *count == usize::MAX || *count == 1 {
                uses.remove(&self.id);
            } else {
                *count -= 1;
            }
        }
    }
}

impl App {
    pub fn use_task_record(&self, id: &str, exclusive: bool) -> Result<TaskRecordUse<'_>> {
        let mut uses = self.task_uses.lock().unwrap();
        let count = uses.get(id).copied().unwrap_or_default();
        if count == usize::MAX || (exclusive && count > 0) {
            return Err(AppError::new("TASK_IN_USE", "任务文件正在使用，请稍后重试"));
        }
        if self.store.task_record_deleted(id)? {
            return Err(AppError::new(
                "TASK_RECORD_DELETED",
                "任务记录已删除，请刷新列表",
            ));
        }
        uses.insert(id.into(), if exclusive { usize::MAX } else { count + 1 });
        Ok(TaskRecordUse {
            app: self,
            id: id.into(),
        })
    }
    fn record_protection(
        &self,
        task: &Task,
        uses: &HashMap<String, usize>,
        pending: &HashSet<String>,
        unsaved: &HashSet<String>,
    ) -> Result<Option<(&'static str, &'static str)>> {
        if task.active() {
            return Ok(Some(("active", "任务仍在进行，请先取消并等待结束")));
        }
        if uses.contains_key(&task.id)
            || pending.contains(&task.id)
            || (task.state == "needs_review" && task.phase == "recovery")
        {
            return Ok(Some(("busy", "任务仍在收尾或恢复，请等待结束")));
        }
        if !matches!(
            task.state.as_str(),
            "succeeded" | "failed" | "cancelled" | "needs_review"
        ) {
            return Ok(Some(("busy", "任务状态无法确认，请刷新后重试")));
        }
        if unsaved.contains(&task.id) {
            return Ok(Some(("files", "有待保存文件，请先处理")));
        }
        if task.state == "needs_review"
            && matches!(
                task.operation.kind.as_str(),
                "file_upload" | "file_download"
            )
            && task.operation.optional("conflicts") == "replace"
            && task.operation.optional("backupsResolved") != "true"
        {
            return Ok(Some((
                "files",
                "替换原件尚待处理，请从原设备的文件传输面板恢复或清理",
            )));
        }
        if task.operation.kind == "log_record" {
            if !task.artifact.is_empty() {
                return Ok(Some(("files", "原始采集仍在，请先处理采集文件")));
            }
            if !safe_id(&task.id) {
                return Ok(Some(("busy", "采集归属无效，请检查诊断")));
            }
            match std::fs::symlink_metadata(self.data.join("recordings").join(&task.id)) {
                Ok(_) => return Ok(Some(("files", "原始采集仍在，请先处理采集文件"))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(None)
    }
    fn unsaved_task_ids(&self) -> Result<HashSet<String>> {
        Ok(self
            .store
            .artifacts()?
            .into_iter()
            .filter(|a| !a.acknowledged && !a.task_id.is_empty())
            .map(|a| a.task_id)
            .collect())
    }
    pub fn task_records_page(&self, query: &TaskQuery) -> Result<TaskPage> {
        let uses = self.task_uses.lock().unwrap();
        let mut page = self.store.task_page(query)?;
        let pending = self.pending_avd_commits()?;
        let unsaved = self.unsaved_task_ids()?;
        for task in self.store.matching_task_records(query)? {
            if let Some((kind, reason)) =
                self.record_protection(&task, &uses, &pending, &unsaved)?
            {
                match kind {
                    "active" => page.deletion.active += 1,
                    "files" => page.deletion.files += 1,
                    _ => page.deletion.busy += 1,
                }
                if page.tasks.iter().any(|t| t.id == task.id) {
                    page.deletion.blocked.insert(task.id, reason.into());
                }
            } else if task.state == "needs_review" {
                page.deletion.review += 1;
            } else {
                page.deletion.eligible += 1;
            }
        }
        Ok(page)
    }
    pub fn delete_task_records(&self, request: TaskDeleteRequest) -> Result<TaskDeleteResult> {
        if request.query.is_some() != request.task_ids.is_empty() || request.task_ids.len() > 10000
        {
            return Err(AppError::new(
                "INVALID_DELETE_SCOPE",
                "请选择任务或指定清理范围",
            ));
        }
        let uses = self.task_uses.lock().unwrap();
        let pending = self.pending_avd_commits()?;
        let unsaved = self.unsaved_task_ids()?;
        // Resolve the complete query once. Later completions/retries are never
        // added to this snapshot, and state/files are rechecked for every record.
        let cleanup = request.query.is_some();
        let mut result = TaskDeleteResult::default();
        let ids = if let Some(query) = request.query {
            let mut ids = Vec::new();
            for task in self.store.matching_task_records(&query)? {
                let reason = match self.record_protection(&task, &uses, &pending, &unsaved) {
                    Ok(Some((_, reason))) => Some((reason.to_string(), false)),
                    Err(error) => Some((error.to_string(), true)),
                    Ok(None) if task.state == "needs_review" => {
                        Some(("结果待检查，已保留".into(), false))
                    }
                    Ok(None) => None,
                };
                if let Some((reason, retryable)) = reason {
                    result.issues.push(TaskDeleteIssue {
                        task_id: task.id,
                        title: task.title,
                        reason,
                        retryable,
                    });
                } else {
                    ids.push(task.id);
                }
            }
            ids
        } else {
            request
                .task_ids
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        for id in ids {
            if self.store.task_record_deleted(&id)? {
                result.already_deleted += 1;
                continue;
            }
            let task = match self.store.task(&id) {
                Ok(task) => task,
                Err(_) => {
                    result.issues.push(TaskDeleteIssue {
                        task_id: id,
                        title: String::new(),
                        reason: "任务记录不存在，请刷新列表".into(),
                        retryable: false,
                    });
                    continue;
                }
            };
            let protection = self.record_protection(&task, &uses, &pending, &unsaved);
            let operation = match protection {
                Ok(Some((_, reason))) => Err((reason.to_string(), false)),
                Err(error) => Err((error.to_string(), true)),
                Ok(None) if cleanup && task.state == "needs_review" => {
                    Err(("结果待检查，已保留".into(), false))
                }
                Ok(None) => self.store.delete_task_record(&id).map_err(|error| {
                    let retryable =
                        !matches!(error.code.as_str(), "TASK_ACTIVE" | "TASK_FILES_PENDING");
                    (error.message, retryable)
                }),
            };
            match operation {
                Ok(true) => result.deleted += 1,
                Ok(false) => result.already_deleted += 1,
                Err((reason, retryable)) => result.issues.push(TaskDeleteIssue {
                    task_id: id,
                    title: task.title,
                    reason,
                    retryable,
                }),
            }
        }
        drop(uses);
        self.notify();
        Ok(result)
    }
    pub fn discard_recording(&self, id: &str) -> Result<()> {
        let _usage = self.use_task_record(id, true)?;
        let task = self.store.task(id)?;
        if !safe_id(id) || task.operation.kind != "log_record" {
            return Err(AppError::new("INVALID_TASK", "采集归属无效"));
        }
        if task.active() || task.phase == "recovery" || self.pending_avd_commits()?.contains(id) {
            return Err(AppError::new(
                "TASK_ACTIVE",
                "采集仍在执行或恢复，请等待结束",
            ));
        }
        let parent = self.data.join("recordings");
        let root = parent.join(id);
        check_plain_path(&parent, true)?;
        check_plain_path(&root, true)?;
        let manifest = root.join("manifest.json");
        match std::fs::read(&manifest) {
            Ok(raw) => {
                let m: crate::recording::Manifest = serde_json::from_slice(&raw)?;
                if m.task_id != id
                    || m.state == "recording"
                    || m.files.iter().any(|s| !s.name.ends_with(".log"))
                {
                    return Err(AppError::new(
                        "RECORDING_ACTIVE",
                        "采集尚未封口，请先完成恢复",
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if root.exists() && std::fs::read_dir(&root)?.next().is_some() {
                    return Err(AppError::new(
                        "UNSAFE_ARTIFACT",
                        "采集清单缺失且目录非空，未清理",
                    ));
                }
            }
            Err(e) => return Err(e.into()),
        }
        let mut files = Vec::new();
        if root.exists() {
            for entry in std::fs::read_dir(&root)? {
                let path = entry?.path();
                check_plain_path(&path, false)?;
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if !(matches!(name, "manifest.json" | "manifest.tmp")
                    || name
                        .strip_prefix("segment-")
                        .and_then(|n| n.strip_suffix(".log"))
                        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
                {
                    return Err(AppError::new(
                        "UNSAFE_ARTIFACT",
                        "采集目录含未知文件，未清理",
                    ));
                }
                files.push(path);
            }
            // Keep the manifest until all segments are gone so partial failures
            // can be retried; no recursive deletion or user paths are involved.
            files.sort_by_key(|p| p == &manifest);
            for file in files {
                std::fs::remove_file(file)?;
            }
            std::fs::remove_dir(&root)?;
        }
        self.store.update_task(id, |t| {
            t.artifact.clear();
            t.message = "原始采集已清理，任务记录已保留".into();
        })?;
        self.notify();
        Ok(())
    }
}
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn check_plain_path(path: &Path, directory: bool) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            let mut link = metadata.file_type().is_symlink();
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                link |= metadata.file_attributes() & 0x400 != 0;
            }
            if link || metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
                return Err(AppError::new("UNSAFE_ARTIFACT", "采集路径不安全，未清理"));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regression_tests::task;
    fn ids(ids: &[&str]) -> TaskDeleteRequest {
        TaskDeleteRequest {
            task_ids: ids.iter().map(|s| s.to_string()).collect(),
            query: None,
        }
    }
    #[test]
    fn deletes_records_persistently_preserving_identity_and_protecting_work() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        for (id, state) in [
            ("run", "running"),
            ("ok", "succeeded"),
            ("review", "needs_review"),
            ("used", "failed"),
            ("journal", "succeeded"),
        ] {
            let mut t = task(id, state);
            t.log.push("private diagnostics".into());
            t.phase = "reconciled".into();
            app.store.save_task(&t).unwrap();
        }
        std::fs::write(dir.path().join("avd-commit-journal.json"), "{}").unwrap();
        let usage = app.use_task_record("used", false).unwrap();
        let result = app
            .delete_task_records(ids(&["run", "ok", "review", "used", "journal"]))
            .unwrap();
        assert_eq!(result.deleted, 2);
        assert_eq!(result.issues.len(), 3);
        assert!(app.store.task("ok").unwrap().log.is_empty());
        assert!(app.store.request("ok").is_some());
        assert_eq!(app.store.tasks().unwrap().len(), 3);
        assert_eq!(
            app.delete_task_records(ids(&["ok"]))
                .unwrap()
                .already_deleted,
            1
        );
        drop(usage);
        drop(app);
        let app = App::new(dir.path().into()).unwrap();
        assert!(app.store.task_record_deleted("ok").unwrap());
        assert!(app
            .store
            .tasks()
            .unwrap()
            .iter()
            .all(|t| t.id != "ok" && t.id != "review"));
    }
    #[test]
    fn cleanup_uses_all_pages_and_literal_search_but_skips_files_and_review() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        for i in 0..75 {
            let mut t = task(&format!("old-{i}"), "succeeded");
            t.title = "SDK 100%_done".into();
            app.store.save_task(&t).unwrap();
        }
        let mut review = task("review", "needs_review");
        review.title = "SDK 100%_done".into();
        review.phase = "reconciled".into();
        app.store.save_task(&review).unwrap();
        let mut capture = task("capture", "succeeded");
        capture.title = "SDK 100%_done".into();
        app.store.save_task(&capture).unwrap();
        let file = dir.path().join("shot.png");
        std::fs::write(&file, "test").unwrap();
        app.store
            .register_artifact("capture", &file, "screenshot", "shot", "capture")
            .unwrap();
        let query = TaskQuery {
            keyword: "100%_done".into(),
            state: "all".into(),
            offset: 60,
        };
        let page = app.task_records_page(&query).unwrap();
        assert_eq!(page.deletion.eligible, 75);
        assert_eq!(page.deletion.files, 1);
        assert_eq!(page.deletion.review, 1);
        let result = app
            .delete_task_records(TaskDeleteRequest {
                query: Some(query),
                task_ids: vec![],
            })
            .unwrap();
        assert_eq!(result.deleted, 75);
        assert_eq!(result.issues.len(), 2);
        assert!(file.exists());
        app.store.acknowledge("capture").unwrap();
        assert_eq!(
            app.delete_task_records(ids(&["capture"])).unwrap().deleted,
            1
        );
        assert!(app.store.artifact("capture").is_ok());
    }
    #[test]
    fn recording_cleanup_is_scoped_retryable_and_waits_for_exports() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let root = dir.path().join("recordings/record");
        std::fs::create_dir_all(&root).unwrap();
        let mut t = task("record", "succeeded");
        t.operation.kind = "log_record".into();
        t.artifact = root.join("manifest.json").to_string_lossy().into();
        app.store.save_task(&t).unwrap();
        std::fs::write(root.join("manifest.json"),r#"{"version":1,"taskId":"record","target":{"serial":"x","serverEpoch":1,"transportId":"1"},"filter":{},"createdAt":1,"state":"stopped","reason":"done","files":[],"bytes":0,"gaps":0}"#).unwrap();
        let saved = dir.path().join("saved.txt");
        std::fs::write(&saved, "saved").unwrap();
        let usage = app.use_task_record("record", false).unwrap();
        assert!(app.discard_recording("record").is_err());
        drop(usage);
        std::fs::write(root.join("unexpected.txt"), "protect").unwrap();
        assert!(app.discard_recording("record").is_err());
        assert!(root.join("manifest.json").exists());
        std::fs::remove_file(root.join("unexpected.txt")).unwrap();
        assert_eq!(
            app.delete_task_records(ids(&["record"])).unwrap().deleted,
            0
        );
        app.discard_recording("record").unwrap();
        app.discard_recording("record").unwrap();
        assert!(!root.exists());
        assert!(saved.exists());
        assert_eq!(
            app.delete_task_records(ids(&["record"])).unwrap().deleted,
            1
        );
    }
    #[cfg(windows)]
    #[test]
    fn recording_cleanup_rejects_directory_junctions() {
        use std::os::windows::process::CommandExt;
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().join("app")).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let parent = app.data.join("recordings");
        std::fs::create_dir_all(&parent).unwrap();
        let linked = parent.join("linked");
        assert!(std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&linked)
            .arg(&outside)
            .creation_flags(0x08000000)
            .output()
            .unwrap()
            .status
            .success());
        let mut record = task("linked", "succeeded");
        record.artifact = linked.join("manifest.json").to_string_lossy().into();
        app.store.save_task(&record).unwrap();
        std::fs::write(outside.join("segment-000001.log"), "keep").unwrap();
        let result = app.discard_recording("linked");
        std::fs::remove_dir(&linked).unwrap();
        assert_eq!(result.unwrap_err().code, "UNSAFE_ARTIFACT");
        assert_eq!(
            std::fs::read_to_string(outside.join("segment-000001.log")).unwrap(),
            "keep"
        );
    }
}
