//! Conflict review and reversible replacement of complete, verified transfer roots.
use crate::{
    app::App,
    domain::*,
    phone_files::{self, quote, regular_local, safe_component, TransferSet},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

type Names = BTreeMap<String, String>;
type Snapshots = BTreeMap<String, Option<String>>;
type ContentRows = Vec<(String, bool, u64, String)>;
#[derive(Serialize, Deserialize)]
struct Journal {
    names: Names,
    old: Snapshots,
    new: BTreeMap<String, String>,
}
pub fn names(set: &TransferSet, op: &Operation) -> Result<Names> {
    let policy = op.optional("conflicts");
    if !["", "fail", "skip", "rename", "replace"].contains(&policy) {
        return Err(AppError::new("FILE_CONFLICT", "冲突处理方式无效"));
    }
    let renames: Names = if policy == "rename" {
        serde_json::from_str(op.arg("renames")?)?
    } else {
        BTreeMap::new()
    };
    let mut result = Names::new();
    let mut used = std::collections::BTreeSet::new();
    for root in &set.roots {
        if policy == "skip" && set.conflicts.contains(root) {
            continue;
        }
        let name = if policy == "rename" && set.conflicts.contains(root) {
            renames
                .get(root)
                .ok_or_else(|| AppError::new("FILE_RENAME", "请为每个冲突项指定新名称"))?
        } else {
            root
        };
        if !safe_component(name) || !used.insert(name.to_lowercase()) {
            return Err(AppError::new(
                "FILE_RENAME",
                "新名称无效或与其他条目存在大小写冲突",
            ));
        }
        result.insert(root.clone(), name.clone());
    }
    if result.is_empty() {
        return Err(AppError::new("TRANSFER_EMPTY", "跳过冲突后没有可传输项目"));
    }
    Ok(result)
}
fn fingerprint(mut rows: Vec<(String, bool, u64, String)>) -> String {
    rows.sort();
    digest(&serde_json::to_vec(&rows).unwrap())
}
pub fn local_fingerprint(path: &Path) -> Result<Option<String>> {
    Ok(local_rows(path)?.map(fingerprint))
}
fn local_rows(path: &Path) -> Result<Option<ContentRows>> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let mut pending = vec![(path.to_path_buf(), String::new())];
    let mut rows = vec![];
    let mut total = 0u64;
    while let Some((path, relative)) = pending.pop() {
        let directory = path.is_dir();
        regular_local(&path, directory)?;
        let bytes = if directory { 0 } else { path.metadata()?.len() };
        total = total.saturating_add(bytes);
        if rows.len() >= 10_000 || total > 4 * 1024 * 1024 * 1024 {
            return Err(AppError::new(
                "OVERWRITE_LIMIT",
                "旧目标超过核对上限，请保留或重命名",
            ));
        }
        let hash = if directory {
            String::new()
        } else {
            phone_files::bounded_hash(&path, bytes)?
        };
        rows.push((relative.clone(), directory, bytes, hash));
        if directory {
            for entry in std::fs::read_dir(&path)? {
                if pending.len() + rows.len() >= 10_000 {
                    return Err(AppError::new(
                        "OVERWRITE_LIMIT",
                        "旧目标超过核对上限，请保留或重命名",
                    ));
                }
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if !safe_component(&name) {
                    return Err(AppError::new(
                        "UNSAFE_FILE_NAME",
                        "旧目标含有不受支持的名称，请保留或重命名",
                    ));
                }
                pending.push((
                    entry.path(),
                    if relative.is_empty() {
                        name
                    } else {
                        format!("{relative}/{name}")
                    },
                ));
            }
        }
    }
    Ok(Some(rows))
}
impl App {
    async fn remote_fingerprint(
        &self,
        settings: &Settings,
        target: &Target,
        path: &str,
    ) -> Result<Option<String>> {
        let Some(rows) = self.remote_rows(settings, target, path).await? else {
            return Ok(None);
        };
        if rows
            .iter()
            .any(|(_, directory, _, hash)| !directory && hash.is_empty())
        {
            return Err(AppError::new(
                "OVERWRITE_NEEDS_HASH",
                "该设备不能核对旧目标摘要，请保留或重命名",
            ));
        }
        Ok(Some(fingerprint(rows)))
    }
    async fn remote_rows(
        &self,
        settings: &Settings,
        target: &Target,
        path: &str,
    ) -> Result<Option<ContentRows>> {
        let stat=self.remote_shell(settings,target,format!("if [ ! -e {p} ] && [ ! -L {p} ]; then printf MISSING; else [ ! -L {p} ] && stat -c '%f %s' {p}; fi",p=quote(path)),15).await?;
        if stat.text().trim() == "MISSING" {
            return Ok(None);
        }
        let text = stat.text();
        let fields: Vec<_> = text.split_whitespace().collect();
        let mode = fields
            .first()
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .ok_or_else(|| AppError::new("FILE_STAT", "无法读取旧目标类型"))?;
        let directory = match mode & 0xf000 {
            0x4000 => true,
            0x8000 => false,
            _ => return Err(AppError::new("UNSAFE_PATH", "不能覆盖链接或特殊文件")),
        };
        let size = fields
            .get(1)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| AppError::new("FILE_STAT", "无法读取旧目标大小"))?;
        let mut pending = vec![(path.to_string(), String::new(), directory, size)];
        let mut rows = vec![];
        let mut total = 0u64;
        while let Some((path, relative, directory, bytes)) = pending.pop() {
            let bytes = if directory { 0 } else { bytes };
            total = total.saturating_add(bytes);
            if rows.len() >= 10_000 || total > 4 * 1024 * 1024 * 1024 {
                return Err(AppError::new(
                    "OVERWRITE_LIMIT",
                    "旧目标超过核对上限，请保留或重命名",
                ));
            }
            let hash = if directory {
                String::new()
            } else {
                self.remote_hash(settings, target, &path).await?
            };
            rows.push((relative.clone(), directory, bytes, hash));
            if directory {
                for entry in self.remote_entries_raw(target, &path).await? {
                    if pending.len() + rows.len() >= 10_000 {
                        return Err(AppError::new(
                            "OVERWRITE_LIMIT",
                            "旧目标超过核对上限，请保留或重命名",
                        ));
                    }
                    if entry.link {
                        return Err(AppError::new(
                            "UNSAFE_PATH",
                            "旧目标含链接或不支持的名称，请保留或重命名",
                        ));
                    }
                    pending.push((
                        entry.path,
                        if relative.is_empty() {
                            entry.name
                        } else {
                            format!("{relative}/{}", entry.name)
                        },
                        entry.directory,
                        entry.bytes,
                    ));
                }
            }
        }
        Ok(Some(rows))
    }
    async fn target_fingerprint(
        &self,
        set: &TransferSet,
        target: &Target,
        path: &str,
    ) -> Result<Option<String>> {
        if set.direction == "upload" {
            self.remote_fingerprint(&self.store.settings(), target, path)
                .await
        } else {
            local_fingerprint(Path::new(path))
        }
    }
    pub async fn prepare_transfer_destinations(&self, op: &mut Operation) -> Result<()> {
        let set = phone_files::load(self, op.arg("transferId")?)?;
        if !set.conflicts.is_empty()
            && !["skip", "rename", "replace"].contains(&op.optional("conflicts"))
        {
            return Err(AppError::new(
                "FILE_CONFLICT",
                "请明确选择保留、重命名或替换；默认不覆盖",
            ));
        }
        if op.target.as_ref() != Some(&set.target) {
            return Err(AppError::new("STALE_SESSION", "请重新选择传输设备"));
        }
        let names = names(&set, op)?;
        let mut snapshots = Snapshots::new();
        for (root, name) in &names {
            let destination = destination(&set, name);
            let current = self
                .target_fingerprint(&set, &set.target, &destination)
                .await?;
            if current.is_some()
                && !(op.optional("conflicts") == "replace" && set.conflicts.contains(root))
            {
                return Err(AppError::new(
                    "FILE_CONFLICT",
                    format!("目标已存在：{name}；请明确选择保留、重命名或替换"),
                ));
            }
            if current.is_none()
                && op.optional("conflicts") == "replace"
                && set.conflicts.contains(root)
            {
                return Err(AppError::new(
                    "EXTERNAL_CHANGE",
                    "旧目标已变化，请刷新文件清单",
                ));
            }
            snapshots.insert(name.clone(), current);
        }
        op.args.insert(
            "destinationSnapshots".into(),
            serde_json::to_string(&snapshots)?,
        );
        Ok(())
    }
    pub async fn publish_transfer(
        &self,
        id: &str,
        op: &Operation,
        set: &TransferSet,
        local_stage: &Path,
        remote_stage: &str,
        published: &mut Vec<String>,
    ) -> Result<()> {
        let names = names(set, op)?;
        let old: Snapshots = serde_json::from_str(op.arg("destinationSnapshots")?)?;
        let mut new = BTreeMap::new();
        for root in names.keys() {
            new.insert(
                root.clone(),
                local_fingerprint(&local_stage.join(root))?
                    .ok_or_else(|| AppError::new("SOURCE_CHANGED", "暂存项目已不存在"))?,
            );
        }
        let journal = Journal {
            names: names.clone(),
            old: old.clone(),
            new,
        };
        write_journal(self, id, op.arg("transferId")?, &journal)?;
        let target = &set.target;
        let settings = self.store.settings();
        let upload = set.direction == "upload";
        let mut protected = false;
        for (index, (root, name)) in names.iter().enumerate() {
            if self.cancelled(id) {
                return Err(AppError::new("CANCELLED", "已停止发布"));
            }
            let final_path = destination(set, name);
            if self.target_fingerprint(set, target, &final_path).await?
                != *old
                    .get(name)
                    .ok_or_else(|| AppError::new("TRANSFER_MANIFEST", "缺少目标核对记录"))?
            {
                return Err(AppError::new(
                    "EXTERNAL_CHANGE",
                    format!("目标在确认后变化：{name}；未覆盖"),
                ));
            }
            if old[name].is_some() {
                self.line(
                    id,
                    format!(
                        "旧目标备份：{}/.devkit-backups/{index}；原目标：{final_path}",
                        if upload {
                            remote_stage.to_string()
                        } else {
                            local_stage.to_string_lossy().into()
                        }
                    ),
                )?;
                // Keep the marker even when a response is lost after the old target moves.
                protected = true;
                if upload {
                    let backups = format!("{remote_stage}/.devkit-backups");
                    self.remote_shell(&settings,target,format!("printf %s {token} > {s}/.devkit-protected; mkdir -p {b}; [ ! -e {b}/{index} ] && mv -n {d} {b}/{index} && [ ! -e {d} ]",token=quote(id),s=quote(remote_stage),b=quote(&backups),d=quote(&final_path)),30).await?;
                } else {
                    std::fs::write(local_stage.join(".devkit-protected"), id)?;
                    let backups = local_stage.join(".devkit-backups");
                    std::fs::create_dir_all(&backups)?;
                    crate::recovery::publish_new_file(
                        Path::new(&final_path),
                        &backups.join(index.to_string()),
                    )?;
                }
                let backup = if upload {
                    format!("{remote_stage}/.devkit-backups/{index}")
                } else {
                    local_stage
                        .join(".devkit-backups")
                        .join(index.to_string())
                        .to_string_lossy()
                        .into()
                };
                if self.target_fingerprint(set, target, &backup).await? != old[name] {
                    return Err(AppError::new(
                        "RESULT_UNKNOWN",
                        "原目标在提交期间发生变化，原件备份保留，未继续替换",
                    ));
                }
                #[cfg(test)]
                crate::fault_tests::checkpoint("phone.transfer.after_backup")?;
            }
            if upload {
                self.check_remote(&settings, target, &set.directory).await?;
                // Moving into the known parent supports Android 10 without mv -T.
                let incoming = format!("{remote_stage}/.devkit-publish");
                let source = format!("{remote_stage}/{root}");
                let renamed = format!("{incoming}/{name}");
                self.remote_shell(&settings,target,format!("mkdir -p {p}; [ ! -e {d} ] && [ ! -L {d} ] && mv -n {s} {r} && mv -n {r} {parent}/ && [ ! -e {r} ]",p=quote(&incoming),d=quote(&final_path),s=quote(&source),r=quote(&renamed),parent=quote(&set.directory)),30).await?;
            } else {
                regular_local(Path::new(&set.directory), true)?;
                crate::recovery::publish_new_file(&local_stage.join(root), Path::new(&final_path))?;
            }
            published.push(name.clone());
            self.line(id, format!("已发布：{name}"))?;
        }
        // A successful mv response alone does not prove publication. Older Android mv
        // can nest a source inside a concurrently created directory; external changes
        // can also occur between individual root commits. Verify every final root
        // before any original backup becomes eligible for cleanup.
        for (root, name) in &names {
            #[cfg(test)]
            crate::fault_tests::checkpoint("phone.transfer.before_final_verify")?;
            let destination = destination(set, name);
            let matches = if upload {
                let actual = self.remote_rows(&settings, target, &destination).await?;
                let expected = local_rows(&local_stage.join(root))?;
                match (expected, actual) {
                    (Some(expected), Some(actual))
                        if fingerprint(expected.clone()) == journal.new[root] =>
                    {
                        rows_match(expected, actual)
                    }
                    _ => false,
                }
            } else {
                local_fingerprint(Path::new(&destination))?.as_ref() == Some(&journal.new[root])
            };
            if !matches {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    format!("发布后的目标内容未核对一致：{name}；保留原件备份，请检查目标"),
                ));
            }
        }
        if protected {
            for (index, (_, name)) in names.iter().enumerate() {
                if old[name].is_some() {
                    let backup = if upload {
                        format!("{remote_stage}/.devkit-backups/{index}")
                    } else {
                        local_stage
                            .join(".devkit-backups")
                            .join(index.to_string())
                            .to_string_lossy()
                            .into()
                    };
                    if self.target_fingerprint(set, target, &backup).await? != old[name] {
                        return Err(AppError::new(
                            "RESULT_UNKNOWN",
                            "旧目标备份被外部修改，已保留备份，请核对新旧内容",
                        ));
                    }
                }
            }
            if upload {
                self.remote_shell(
                    &settings,
                    target,
                    format!("rm -f -- {}/.devkit-protected", quote(remote_stage)),
                    15,
                )
                .await?;
            } else {
                std::fs::remove_file(local_stage.join(".devkit-protected"))?;
            }
        }
        Ok(())
    }
    pub async fn restore_transfer(&self, op: &Operation) -> Result<String> {
        let source = self.store.task(op.arg("sourceTaskId")?)?;
        let _source_use = self.use_task_record(&source.id, true)?;
        if source.active() || source.operation.optional("conflicts") != "replace" {
            return Err(AppError::new("TRANSFER_RESTORE", "请选择已结束的替换传输"));
        }
        let set = phone_files::load(self, source.operation.arg("transferId")?)?;
        if phone_files::manifest_hash(self, &set.id)? != op.arg("manifestHash")? {
            return Err(AppError::new("SOURCE_CHANGED", "传输清单已变化，未恢复"));
        }
        let target = op
            .target
            .as_ref()
            .ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择原设备"))?;
        if target.serial != set.target.serial {
            return Err(AppError::new("STALE_SESSION", "只能恢复原设备上的旧目标"));
        }
        let path = journal_path(self, &source.id, &set.id)?;
        if digest_file(&path)? != op.arg("journalHash")? {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "恢复日志已变化，未覆盖目标",
            ));
        }
        let journal: Journal = serde_json::from_slice(&std::fs::read(path)?)?;
        if journal.names != names(&set, &source.operation)?
            || journal.old
                != serde_json::from_str::<Snapshots>(source.operation.arg("destinationSnapshots")?)?
        {
            return Err(AppError::new(
                "TRANSFER_RESTORE",
                "备份日志与原计划不一致，未恢复",
            ));
        }
        let upload = set.direction == "upload";
        let stage = if upload {
            phone_files::owned_remote(&set.directory, &source.id)?
        } else {
            Path::new(&set.directory)
                .join(format!(".devkit-transfer-{}", source.id))
                .to_string_lossy()
                .into()
        };
        let settings = self.store.settings();
        if upload {
            self.check_remote(&settings, target, &set.directory).await?;
            self.remote_shell(&settings,target,format!("[ ! -L {s} ] && [ ! -L {s}/.devkit-owner ] && [ \"$(cat {s}/.devkit-owner)\" = {id} ]",s=quote(&stage),id=quote(&source.id)),15).await?;
        } else {
            regular_local(Path::new(&stage), true)?;
            if std::fs::read(Path::new(&stage).join(".devkit-owner"))? != source.id.as_bytes() {
                return Err(AppError::new("UNSAFE_PATH", "暂存归属无效，未恢复"));
            }
        }
        let mut restored = 0;
        for (index, (root, name)) in journal.names.iter().enumerate() {
            let Some(expected) = journal.old.get(name).and_then(Option::as_ref) else {
                continue;
            };
            let final_path = destination(&set, name);
            let backup = format!("{stage}/.devkit-backups/{index}");
            let restore_path = format!("{stage}/.devkit-restore/{name}");
            let mut backup_path = backup;
            let mut backup_hash = self.target_fingerprint(&set, target, &backup_path).await?;
            if backup_hash.is_none() {
                backup_path = restore_path.clone();
                backup_hash = self.target_fingerprint(&set, target, &backup_path).await?;
            }
            let current = self.target_fingerprint(&set, target, &final_path).await?;
            if current.as_ref() == Some(expected) {
                if backup_hash.is_some() && backup_hash.as_ref() != Some(expected) {
                    return Err(AppError::new(
                        "EXTERNAL_CHANGE",
                        "原件备份已被外部修改，保留备份，未清理",
                    ));
                }
                restored += 1;
                continue;
            }
            if backup_hash.as_ref() != Some(expected) {
                return Err(AppError::new(
                    "TRANSFER_RESTORE",
                    "原件备份缺失或已变化，未覆盖目标",
                ));
            }
            if current.is_some() && current.as_ref() != journal.new.get(root) {
                return Err(AppError::new(
                    "EXTERNAL_CHANGE",
                    "目标在传输后被外部修改，保留目标与备份，未恢复",
                ));
            }
            if upload {
                let rollback = format!("{stage}/.devkit-rollback");
                let restore_dir = format!("{stage}/.devkit-restore");
                if current.is_some() {
                    self.remote_shell(&settings,target,format!("mkdir -p {r}; [ ! -e {r}/{index} ] && mv -n {d} {r}/{index} && [ ! -e {d} ]",r=quote(&rollback),d=quote(&final_path)),30).await?;
                }
                self.remote_shell(&settings,target,format!("mkdir -p {r}; [ ! -e {d} ] && [ ! -L {d} ] && {rename}mv -n {b} {parent}/ && [ ! -e {b} ]",r=quote(&restore_dir),d=quote(&final_path),rename=if backup_path!=restore_path {format!("mv -n {} {} && ",quote(&backup_path),quote(&restore_path))}else{String::new()},b=quote(&restore_path),parent=quote(&set.directory)),30).await?;
            } else {
                if current.is_some() {
                    let rollback = Path::new(&stage).join(".devkit-rollback");
                    std::fs::create_dir_all(&rollback)?;
                    crate::recovery::publish_new_file(
                        Path::new(&final_path),
                        &rollback.join(index.to_string()),
                    )?;
                }
                crate::recovery::publish_new_file(Path::new(&backup_path), Path::new(&final_path))?;
            }
            if self
                .target_fingerprint(&set, target, &final_path)
                .await?
                .as_ref()
                != Some(expected)
            {
                return Err(AppError::new(
                    "RESULT_UNKNOWN",
                    "恢复后的原件尚未核对一致，保留暂存",
                ));
            }
            restored += 1;
        }
        // Only after every original target is verified may the retained staging be discarded.
        if upload {
            self.remote_shell(
                &settings,
                target,
                format!(
                    "rm -f -- {s}/.devkit-protected; rm -rf -- {s}",
                    s = quote(&stage)
                ),
                30,
            )
            .await?;
        } else {
            let marker = Path::new(&stage).join(".devkit-protected");
            if marker.exists() {
                std::fs::remove_file(marker)?;
            }
            phone_files::cleanup_local(Path::new(&stage), &source.id, true)?;
        }
        self.update(&source.id, |t| {
            t.state = "cancelled".into();
            t.phase = "complete".into();
            t.message = "已恢复被替换的旧目标；已发布的新建文件保留，未重放传输".into();
            t.operation
                .args
                .insert("backupsResolved".into(), "true".into());
        })?;
        Ok(format!(
            "已恢复并核对 {restored} 个旧目标；新建已发布文件保留"
        ))
    }
}
fn rows_match(mut expected: ContentRows, actual: ContentRows) -> bool {
    // Size-only devices still require the exact file/directory tree and every size.
    for (relative, directory, _, hash) in &actual {
        if !directory && hash.is_empty() {
            if let Some(row) = expected.iter_mut().find(|row| &row.0 == relative) {
                row.3.clear();
            }
        }
    }
    fingerprint(expected) == fingerprint(actual)
}
fn destination(set: &TransferSet, name: &str) -> String {
    if set.direction == "upload" {
        format!("{}/{name}", set.directory)
    } else {
        Path::new(&set.directory)
            .join(name)
            .to_string_lossy()
            .into()
    }
}
fn journal_path(app: &App, id: &str, transfer: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("TRANSFER_ID", "任务标识无效"))?;
    Ok(phone_files::transfer_dir(app, transfer)?.join(format!("{id}.replacement.json")))
}
fn write_journal(app: &App, id: &str, transfer: &str, journal: &Journal) -> Result<()> {
    let mut f = std::fs::File::create(journal_path(app, id, transfer)?)?;
    f.write_all(&serde_json::to_vec(journal)?)?;
    f.sync_all()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_verification_requires_the_complete_tree_even_without_hashes() {
        let expected = vec![
            ("".into(), true, 0, String::new()),
            ("file.txt".into(), false, 3, "abc".into()),
            ("empty".into(), true, 0, String::new()),
        ];
        assert!(rows_match(expected.clone(), expected.clone()));
        let mut size_only = expected.clone();
        size_only[1].3.clear();
        assert!(rows_match(expected.clone(), size_only.clone()));
        size_only[1].2 = 4;
        assert!(!rows_match(expected.clone(), size_only));
        let mut nested = expected.clone();
        nested[1].0 = "file.txt/file.txt".into();
        assert!(!rows_match(expected.clone(), nested));
        let mut changed = expected.clone();
        changed[1].3 = "changed".into();
        assert!(!rows_match(expected.clone(), changed));
        let mut extra = expected.clone();
        extra.push(("unreviewed.txt".into(), false, 1, String::new()));
        assert!(!rows_match(expected.clone(), extra));
        let mut missing = expected.clone();
        missing.pop();
        assert!(!rows_match(expected, missing));
    }
    #[test]
    fn unresolved_replacement_backups_protect_the_task_record_until_recovery_is_verified() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(temp.path().join("app")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let mut source = crate::regression_tests::task(&id, "needs_review");
        source.phase = "reconciled".into();
        source.operation.kind = "file_upload".into();
        source
            .operation
            .args
            .insert("conflicts".into(), "replace".into());
        app.store.save_task(&source).unwrap();
        let request = crate::domain::TaskDeleteRequest {
            task_ids: vec![id.clone()],
            ..Default::default()
        };
        assert_eq!(app.delete_task_records(request).unwrap().deleted, 0);
        assert!(app
            .task_records_page(&Default::default())
            .unwrap()
            .deletion
            .blocked
            .contains_key(&id));
        app.update(&id, |t| {
            t.operation
                .args
                .insert("backupsResolved".into(), "true".into());
        })
        .unwrap();
        let request = crate::domain::TaskDeleteRequest {
            task_ids: vec![id],
            ..Default::default()
        };
        assert_eq!(app.delete_task_records(request).unwrap().deleted, 1);
    }
    #[tokio::test]
    async fn rollback_restores_files_and_directories_but_refuses_external_edits() {
        for directory in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let app = App::new(temp.path().join("app")).unwrap();
            let destination = temp.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            let final_path = destination.join("replace");
            let write = |path: &Path, bytes: &[u8]| {
                if directory {
                    std::fs::create_dir_all(path).unwrap();
                    std::fs::write(path.join("file"), bytes).unwrap();
                } else {
                    std::fs::write(path, bytes).unwrap();
                }
            };
            write(&final_path, b"original");
            let old = local_fingerprint(&final_path).unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            let set_id = uuid::Uuid::new_v4().to_string();
            let dir = phone_files::transfer_dir(&app, &set_id).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            let target = Target {
                serial: "frozen".into(),
                transport_id: "1".into(),
                server_epoch: 1,
            };
            let set = TransferSet {
                id: set_id.clone(),
                direction: "download".into(),
                target: target.clone(),
                directory: destination.to_string_lossy().into(),
                items: vec![],
                roots: vec!["replace".into()],
                conflicts: vec!["replace".into()],
            };
            std::fs::write(dir.join("set.json"), serde_json::to_vec(&set).unwrap()).unwrap();
            let stage = destination.join(format!(".devkit-transfer-{id}"));
            std::fs::create_dir_all(stage.join(".devkit-backups")).unwrap();
            std::fs::write(stage.join(".devkit-owner"), &id).unwrap();
            std::fs::write(stage.join(".devkit-protected"), &id).unwrap();
            crate::recovery::publish_new_file(&final_path, &stage.join(".devkit-backups/0"))
                .unwrap();
            write(&final_path, b"replacement");
            let snapshots = BTreeMap::from([("replace".into(), old.clone())]);
            let journal = Journal {
                names: BTreeMap::from([("replace".into(), "replace".into())]),
                old: snapshots.clone(),
                new: BTreeMap::from([(
                    "replace".into(),
                    local_fingerprint(&final_path).unwrap().unwrap(),
                )]),
            };
            write_journal(&app, &id, &set_id, &journal).unwrap();
            let mut source = crate::regression_tests::task(&id, "needs_review");
            source.operation = Operation {
                kind: "file_download".into(),
                target: Some(target.clone()),
                args: BTreeMap::from([
                    ("transferId".into(), set_id.clone()),
                    ("conflicts".into(), "replace".into()),
                    (
                        "destinationSnapshots".into(),
                        serde_json::to_string(&snapshots).unwrap(),
                    ),
                    (
                        "manifestHash".into(),
                        phone_files::manifest_hash(&app, &set_id).unwrap(),
                    ),
                ]),
            };
            app.store.save_task(&source).unwrap();
            let mut op = Operation {
                kind: "file_restore".into(),
                target: Some(target),
                args: BTreeMap::from([("sourceTaskId".into(), id.clone())]),
            };
            phone_files::plan(&app, &mut op, &mut vec![], &mut vec![]).unwrap();
            write(&final_path, b"external edit");
            assert_eq!(
                app.restore_transfer(&op).await.unwrap_err().code,
                "EXTERNAL_CHANGE"
            );
            assert!(stage.join(".devkit-backups/0").exists());
            write(&final_path, b"replacement");
            app.restore_transfer(&op).await.unwrap();
            assert_eq!(local_fingerprint(&final_path).unwrap(), old);
            assert!(!stage.exists());
            assert_eq!(app.store.task(&id).unwrap().state, "cancelled");
        }
    }
}
