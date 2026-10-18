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
