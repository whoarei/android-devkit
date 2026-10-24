//! Shared-storage transfers. Prepare immutable manifests, verify staging, publish last.
use crate::{app::App, domain::*};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Finite reads prevent a concurrently growing source from consuming unbounded staging space.
pub fn stage_file(source: &Path, destination: &Path, expected: u64) -> Result<String> {
    regular_local(source, false)?;
    if expected > MAX_BYTES {
        return Err(AppError::new("TRANSFER_LIMIT", "文件超过暂存上限"));
    }
    fn hash(path: &Path, expected: u64) -> Result<String> {
        let mut input = std::fs::File::open(path)?.take(expected + 1);
        let mut bytes = 0u64;
        let mut digest = sha2::Sha256::new();
        let mut buffer = vec![0; 65536];
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            digest.update(&buffer[..n]);
        }
        if bytes != expected {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "文件在暂存期间改变，请停止写入后重新选择",
            ));
        }
        Ok(format!("{:x}", digest.finalize()))
    }
    let temporary = destination.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut input = std::fs::File::open(source)?.take(expected + 1);
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let bytes = std::io::copy(&mut input, &mut output)?;
        output.flush()?;
        output.sync_all()?;
        drop(output);
        if bytes != expected {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "文件在暂存期间改变，请停止写入后重新选择",
            ));
        }
        let staged = hash(&temporary, expected)?;
        if hash(source, expected)? != staged {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "文件在暂存期间改变，请停止写入后重新选择",
            ));
        }
        crate::recovery::publish_new_file(&temporary, destination)?;
        Ok(staged)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub link: bool,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub modified: u64,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferItem {
    pub relative: String,
    pub directory: bool,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    pub sha256: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSet {
    pub id: String,
    pub direction: String,
    pub target: Target,
    pub directory: String,
    pub items: Vec<TransferItem>,
    pub roots: Vec<String>,
    pub conflicts: Vec<String>,
}

pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn regular_local(path: &Path, directory: bool) -> Result<()> {
    for ancestor in path.ancestors() {
        let m = std::fs::symlink_metadata(ancestor)?;
        let mut link = m.file_type().is_symlink();
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            link |= m.file_attributes() & 0x400 != 0;
        }
        if link {
            return Err(AppError::new(
                "UNSAFE_PATH",
                "不允许符号链接或 Windows 重解析点",
            ));
        }
    }
    let meta = path.metadata()?;
    if if directory {
        !meta.is_dir()
    } else {
        !meta.is_file()
    } {
        return Err(AppError::new("UNSAFE_PATH", "需要普通文件或目录"));
    }
    Ok(())
}
pub fn safe_component(s: &str) -> bool {
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.len() <= 240
        && !s.ends_with(['.', ' '])
        && !s.starts_with(".devkit-")
        && !s
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}
pub fn remote_path(path: &str) -> Result<String> {
    let path = path.trim_end_matches('/');
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() < 3
        || !parts[0].is_empty()
        || parts[1] != "sdcard"
        || !["Download", "Pictures", "Movies", "DCIM"].contains(&parts[2])
        || parts[2..].iter().any(|s| !safe_component(s))
    {
        return Err(AppError::new(
            "SHARED_STORAGE_ONLY",
            "仅允许 /sdcard 下的 Download、Pictures、Movies、DCIM；不允许越界或链接",
        ));
    }
    Ok(path.into())
}
pub fn owned_remote(directory: &str, id: &str) -> Result<String> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("TRANSFER_ID", "任务标识无效"))?;
    Ok(format!("{}/.devkit-transfer-{id}", remote_path(directory)?))
}
impl App {
    pub async fn remote_shell(
        &self,
        settings: &Settings,
        target: &Target,
        script: String,
        timeout: u64,
    ) -> Result<crate::process::Output> {
        self.bound_adb(settings, target, vec!["shell".into(), script], timeout)
            .await?
            .success()
    }
    // /sdcard itself is an Android alias. All components beneath it must be real directories/files.
    pub async fn check_remote(
        &self,
        settings: &Settings,
        target: &Target,
        path: &str,
    ) -> Result<()> {
        let path = remote_path(path)?;
        let mut current = String::from("/sdcard");
        let mut script = String::from("set -e; ");
        for part in path.trim_start_matches("/sdcard/").split('/') {
            current.push('/');
            current.push_str(part);
            script.push_str(&format!("[ ! -L {} ]; ", quote(&current)));
        }
        script.push_str(&format!("p=$(readlink -f {}); case \"$p\" in /storage/emulated/[0-9]*/Download|/storage/emulated/[0-9]*/Download/*|/storage/emulated/[0-9]*/Pictures|/storage/emulated/[0-9]*/Pictures/*|/storage/emulated/[0-9]*/Movies|/storage/emulated/[0-9]*/Movies/*|/storage/emulated/[0-9]*/DCIM|/storage/emulated/[0-9]*/DCIM/*) ;; *) exit 73;; esac",quote(&path)));
        self.remote_shell(settings, target, script, 15)
            .await
            .map_err(|e| {
                AppError::new("UNSAFE_REMOTE_PATH", "设备路径不存在、包含链接或不可访问")
                    .detail(format!("{}\n{}", e, e.detail))
            })?;
        Ok(())
    }
    pub async fn remote_entries(&self, target: &Target, path: &str) -> Result<Vec<RemoteEntry>> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        let settings = self.store.settings();
        if path == "/sdcard" {
            // Still verify the chosen transport for the virtual roots view.
            self.bound_adb(&settings, target, vec!["get-state".into()], 10)
                .await?
                .success()?;
            return Ok(["Download", "Pictures", "Movies", "DCIM"]
                .iter()
                .map(|s| RemoteEntry {
                    name: s.to_string(),
                    path: format!("/sdcard/{s}"),
                    directory: true,
                    link: false,
                    bytes: 0,
                    modified: 0,
                })
                .collect());
        }
        let path = remote_path(path)?;
        self.check_remote(&settings, target, &path).await?;
        let entries = self.remote_entries_raw(target, &path).await?;
        if self.epoch.load(Ordering::SeqCst) != epoch {
            return Err(AppError::new("STALE_SESSION", "ADB 会话已变化"));
        }
        Ok(entries)
    }
    pub(crate) async fn remote_entries_raw(
        &self,
        target: &Target,
        path: &str,
    ) -> Result<Vec<RemoteEntry>> {
        let settings = self.store.settings();
        let script=format!("set -e; [ -d {d} ]; for p in {d}/* {d}/.[!.]* {d}/..?*; do [ -e \"$p\" ] || [ -L \"$p\" ] || continue; printf '%s\\000' \"$p\"; stat -c '%f %s %Y' \"$p\"; done",d=quote(path));
        let out = self.remote_shell(&settings, target, script, 30).await?;
        parse_listing(&out.stdout)
    }
    pub(crate) async fn remote_hash(
        &self,
        settings: &Settings,
        target: &Target,
        path: &str,
    ) -> Result<String> {
        let out=self.remote_shell(settings,target,format!("if command -v sha256sum >/dev/null 2>&1; then sha256sum {}; else printf 'SIZE_ONLY'; fi",quote(path)),60).await?;
        let text = out.text();
        let hash = text.split_whitespace().next().unwrap_or("");
        if hash == "SIZE_ONLY" {
            return Ok(String::new());
        }
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AppError::new("TRANSFER_VERIFY", "无法读取设备 SHA-256"));
        }
        Ok(hash.to_ascii_lowercase())
    }
    pub async fn prepare_transfer(
        &self,
        direction: &str,
        paths: Vec<String>,
        target: Target,
        directory: String,
    ) -> Result<TransferSet> {
        if !["upload", "download"].contains(&direction) || paths.is_empty() || paths.len() > 256 {
            return Err(AppError::new(
                "TRANSFER_SELECTION",
                "请选择 1～256 个文件或目录",
            ));
        }
        let settings = self.store.settings();
        let id = uuid::Uuid::new_v4().to_string();
        let dir = transfer_dir(self, &id)?;
        std::fs::create_dir_all(&dir)?;
        let mut set = TransferSet {
            id: id.clone(),
            direction: direction.into(),
            target: target.clone(),
            directory: directory.clone(),
            items: vec![],
            roots: vec![],
            conflicts: vec![],
        };
        let result = async {
            if direction == "upload" {
                let directory = remote_path(&directory)?;
                self.check_remote(&settings, &target, &directory).await?;
                let existing = self.remote_entries(&target, &directory).await?;
                for source in paths.iter().map(PathBuf::from) {
                    let name = source
                        .file_name()
                        .and_then(|s| s.to_str())
                        .filter(|s| safe_component(s))
                        .ok_or_else(|| AppError::new("UNSAFE_FILE_NAME", "文件名不受支持"))?
                        .to_owned();
                    if set.roots.contains(&name) {
                        return Err(AppError::new("TRANSFER_DUPLICATE", "选择的顶层文件名重复"));
                    }
                    set.roots.push(name.clone());
                    if existing.iter().any(|e| e.name == name) {
                        set.conflicts.push(name.clone());
                    }
                    let mut pending = vec![(source, name)];
                    while let Some((source, relative)) = pending.pop() {
                        unique_relative(&set.items, &relative)?;
                        let directory = source.is_dir();
                        regular_local(&source, directory)?;
                        if directory {
                            set.items.push(TransferItem {
                                relative: relative.clone(),
                                directory: true,
                                bytes: 0,
                                sha256: String::new(),
                            });
                            std::fs::create_dir_all(dir.join("payload").join(&relative))?;
                            for entry in std::fs::read_dir(&source)? {
                                let entry = entry?;
                                let name = entry.file_name().to_string_lossy().into_owned();
                                if !safe_component(&name) {
                                    return Err(AppError::new(
                                        "UNSAFE_FILE_NAME",
                                        "目录含有不受支持的文件名",
                                    ));
                                }
                                pending.push((entry.path(), format!("{relative}/{name}")));
                            }
                        } else {
                            let bytes = source.metadata()?.len();
                            check_limits(&set.items, bytes)?;
                            crate::capacity::budget(&[(dir.clone(), bytes, "上传暂存")])?;
                            let destination = dir.join("payload").join(&relative);
                            std::fs::create_dir_all(destination.parent().unwrap())?;
                            let sha256 = stage_file(&source, &destination, bytes)?;
                            set.items.push(TransferItem {
                                relative,
                                directory: false,
                                bytes,
                                sha256,
                            });
                        }
                        check_limits(&set.items, 0)?;
                    }
                }
            } else {
                regular_local(Path::new(&directory), true)?;
                let mut pending = vec![];
                for path in &paths {
                    let path = remote_path(path)?;
                    let name = path.rsplit('/').next().unwrap().to_owned();
                    if set.roots.contains(&name) {
                        return Err(AppError::new("TRANSFER_DUPLICATE", "选择的顶层文件名重复"));
                    }
                    set.roots.push(name.clone());
                    if Path::new(&directory).join(&name).exists() {
                        set.conflicts.push(name.clone());
                    }
                    let parent = path.rsplit_once('/').unwrap().0;
                    let entry = self
                        .remote_entries(&target, parent)
                        .await?
                        .into_iter()
                        .find(|e| e.path == path)
                        .ok_or_else(|| AppError::new("FILE_NOT_FOUND", "设备文件已不存在"))?;
                    pending.push((entry, name));
                }
                while let Some((entry, relative)) = pending.pop() {
                    unique_relative(&set.items, &relative)?;
                    if entry.link || !relative.split('/').all(safe_component) {
                        return Err(AppError::new(
                            "UNSAFE_FILE_NAME",
                            "选择中含有链接或不受支持的文件名",
                        ));
                    }
                    self.check_remote(&settings, &target, &entry.path).await?;
                    check_limits(&set.items, entry.bytes)?;
                    let hash = if entry.directory {
                        String::new()
                    } else {
                        self.remote_hash(&settings, &target, &entry.path).await?
                    };
                    set.items.push(TransferItem {
                        relative: relative.clone(),
                        directory: entry.directory,
                        bytes: if entry.directory { 0 } else { entry.bytes },
                        sha256: hash,
                    });
                    if entry.directory {
                        for child in self.remote_entries(&target, &entry.path).await? {
                            let child_relative = format!("{relative}/{}", child.name);
                            pending.push((child, child_relative));
                        }
                    }
                }
                // Store source roots separately from the user's destination.
                std::fs::write(dir.join("sources.json"), serde_json::to_vec(&paths)?)?;
            }
            std::fs::write(dir.join("set.json"), serde_json::to_vec(&set)?)?;
            Ok(set)
        }
        .await;
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        result
    }
    async fn cancellable_adb(
        &self,
        id: &str,
        settings: &Settings,
        target: &Target,
        args: Vec<String>,
        cwd: Option<&Path>,
    ) -> Result<()> {
        if self.cancelled(id) {
            return Err(AppError::new("CANCELLED", "传输已取消"));
        }
        let mut spec = self.bound_adb_spec(settings, target, args, 600).await?;
        spec.cwd = cwd.map(Path::to_path_buf);
        let run = crate::process::run(spec);
        tokio::pin!(run);
        loop {
            tokio::select! {
                out=&mut run => {out?.success()?;return Ok(());},
                _=tokio::time::sleep(std::time::Duration::from_millis(200)) => {if self.cancelled(id) {return Err(AppError::new("CANCELLED","传输已取消；暂存内容不会发布为完整文件"));}}
            }
        }
    }
    pub async fn transfer_files(
        &self,
        id: &str,
        op: &Operation,
        settings: &Settings,
    ) -> Result<String> {
        if manifest_hash(self, op.arg("transferId")?)? != op.arg("manifestHash")? {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "已确认的传输清单发生变化，请重新选择",
            ));
        }
        let set = load(self, op.arg("transferId")?)?;
        if op.target.as_ref() != Some(&set.target) {
            return Err(AppError::new(
                "STALE_SESSION",
                "传输目标与已确认的文件集合不一致",
            ));
        }
        let roots: Vec<_> = crate::phone_publish::names(&set, op)?
            .keys()
            .cloned()
            .collect();
        let target = &set.target;
        let dir = transfer_dir(self, &set.id)?;
        let total: u64 = set
            .items
            .iter()
            .filter(|m| {
                !m.directory
                    && roots
                        .iter()
                        .any(|r| m.relative == *r || m.relative.starts_with(&format!("{r}/")))
            })
            .map(|m| m.bytes)
            .sum();
        let mut completed = 0u64;
        let mut published = vec![];
        let mut size_only = false;
        self.update(id, |t| {
            t.cancellable = true;
            t.phase = "transferring".into();
            t.message = "传输到暂存区，验证完成后发布".into();
        })?;
        let upload = set.direction == "upload";
        let remote_stage = if upload {
            owned_remote(&set.directory, id)?
        } else {
            String::new()
        };
        let local_stage = if upload {
            dir.join("payload")
        } else {
            Path::new(&set.directory).join(format!(".devkit-transfer-{id}"))
        };
        self.line(
            id,
            format!(
                "本任务传输暂存：{}",
                if upload {
                    remote_stage.clone()
                } else {
                    local_stage.to_string_lossy().into()
                }
            ),
        )?;
        self.line(
            id,
            "每个目标发布前重新核对；替换旧目标时保留原件备份，异常时可恢复".into(),
        )?;
        let result = async {
            if upload {
                self.check_remote(settings, target, &set.directory).await?;
                let free = self
                    .remote_shell(
                        settings,
                        target,
                        format!("df -k {}", quote(&set.directory)),
                        15,
                    )
                    .await?;
                let available = free
                    .text()
                    .lines()
                    .last()
                    .and_then(|l| l.split_whitespace().nth(3))
                    .and_then(|s| s.parse::<u64>().ok());
                if available
                    .is_some_and(|n| n.saturating_mul(1024) < total.saturating_add(8 * 1024 * 1024))
                {
                    return Err(AppError::new(
                        "DISK_SPACE",
                        "设备空间不足以容纳传输暂存，未发布文件",
                    ));
                }
                self.remote_shell(
                    settings,
                    target,
                    format!(
                        "mkdir {p} && printf %s {token} > {p}/.devkit-owner",
                        p = quote(&remote_stage),
                        token = quote(id)
                    ),
                    15,
                )
                .await?;
            } else {
                regular_local(Path::new(&set.directory), true)?;
                crate::capacity::budget(&[(PathBuf::from(&set.directory), total, "下载暂存")])?;
                std::fs::create_dir(&local_stage)?;
                std::fs::write(local_stage.join(".devkit-owner"), id)?;
            }
            for root in &roots {
                if self.cancelled(id) {
                    return Err(AppError::new("CANCELLED", "传输已取消"));
                }
                let members: Vec<_> = set
                    .items
                    .iter()
                    .filter(|m| m.relative == *root || m.relative.starts_with(&format!("{root}/")))
                    .collect();
                if upload {
                    for m in &members {
                        regular_local(&local_stage.join(&m.relative), m.directory)?;
                        if !m.directory && digest_file(&local_stage.join(&m.relative))? != m.sha256
                        {
                            return Err(AppError::new("SOURCE_CHANGED", "上传暂存文件发生变化"));
                        }
                    }
                    // adb push's directory traversal drops empty folders on older platform-tools.
                    // Materialize the reviewed tree ourselves and copy only manifest files.
                    for m in &members {
                        if self.cancelled(id) {
                            return Err(AppError::new("CANCELLED", "传输已取消"));
                        }
                        let remote = format!("{remote_stage}/{}", m.relative);
                        if m.directory {
                            self.remote_shell(
                                settings,
                                target,
                                format!("mkdir -p {}", quote(&remote)),
                                15,
                            )
                            .await?;
                        } else {
                            let local = local_stage.join(&m.relative);
                            self.phase(id, "transferring", &format!("正在上传：{}", m.relative))?;
                            self.cancellable_adb(
                                id,
                                settings,
                                target,
                                vec![
                                    "push".into(),
                                    local.file_name().unwrap().to_string_lossy().into(),
                                    remote,
                                ],
                                Some(local.parent().unwrap()),
                            )
                            .await?;
                        }
                    }
                } else {
                    let sources: Vec<String> =
                        serde_json::from_slice(&std::fs::read(dir.join("sources.json"))?)?;
                    let source = sources
                        .iter()
                        .find(|p| p.rsplit('/').next() == Some(root.as_str()))
                        .ok_or_else(|| AppError::new("TRANSFER_SOURCE", "来源记录无效"))?;
                    self.check_remote(settings, target, source).await?;
                    // Pull each manifest file separately: adb pull must never follow an unreviewed tree link.
                    for m in &members {
                        if self.cancelled(id) {
                            return Err(AppError::new("CANCELLED", "传输已取消"));
                        }
                        let suffix = m.relative.strip_prefix(root).unwrap();
                        let remote = format!("{source}{suffix}");
                        self.check_remote(settings, target, &remote).await?;
                        let local = local_stage.join(&m.relative);
                        if m.directory {
                            std::fs::create_dir_all(&local)?;
                        } else {
                            std::fs::create_dir_all(local.parent().unwrap())?;
                            self.cancellable_adb(
                                id,
                                settings,
                                target,
                                vec![
                                    "pull".into(),
                                    remote,
                                    local.file_name().unwrap().to_string_lossy().into(),
                                ],
                                Some(local.parent().unwrap()),
                            )
                            .await?;
                        }
                    }
                }
                for m in &members {
                    if self.cancelled(id) {
                        return Err(AppError::new("CANCELLED", "传输已取消"));
                    }
                    if m.directory {
                        continue;
                    }
                    if upload {
                        let remote = format!("{remote_stage}/{}", m.relative);
                        let stat = self
                            .remote_shell(
                                settings,
                                target,
                                format!("[ ! -L {p} ] && stat -c %s {p}", p = quote(&remote)),
                                15,
                            )
                            .await?;
                        if stat.text().trim().parse::<u64>().ok() != Some(m.bytes) {
                            return Err(AppError::new("TRANSFER_VERIFY", "上传文件大小不一致"));
                        }
                        let hash = self.remote_hash(settings, target, &remote).await?;
                        if hash.is_empty() {
                            size_only = true;
                        } else if hash != m.sha256 {
                            return Err(AppError::new("TRANSFER_VERIFY", "上传 SHA-256 不一致"));
                        }
                    } else {
                        let path = local_stage.join(&m.relative);
                        regular_local(&path, false)?;
                        if path.metadata()?.len() != m.bytes
                            || (!m.sha256.is_empty() && digest_file(&path)? != m.sha256)
                        {
                            return Err(AppError::new(
                                "SOURCE_CHANGED",
                                "下载文件与已确认的大小或 SHA-256 不一致",
                            ));
                        }
                        if m.sha256.is_empty() {
                            size_only = true;
                        }
                    }
                    completed = completed.saturating_add(m.bytes);
                    self.update(id, |t| {
                        t.progress = Some(if total == 0 {
                            0.0
                        } else {
                            completed as f64 / total as f64 * 95.0
                        });
                        t.message = format!("已验证 {completed}/{total} 字节 · {}", m.relative);
                    })?;
                }
            }
            // The complete selected set is verified before the first destination is published.
            self.phase(
                id,
                "publishing",
                "暂存校验完成，重新核对目标后按已确认的冲突策略发布",
            )?;
            self.publish_transfer(id, op, &set, &local_stage, &remote_stage, &mut published)
                .await?;
            Ok(())
        }
        .await;
        // Remove only the task-owned staging tree, never the user's destination or published files.
        let cleanup = if upload {
            let cleanup = format!(
                "[ ! -L {p} ] && [ ! -e {p}/.devkit-protected ] && [ \"$(cat {p}/.devkit-owner)\" = {token} ] && rm -rf -- {p}",
                p = quote(&remote_stage),
                token = quote(id)
            );
            self.remote_shell(settings, target, cleanup, 30)
                .await
                .map(|_| ())
        } else if local_stage.is_dir() && regular_local(&local_stage, true).is_ok() {
            cleanup_local(&local_stage, id, result.is_ok())
        } else {
            Ok(())
        };
        if let Err(error) = cleanup {
            self.line(
                id,
                format!(
                    "暂存保留或清理未完成：{}；{}",
                    if upload {
                        remote_stage.clone()
                    } else {
                        local_stage.to_string_lossy().into()
                    },
                    error
                ),
            )?;
            if result.is_ok() {
                return Err(AppError::new(
                    "PARTIAL_COMPLETE",
                    "目标已发布，但暂存清理未完成，请核对任务日志后清理",
                )
                .detail(format!("{}\n{}", error, error.detail)));
            }
        }
        match result {
            Ok(()) => Ok(format!(
                "已传输 {} 项；{}；跳过 {} 个冲突项",
                published.len(),
                if size_only {
                    "仅校验大小（设备无 SHA-256 工具）"
                } else {
                    "大小与 SHA-256 校验通过"
                },
                set.roots.len() - roots.len()
            )),
            Err(e) if !published.is_empty() => Err(AppError::new(
                "PARTIAL_COMPLETE",
                format!("已发布 {} 项，其余未完成；请核对任务日志", published.len()),
            )
            .detail(format!("{}\n{}", e, e.detail))),
            Err(e)
                if matches!(
                    e.code.as_str(),
                    "TOOL_FAILED" | "TOOL_TIMEOUT" | "DEVICE_UNAVAILABLE" | "STALE_SESSION"
                ) =>
            {
                Err(AppError::new(
                    "RESULT_UNKNOWN",
                    "设备连接或发布结果未能确认；请核对原目标及任务日志，传输不会自动重放",
                )
                .detail(format!("{}\n{}", e, e.detail)))
            }
            Err(e) => Err(e),
        }
    }
    pub async fn cleanup_transfer(&self, op: &Operation, settings: &Settings) -> Result<String> {
        let source = self.store.task(op.arg("sourceTaskId")?)?;
        let _source_use = self.use_task_record(&source.id, false)?;
        if source.active()
            || !matches!(
                source.operation.kind.as_str(),
                "file_upload" | "file_download"
            )
        {
            return Err(AppError::new(
                "TRANSFER_ACTIVE",
                "只能清理已结束传输的专属暂存目录",
            ));
        }
        let set = load(self, op.arg("transferId")?)?;
        if manifest_hash(self, &set.id)? != op.arg("manifestHash")?
            || source.operation.optional("transferId") != set.id
        {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "传输归属记录发生变化，未清理",
            ));
        }
        if set.direction == "upload" {
            let target = op
                .target
                .as_ref()
                .ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择原设备"))?;
            if target.serial != set.target.serial {
                return Err(AppError::new(
                    "STALE_SESSION",
                    "只能清理原设备的本任务暂存目录",
                ));
            }
            self.check_remote(settings, target, &set.directory).await?;
            let stage = owned_remote(&set.directory, &source.id)?;
            self.remote_shell(settings,target,format!(r#"if [ -e {p} ] || [ -L {p} ]; then [ ! -L {p} ] && [ ! -L {p}/.devkit-owner ] && [ ! -e {p}/.devkit-protected ] && [ "$(cat {p}/.devkit-owner)" = {token} ] && [ ! -L {p}/.devkit-backups ] && [ ! -L {p}/.devkit-restore ] || exit 77; for f in {p}/.devkit-backups/* {p}/.devkit-backups/.[!.]* {p}/.devkit-backups/..?* {p}/.devkit-restore/* {p}/.devkit-restore/.[!.]* {p}/.devkit-restore/..?*; do [ ! -e "$f" ] && [ ! -L "$f" ] || exit 77; done; rm -rf -- {p}; fi"#,p=quote(&stage),token=quote(&source.id)),30).await.map_err(|e|AppError::new("BACKUPS_REVIEW_REQUIRED","暂存归属未确认或保留旧目标备份，请先核对并恢复旧目标").detail(format!("{}\n{}",e,e.detail)))?;
        } else {
            let stage = Path::new(&set.directory).join(format!(".devkit-transfer-{}", source.id));
            if stage.exists() {
                cleanup_local(&stage, &source.id, false)?;
            }
        }
        self.update(&source.id, |t| {
            t.operation
                .args
                .insert("backupsResolved".into(), "true".into());
        })?;
        Ok("已清理原传输的专属暂存；已发布文件与任务记录保留".into())
    }
}
fn unique_relative(items: &[TransferItem], name: &str) -> Result<()> {
    if items
        .iter()
        .any(|item| item.relative.to_lowercase() == name.to_lowercase())
    {
        return Err(AppError::new(
            "TRANSFER_DUPLICATE",
            "文件名在 Windows 上存在大小写冲突，请分别传输并重命名",
        ));
    }
    Ok(())
}
pub(crate) fn cleanup_local(stage: &Path, id: &str, allow_backups: bool) -> Result<()> {
    regular_local(stage, true)?;
    regular_local(&stage.join(".devkit-owner"), false)?;
    if !allow_backups {
        for name in [".devkit-backups", ".devkit-restore"] {
            let path = stage.join(name);
            if path.exists() {
                regular_local(&path, true)?;
                if std::fs::read_dir(path)?.next().is_some() {
                    return Err(AppError::new(
                        "BACKUPS_REVIEW_REQUIRED",
                        "暂存中保留原件备份，请先恢复，不会删除原件",
                    ));
                }
            }
        }
    }
    if stage.join(".devkit-protected").exists() {
        return Err(AppError::new(
            "BACKUPS_REVIEW_REQUIRED",
            "暂存中保留旧目标备份，请先恢复旧目标；不会自动删除原件",
        ));
    }
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("TRANSFER_ID", "任务标识无效"))?;
    if stage.file_name().and_then(|n| n.to_str()) != Some(&format!(".devkit-transfer-{id}"))
        || std::fs::read(stage.join(".devkit-owner"))? != id.as_bytes()
    {
        return Err(AppError::new("UNSAFE_PATH", "暂存目录归属无法确认，未清理"));
    }
    let mut pending = vec![stage.to_path_buf()];
    let mut count = 0;
    while let Some(path) = pending.pop() {
        let directory = path.is_dir();
        regular_local(&path, directory)?;
        if directory {
            for entry in std::fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        }
        count += 1;
        if count > 2_600_000 {
            return Err(AppError::new("TRANSFER_LIMIT", "暂存条目超限，未清理"));
        }
    }
    std::fs::remove_dir_all(stage)?;
    Ok(())
}
fn check_limits(items: &[TransferItem], extra: u64) -> Result<()> {
    if items.len() >= 10_000
        || items
            .iter()
            .map(|m| m.bytes)
            .sum::<u64>()
            .saturating_add(extra)
            > MAX_BYTES
    {
        return Err(AppError::new(
            "TRANSFER_LIMIT",
            "单次最多 10000 个条目、4 GiB",
        ));
    }
    Ok(())
}
pub fn transfer_dir(app: &App, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("TRANSFER_ID", "传输标识无效"))?;
    Ok(app.data.join("transfers").join(id))
}
pub fn load(app: &App, id: &str) -> Result<TransferSet> {
    let set: TransferSet =
        serde_json::from_slice(&std::fs::read(transfer_dir(app, id)?.join("set.json"))?)?;
    if set.id != id
        || set
            .items
            .iter()
            .any(|m| !m.relative.split('/').all(safe_component))
        || set.roots.iter().any(|r| !safe_component(r))
    {
        return Err(AppError::new("TRANSFER_MANIFEST", "传输清单无效"));
    }
    Ok(set)
}
pub(crate) fn manifest_hash(app: &App, id: &str) -> Result<String> {
    let dir = transfer_dir(app, id)?;
    let mut bytes = std::fs::read(dir.join("set.json"))?;
    if dir.join("sources.json").is_file() {
        bytes.extend(std::fs::read(dir.join("sources.json"))?);
    }
    Ok(digest(&bytes))
}
pub fn plan(
    app: &App,
    op: &mut Operation,
    effects: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if matches!(op.kind.as_str(), "file_cleanup" | "file_restore") {
        let source = app.store.task(op.arg("sourceTaskId")?)?;
        if source.active()
            || !matches!(
                source.operation.kind.as_str(),
                "file_upload" | "file_download"
            )
        {
            return Err(AppError::new(
                "TRANSFER_ACTIVE",
                "只能清理已结束传输的专属暂存",
            ));
        }
        let set = load(app, source.operation.arg("transferId")?)?;
        let hash = manifest_hash(app, &set.id)?;
        if hash != source.operation.arg("manifestHash")? {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "原传输清单发生变化，未清理",
            ));
        }
        if op.target.as_ref().map(|t| &t.serial) != Some(&set.target.serial) {
            return Err(AppError::new("TARGET_REQUIRED", "请选择原传输设备"));
        }
        op.args.insert("transferId".into(), set.id.clone());
        op.args.insert("manifestHash".into(), hash);
        if op.kind == "file_restore" {
            if source.operation.optional("conflicts") != "replace" {
                return Err(AppError::new("TRANSFER_RESTORE", "原任务没有替换旧目标"));
            }
            let journal =
                transfer_dir(app, &set.id)?.join(format!("{}.replacement.json", source.id));
            op.args.insert("journalHash".into(), digest_file(&journal)?);
            effects.push(format!(
                "从任务 {} 的原件备份恢复被替换目标；先核对目标，外部修改时拒绝覆盖",
                source.id
            ));
            warnings.push(
                "仅在备份与旧目标摘要一致、当前目标仍为本任务发布内容时恢复；新建已发布文件保留。"
                    .into(),
            );
            return Ok(());
        }
        effects.push(format!(
            "仅清理任务 {} 的 .devkit-transfer-{} 专属目录，位置：{}",
            source.id, source.id, set.directory
        ));
        warnings
            .push("核对目录归属标记，不跟随链接；不会删除已经发布的目标文件或重放传输。".into());
        return Ok(());
    }
    let set = load(app, op.arg("transferId")?)?;
    op.args.remove("backupsResolved");
    op.args
        .insert("manifestHash".into(), manifest_hash(app, &set.id)?);
    if op.target.as_ref() != Some(&set.target) || op.kind != format!("file_{}", set.direction) {
        return Err(AppError::new(
            "STALE_SESSION",
            "文件集合与目标设备不一致，请重新选择",
        ));
    }
    if !set.conflicts.is_empty()
        && !["skip", "rename", "replace"].contains(&op.optional("conflicts"))
    {
        return Err(AppError::new(
            "FILE_CONFLICT",
            format!(
                "{} 个目标已存在，请明确选择保留、重命名或替换；默认不覆盖",
                set.conflicts.len()
            ),
        ));
    }
    let names = crate::phone_publish::names(&set, op)?;
    let snapshots: std::collections::BTreeMap<String, Option<String>> =
        serde_json::from_str(op.arg("destinationSnapshots")?)?;
    if snapshots.len() != names.len() || names.values().any(|n| !snapshots.contains_key(n)) {
        return Err(AppError::new(
            "TRANSFER_MANIFEST",
            "目标核对记录无效，请重新生成计划",
        ));
    }
    effects.push(format!(
        "{} · {} 个顶层条目 · {} 个文件/目录 · {} 字节",
        set.directory,
        set.roots.len(),
        set.items.len(),
        set.items.iter().map(|m| m.bytes).sum::<u64>()
    ));
    effects.extend(set.roots.iter().map(|r| {
        format!(
            "{} → {}{}",
            r,
            names.get(r).map(String::as_str).unwrap_or("—"),
            if !names.contains_key(r) {
                "（跳过：目标已存在）"
            } else if snapshots.get(&names[r]).is_some_and(Option::is_some) {
                "（替换已核对的旧目标）"
            } else {
                ""
            }
        )
    }));
    if op.optional("conflicts") == "replace" {
        warnings.push("将替换冲突目标及其全部内容；仅在摘要仍一致时提交。旧目标保留到全部发布成功，异常时保留备份并提供恢复计划；不会合并目录或自动覆盖外部修改。".into());
    }
    warnings.push("传输至任务专属暂存区，验证后发布；仅操作共享存储，不跟随链接。设备不支持 SHA-256 时明确报告仅校验大小。中断不自动重放；已发布项目保留在任务日志。".into());
    Ok(())
}
fn parse_listing(bytes: &[u8]) -> Result<Vec<RemoteEntry>> {
    let mut remaining = bytes;
    let mut result = vec![];
    while !remaining.is_empty() {
        let null = remaining
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| AppError::new("FILE_LIST_FORMAT", "设备列表缺少分隔符"))?;
        let path = std::str::from_utf8(&remaining[..null])
            .map_err(|_| AppError::new("FILE_NAME_ENCODING", "文件名不是 UTF-8"))?
            .to_string();
        remaining = &remaining[null + 1..];
        let end = remaining
            .iter()
            .position(|b| *b == b'\n')
            .ok_or_else(|| AppError::new("FILE_LIST_FORMAT", "设备列表不完整"))?;
        let fields: Vec<_> = std::str::from_utf8(&remaining[..end])
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if fields.len() != 3 {
            return Err(AppError::new("FILE_LIST_FORMAT", "设备文件元数据无效"));
        }
        let mode = u32::from_str_radix(fields[0], 16)
            .map_err(|_| AppError::new("FILE_LIST_FORMAT", "文件类型无效"))?;
        let name = path.rsplit('/').next().unwrap_or("").to_string();
        let link = mode & 0xf000 == 0xa000 || !safe_component(&name);
        result.push(RemoteEntry {
            name,
            path,
            directory: mode & 0xf000 == 0x4000,
            link: link || !matches!(mode & 0xf000, 0x4000 | 0x8000 | 0xa000),
            bytes: fields[1]
                .parse()
                .map_err(|_| AppError::new("FILE_LIST_FORMAT", "文件大小无效"))?,
            modified: fields[2].parse().unwrap_or(0),
        });
        if result.len() > 10_000 {
            return Err(AppError::new("TRANSFER_LIMIT", "目录条目超过 10000"));
        }
        remaining = &remaining[end + 1..];
    }
    result.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.name.cmp(&b.name)));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_source_is_bounded_and_never_commits_a_frozen_copy() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("frozen");
        std::fs::write(&source, b"growing file").unwrap();
        assert_eq!(
            stage_file(&source, &destination, 1).unwrap_err().code,
            "SOURCE_CHANGED"
        );
        assert!(!destination.exists());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        let hash = stage_file(&source, &destination, 12).unwrap();
        assert_eq!(hash, digest_file(&source).unwrap());
        assert_eq!(std::fs::read(destination).unwrap(), b"growing file");
    }
    #[test]
    fn listing_is_framed_and_blocks_links_special_files_and_windows_aliases() {
        let entries=parse_listing(b"/sdcard/Download/empty\x0041fd 4096 12\n/sdcard/Download/Link\x00a1ff 12 13\n/sdcard/Download/CON.txt\x0081b6 1 14\n").unwrap();
        assert!(entries
            .iter()
            .any(|e| e.name == "empty" && e.directory && !e.link));
        assert!(entries.iter().filter(|e| e.name != "empty").all(|e| e.link));
        assert!(parse_listing(b"/sdcard/Download/file\x0081b6 4").is_err());
        assert!(parse_listing(b"/sdcard/Download/file").is_err());
        assert!(unique_relative(
            &[TransferItem {
                relative: "Folder/File.txt".into(),
                directory: false,
                bytes: 1,
                sha256: String::new()
            }],
            "folder/FILE.txt"
        )
        .is_err());
    }
    #[test]
    fn local_staging_cleanup_requires_marker_and_preserves_neighbor_files() {
        let temp = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let stage = temp.path().join(format!(".devkit-transfer-{id}"));
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join(".devkit-owner"), "foreign").unwrap();
        std::fs::write(stage.join("payload"), b"partial").unwrap();
        let neighbor = temp.path().join("user-file");
        std::fs::write(&neighbor, b"keep").unwrap();
        assert!(cleanup_local(&stage, &id, false).is_err());
        assert!(stage.join("payload").is_file());
        std::fs::write(stage.join(".devkit-owner"), &id).unwrap();
        cleanup_local(&stage, &id, false).unwrap();
        assert!(!stage.exists());
        assert_eq!(std::fs::read(neighbor).unwrap(), b"keep");
    }
    #[tokio::test]
    async fn changing_reviewed_manifest_is_rejected_before_device_or_destination_writes() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(temp.path().join("app")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let dir = transfer_dir(&app, &id).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let target = Target {
            serial: "frozen".into(),
            transport_id: "1".into(),
            server_epoch: 1,
        };
        let set = TransferSet {
            id: id.clone(),
            direction: "download".into(),
            target: target.clone(),
            directory: temp.path().join("destination").to_string_lossy().into(),
            items: vec![TransferItem {
                relative: "file.txt".into(),
                directory: false,
                bytes: 1,
                sha256: String::new(),
            }],
            roots: vec!["file.txt".into()],
            conflicts: vec![],
        };
        std::fs::write(dir.join("set.json"), serde_json::to_vec(&set).unwrap()).unwrap();
        std::fs::write(dir.join("sources.json"), b"[\"/sdcard/Download/file.txt\"]").unwrap();
        let mut op = Operation {
            kind: "file_download".into(),
            args: std::collections::BTreeMap::from([("transferId".into(), id)]),
            target: Some(target),
        };
        app.prepare_transfer_destinations(&mut op).await.unwrap();
        plan(&app, &mut op, &mut vec![], &mut vec![]).unwrap();
        std::fs::write(
            dir.join("sources.json"),
            b"[\"/sdcard/Download/changed.txt\"]",
        )
        .unwrap();
        assert_eq!(
            app.transfer_files("unused", &op, &app.store.settings())
                .await
                .unwrap_err()
                .code,
            "SOURCE_CHANGED"
        );
        assert!(!Path::new(&set.directory).exists());
    }
}
