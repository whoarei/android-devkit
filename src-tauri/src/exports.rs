use crate::{app::App, domain::*, privacy::Redactor};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
struct Meta {
    rows: usize,
    redacted: bool,
    truncated: bool,
}
pub fn atomic_copy(source: &Path, destination: &Path) -> Result<()> {
    atomic_copy_verified(source, destination, &digest_file(source)?)
}
pub fn atomic_copy_verified(source: &Path, destination: &Path, expected: &str) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::new("INVALID_PATH", "保存位置无父目录"))?;
    let tmp = parent.join(format!(".devkit-save-{}.part", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        std::io::copy(&mut File::open(source)?, &mut out)?;
        out.sync_all()?;
        drop(out);
        if digest_file(&tmp)? != expected {
            return Err(AppError::new(
                "ARTIFACT_CHANGED",
                "保存过程中产物内容发生变化，未覆盖目标文件",
            ));
        }
        std::fs::rename(&tmp, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
impl App {
    pub fn discard_artifacts(&self, ids: &[String]) -> Result<()> {
        // Resolve the full reviewed set before deleting anything. Only immutable
        // screenshot/export copies belong here; recordings and user saves do not.
        let mut pending = Vec::new();
        for id in ids.iter().collect::<std::collections::BTreeSet<_>>() {
            let (artifact, path) = self.store.artifact(id)?;
            if !artifact.task_id.is_empty() && self.store.task(&artifact.task_id)?.active() {
                return Err(AppError::new(
                    "ARTIFACT_IN_USE",
                    "截图任务仍在收尾，请稍后清理",
                ));
            }
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(AppError::new(
                    "UNSAFE_ARTIFACT",
                    "产物缓存路径不安全，未清理",
                ));
            }
            let (directory, extension) = match artifact.kind.as_str() {
                "screenshot" => ("artifacts", "png"),
                "export" => ("exports", "txt"),
                _ => {
                    return Err(AppError::new(
                        "UNSAFE_ARTIFACT",
                        "只能清理截图或导出副本缓存",
                    ))
                }
            };
            let root = self.data.join(directory);
            if path != root.join(format!("{id}.{extension}")) {
                return Err(AppError::new(
                    "UNSAFE_ARTIFACT",
                    "产物缓存路径不安全，未清理",
                ));
            }
            let mut files = vec![path.clone()];
            if artifact.kind == "export" {
                files.push(path.with_extension("meta.json"));
            }
            // Missing files are already clean. Links/reparse points and directories
            // must never redirect deletion outside the application's cache.
            for candidate in std::iter::once(&root).chain(files.iter()) {
                match std::fs::symlink_metadata(candidate) {
                    Ok(metadata) => {
                        let mut link = metadata.file_type().is_symlink();
                        #[cfg(windows)]
                        {
                            use std::os::windows::fs::MetadataExt;
                            link |= metadata.file_attributes() & 0x400 != 0;
                        }
                        if link || (candidate != &root && !metadata.is_file()) {
                            return Err(AppError::new(
                                "UNSAFE_ARTIFACT",
                                "产物缓存路径不安全，未清理",
                            ));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            pending.push((id, path, files));
        }
        let result = (|| {
            for (id, path, files) in pending {
                for file in files {
                    match std::fs::remove_file(file) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(AppError::from(error)),
                    }
                }
                // A failed deletion retains the registry entry so the user can retry.
                self.store.forget_artifact(id, &path)?;
            }
            Ok(())
        })();
        self.notify();
        result
    }
    pub fn prepare_text_export(
        &self,
        title: &str,
        inputs: Vec<String>,
        redacted: bool,
        identifiers: Vec<String>,
    ) -> Result<ExportPreview> {
        let rows = inputs.into_iter().map(Ok);
        self.export_lines(title, rows, redacted, identifiers)
    }
    fn export_lines(
        &self,
        title: &str,
        lines: impl Iterator<Item = std::io::Result<String>>,
        redacted: bool,
        identifiers: Vec<String>,
    ) -> Result<ExportPreview> {
        let id = format!("export-{}", uuid::Uuid::new_v4());
        let root = self.data.join("exports");
        std::fs::create_dir_all(&root)?;
        let path = root.join(format!("{id}.txt"));
        let mut out = File::create(&path)?;
        let mut redact = Redactor::new(identifiers);
        let mut meta = Meta {
            rows: 0,
            redacted,
            truncated: false,
        };
        let mut bytes = 0;
        for raw in lines {
            let value = raw?;
            let value = if redacted {
                redact.redact(&value)
            } else {
                value
            };
            if bytes + value.len() > 64 * 1024 * 1024 {
                meta.truncated = true;
                writeln!(
                    out,
                    "--- EXPORT TRUNCATED at 64 MiB; choose fewer segments ---"
                )?;
                meta.rows += 1;
                break;
            }
            writeln!(out, "{value}")?;
            bytes += value.len() + 1;
            meta.rows += 1;
        }
        out.sync_all()?;
        std::fs::write(path.with_extension("meta.json"), serde_json::to_vec(&meta)?)?;
        self.store
            .register_artifact(&id, &path, "export", title, "")?;
        self.export_page(&id, 0)
    }
    pub fn export_recording(
        &self,
        task_id: &str,
        segments: &[usize],
        redacted: bool,
    ) -> Result<ExportPreview> {
        let _task_use = self.use_task_record(task_id, false)?;
        let task = self.store.task(task_id)?;
        if task.operation.kind != "log_record" || task.active() {
            return Err(AppError::new(
                "RECORDING_ACTIVE",
                "停止并封口采集后再选择采集段导出",
            ));
        }
        let root = self.data.join("recordings").join(task_id);
        let m: crate::recording::Manifest =
            serde_json::from_slice(&std::fs::read(root.join("manifest.json"))?)?;
        if m.task_id != task_id {
            return Err(AppError::new("INVALID_RECORDING", "采集归属不符"));
        }
        if segments.is_empty() || segments.len() > 1000 {
            return Err(AppError::new("NO_SEGMENTS", "请选择 1～1000 个采集段"));
        }
        let mut inputs = vec![];
        for index in segments {
            let s = m
                .files
                .get(*index)
                .ok_or_else(|| AppError::new("NO_SEGMENT", "采集段不存在"))?;
            if s.name.contains(['\\', '/']) || !s.name.ends_with(".log") {
                return Err(AppError::new(
                    "UNSEALED_SEGMENT",
                    "只能导出完成封口的采集段",
                ));
            }
            let path = root.join(&s.name);
            if !std::fs::canonicalize(&path)?.starts_with(std::fs::canonicalize(&root)?) {
                return Err(AppError::new("UNSAFE_PATH", "采集段指向归属目录之外"));
            }
            if digest_file(&path)? != s.sha256 {
                return Err(AppError::new(
                    "ARTIFACT_CHANGED",
                    "采集段内容已变化，请检查文件",
                ));
            }
            inputs.push(path);
        }
        let readers = inputs
            .into_iter()
            .map(|p| File::open(p).map(|f| BufReader::new(f).lines()))
            .collect::<std::io::Result<Vec<_>>>()?;
        self.export_lines(
            "所选采集段",
            readers.into_iter().flatten(),
            redacted,
            vec![m.target.serial],
        )
    }
    pub fn export_page(&self, id: &str, offset: usize) -> Result<ExportPreview> {
        let (a, path) = self.store.artifact(id)?;
        if a.kind != "export" || !path.starts_with(self.data.join("exports")) {
            return Err(AppError::new("INVALID_ARTIFACT", "不是可预览的文本导出"));
        }
        let meta: Meta = serde_json::from_slice(&std::fs::read(path.with_extension("meta.json"))?)?;
        let lines = BufReader::new(File::open(path)?)
            .lines()
            .skip(offset)
            .take(300)
            .collect::<std::io::Result<Vec<_>>>()?;
        let next = offset + lines.len();
        Ok(ExportPreview{artifact:a,text:lines.join("\n"),offset,next_offset:(next<meta.rows).then_some(next),rows:meta.rows,redacted:meta.redacted,truncated:meta.truncated,warning:"这是待保存文件的实际内容，可分页检查。规则脱敏不能覆盖所有应用自由文本；日志可能包含应用数据。仅保存到你选择的本地位置，不上传。".into()})
    }
    pub fn checked_artifact(&self, id: &str) -> Result<(Artifact, PathBuf)> {
        let (a, p) = self.store.artifact(id)?;
        let canonical = std::fs::canonicalize(&p)?;
        if !canonical.starts_with(std::fs::canonicalize(&self.data)?)
            || digest_file(&p)? != a.sha256
        {
            return Err(AppError::new(
                "ARTIFACT_CHANGED",
                "产物范围或内容已变化，请重新预览",
            ));
        }
        Ok((a, p))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discard_removes_only_reviewed_copies_and_preserves_task_history() {
        let d = tempfile::tempdir().unwrap();
        let app = App::new(d.path().join("app")).unwrap();
        let first = app
            .prepare_text_export("first", vec!["one".into()], false, vec![])
            .unwrap();
        let other = app
            .prepare_text_export("other", vec!["two".into()], false, vec![])
            .unwrap();
        let (_, source) = app.store.artifact(&first.artifact.id).unwrap();
        let saved = d.path().join("saved.txt");
        atomic_copy(&source, &saved).unwrap();
        let recording = app.data.join("recordings/original.log");
        std::fs::create_dir_all(recording.parent().unwrap()).unwrap();
        std::fs::write(&recording, "raw recording").unwrap();
        let screenshot = app.data.join("artifacts/capture.png");
        std::fs::create_dir_all(screenshot.parent().unwrap()).unwrap();
        std::fs::write(&screenshot, b"screenshot").unwrap();
        let mut task = crate::regression_tests::task("capture", "succeeded");
        task.operation.kind = "capture".into();
        task.artifact = screenshot.to_string_lossy().into();
        app.store.save_task(&task).unwrap();
        app.store
            .register_artifact("capture", &screenshot, "screenshot", "设备截图", "capture")
            .unwrap();
        app.discard_artifacts(&[first.artifact.id.clone(), "capture".into()])
            .unwrap();
        assert!(!source.exists());
        assert!(!source.with_extension("meta.json").exists());
        assert!(!screenshot.exists());
        assert_eq!(std::fs::read_to_string(saved).unwrap(), "one\n");
        assert_eq!(std::fs::read_to_string(recording).unwrap(), "raw recording");
        assert!(app.store.task("capture").unwrap().artifact.is_empty());
        assert_eq!(app.store.task("capture").unwrap().state, "succeeded");
        assert_eq!(app.close_status().unwrap().unsaved.len(), 1);
        assert_eq!(app.close_status().unwrap().unsaved[0].id, other.artifact.id);
    }
    #[test]
    fn discard_prevalidates_paths_and_handles_already_missing_files() {
        let d = tempfile::tempdir().unwrap();
        let app = App::new(d.path().join("app")).unwrap();
        let p = app
            .prepare_text_export("test", vec!["cache".into()], false, vec![])
            .unwrap();
        let (_, source) = app.store.artifact(&p.artifact.id).unwrap();
        let outside = d.path().join("saved.txt");
        std::fs::write(&outside, "saved").unwrap();
        app.store
            .register_artifact("outside", &outside, "export", "outside", "")
            .unwrap();
        assert_eq!(
            app.discard_artifacts(&[p.artifact.id.clone(), "outside".into()])
                .unwrap_err()
                .code,
            "UNSAFE_ARTIFACT"
        );
        assert!(source.exists());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "saved");
        std::fs::remove_file(&source).unwrap();
        app.discard_artifacts(std::slice::from_ref(&p.artifact.id))
            .unwrap();
        assert!(!source.with_extension("meta.json").exists());
        assert!(app.store.artifact(&p.artifact.id).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn discard_failure_remains_pending_and_retry_finishes_metadata_cleanup() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = tempfile::tempdir().unwrap();
        let app = App::new(d.path().into()).unwrap();
        let p = app
            .prepare_text_export("test", vec!["cache".into()], false, vec![])
            .unwrap();
        let (_, source) = app.store.artifact(&p.artifact.id).unwrap();
        let metadata = source.with_extension("meta.json");
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&metadata)
            .unwrap();
        let result = app.discard_artifacts(std::slice::from_ref(&p.artifact.id));
        drop(locked);
        assert!(result.is_err());
        assert!(!source.exists());
        assert!(metadata.exists());
        assert!(!app.store.artifact(&p.artifact.id).unwrap().0.acknowledged);
        app.discard_artifacts(std::slice::from_ref(&p.artifact.id))
            .unwrap();
        assert!(!metadata.exists());
        assert!(app.close_status().unwrap().unsaved.is_empty());
    }
    #[cfg(windows)]
    #[test]
    fn discard_rejects_cache_junctions() {
        use std::os::windows::process::CommandExt;
        let d = tempfile::tempdir().unwrap();
        let app = App::new(d.path().join("app")).unwrap();
        let outside = d.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let cache = app.data.join("exports");
        assert!(std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&cache)
            .arg(&outside)
            .creation_flags(0x08000000)
            .output()
            .unwrap()
            .status
            .success());
        std::fs::write(outside.join("linked.txt"), "keep").unwrap();
        app.store
            .register_artifact("linked", &cache.join("linked.txt"), "export", "linked", "")
            .unwrap();
        let result = app.discard_artifacts(&["linked".into()]);
        std::fs::remove_dir(&cache).unwrap();
        assert_eq!(result.unwrap_err().code, "UNSAFE_ARTIFACT");
        assert_eq!(
            std::fs::read_to_string(outside.join("linked.txt")).unwrap(),
            "keep"
        );
    }
    #[test]
    fn failed_save_does_not_acknowledge_or_lose_source() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("a.txt");
        std::fs::write(&source, "valuable").unwrap();
        assert!(atomic_copy(&source, &d.path().join("missing/b.txt")).is_err());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "valuable");
    }
    #[test]
    fn preview_pages_are_frozen_and_tampering_blocks_save() {
        let d = tempfile::tempdir().unwrap();
        let app = App::new(d.path().into()).unwrap();
        let p = app
            .prepare_text_export(
                "view",
                (0..650)
                    .map(|i| format!("line {i} token=private 192.168.1.2 SERIAL123"))
                    .collect(),
                true,
                vec!["SERIAL123".into()],
            )
            .unwrap();
        assert_eq!(p.rows, 650);
        assert_eq!(p.next_offset, Some(300));
        assert!(
            !p.text.contains("private")
                && !p.text.contains("192.168.1.2")
                && !p.text.contains("SERIAL123")
        );
        let page = app.export_page(&p.artifact.id, 600).unwrap();
        assert_eq!(page.next_offset, None);
        assert!(page.text.contains("line 649"));
        let (_, file) = app.checked_artifact(&p.artifact.id).unwrap();
        std::fs::write(file, "changed").unwrap();
        assert_eq!(
            app.checked_artifact(&p.artifact.id).unwrap_err().code,
            "ARTIFACT_CHANGED"
        );
    }
}
