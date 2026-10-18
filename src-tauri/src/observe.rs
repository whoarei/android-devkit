use crate::{android, app::App, domain::*, process::Spec};
use std::{
    collections::HashSet,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;
pub struct ProcessCache {
    at: Instant,
    pids: Vec<u32>,
    status: String,
}
#[cfg(test)]
impl ProcessCache {
    pub(crate) fn expired_fixture(pids: Vec<u32>) -> Self {
        Self {
            at: Instant::now() - Duration::from_secs(30),
            pids,
            status: "最后确认的应用进程".into(),
        }
    }
}
pub fn key(t: &Target) -> String {
    format!("{}:{}:{}", t.server_epoch, t.transport_id, t.serial)
}
fn validate_filter(f: &LogFilter) -> Result<()> {
    if !f.package.is_empty() {
        crate::app::validate_app_package(&f.package)?;
    }
    if f.keyword.len() > 512
        || f.tag.len() > 128
        || f.pids.len() > 64
        || (!f.level.is_empty()
            && !matches!(f.level.as_str(), "V" | "D" | "I" | "W" | "E" | "F" | "A"))
    {
        return Err(AppError::new("INVALID_FILTER", "日志过滤条件无效或过长"));
    }
    Ok(())
}
pub fn process_ids(output: &str, package: &str) -> Vec<u32> {
    let mut lines = output.lines();
    let h: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    let Some(pid) = h.iter().position(|s| *s == "PID") else {
        return vec![];
    };
    let col = h
        .iter()
        .position(|s| matches!(*s, "NAME" | "CMD" | "COMMAND" | "CMDLINE"))
        .unwrap_or(h.len().saturating_sub(1));
    lines
        .filter_map(|l| {
            let v: Vec<_> = l.split_whitespace().collect();
            let name = *v.get(col)?;
            if name == package
                || name
                    .strip_prefix(package)
                    .is_some_and(|s| s.starts_with(':'))
            {
                v.get(pid)?.parse().ok()
            } else {
                None
            }
        })
        .collect()
}
impl App {
    pub fn prune_logs(&self) {
        const CACHED_SESSIONS: usize = 4;
        const CACHED_BYTES: usize = 32 * 1024 * 1024;
        let Ok(tasks) = self.store.active_tasks() else {
            return;
        };
        let mut protected: HashSet<_> = tasks
            .iter()
            .filter(|t| t.operation.kind == "log_record")
            .filter_map(|t| t.operation.target.as_ref().map(key))
            .collect();
        // Keep this lock order consistent with registration and view release.
        let children = self.log_children.lock().unwrap();
        let views = self.log_views.lock().unwrap();
        protected.extend(children.keys().cloned());
        protected.extend(views.iter().cloned());
        let mut logs = self.logs.lock().unwrap();
        let current = format!("{}:", self.epoch.load(Ordering::SeqCst));
        let mut evicted = vec![];
        logs.retain(|k, _| {
            let keep = k.starts_with(&current) || protected.contains(k);
            if !keep {
                evicted.push(k.clone());
            }
            keep
        });
        let mut idle: Vec<_> = logs
            .iter()
            .filter(|(k, _)| !protected.contains(*k))
            .map(|(k, b)| (k.clone(), b.last_used, b.bytes))
            .collect();
        idle.sort_by_key(|(_, used, _)| *used);
        let mut count = idle.len();
        let mut bytes: usize = idle.iter().map(|(_, _, size)| *size).sum();
        for (k, _, size) in idle {
            if count <= CACHED_SESSIONS && bytes <= CACHED_BYTES {
                break;
            }
            logs.remove(&k);
            evicted.push(k);
            count -= 1;
            bytes -= size;
        }
        drop(logs);
        drop(views);
        drop(children);
        if !evicted.is_empty() {
            self.process_cache
                .lock()
                .unwrap()
                .retain(|k, _| !evicted.iter().any(|old| k.starts_with(&format!("{old}:"))));
        }
    }
    pub async fn log_start(self: &Arc<Self>, target: Target) -> Result<()> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        let raw = android::host_request("host:devices-l")?;
        checked_target(
            &target,
            &android::parse_devices(&String::from_utf8_lossy(&raw), epoch),
            epoch,
        )?;
        let k = key(&target);
        if self.log_children.lock().unwrap().contains_key(&k) {
            return Ok(());
        }
        let adb = android::compatible_adb(&self.store.settings()).await?;
        let mut child = Spec::new(
            adb,
            vec![
                "-t".into(),
                target.transport_id.clone(),
                "logcat".into(),
                "-v".into(),
                "threadtime".into(),
            ],
        )
        .command()
        .kill_on_drop(true)
        .spawn()?;
        let mut out = child.stdout.take().unwrap();
        let mut err = child.stderr.take().unwrap();
        let (tx, mut stop) = tokio::sync::mpsc::channel(1);
        {
            let mut c = self.log_children.lock().unwrap();
            if c.contains_key(&k) {
                return Ok(());
            }
            c.insert(k.clone(), tx);
        }
        {
            let mut logs = self.logs.lock().unwrap();
            let b = logs.entry(k.clone()).or_default();
            b.push(&format!(
                "--- 新采集段 {} · 设备 {} ---",
                now(),
                target.serial
            ));
            b.status = "正在采集".into();
        }
        self.prune_logs();
        let app = self.clone();
        tokio::spawn(async move {
            let errors = tokio::spawn(async move {
                let mut b = [0; 4096];
                while err.read(&mut b).await.is_ok_and(|n| n > 0) {}
            });
            let mut bytes = [0; 32768];
            let mut pending = Vec::new();
            let mut overlong = false;
            let mut check = tokio::time::interval(Duration::from_secs(2));
            let mut reason = "采集已结束".to_string();
            loop {
                tokio::select! {result=out.read(&mut bytes)=>match result{Ok(0)|Err(_)=>{reason="连接中断或日志流已结束；请检查设备后开始新采集段".into();break;},Ok(n)=>{let mut batch=Vec::new();for b in &bytes[..n]{if *b==b'\n'{let mut text=String::from_utf8_lossy(&pending).trim_end_matches('\r').to_string();if overlong{text.push_str(" [单行截断]");}batch.push(text);pending.clear();overlong=false;}else if pending.len()<32768{pending.push(*b);}else{overlong=true;}}let mut logs=app.logs.lock().unwrap();if let Some(buffer)=logs.get_mut(&k){for line in batch{buffer.push(&line);}}}},_=stop.recv()=>break,_=check.tick()=>{let current=app.epoch.load(Ordering::SeqCst);let data=tokio::task::spawn_blocking(||android::host_request("host:devices-l")).await.ok().and_then(|r|r.ok());if current!=target.server_epoch||data.as_ref().is_none_or(|raw|checked_target(&target,&android::parse_devices(&String::from_utf8_lossy(raw),current),current).is_err()){reason="设备会话已变化，采集结束；没有自动切换目标".into();break;}}}
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            errors.abort();
            {
                let mut logs = app.logs.lock().unwrap();
                if let Some(b) = logs.get_mut(&k) {
                    b.push(&format!("--- {reason} ---"));
                    b.status = reason;
                }
            }
            app.log_children.lock().unwrap().remove(&k);
            app.prune_logs();
            app.notify();
        });
        Ok(())
    }
    pub fn log_stop(&self, t: &Target) {
        if let Some(stop) = self.log_children.lock().unwrap().get(&key(t)) {
            let _ = stop.try_send(());
        }
    }
    pub fn log_release(&self, t: &Target) {
        self.log_views.lock().unwrap().remove(&key(t));
        if !self.recording_target(t) {
            self.log_stop(t);
        }
        self.prune_logs();
    }
    pub fn recording_target(&self, t: &Target) -> bool {
        self.store
            .active_tasks()
            .unwrap_or_default()
            .iter()
            .any(|a| a.operation.kind == "log_record" && a.operation.target.as_ref() == Some(t))
    }
    pub async fn log_snapshot(
        &self,
        t: &Target,
        f: &LogFilter,
        limit: usize,
        tail: bool,
    ) -> Result<LogSnapshot> {
        validate_filter(f)?;
        let k = key(t);
        let active = self.log_children.lock().unwrap().contains_key(&k);
        let cache_key = format!("{k}:{}", f.package);
        let cached = self
            .process_cache
            .lock()
            .unwrap()
            .get(&cache_key)
            .filter(|p| !active || p.at.elapsed() < Duration::from_secs(2))
            .map(|p| (p.pids.clone(), p.status.clone()));
        let (pids, process_status) = if f.package.is_empty() {
            (vec![], "全部进程".into())
        } else if let Some(c) = cached {
            c
        } else if !active {
            (vec![], "采集已结束，缺少可验证的应用进程集合".into())
        } else {
            let current = self.epoch.load(Ordering::SeqCst);
            if current != t.server_epoch {
                return Err(AppError::new(
                    "STALE_SESSION",
                    "ADB 服务会话已经变化，请重新选择设备",
                ));
            }
            let raw = android::host_request("host:devices-l")?;
            checked_target(
                t,
                &android::parse_devices(&String::from_utf8_lossy(&raw), current),
                current,
            )?;
            let output = android::adb(
                &self.store.settings(),
                Some(t),
                vec![
                    "shell".into(),
                    "ps".into(),
                    "-A".into(),
                    "-o".into(),
                    "PID,NAME".into(),
                ],
                5,
            )
            .await?;
            let output = if output.code == 0 {
                output
            } else {
                android::adb(
                    &self.store.settings(),
                    Some(t),
                    vec!["shell".into(), "ps".into()],
                    5,
                )
                .await?
                .success()?
            };
            let output_text = output.text();
            if !output_text
                .lines()
                .next()
                .is_some_and(|h| h.split_whitespace().any(|c| c == "PID"))
            {
                return Err(AppError::new(
                    "PROCESS_FILTER_UNAVAILABLE",
                    "设备 ps 输出无法可靠识别 PID；请使用全部进程或显式 PID 过滤",
                ));
            }
            let found = process_ids(&output_text, &f.package);
            let previous = self
                .process_cache
                .lock()
                .unwrap()
                .get(&cache_key)
                .map(|p| p.pids.clone());
            if previous.as_ref().is_some_and(|old| *old != found) {
                if let Some(b) = self.logs.lock().unwrap().get_mut(&k) {
                    b.push(&format!(
                        "--- 应用进程集合变化 {:?} → {:?}；2 秒刷新期间可能遗漏新进程初始日志 ---",
                        previous.unwrap_or_default(),
                        found
                    ));
                }
            }
            let status = if found.is_empty() {
                "等待应用进程；不包含无法归属的隔离进程"
            } else {
                "主进程及包名冒号子进程；不包含无法归属的隔离进程"
            }
            .to_string();
            {
                let mut cache = self.process_cache.lock().unwrap();
                if cache.len() > 128 {
                    cache.clear();
                }
            }
            self.process_cache.lock().unwrap().insert(
                cache_key,
                ProcessCache {
                    at: Instant::now(),
                    pids: found.clone(),
                    status: status.clone(),
                },
            );
            (found, status)
        };
        Ok(self.buffered_snapshot(t, f, limit, tail, active, (pids, process_status)))
    }
    // Draining belongs to the original session. Never query the replacement
    // device, and keep one PID set for every batch up to the captured boundary.
    pub(crate) fn log_drain_snapshot(
        &self,
        t: &Target,
        f: &LogFilter,
        limit: usize,
        frozen_pids: Option<&[u32]>,
    ) -> Result<LogSnapshot> {
        validate_filter(f)?;
        let k = key(t);
        let active = self.log_children.lock().unwrap().contains_key(&k);
        let remembered = if let Some(pids) = frozen_pids {
            Some(pids.to_vec())
        } else {
            self.process_cache
                .lock()
                .unwrap()
                .get(&format!("{k}:{}", f.package))
                .map(|p| p.pids.clone())
        };
        let unverified = !f.package.is_empty() && remembered.is_none();
        let mut snapshot = self.buffered_snapshot(
            t,
            f,
            limit,
            false,
            active,
            (
                remembered.unwrap_or_default(),
                "收尾使用最后确认的应用进程集合".into(),
            ),
        );
        if unverified {
            snapshot.active = false;
            snapshot.status =
                "PROCESS_FILTER_UNAVAILABLE: 缺少最后确认的应用进程集合，无法归属的日志未纳入"
                    .into();
        } else if self.epoch.load(Ordering::SeqCst) != t.server_epoch {
            snapshot.active = false;
            snapshot.status = "STALE_SESSION: ADB 服务会话已经变化，保留原会话缓冲".into();
        }
        Ok(snapshot)
    }
    fn buffered_snapshot(
        &self,
        t: &Target,
        f: &LogFilter,
        limit: usize,
        tail: bool,
        active: bool,
        process: (Vec<u32>, String),
    ) -> LogSnapshot {
        let (pids, process_status) = process;
        let k = key(t);
        let mut logs = self.logs.lock().unwrap();
        let b = logs.get_mut(&k);
        let b = b.map(|b| {
            b.last_used = Some(Instant::now());
            &*b
        });
        let mut rows: Vec<_> = b
            .map(|b| {
                b.lines
                    .iter()
                    .filter(|r| crate::log_buffer::matches(r, f, &pids))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let limit = limit.min(10_000);
        if rows.len() > limit {
            if tail {
                rows.drain(..rows.len() - limit);
            } else {
                rows.truncate(limit);
            }
        }
        LogSnapshot {
            rows,
            latest: b.map(|b| b.latest).unwrap_or(0),
            oldest: b.and_then(|b| b.lines.front().map(|r| r.seq)).unwrap_or(0),
            dropped: b.map(|b| b.dropped).unwrap_or(0),
            buffered: b.map(|b| b.lines.len()).unwrap_or(0),
            active,
            status: b
                .map(|b| b.status.clone())
                .unwrap_or_else(|| "尚未开始采集".into()),
            process_status,
            pids,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drain_freezes_processes_and_sequence_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        app.epoch.store(1, Ordering::SeqCst);
        let t = crate::regression_tests::target(1, 1);
        let k = key(&t);
        let cache_key = format!("{k}:dev.example");
        app.process_cache
            .lock()
            .unwrap()
            .insert(cache_key.clone(), ProcessCache::expired_fixture(vec![123]));
        {
            let mut logs = app.logs.lock().unwrap();
            let buffer = logs.entry(k.clone()).or_default();
            buffer.push("10-18 01:23:45.123 123 123 I App: original process");
            buffer.push("10-18 01:23:45.124 123 123 I App: original tail");
            buffer.push("10-18 01:23:45.125 999 999 I Other: unrelated");
        }
        let mut filter = LogFilter {
            package: "dev.example".into(),
            ..Default::default()
        };
        let first = app.log_drain_snapshot(&t, &filter, 1, None).unwrap();
        assert_eq!(first.rows.len(), 1);
        filter.after = first.rows[0].seq;
        filter.before = first.latest;
        app.process_cache
            .lock()
            .unwrap()
            .insert(cache_key, ProcessCache::expired_fixture(vec![999]));
        app.logs
            .lock()
            .unwrap()
            .get_mut(&k)
            .unwrap()
            .push("10-18 01:23:45.126 123 123 I App: after boundary");
        let second = app
            .log_drain_snapshot(&t, &filter, 2000, Some(&first.pids))
            .unwrap();
        assert_eq!(second.rows.len(), 1);
        assert!(second.rows[0].raw.contains("original tail"));
        app.process_cache.lock().unwrap().clear();
        let missing = app.log_drain_snapshot(&t, &filter, 2000, None).unwrap();
        assert!(!missing.active);
        assert!(missing.status.contains("PROCESS_FILTER_UNAVAILABLE"));
        assert!(missing.rows.is_empty());
        filter.package = "invalid/package".into();
        assert!(app.log_drain_snapshot(&t, &filter, 2000, None).is_err());
    }
    #[tokio::test]
    async fn ended_stream_uses_last_verified_process_map_without_device_queries() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let t = crate::regression_tests::target(1, 1);
        let k = key(&t);
        app.process_cache.lock().unwrap().insert(
            format!("{k}:dev.example"),
            ProcessCache {
                at: Instant::now() - Duration::from_secs(30),
                pids: vec![221],
                status: "最后确认的应用进程".into(),
            },
        );
        {
            let mut logs = app.logs.lock().unwrap();
            let buffer = logs.entry(k).or_default();
            buffer.push("10-18 01:23:45.123 221 221 I App: own tail");
            buffer.push("10-18 01:23:45.124 777 777 I Other: other application");
        }
        let filter = LogFilter {
            package: "dev.example".into(),
            ..Default::default()
        };
        let snapshot = app.log_snapshot(&t, &filter, 2000, false).await.unwrap();
        assert!(!snapshot.active);
        assert_eq!(snapshot.rows.len(), 1);
        assert!(snapshot.rows[0].raw.contains("own tail"));
        assert_eq!(snapshot.pids, vec![221]);
    }
    #[test]
    fn matches_main_and_child_without_prefix_collision() {
        assert_eq!(
            process_ids(
                " PID NAME\n 123 dev.app\n124 dev.app:worker\n125 dev.application\n126 isolated",
                "dev.app"
            ),
            vec![123, 124]
        );
    }
}
