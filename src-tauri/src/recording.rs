use crate::{app::App, domain::*};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub task_id: String,
    pub target: Target,
    pub filter: LogFilter,
    pub created_at: u64,
    pub state: String,
    pub reason: String,
    pub files: Vec<Segment>,
    pub bytes: u64,
    pub gaps: u64,
}
pub struct Recorder {
    pub manifest: Manifest,
    root: PathBuf,
    file: Option<BufWriter<File>>,
    current: u64,
    rotation: u64,
    quota: u64,
}
pub fn save_manifest(root: &Path, m: &Manifest) -> Result<()> {
    let tmp = root.join("manifest.tmp");
    let mut f = File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(m)?)?;
    f.sync_all()?;
    std::fs::rename(tmp, root.join("manifest.json"))?;
    Ok(())
}
impl Recorder {
    pub fn new(
        root: &Path,
        task_id: &str,
        target: Target,
        filter: LogFilter,
        quota: u64,
        rotation: u64,
    ) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let mut r = Self {
            manifest: Manifest {
                version: 1,
                task_id: task_id.into(),
                target,
                filter,
                created_at: now(),
                state: "recording".into(),
                reason: String::new(),
                files: vec![],
                bytes: 0,
                gaps: 0,
            },
            root: root.into(),
            file: None,
            current: 0,
            rotation: rotation.min(quota),
            quota,
        };
        r.open()?;
        Ok(r)
    }
    fn open(&mut self) -> Result<()> {
        let name = format!("segment-{:04}.log.part", self.manifest.files.len() + 1);
        self.file = Some(BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.root.join(&name))?,
        ));
        self.current = 0;
        self.manifest.files.push(Segment {
            name,
            bytes: 0,
            sha256: String::new(),
        });
        save_manifest(&self.root, &self.manifest)
    }
    fn seal(&mut self) -> Result<()> {
        if let Some(mut f) = self.file.take() {
            f.flush()?;
            f.get_ref().sync_all()?;
            drop(f);
            let s = self.manifest.files.last_mut().unwrap();
            let from = self.root.join(&s.name);
            let name = s.name.trim_end_matches(".part").to_string();
            std::fs::rename(&from, self.root.join(&name))?;
            s.name = name;
            s.bytes = self.current;
            s.sha256 = digest_file(&self.root.join(&s.name))?;
            save_manifest(&self.root, &self.manifest)?;
        }
        Ok(())
    }
    pub fn line(&mut self, value: &str) -> Result<bool> {
        let line = format!("{value}\n");
        let n = line.len() as u64;
        if self.manifest.bytes + n > self.quota {
            return Ok(false);
        }
        if self.current > 0 && self.current + n > self.rotation {
            self.seal()?;
            self.open()?;
        }
        self.file.as_mut().unwrap().write_all(line.as_bytes())?;
        self.current += n;
        self.manifest.bytes += n;
        Ok(true)
    }
    pub fn gap(&mut self, value: &str) -> Result<()> {
        self.manifest.gaps += 1;
        let _ = self.line(&format!("--- GAP: {value} ---"))?;
        Ok(())
    }
    pub fn checkpoint(&mut self) -> Result<()> {
        if let Some(f) = &mut self.file {
            f.flush()?;
        }
        if let Some(s) = self.manifest.files.last_mut() {
            s.bytes = self.current;
        }
        save_manifest(&self.root, &self.manifest)
    }
    pub fn finish(mut self, state: &str, reason: &str) -> Result<Manifest> {
        self.seal()?;
        self.manifest.state = state.into();
        self.manifest.reason = reason.into();
        save_manifest(&self.root, &self.manifest)?;
        Ok(self.manifest)
    }
}
impl App {
    pub async fn record_logs(self: &Arc<Self>, id: &str, op: &Operation) -> Result<String> {
        let t = op
            .target
            .as_ref()
            .ok_or_else(|| AppError::new("TARGET_REQUIRED", "需要明确日志目标"))?
            .clone();
        let mut f: LogFilter = serde_json::from_str(op.arg("filter")?)?;
        self.log_start(t.clone()).await?;
        let k = crate::observe::key(&t);
        let root = self.data.join("recordings").join(id);
        let quota = op.arg("quotaMiB")?.parse::<u64>().unwrap_or(64) * 1024 * 1024;
        let seconds = op.arg("seconds")?.parse::<u64>().unwrap_or(3600);
        let mut w = Recorder::new(&root, id, t.clone(), f.clone(), quota, 100 * 1024 * 1024)?;
        self.update(id, |task| {
            task.artifact = root.join("manifest.json").to_string_lossy().into();
            task.cancellable = true;
            task.phase = "recording".into();
            task.message = "正在采集；停止后封口，关闭视图不影响采集".into();
        })?;
        let initial = self.log_snapshot(&t, &f, 1, true).await?;
        f.after = initial.latest;
        let (unknown, reason) = self.collect_buffer(id, &t, f, &mut w, seconds).await?;
        w.finish(if unknown { "interrupted" } else { "complete" }, &reason)?;
        if !self.log_views.lock().unwrap().contains(&k) {
            self.log_stop(&t);
        }
        if unknown {
            Err(AppError::new("RESULT_UNKNOWN", reason))
        } else {
            Ok(reason)
        }
    }
    async fn collect_buffer(
        &self,
        id: &str,
        t: &Target,
        mut f: LogFilter,
        w: &mut Recorder,
        seconds: u64,
    ) -> Result<(bool, String)> {
        let mut cursor = f.after;
        let quota = w.quota;
        let root = w.root.clone();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        let mut checkpoint = tokio::time::Instant::now();
        let mut unknown = false;
        let mut ending: Option<(u64, String)> = None;
        let mut drain_pids = None;
        let reason = loop {
            let requested = if ending.is_some() {
                None
            } else if self.cancelled(id) {
                Some("用户停止，已完成文件封口".to_string())
            } else if tokio::time::Instant::now() >= deadline {
                Some("达到采集时限，已完成文件封口".into())
            } else {
                None
            };
            let mut snapshot_error = None;
            let s = if ending.is_some() || requested.is_some() {
                self.log_drain_snapshot(t, &f, 2000, drain_pids.as_deref())?
            } else {
                match self.log_snapshot(t, &f, 2000, false).await {
                    Ok(s) => s,
                    Err(e) => {
                        snapshot_error = Some(e.to_string());
                        self.log_drain_snapshot(t, &f, 2000, None)?
                    }
                }
            };
            if ending.is_none() && (snapshot_error.is_some() || !s.active || requested.is_some()) {
                let gap = snapshot_error.or_else(|| (!s.active).then(|| s.status.clone()));
                let reason = if gap.is_some() {
                    unknown = true;
                    "日志连接或进程查询已结束，采集段已保留".into()
                } else {
                    requested.unwrap()
                };
                if let Some(status) = &gap {
                    // Keep the known gap even if draining exhausts the quota.
                    w.gap(status)?;
                }
                drain_pids = Some(s.pids.clone());
                ending = Some((s.latest, reason));
                // Capture a finite boundary. A user stop must not keep following
                // new lines from a collector still used by a live view.
                f.before = if f.before == 0 {
                    s.latest
                } else {
                    f.before.min(s.latest)
                };
            }
            if s.oldest > cursor + 1 {
                w.gap(&format!(
                    "缓冲超限，遗漏序号 {}..{}",
                    cursor + 1,
                    s.oldest - 1
                ))?;
            }
            let mut quota_hit = false;
            for row in &s.rows {
                if row.level == "MARK" && row.raw.contains("应用进程集合变化") {
                    w.gap("应用进程变化，采集分段；进程刷新间隔可能存在缺口")?;
                }
                if !w.line(&row.raw)? {
                    quota_hit = true;
                    break;
                }
            }
            cursor = s.rows.last().map(|r| r.seq).unwrap_or(s.latest);
            if s.rows.len() < 2000 {
                cursor = s.latest;
            }
            f.after = cursor;
            if quota_hit {
                break if unknown {
                    format!(
                        "达到总配额，已保留完成的采集段；{}",
                        ending.as_ref().unwrap().1
                    )
                } else {
                    "达到总配额，已保留完成的采集段".into()
                };
            }
            if let Some((last, reason)) = &ending {
                if s.rows.len() < 2000 || cursor >= *last {
                    break reason.clone();
                }
            }
            if checkpoint.elapsed() > Duration::from_secs(1) {
                w.checkpoint()?;
                self.update(id, |task| {
                    task.message = format!(
                        "已采集 {:.1} MiB / {} MiB · {} 段 · 缺口 {}",
                        w.manifest.bytes as f64 / 1048576.0,
                        quota / 1048576,
                        w.manifest.files.len(),
                        w.manifest.gaps
                    )
                })?;
                if crate::bootstrap::free_space(&root) < 16 * 1024 * 1024 {
                    w.gap("磁盘空间不足，停止采集")?;
                    unknown = true;
                    break "磁盘空间不足，完成段已保留".into();
                }
                checkpoint = tokio::time::Instant::now();
            }
            if s.rows.len() < 2000 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        Ok((unknown, reason))
    }
    pub fn recover_recordings(&self) -> Result<()> {
        for task in self
            .store
            .review_tasks()?
            .into_iter()
            .filter(|t| t.operation.kind == "log_record")
        {
            let root = self.data.join("recordings").join(&task.id);
            let Ok(_task_use) = self.use_task_record(&task.id, false) else {
                continue;
            };
            let path = root.join("manifest.json");
            if !path.is_file() {
                continue;
            }
            if let Err(error) = self.recover_recording(&task.id, &root, &path) {
                self.report_recovery_failure(&path, &error, error.code == "FORMAT_ERROR");
                let _ = self.update(&task.id, |t| {
                    t.message = format!("采集恢复未完成，原文件与诊断已保留：{error}");
                });
            }
        }
        Ok(())
    }
    fn recover_recording(&self, task_id: &str, root: &Path, path: &Path) -> Result<()> {
        let mut m: Manifest = serde_json::from_slice(&std::fs::read(path)?)?;
        if m.task_id != task_id
            || (m.state != "recording" && !m.files.iter().any(|s| s.name.ends_with(".part")))
        {
            return Ok(());
        }
        let interrupted = m.state == "recording";
        // A sealed segment already has committed integrity evidence. Verify
        // all such files before renaming any parts or modifying the manifest.
        for s in &m.files {
            if s.name.contains(['\\', '/'])
                || !s.name.starts_with("segment-")
                || !(s.name.ends_with(".log") || s.name.ends_with(".log.part"))
            {
                return Err(AppError::new("UNSAFE_RECORDING", "采集清单路径无效"));
            }
            if s.name.ends_with(".log") {
                let sealed = root.join(&s.name);
                if s.sha256.is_empty()
                    || sealed.metadata()?.len() != s.bytes
                    || digest_file(&sealed)? != s.sha256
                {
                    return Err(AppError::new(
                        "ARTIFACT_CHANGED",
                        "已封口采集段内容与原摘要不符，保留原清单供检查",
                    )
                    .detail(&s.name));
                }
            }
        }
        for s in &mut m.files {
            if !s.name.ends_with(".part") {
                continue;
            }
            let from = root.join(&s.name);
            let name = s.name.trim_end_matches(".part").to_string();
            let sealed = root.join(&name);
            if name != s.name {
                if from.is_file() && sealed.exists() {
                    return Err(AppError::new(
                        "RECOVERY_CONFLICT",
                        "采集段同时存在暂存与封口文件，已保留两者供检查",
                    ));
                }
                if from.is_file() {
                    std::fs::rename(&from, root.join(&name))?;
                }
            }
            // seal() may have renamed the file before the manifest commit.
            // Complete only uncommitted metadata, including manifests
            // already marked interrupted by an older application version.
            s.bytes = sealed.metadata()?.len();
            s.sha256 = digest_file(&sealed)?;
            s.name = name;
        }
        m.bytes = m.files.iter().map(|s| s.bytes).sum();
        m.state = "interrupted".into();
        m.reason = "应用中断；已保留并封口现有字节，末尾可能是不完整日志行；未重放采集".into();
        if interrupted {
            m.gaps += 1;
        }
        save_manifest(root, &m)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn process_refresh_failure_drains_last_verified_tail() {
        for mode in [
            "query-failure",
            "query-quota",
            "cancel",
            "deadline",
            "cancel-after-restart",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let app = App::new(dir.path().into()).unwrap();
            let t = crate::regression_tests::target(1, 1);
            app.epoch.store(
                if mode.starts_with("query") || mode == "cancel-after-restart" {
                    2
                } else {
                    1
                },
                std::sync::atomic::Ordering::SeqCst,
            );
            let k = crate::observe::key(&t);
            let mut task = crate::regression_tests::task(mode, "running");
            task.cancel_requested = mode.starts_with("cancel");
            app.store.save_task(&task).unwrap();
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            app.log_children.lock().unwrap().insert(k.clone(), tx);
            app.process_cache.lock().unwrap().insert(
                format!("{k}:dev.example"),
                crate::observe::ProcessCache::expired_fixture(vec![123]),
            );
            {
                let mut logs = app.logs.lock().unwrap();
                let buffer = logs.entry(k).or_default();
                for i in 0..5000 {
                    buffer.push(&format!("10-18 01:23:45.123 123 123 I App: buffered-{i}"));
                }
                buffer.push("10-18 01:23:45.124 999 999 I Other: unrelated application");
            }
            let root = app.data.join("recordings").join(mode);
            let filter = LogFilter {
                package: "dev.example".into(),
                ..Default::default()
            };
            let mut writer = Recorder::new(
                &root,
                mode,
                t.clone(),
                filter.clone(),
                if mode == "query-quota" {
                    512
                } else {
                    8 * 1024 * 1024
                },
                1024 * 1024,
            )
            .unwrap();
            let (unknown, reason) = app
                .collect_buffer(
                    mode,
                    &t,
                    filter,
                    &mut writer,
                    if mode == "deadline" { 0 } else { 60 },
                )
                .await
                .unwrap();
            assert_eq!(
                unknown,
                mode.starts_with("query") || mode == "cancel-after-restart"
            );
            let manifest = writer
                .finish(if unknown { "interrupted" } else { "complete" }, &reason)
                .unwrap();
            let text: String = manifest
                .files
                .iter()
                .map(|s| std::fs::read_to_string(root.join(&s.name)).unwrap())
                .collect();
            let recorded = text
                .lines()
                .filter(|l| l.contains("App: buffered-"))
                .count();
            if mode == "query-quota" {
                assert!(recorded > 0 && recorded < 5000);
            } else {
                assert_eq!(recorded, 5000);
            }
            assert!(!text.contains("unrelated application"));
            if unknown {
                assert!(text.contains("STALE_SESSION"));
                assert_eq!(manifest.gaps, 1);
            }
        }
    }
    #[tokio::test]
    async fn ended_stream_drains_every_buffered_batch_before_sealing() {
        for count in [0, 1999, 2000, 5000, 10000] {
            let dir = tempfile::tempdir().unwrap();
            let app = App::new(dir.path().into()).unwrap();
            let t = crate::regression_tests::target(1, 1);
            app.store
                .save_task(&crate::regression_tests::task("ended", "running"))
                .unwrap();
            {
                let mut logs = app.logs.lock().unwrap();
                let buffer = logs.entry(crate::observe::key(&t)).or_default();
                for i in 0..count {
                    buffer.push(&format!("10-18 01:23:45.123 123 123 I Review: line-{i}"));
                }
            }
            let root = app.data.join("recordings/ended");
            let mut writer = Recorder::new(
                &root,
                "ended",
                t.clone(),
                Default::default(),
                8 * 1024 * 1024,
                1024 * 1024,
            )
            .unwrap();
            let (unknown, reason) = app
                .collect_buffer("ended", &t, Default::default(), &mut writer, 60)
                .await
                .unwrap();
            assert!(unknown);
            let manifest = writer.finish("interrupted", &reason).unwrap();
            let text: String = manifest
                .files
                .iter()
                .map(|s| std::fs::read_to_string(root.join(&s.name)).unwrap())
                .collect();
            assert_eq!(
                text.lines()
                    .filter(|line| line.contains("Review: line-"))
                    .count(),
                count
            );
            assert_eq!(manifest.gaps, 1);
            if count > 0 {
                assert!(text.contains(&format!("line-{}\n", count - 1)));
            }
        }
    }
    #[tokio::test]
    async fn requested_stop_drains_a_finite_boundary_and_honors_quota() {
        for mode in ["cancel", "deadline", "quota"] {
            let dir = tempfile::tempdir().unwrap();
            let app = App::new(dir.path().into()).unwrap();
            app.epoch.store(1, std::sync::atomic::Ordering::SeqCst);
            let t = crate::regression_tests::target(1, 1);
            let mut task = crate::regression_tests::task("stop", "running");
            task.cancel_requested = mode == "cancel";
            app.store.save_task(&task).unwrap();
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            app.log_children
                .lock()
                .unwrap()
                .insert(crate::observe::key(&t), tx);
            {
                let mut logs = app.logs.lock().unwrap();
                let buffer = logs.entry(crate::observe::key(&t)).or_default();
                for i in 0..5000 {
                    buffer.push(&format!("10-18 01:23:45.123 123 123 I Review: line-{i}"));
                }
            }
            let root = app.data.join("recordings/stop");
            let quota = if mode == "quota" {
                512
            } else {
                8 * 1024 * 1024
            };
            let mut writer = Recorder::new(
                &root,
                "stop",
                t.clone(),
                Default::default(),
                quota,
                1024 * 1024,
            )
            .unwrap();
            let (unknown, reason) = app
                .collect_buffer(
                    "stop",
                    &t,
                    Default::default(),
                    &mut writer,
                    if mode == "deadline" { 0 } else { 60 },
                )
                .await
                .unwrap();
            assert!(!unknown);
            let manifest = writer.finish("complete", &reason).unwrap();
            assert!(manifest.bytes <= quota);
            let saved: usize = manifest
                .files
                .iter()
                .map(|s| {
                    std::fs::read_to_string(root.join(&s.name))
                        .unwrap()
                        .lines()
                        .count()
                })
                .sum();
            if mode == "quota" {
                assert!(saved < 5000 && reason.contains("配额"));
            } else {
                assert_eq!(saved, 5000);
            }
        }
    }
    #[test]
    fn rotates_and_stops_at_quota() {
        let d = tempfile::tempdir().unwrap();
        let mut r = Recorder::new(
            d.path(),
            "t",
            Target {
                serial: "A".into(),
                transport_id: "1".into(),
                server_epoch: 1,
            },
            Default::default(),
            24,
            10,
        )
        .unwrap();
        assert!(r.line("12345678").unwrap());
        assert!(r.line("abcdefgh").unwrap());
        assert!(!r.line("morethanquota").unwrap());
        let m = r.finish("complete", "quota").unwrap();
        assert_eq!(m.files.len(), 2);
        assert!(m
            .files
            .iter()
            .all(|s| !s.name.ends_with("part") && !s.sha256.is_empty()));
    }
    #[test]
    fn interrupted_stream_is_sealed_without_replaying() {
        let d = tempfile::tempdir().unwrap();
        let app = App::new(d.path().into()).unwrap();
        let target = Target {
            serial: "A".into(),
            transport_id: "1".into(),
            server_epoch: 1,
        };
        let root = app.data.join("recordings/crash");
        let mut w = Recorder::new(&root, "crash", target, Default::default(), 1024, 512).unwrap();
        w.line("already captured").unwrap();
        w.checkpoint().unwrap();
        drop(w);
        let task:Task=serde_json::from_value(serde_json::json!({"id":"crash","requestId":"crash","title":"record","operation":{"kind":"log_record","args":{},"target":null},"state":"needs_review","phase":"recovery","progress":null,"message":"interrupted","log":[],"createdAt":1,"updatedAt":1,"cancelRequested":false,"cancellable":false,"artifact":root.join("manifest.json"),"licensePrompt":"","confirmedLicenses":[]})).unwrap();
        app.store.save_task(&task).unwrap();
        app.recover_recordings().unwrap();
        app.recover_recordings().unwrap();
        let m: Manifest =
            serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(m.state, "interrupted");
        assert_eq!(m.files.len(), 1);
        assert_eq!(
            std::fs::read_to_string(root.join(&m.files[0].name)).unwrap(),
            "already captured\n"
        );
        assert_eq!(app.store.task("crash").unwrap().state, "needs_review");
    }
}
