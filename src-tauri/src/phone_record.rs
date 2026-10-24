use crate::{app::App, domain::*, phone_files::quote};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

pub fn paths(id: &str) -> Result<(String, String)> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("RECORDING_ID", "录屏任务标识无效"))?;
    Ok((
        format!("/sdcard/Movies/.devkit-record-{id}.mp4"),
        format!("/sdcard/Movies/.devkit-record-{id}.pid"),
    ))
}
pub fn plan(
    app: &App,
    op: &mut Operation,
    effects: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let target = op
        .target
        .as_ref()
        .ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择录屏设备"))?;
    op.args.remove("recordingResolved");
    op.args.remove("deviceCleanupPending");
    if matches!(op.kind.as_str(), "screen_recover" | "screen_cleanup") {
        let source = app.store.task(op.arg("sourceTaskId")?)?;
        if source.operation.kind != "screen_record"
            || source.active()
            || source.operation.target.as_ref().map(|t| &t.serial) != Some(&target.serial)
        {
            return Err(AppError::new(
                "RECORDING_SOURCE",
                "只能取回同一设备上已结束或中断的原录屏",
            ));
        }
        paths(&source.id)?;
        if op.kind == "screen_cleanup" {
            effects.push(format!(
                "放弃原任务 {} 的设备录屏，仅删除本任务的 MP4、PID 与日志；不会停止其他进程",
                source.id
            ));
            warnings.push("未取回的设备原录屏将永久删除，不能再恢复；已经保存到本机的 MP4 与待保存文件保留。仍在录制时拒绝清理。".into());
            return Ok(());
        }
        effects.push(format!(
            "取回原任务 {} 的设备临时 MP4；不会重新录制",
            source.id
        ));
    } else {
        let seconds = op.arg("seconds")?.parse::<u32>().unwrap_or(0);
        if !(1..=180).contains(&seconds)
            || !["original", "720p"].contains(&op.optional("resolution"))
        {
            return Err(AppError::new(
                "RECORDING_OPTIONS",
                "录屏时长限 1～180 秒，分辨率选择原始或 720p",
            ));
        }
        if app
            .store
            .active_tasks()?
            .iter()
            .any(|t| t.operation.kind == "screen_record" && t.operation.target == op.target)
        {
            return Err(AppError::new("RECORDING_ACTIVE", "该设备已有录屏任务"));
        }
        effects.push(format!(
            "静音 MP4 · {seconds} 秒 · {} · 8 Mbps",
            if op.optional("resolution") == "original" {
                "原始分辨率"
            } else {
                "最长边 1280，保持比例"
            }
        ));
        effects.push("仅停止本任务创建并核验过 PID 与输出路径的 screenrecord 进程".into());
    }
    crate::capacity::budget(&[(app.data.clone(), 256 * 1024 * 1024, "录屏取回暂存")])?;
    warnings.push("Android 系统录屏不含声音，受保护内容可能黑屏。录制时请保持方向；编码器不支持原始分辨率时可重新选择 720p。完整取回并验证 MP4 后才清理本任务的设备临时文件。".into());
    Ok(())
}
// Checks both PID and the exact output argument before sending SIGINT. Never uses killall/pkill.
fn process_script(video: &str, pid: &str, stop: bool) -> String {
    format!("set -e; [ ! -L {p} ]; if [ -f {p} ]; then n=$(cat {p}); case \"$n\" in ''|*[!0-9]*) exit 74;; esac; if [ -e /proc/$n/cmdline ]; then c=$(tr '\\000' '\\n' < /proc/$n/cmdline); if [ -z \"$c\" ]; then printf FINISHED; exit 0; fi; printf '%s\\n' \"$c\" | grep -Fx {v} >/dev/null; printf '%s\\n' \"$c\" | grep -E '^(/system/bin/)?screenrecord$' >/dev/null; {action}; else printf FINISHED; fi; else printf FINISHED; fi",p=quote(pid),v=quote(video),action=if stop {"kill -2 \"$n\"; printf STOPPED"} else {"printf ACTIVE"})
}
impl App {
    pub fn recover_committed_phone_recordings(&self) -> Result<()> {
        for source in self.store.review_tasks()? {
            if source.operation.kind != "screen_record" || !source.artifact.is_empty() {
                continue;
            }
            let Ok((_, _)) = paths(&source.id) else {
                continue;
            };
            let path = self
                .data
                .join("artifacts")
                .join(format!("{}.mp4", source.id));
            if !path.is_file()
                || crate::phone_files::regular_local(&path, false).is_err()
                || validate_mp4(&path).is_err()
            {
                continue;
            }
            self.store
                .register_artifact(&source.id, &path, "video", "设备录屏", &source.id)?;
            self.update(&source.id, |task| {
                task.artifact = path.to_string_lossy().into();
                task.state = "succeeded".into();
                task.phase = "complete".into();
                task.progress = Some(100.0);
                task.cancellable = false;
                task.operation
                    .args
                    .insert("deviceCleanupPending".into(), "true".into());
                task.message =
                    "已恢复本机完整 MP4，无需重录；连接原设备后可清理设备临时文件".into();
            })?;
        }
        Ok(())
    }
    pub async fn cleanup_phone_recording(
        &self,
        op: &Operation,
        settings: &Settings,
    ) -> Result<String> {
        let source = self.store.task(op.arg("sourceTaskId")?)?;
        let _source_use = self.use_task_record(&source.id, true)?;
        let target = op
            .target
            .as_ref()
            .ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择原设备"))?;
        if source.active()
            || source.operation.kind != "screen_record"
            || source
                .operation
                .target
                .as_ref()
                .map(|target| &target.serial)
                != Some(&target.serial)
        {
            return Err(AppError::new(
                "RECORDING_SOURCE",
                "只能清理原设备已结束的本任务录屏",
            ));
        }
        let (video, pid) = paths(&source.id)?;
        self.check_remote(settings, target, "/sdcard/Movies")
            .await?;
        if self
            .remote_shell(settings, target, process_script(&video, &pid, false), 15)
            .await?
            .text()
            .trim()
            != "FINISHED"
        {
            return Err(AppError::new(
                "RECORDING_ACTIVE",
                "原录屏仍在运行，请等待原定时长结束后清理",
            ));
        }
        self.remote_shell(
            settings,
            target,
            format!(
                "[ ! -L {v} ] && [ ! -L {p} ] && [ ! -L {l} ] && rm -f -- {v} {p} {l}",
                v = quote(&video),
                p = quote(&pid),
                l = quote(&format!("{pid}.log"))
            ),
            15,
        )
        .await?;
        self.update(&source.id, |task| {
            task.operation
                .args
                .insert("recordingResolved".into(), "true".into());
            task.operation.args.remove("deviceCleanupPending");
            task.state = if task.artifact.is_empty() {
                "cancelled"
            } else {
                "succeeded"
            }
            .into();
            task.phase = "complete".into();
            task.message = "已清理本任务设备录屏原件；本机 MP4 与待保存文件保留".into();
        })?;
        Ok("已清理本任务设备录屏原件；本机 MP4 与待保存文件保留".into())
    }
    pub async fn screen_record(
        &self,
        id: &str,
        op: &Operation,
        settings: &Settings,
    ) -> Result<String> {
        let target = op
            .target
            .as_ref()
            .ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择设备"))?;
        let source_id = if op.kind == "screen_recover" {
            op.arg("sourceTaskId")?
        } else {
            id
        };
        let _source_use = if source_id != id {
            Some(self.use_task_record(source_id, true)?)
        } else {
            None
        };
        let (video, pid) = paths(source_id)?;
        self.line(id, format!("本任务设备原件：{video}；进程归属记录：{pid}"))?;
        self.check_remote(settings, target, "/sdcard/Movies")
            .await?;
        let recovering = op.kind == "screen_recover";
        if !recovering {
            let seconds = op
                .arg("seconds")?
                .parse::<u64>()
                .map_err(|_| AppError::new("RECORDING_OPTIONS", "录屏时长无效"))?;
            let mut options = format!("--time-limit {seconds} --bit-rate 8000000");
            if op.optional("resolution") == "720p" {
                let out = self
                    .remote_shell(settings, target, "wm size".into(), 15)
                    .await?;
                options.push_str(&format!(" --size {}", reduced_size(&out.text())?));
            }
            crate::capacity::budget(&[(self.data.clone(), 256 * 1024 * 1024, "录屏取回")])?;
            let stat = self
                .remote_shell(settings, target, "df -k /sdcard/Movies".into(), 15)
                .await?;
            let available = stat
                .text()
                .lines()
                .last()
                .and_then(|l| l.split_whitespace().nth(3))
                .and_then(|s| s.parse::<u64>().ok());
            if available.is_some_and(|n| n < seconds * 1100 + 8192) {
                return Err(AppError::new("DISK_SPACE", "设备可用空间不足，未开始录屏"));
            }
            self.update(id, |t| {
                t.cancellable = true;
                t.phase = "recording".into();
                t.message = format!("录制中 · 最多 {seconds} 秒；停止后自动取回");
            })?;
            let log = format!("{pid}.log");
            let script=format!("set -e; [ ! -e {v} ]; [ ! -L {v} ]; [ ! -e {p} ]; [ ! -e {l} ]; command -v nohup >/dev/null; nohup screenrecord {options} {v} > {l} 2>&1 < /dev/null & n=$!; printf '%s' \"$n\" > {p}",v=quote(&video),p=quote(&pid),l=quote(&log));
            self.remote_shell(settings, target, script, 15)
                .await
                .map_err(|e| {
                    AppError::new(
                        "RESULT_UNKNOWN",
                        "录屏启动响应未能确认；请稍后取回原文件，不会自动重新录制",
                    )
                    .detail(format!("{}\n{}", e, e.detail))
                })?;
            // Detach only this finite, owned recorder. It can finish its MP4 after the app exits.
            let started = tokio::time::Instant::now();
            let mut stopped = false;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                self.check_remote(settings, target, "/sdcard/Movies")
                    .await?;
                let elapsed = started.elapsed().as_secs();
                let response = self
                    .remote_shell(
                        settings,
                        target,
                        process_script(&video, &pid, self.cancelled(id) && !stopped),
                        15,
                    )
                    .await
                    .map_err(|e| {
                        AppError::new("RESULT_UNKNOWN", "录屏连接中断；设备原件保留，请稍后取回")
                            .detail(format!("{}\n{}", e, e.detail))
                    })?;
                match response.text().trim() {
                    "FINISHED" => break,
                    "STOPPED" => stopped = true,
                    "ACTIVE" => {}
                    _ => {
                        return Err(AppError::new(
                            "RESULT_UNKNOWN",
                            "无法确认原录屏进程状态，未停止其他进程",
                        ))
                    }
                }
                self.update(id, |t| {
                    t.progress = Some((elapsed as f64 / seconds as f64 * 85.0).min(85.0));
                    t.message = format!(
                        "录制中 · {elapsed} 秒 / {seconds} 秒{}",
                        if stopped {
                            " · 正在停止并封口"
                        } else {
                            ""
                        }
                    );
                })?;
                if elapsed > seconds + 30 {
                    return Err(AppError::new(
                        "RESULT_UNKNOWN",
                        "录屏结束尚未确认；保留原件，请稍后取回",
                    ));
                }
            }
        } else {
            let status = self
                .remote_shell(settings, target, process_script(&video, &pid, false), 15)
                .await?;
            if status.text().trim() != "FINISHED" {
                return Err(AppError::new(
                    "RECORDING_ACTIVE",
                    "原录屏仍在运行，请等待原定时长结束后取回",
                ));
            }
        }
        self.update(id, |t| {
            t.phase = "pulling".into();
            t.message = "录屏已停止，正在取回并验证 MP4".into();
            t.cancellable = false;
        })?;
        if let Ok(log) = self
            .remote_shell(
                settings,
                target,
                format!(
                    "[ ! -L {p} ] && tail -c 4096 {p}",
                    p = quote(&format!("{pid}.log"))
                ),
                15,
            )
            .await
        {
            if !log.text().trim().is_empty() {
                self.line(id, log.text())?;
            }
        }
        let root = self.data.join("artifacts");
        std::fs::create_dir_all(&root)?;
        let temporary = root.join(format!("{source_id}.mp4.part"));
        let destination = root.join(format!("{source_id}.mp4"));
        self.line(id, format!("本地录屏暂存：{}", temporary.display()))?;
        if destination.exists() {
            validate_mp4(&destination)?;
        } else {
            let size = self
                .remote_shell(
                    settings,
                    target,
                    format!(
                        "[ ! -L {v} ] && [ -f {v} ] && stat -c %s {v}",
                        v = quote(&video)
                    ),
                    15,
                )
                .await?;
            if !size
                .text()
                .trim()
                .parse::<u64>()
                .ok()
                .is_some_and(|n| (32..=256 * 1024 * 1024).contains(&n))
            {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    "设备录屏为空或超过 256 MiB，保留原件，请在设备上检查",
                ));
            }
            let mut pull = self
                .bound_adb_spec(
                    settings,
                    target,
                    vec![
                        "pull".into(),
                        video.clone(),
                        temporary.file_name().unwrap().to_string_lossy().into(),
                    ],
                    300,
                )
                .await?;
            pull.cwd = Some(root.clone());
            crate::process::run(pull)
                .await
                .map_err(|e| {
                    AppError::new("RESULT_UNKNOWN", "录屏取回未完成，设备原件保留")
                        .detail(format!("{}\n{}", e, e.detail))
                })?
                .success()?;
            validate_mp4(&temporary).map_err(|e| {
                AppError::new(
                    "RESULT_UNKNOWN",
                    "MP4 尚不完整；已保留设备原件及本地临时文件",
                )
                .detail(format!("{}\n{}", e, e.detail))
            })?;
            let remote_hash = self.remote_hash(settings, target, &video).await?;
            if !remote_hash.is_empty() && remote_hash != digest_file(&temporary)? {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    "录屏 SHA-256 不一致，已保留设备原件与本地暂存文件",
                ));
            }
            let stat = self
                .remote_shell(
                    settings,
                    target,
                    format!("stat -c %s {}", quote(&video)),
                    15,
                )
                .await?;
            if stat.text().trim().parse::<u64>().ok() != Some(temporary.metadata()?.len()) {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    "录屏大小不一致；设备原件保留",
                ));
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(&temporary)?
                .sync_all()?;
            std::fs::rename(&temporary, &destination)?;
        }
        self.store
            .register_artifact(source_id, &destination, "video", "设备录屏", source_id)?;
        self.update(source_id, |t| {
            t.artifact = destination.to_string_lossy().into();
        })?;
        // The recover task points to the same immutable source artifact without duplicating it.
        if recovering {
            self.update(id, |t| {
                t.artifact = destination.to_string_lossy().into();
            })?;
            self.update(source_id, |t| {
                t.state = "succeeded".into();
                t.phase = "complete".into();
                t.message = "已从原设备取回并验证 MP4，未重新录制".into();
                t.progress = Some(100.0);
                t.cancellable = false;
            })?;
        }
        let cleanup = async {
            self.check_remote(settings, target, "/sdcard/Movies")
                .await?;
            self.remote_shell(
                settings,
                target,
                format!(
                    "[ ! -L {v} ] && [ ! -L {p} ] && [ ! -L {l} ] && rm -f -- {v} {p} {l}",
                    v = quote(&video),
                    p = quote(&pid),
                    l = quote(&format!("{pid}.log"))
                ),
                15,
            )
            .await
        }
        .await;
        self.update(source_id, |task| {
            task.operation
                .args
                .insert("recordingResolved".into(), cleanup.is_ok().to_string());
            if cleanup.is_err() {
                task.operation
                    .args
                    .insert("deviceCleanupPending".into(), "true".into());
            } else {
                task.operation.args.remove("deviceCleanupPending");
            }
        })?;
        Ok(format!(
            "MP4 已完整保存到待保存文件；已验证 MP4 结构及大小{}",
            if cleanup.is_err() {
                "；设备临时文件清理未完成，请在设备上检查"
            } else {
                "，已清理本任务设备临时文件"
            }
        ))
    }
}
fn reduced_size(text: &str) -> Result<String> {
    let (w, h) = text
        .lines()
        .rev()
        .filter_map(|l| l.split_once(':'))
        .filter_map(|(_, s)| s.trim().split_once('x'))
        .find_map(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .ok_or_else(|| AppError::new("SCREEN_SIZE", "无法识别设备分辨率"))?;
    if w == 0 || h == 0 {
        return Err(AppError::new("SCREEN_SIZE", "设备分辨率无效"));
    }
    let scale = (1280.0 / w.max(h) as f64).min(1.0);
    Ok(format!(
        "{}x{}",
        ((w as f64 * scale) as u32 / 2 * 2).max(2),
        ((h as f64 * scale) as u32 / 2 * 2).max(2)
    ))
}
pub fn validate_mp4(path: &Path) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    if !(32..=256 * 1024 * 1024).contains(&length) {
        return Err(AppError::new("MP4_INVALID", "录屏大小无效或超过 256 MiB"));
    }
    let mut offset = 0;
    let mut found = std::collections::BTreeSet::new();
    let mut valid_video = false;
    let mut media_bytes = 0u64;
    let mut sample_bytes = 0u64;
    while offset < length {
        if length - offset < 8 {
            return Err(AppError::new("MP4_INVALID", "MP4 尾部不完整"));
        }
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let mut size = u32::from_be_bytes(header[..4].try_into().unwrap()) as u64;
        let mut minimum = 8;
        if size == 1 {
            let mut large = [0; 8];
            file.read_exact(&mut large)?;
            size = u64::from_be_bytes(large);
            minimum = 16;
        }
        if size == 0 {
            size = length - offset;
        }
        if size < minimum || size > length - offset {
            return Err(AppError::new("MP4_INVALID", "MP4 box 越界"));
        }
        if size > minimum {
            found.insert(header[4..8].to_vec());
        }
        if &header[4..8] == b"mdat" {
            media_bytes = media_bytes.saturating_add(size - minimum);
        }
        if &header[4..8] == b"moov" {
            if size - minimum > 16 * 1024 * 1024 {
                return Err(AppError::new("MP4_INVALID", "MP4 索引超限"));
            }
            let mut bytes = vec![0; (size - minimum) as usize];
            file.read_exact(&mut bytes)?;
            for (kind, track) in boxes(&bytes)? {
                if kind == b"trak" {
                    let mut info = Track::default();
                    track_info(track, 0, &mut info)?;
                    if info.video
                        && info.samples > 0
                        && info.duration > 0
                        && info.timescale > 0
                        && info.sample_bytes > 0
                    {
                        valid_video = true;
                        sample_bytes = sample_bytes.saturating_add(info.sample_bytes);
                    }
                }
            }
        }
        offset += size;
        file.seek(SeekFrom::Start(offset))?;
    }
    if ![b"ftyp", b"moov", b"mdat"]
        .iter()
        .all(|s| found.contains(s.as_slice()))
    {
        return Err(AppError::new("MP4_INVALID", "缺少完整的 ftyp/moov/mdat"));
    }
    if !valid_video || sample_bytes > media_bytes {
        return Err(AppError::new(
            "MP4_INVALID",
            "MP4 没有完整、可播放的视频采样或媒体数据不完整",
        ));
    }
    Ok(())
}
#[derive(Default)]
struct Track {
    video: bool,
    samples: u32,
    duration: u64,
    timescale: u32,
    sample_bytes: u64,
}
fn invalid_mp4() -> AppError {
    AppError::new("MP4_INVALID", "MP4 视频索引不完整")
}
fn be32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or_else(invalid_mp4)?
            .try_into()
            .unwrap(),
    ))
}
fn be64(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_be_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or_else(invalid_mp4)?
            .try_into()
            .unwrap(),
    ))
}
fn boxes(mut bytes: &[u8]) -> Result<Vec<(&[u8], &[u8])>> {
    let mut result = vec![];
    while !bytes.is_empty() {
        if bytes.len() < 8 {
            return Err(invalid_mp4());
        }
        let size = be32(bytes, 0)? as u64;
        let header = if size == 1 { 16 } else { 8 };
        let size = if size == 1 {
            be64(bytes, 8)?
        } else if size == 0 {
            bytes.len() as u64
        } else {
            size
        };
        if size < header as u64 || size > bytes.len() as u64 || result.len() > 100_000 {
            return Err(invalid_mp4());
        }
        let size = size as usize;
        result.push((&bytes[4..8], &bytes[header..size]));
        bytes = &bytes[size..];
    }
    Ok(result)
}
fn track_info(bytes: &[u8], depth: u8, track: &mut Track) -> Result<()> {
    if depth > 6 {
        return Err(invalid_mp4());
    }
    for (kind, payload) in boxes(bytes)? {
        match kind {
            b"mdia" | b"minf" | b"stbl" => track_info(payload, depth + 1, track)?,
            b"hdlr" => track.video = payload.get(8..12) == Some(b"vide"),
            b"mdhd" => {
                let (timescale, duration) = if payload.first() == Some(&1) {
                    (be32(payload, 20)?, be64(payload, 24)?)
                } else {
                    (be32(payload, 12)?, be32(payload, 16)? as u64)
                };
                track.timescale = timescale;
                track.duration = duration;
            }
            b"stsz" => {
                let fixed = be32(payload, 4)?;
                let count = be32(payload, 8)?;
                if count > 200_000 {
                    return Err(invalid_mp4());
                }
                let mut total = 0u64;
                if fixed > 0 {
                    total = fixed as u64 * count as u64;
                } else {
                    for i in 0..count as usize {
                        total = total.saturating_add(be32(payload, 12 + i * 4)? as u64);
                    }
                }
                track.samples = count;
                track.sample_bytes = total;
            }
            _ => {}
        }
    }
    Ok(())
}
