use crate::domain::*;
use crate::preferences::{UserPreferences, WindowPlacement, LEGACY_FIELDS};
use rusqlite::{params, Connection};
use std::{path::Path, sync::Mutex};

const TASK_SEARCH: &str =
    "id NOT IN (SELECT task_id FROM deleted_task_records) AND (?1 = '' OR instr(lower(
    coalesce(json_extract(json,'$.title'),'') || char(10) ||
    coalesce(json_extract(json,'$.message'),'') || char(10) || id || char(10) ||
    coalesce(json_extract(json,'$.operation.kind'),'') || char(10) ||
    coalesce(json_extract(json,'$.operation.target.serial'),'') || char(10) ||
    coalesce(json_extract(json,'$.operation.args.address'),'') || char(10) ||
    coalesce(json_extract(json,'$.operation.args.avdId'),'') || char(10) ||
    coalesce(json_extract(json,'$.operation.args.package'),'') || char(10) ||
    coalesce(json_extract(json,'$.operation.args.packages'),'')), lower(?1)) > 0)";
const TASK_STATE: &str = "(?2 IN ('','all') OR json_extract(json,'$.state') = ?2 OR
    (?2 = 'active' AND json_extract(json,'$.state') IN ('queued','running','waiting_confirmation')))";

pub struct Store {
    connection: Mutex<Connection>,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 2 {
            return Err(AppError::new(
                "DATABASE_VERSION",
                "数据库来自较新版本，保留原库，请使用匹配的应用版本",
            ));
        }
        if version == 1 {
            let backup = path.with_file_name(format!("devkit.pre-v2-{}.db", uuid::Uuid::new_v4()));
            connection.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
        }
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1),json TEXT NOT NULL); CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY,request_id TEXT UNIQUE NOT NULL,json TEXT NOT NULL); CREATE TABLE IF NOT EXISTS audit(id INTEGER PRIMARY KEY,created_at INTEGER NOT NULL,task_id TEXT NOT NULL,event TEXT NOT NULL); CREATE TABLE IF NOT EXISTS artifacts(id TEXT PRIMARY KEY,json TEXT NOT NULL,path TEXT NOT NULL); PRAGMA user_version=2;")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS sdk_catalog (id INTEGER PRIMARY KEY CHECK(id=1),packages TEXT NOT NULL,sites TEXT NOT NULL);")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS deleted_task_records(task_id TEXT PRIMARY KEY);",
        )?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS ui_preferences(key TEXT PRIMARY KEY,json TEXT NOT NULL);",
        )?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        for mut task in store.active_tasks()? {
            if task.active() {
                task.state = "needs_review".into();
                task.phase = "recovery".into();
                task.message = "应用中断，正在复查实际状态；不会自动重放写操作".into();
                task.license_prompt.clear();
                store.save_task(&task)?;
            }
        }
        Ok(store)
    }
    pub fn settings(&self) -> Settings {
        self.connection
            .lock()
            .unwrap()
            .query_row("SELECT json FROM settings WHERE id=1", [], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn has_settings(&self) -> Result<bool> {
        Ok(self.connection.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM settings WHERE id=1)",
            [],
            |row| row.get(0),
        )?)
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.connection.lock().unwrap().execute("INSERT INTO settings(id,json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json",[serde_json::to_string(settings)?])?;
        Ok(())
    }
    fn read_preferences(connection: &Connection) -> Result<UserPreferences> {
        let mut preferences = UserPreferences::default();
        let mut statement = connection.prepare("SELECT key,json FROM ui_preferences")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        // One corrupt/unknown field must not discard the remaining preferences.
        // Leave unknown fields on disk for forward compatibility.
        for row in rows {
            let (key, json) = row?;
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Ok(next) = preferences.patched(&serde_json::json!({key: value})) {
                    preferences = next;
                }
            }
        }
        Ok(preferences)
    }
    pub fn preferences(&self) -> Result<UserPreferences> {
        Self::read_preferences(&self.connection.lock().unwrap())
    }
    pub fn patch_preferences(&self, patch: &serde_json::Value) -> Result<UserPreferences> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let next = Self::read_preferences(&transaction)?.patched(patch)?;
        for (key, value) in patch.as_object().unwrap() {
            transaction.execute(
                "INSERT INTO ui_preferences(key,json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET json=excluded.json WHERE json<>excluded.json",
                params![key, serde_json::to_string(value)?],
            )?;
        }
        transaction.commit()?;
        Ok(next)
    }
    pub fn import_legacy_preferences(
        &self,
        legacy: Option<&serde_json::Value>,
    ) -> Result<UserPreferences> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        if let Some(legacy) = legacy.and_then(|value| value.as_object()) {
            for key in LEGACY_FIELDS {
                if let Some(value) = legacy.get(*key) {
                    if UserPreferences::default()
                        .patched(&serde_json::json!({*key: value}))
                        .is_ok()
                    {
                        transaction.execute(
                            "INSERT OR IGNORE INTO ui_preferences(key,json) VALUES(?1,?2)",
                            params![key, serde_json::to_string(value)?],
                        )?;
                    }
                }
            }
        }
        let preferences = Self::read_preferences(&transaction)?;
        transaction.commit()?;
        Ok(preferences)
    }
    pub fn zoom_factor(&self) -> Result<f64> {
        Ok(self.preferences()?.zoom_factor)
    }
    pub fn save_zoom_factor(&self, factor: f64) -> Result<()> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(AppError::new("INVALID_ZOOM", "界面缩放比例无效"));
        }
        self.patch_preferences(&serde_json::json!({"zoomFactor": factor}))?;
        Ok(())
    }
    pub fn save_window_placement(&self, placement: &WindowPlacement) -> Result<()> {
        self.patch_preferences(&serde_json::json!({"window": placement}))?;
        Ok(())
    }
    pub fn tasks(&self) -> Result<Vec<Task>> {
        self.query_tasks("SELECT json FROM tasks WHERE id NOT IN (SELECT task_id FROM deleted_task_records) ORDER BY CASE WHEN json_extract(json,'$.state') IN ('queued','running','waiting_confirmation') THEN 0 ELSE 1 END, rowid DESC LIMIT 200")
    }
    pub fn workspace_tasks(&self) -> Result<Vec<Task>> {
        let mut tasks = self.tasks()?;
        // Recovery entry points must survive the bounded recent-history window.
        for task in self.query_tasks("SELECT json_set(json,'$.log',json('[]')) FROM tasks WHERE id NOT IN (SELECT task_id FROM deleted_task_records) AND (json_extract(json,'$.state')='needs_review' OR json_extract(json,'$.operation.args.deviceCleanupPending')='true') ORDER BY rowid")? {
            if matches!(
                task.operation.kind.as_str(),
                "screen_record" | "file_upload" | "file_download"
            ) && task.operation.optional("backupsResolved") != "true"
                && !tasks.iter().any(|recent| recent.id == task.id)
            {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }
    pub fn sdk_catalog(&self) -> Option<(Vec<SdkPackage>, Vec<SdkSiteStatus>)> {
        let (packages, sites): (String, String) = self
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT packages,sites FROM sdk_catalog WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()?;
        Some((
            serde_json::from_str(&packages).ok()?,
            serde_json::from_str(&sites).ok()?,
        ))
    }
    pub fn save_sdk_catalog(
        &self,
        settings: Option<&Settings>,
        packages: &[SdkPackage],
        sites: &[SdkSiteStatus],
    ) -> Result<()> {
        let packages = serde_json::to_string(packages)?;
        let sites = serde_json::to_string(sites)?;
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        if let Some(settings) = settings {
            transaction.execute("INSERT INTO settings(id,json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json",[serde_json::to_string(settings)?])?;
        }
        transaction.execute("INSERT INTO sdk_catalog(id,packages,sites) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET packages=excluded.packages,sites=excluded.sites",params![packages,sites])?;
        transaction.commit()?;
        Ok(())
    }
    pub fn task_page(&self, query: &TaskQuery) -> Result<TaskPage> {
        let state = query.state.as_str();
        if !matches!(
            state,
            "" | "all"
                | "active"
                | "queued"
                | "running"
                | "waiting_confirmation"
                | "succeeded"
                | "failed"
                | "cancelled"
                | "needs_review"
        ) {
            return Err(AppError::new("INVALID_TASK_STATE", "未知的任务筛选状态"));
        }
        let keyword = query.keyword.trim();
        // instr treats '%' and '_' literally and all input remains parameterized.
        let search = TASK_SEARCH;
        let connection = self.connection.lock().unwrap();
        let mut counts = std::collections::BTreeMap::<String, u32>::new();
        let mut statement = connection.prepare(&format!(
            "SELECT json_extract(json,'$.state'), count(*) FROM tasks WHERE {search} GROUP BY json_extract(json,'$.state')"
        ))?;
        for row in statement.query_map([keyword], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
        })? {
            let (state, count) = row?;
            counts.insert(state, count);
        }
        let total = counts
            .iter()
            .filter(|(s, _)| match state {
                "" | "all" => true,
                "active" => matches!(s.as_str(), "queued" | "running" | "waiting_confirmation"),
                _ => s.as_str() == state,
            })
            .map(|(_, count)| count)
            .sum::<u32>();
        let page_size = 30;
        // A background status change may remove the last row on the last page.
        let offset = query
            .offset
            .min(total.saturating_sub(1) / page_size * page_size);
        let mut statement = connection.prepare(&format!(
            "SELECT json FROM tasks WHERE {search} AND {TASK_STATE} ORDER BY CASE WHEN json_extract(json,'$.state') IN ('queued','running','waiting_confirmation') THEN 0 ELSE 1 END, rowid DESC LIMIT ?3 OFFSET ?4"
        ))?;
        let rows = statement.query_map(params![keyword, state, page_size, offset], |r| {
            r.get::<_, String>(0)
        })?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(serde_json::from_str(&row?)?);
        }
        Ok(TaskPage {
            tasks,
            total,
            counts,
            offset,
            page_size,
            deletion: TaskDeletionSummary::default(),
        })
    }
    fn query_tasks(&self, sql: &str) -> Result<Vec<Task>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(sql)?;
        let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(serde_json::from_str(&row?)?);
        }
        Ok(tasks)
    }
    pub fn active_tasks(&self) -> Result<Vec<Task>> {
        self.query_tasks("SELECT json FROM tasks WHERE json_extract(json,'$.state') IN ('queued','running','waiting_confirmation') ORDER BY rowid")
    }
    pub fn review_tasks(&self) -> Result<Vec<Task>> {
        self.query_tasks("SELECT json FROM tasks WHERE id NOT IN (SELECT task_id FROM deleted_task_records) AND json_extract(json,'$.state')='needs_review' ORDER BY rowid")
    }
    pub fn task_record_deleted(&self, id: &str) -> Result<bool> {
        Ok(self.connection.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM deleted_task_records WHERE task_id=?1)",
            [id],
            |row| row.get(0),
        )?)
    }
    pub fn matching_task_records(&self, query: &TaskQuery) -> Result<Vec<Task>> {
        self.task_page(query)?;
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(&format!("SELECT json_set(json,'$.log',json('[]')) FROM tasks WHERE {TASK_SEARCH} AND {TASK_STATE} ORDER BY rowid DESC"))?;
        let rows = statement.query_map(params![query.keyword.trim(), query.state], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    pub fn delete_task_record(&self, id: &str) -> Result<bool> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM deleted_task_records WHERE task_id=?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )? {
            return Ok(false);
        }
        let raw: String =
            tx.query_row("SELECT json FROM tasks WHERE id=?1", [id], |row| row.get(0))?;
        let mut task: Task = serde_json::from_str(&raw)?;
        if task.active() || (task.state == "needs_review" && task.phase == "recovery") {
            return Err(AppError::new(
                "TASK_ACTIVE",
                "任务仍在执行或恢复，请等待结束",
            ));
        }
        let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM artifacts WHERE json_extract(json,'$.taskId')=?1 AND coalesce(json_extract(json,'$.acknowledged'),0)=0)", [id], |row| row.get(0))?;
        if pending || (task.operation.kind == "log_record" && !task.artifact.is_empty()) {
            return Err(AppError::new("TASK_FILES_PENDING", "请先处理任务文件"));
        }
        // Retain identity/ownership for deduplicated submissions, AVD recovery
        // and acknowledged file caches. User-visible diagnostics are removed.
        task.log.clear();
        task.message = "任务记录已删除".into();
        task.license_prompt.clear();
        tx.execute(
            "UPDATE tasks SET json=?1 WHERE id=?2",
            params![serde_json::to_string(&task)?, id],
        )?;
        tx.execute("INSERT INTO deleted_task_records(task_id) VALUES(?1)", [id])?;
        tx.commit()?;
        Ok(true)
    }
    pub fn register_artifact(
        &self,
        id: &str,
        path: &Path,
        kind: &str,
        title: &str,
        task_id: &str,
    ) -> Result<Artifact> {
        let a = Artifact {
            id: id.into(),
            kind: kind.into(),
            title: title.into(),
            task_id: task_id.into(),
            bytes: path.metadata()?.len() as usize,
            sha256: digest_file(path)?,
            created_at: now(),
            acknowledged: false,
        };
        self.connection.lock().unwrap().execute(
            "INSERT INTO artifacts(id,json,path) VALUES(?1,?2,?3) ON CONFLICT(id) DO NOTHING",
            params![
                id,
                serde_json::to_string(&a)?,
                path.to_string_lossy().as_ref()
            ],
        )?;
        Ok(a)
    }
    pub fn artifacts(&self) -> Result<Vec<Artifact>> {
        let c = self.connection.lock().unwrap();
        let mut q = c.prepare("SELECT json FROM artifacts ORDER BY rowid DESC")?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn artifact(&self, id: &str) -> Result<(Artifact, std::path::PathBuf)> {
        let (a, p): (String, String) = self.connection.lock().unwrap().query_row(
            "SELECT json,path FROM artifacts WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((serde_json::from_str(&a)?, p.into()))
    }
    pub fn acknowledge(&self, id: &str) -> Result<()> {
        let (mut a, _) = self.artifact(id)?;
        a.acknowledged = true;
        self.connection.lock().unwrap().execute(
            "UPDATE artifacts SET json=?1 WHERE id=?2",
            params![serde_json::to_string(&a)?, id],
        )?;
        Ok(())
    }
    pub fn forget_artifact(&self, id: &str, path: &Path) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let tasks = {
            let mut statement = transaction
                .prepare("SELECT json FROM tasks WHERE json_extract(json,'$.artifact')=?1")?;
            let rows = statement
                .query_map([path.to_string_lossy().as_ref()], |r| r.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for json in tasks {
            let mut task: Task = serde_json::from_str(&json)?;
            task.artifact.clear();
            task.updated_at = now();
            task.message = "截图缓存已清理，任务记录已保留".into();
            transaction.execute(
                "UPDATE tasks SET json=?1 WHERE id=?2",
                params![serde_json::to_string(&task)?, task.id],
            )?;
        }
        transaction.execute("DELETE FROM artifacts WHERE id=?1", [id])?;
        transaction.commit()?;
        Ok(())
    }
    pub fn task(&self, id: &str) -> Result<Task> {
        let raw: String = self.connection.lock().unwrap().query_row(
            "SELECT json FROM tasks WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }
    pub fn request(&self, id: &str) -> Option<Task> {
        self.connection
            .lock()
            .unwrap()
            .query_row("SELECT json FROM tasks WHERE request_id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| serde_json::from_str(&v).ok())
    }
    pub fn save_task(&self, task: &Task) -> Result<()> {
        self.connection.lock().unwrap().execute("INSERT INTO tasks(id,request_id,json) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET json=excluded.json",params![task.id,task.request_id,serde_json::to_string(task)?])?;
        Ok(())
    }
    pub fn update_task(&self, id: &str, edit: impl FnOnce(&mut Task)) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let raw: String =
            transaction.query_row("SELECT json FROM tasks WHERE id=?1", [id], |r| r.get(0))?;
        let mut task: Task = serde_json::from_str(&raw)?;
        edit(&mut task);
        task.updated_at = now();
        transaction.execute(
            "UPDATE tasks SET json=?1 WHERE id=?2",
            params![serde_json::to_string(&task)?, id],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn audit(&self, id: &str, event: &str) -> Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO audit(created_at,task_id,event) VALUES(?1,?2,?3)",
            params![now(), id, event],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_user_preferences_survive_a_restart_and_old_settings_keep_defaults() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let store = Store::open(&path).unwrap();
        let patch = serde_json::json!({
            "theme":"dark", "locale":"en-US", "defaultTarget":"avd:fixture", "autoCheckUpdates":false,
            "zoomFactor":1.75, "window":{"x":-1800,"y":120,"width":1100.0,"height":720.0,"scaleFactor":1.5,"monitor":"secondary","maximized":true},
            "lastPage":"terminal", "deviceFilter":"emulator", "sdkTab":"tools", "sdkStatus":"updates", "hideObsolete":false,
            "taskState":"failed", "recordingSeconds":120, "recordingResolution":"720p", "logQuotaMib":128, "logMinutes":45
        });
        let expected = store.patch_preferences(&patch).unwrap();
        assert_ne!(expected, UserPreferences::default());
        drop(store);
        assert_eq!(Store::open(&path).unwrap().preferences().unwrap(), expected);
        let old_settings: Settings =
            serde_json::from_value(serde_json::json!({"sdkRoot":"X:\\retained"})).unwrap();
        assert_eq!(old_settings.sdk_root, "X:\\retained");
        assert_eq!(old_settings.boot_timeout_secs, 180);
        assert!(!old_settings.avd_roots.is_empty());
    }

    #[test]
    fn preferences_import_is_idempotent_and_keeps_database_authoritative() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let store = Store::open(&path).unwrap();
        store.save_zoom_factor(1.25).unwrap();
        let legacy = serde_json::json!({"theme":"dark", "locale":"en-US", "defaultTarget":"device:usb:fixture", "autoCheckUpdates":false});
        let imported = store.import_legacy_preferences(Some(&legacy)).unwrap();
        assert_eq!(imported.theme, "dark");
        assert_eq!(imported.locale, "en-US");
        assert!(!imported.auto_check_updates);
        assert_eq!(imported.zoom_factor, 1.25);
        assert!(!store.has_settings().unwrap());
        store
            .patch_preferences(&serde_json::json!({"theme":"light", "defaultTarget":""}))
            .unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let imported = store.import_legacy_preferences(Some(&legacy)).unwrap();
        assert_eq!(imported.theme, "light");
        assert_eq!(imported.default_target, "");
        assert_eq!(imported.locale, "en-US");
    }

    #[test]
    fn preference_patch_rolls_back_all_fields_on_storage_failure() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        store.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_preferences BEFORE INSERT ON ui_preferences WHEN NEW.key='theme' BEGIN SELECT RAISE(FAIL, 'injected preference failure'); END;").unwrap();
        let patch = serde_json::json!({"autoCheckUpdates":false,"locale":"en-US","theme":"dark"});
        assert!(store.patch_preferences(&patch).is_err());
        assert_eq!(store.preferences().unwrap(), UserPreferences::default());
        store
            .connection
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_preferences")
            .unwrap();
        let saved = store.patch_preferences(&patch).unwrap();
        assert_eq!(saved.theme, "dark");
        assert_eq!(saved.locale, "en-US");
        assert!(!saved.auto_check_updates);
    }

    #[test]
    fn corrupt_preference_is_isolated_and_unknown_future_fields_are_preserved() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        store.connection.lock().unwrap().execute_batch("INSERT INTO ui_preferences VALUES('theme','\"dark\"'),('locale','invalid'),('recordingSeconds','999'),('futureKey','{\"future\":true}')").unwrap();
        let saved = store.preferences().unwrap();
        assert_eq!(saved.theme, "dark");
        assert_eq!(saved.locale, "zh-CN");
        assert_eq!(saved.recording_seconds, 30);
        for patch in [
            serde_json::json!({"unexpected":true}),
            serde_json::json!({"theme":"other"}),
            serde_json::json!({"logMinutes":0}),
            serde_json::json!({"sdkStatus":"selected"}),
            serde_json::json!({"autoCheckUpdates":"false"}),
        ] {
            assert!(store.patch_preferences(&patch).is_err());
        }
        store
            .patch_preferences(&serde_json::json!({"locale":"en-US"}))
            .unwrap();
        let future: String = store
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT json FROM ui_preferences WHERE key='futureKey'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(future, "{\"future\":true}");
    }

    #[test]
    fn concurrent_ui_and_native_patches_do_not_overwrite_each_other() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let store = std::sync::Arc::new(Store::open(&path).unwrap());
        let placement = WindowPlacement {
            x: -1200,
            y: 80,
            width: 900.0,
            height: 700.0,
            scale_factor: 1.5,
            monitor: "secondary".into(),
            maximized: true,
        };
        store.save_window_placement(&placement).unwrap();
        let ui = store.clone();
        let native = store.clone();
        let ui_thread = std::thread::spawn(move || {
            for _ in 0..20 {
                ui.patch_preferences(
                    &serde_json::json!({"theme":"dark","sdkTab":"tools","recordingSeconds":90}),
                )
                .unwrap();
            }
        });
        let native_thread = std::thread::spawn(move || {
            for _ in 0..20 {
                native.save_zoom_factor(1.5).unwrap();
            }
        });
        ui_thread.join().unwrap();
        native_thread.join().unwrap();
        drop(store);
        let restored = Store::open(&path).unwrap().preferences().unwrap();
        assert_eq!(restored.theme, "dark");
        assert_eq!(restored.sdk_tab, "tools");
        assert_eq!(restored.recording_seconds, 90);
        assert_eq!(restored.zoom_factor, 1.5);
        assert_eq!(restored.window, Some(placement));
    }

    #[test]
    fn zoom_preference_survives_reopen_without_changing_environment_settings() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devkit.db");
        let store = Store::open(&path).unwrap();
        assert_eq!(store.zoom_factor().unwrap(), 1.0);
        store.save_zoom_factor(1.5).unwrap();
        assert!(!store.has_settings().unwrap());
        let settings = Settings {
            sdk_root: "X:\\custom".into(),
            ..Default::default()
        };
        store.save_settings(&settings).unwrap();
        assert_eq!(store.zoom_factor().unwrap(), 1.5);
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.zoom_factor().unwrap(), 1.5);
        assert_eq!(reopened.settings().sdk_root, settings.sdk_root);
        reopened.save_zoom_factor(1.0).unwrap();
        drop(reopened);
        assert_eq!(Store::open(&path).unwrap().zoom_factor().unwrap(), 1.0);
    }

    #[test]
    fn zoom_preference_rejects_invalid_values_and_recovers_from_corrupt_data() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("devkit.db")).unwrap();
        store.save_zoom_factor(1.25).unwrap();
        for factor in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                store.save_zoom_factor(factor).unwrap_err().code,
                "INVALID_ZOOM"
            );
            assert_eq!(store.zoom_factor().unwrap(), 1.25);
        }
        for json in ["invalid", "null", "0", "-1", "1e999"] {
            store
                .connection
                .lock()
                .unwrap()
                .execute(
                    "UPDATE ui_preferences SET json=?1 WHERE key='zoomFactor'",
                    [json],
                )
                .unwrap();
            assert_eq!(store.zoom_factor().unwrap(), 1.0);
        }
    }

    #[test]
    fn fault_sqlite_full_rolls_back_task_and_catalog_settings_atomically() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let store = Store::open(&path).unwrap();
        let old = crate::regression_tests::task("full", "failed");
        store.save_task(&old).unwrap();
        let settings = Settings {
            sdk_root: "before".into(),
            ..Default::default()
        };
        let package = SdkPackage {
            id: "old-package".into(),
            ..Default::default()
        };
        store
            .save_sdk_catalog(Some(&settings), &[package], &[])
            .unwrap();
        {
            let connection = store.connection.lock().unwrap();
            let pages: u32 = connection
                .query_row("PRAGMA page_count", [], |row| row.get(0))
                .unwrap();
            connection
                .pragma_update(None, "max_page_count", pages)
                .unwrap();
        }
        let mut enlarged = old.clone();
        enlarged.message = "new value requiring more database pages".repeat(65536);
        let error = store.save_task(&enlarged).unwrap_err();
        assert_eq!(error.code, "DATABASE_ERROR");
        assert!(error.message.contains("full"), "{error}");
        assert_eq!(store.task("full").unwrap().message, old.message);
        let changed = Settings {
            sdk_root: "after".into(),
            ..settings
        };
        let large = SdkPackage {
            id: "new-package".into(),
            name: "x".repeat(2 * 1024 * 1024),
            ..Default::default()
        };
        let error = store
            .save_sdk_catalog(Some(&changed), &[large], &[])
            .unwrap_err();
        assert_eq!(error.code, "DATABASE_ERROR");
        assert!(error.message.contains("full"), "{error}");
        assert_eq!(store.settings().sdk_root, "before");
        assert_eq!(store.sdk_catalog().unwrap().0[0].id, "old-package");
        store
            .connection
            .lock()
            .unwrap()
            .pragma_update(None, "max_page_count", 1_000_000)
            .unwrap();
        store.save_task(&enlarged).unwrap();
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.task("full").unwrap().message, enlarged.message);
        assert_eq!(reopened.settings().sdk_root, "before");
        crate::fault_tests::receipt(
            "sqlite-full-rollback",
            serde_json::json!({"realSqlitePagerLimit":true,"task":"rollback then retry","settingsAndCatalog":"atomic rollback","reopen":"passed"}),
        );
    }

    #[test]
    fn fault_sqlite_read_only_failure_preserves_persisted_task() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let store = Store::open(&path).unwrap();
        let mut task = crate::regression_tests::task("read-only", "failed");
        store.save_task(&task).unwrap();
        drop(store);
        let store = Store {
            connection: Mutex::new(
                Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .unwrap(),
            ),
        };
        task.state = "succeeded".into();
        let error = store.save_task(&task).unwrap_err();
        assert_eq!(error.code, "DATABASE_ERROR");
        assert_eq!(store.task("read-only").unwrap().state, "failed");
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.task("read-only").unwrap().state, "failed");
        reopened.save_task(&task).unwrap();
        assert_eq!(reopened.task("read-only").unwrap().state, "succeeded");
        crate::fault_tests::receipt(
            "sqlite-read-only",
            serde_json::json!({"realReadOnlyConnection":true,"falseSuccess":false,"retry":"passed"}),
        );
    }

    #[test]
    fn partial_task_deletion_rolls_back_diagnostics_and_only_retries_failures() {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::app::App::new(dir.path().into()).unwrap();
        for id in ["good", "bad"] {
            let mut task = crate::regression_tests::task(id, "failed");
            task.log.push("retained on failure".into());
            app.store.save_task(&task).unwrap();
        }
        app.store.connection.lock().unwrap().execute_batch("CREATE TRIGGER reject_record_delete BEFORE INSERT ON deleted_task_records WHEN NEW.task_id='bad' BEGIN SELECT RAISE(ABORT,'fixture storage failure'); END;").unwrap();
        let result = app
            .delete_task_records(TaskDeleteRequest {
                task_ids: vec!["good".into(), "bad".into()],
                query: None,
            })
            .unwrap();
        assert_eq!(result.deleted, 1);
        assert_eq!(result.issues.len(), 1);
        assert!(result.issues[0].retryable);
        assert_eq!(result.issues[0].task_id, "bad");
        assert_eq!(app.store.task("bad").unwrap().log, ["retained on failure"]);
        app.store
            .connection
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_record_delete")
            .unwrap();
        let result = app
            .delete_task_records(TaskDeleteRequest {
                task_ids: vec!["good".into(), "bad".into()],
                query: None,
            })
            .unwrap();
        assert_eq!(result.deleted, 1);
        assert_eq!(result.already_deleted, 1);
        assert!(app.store.tasks().unwrap().is_empty());
    }
    #[test]
    fn source_and_catalog_rollback_together_when_the_catalog_write_fails() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(&root.path().join("state.db")).unwrap();
        let old = Settings::default();
        let package = SdkPackage {
            id: "platform-tools".into(),
            version: "1.0.0".into(),
            ..Default::default()
        };
        store
            .save_sdk_catalog(Some(&old), std::slice::from_ref(&package), &[])
            .unwrap();
        store.connection.lock().unwrap().execute_batch("CREATE TRIGGER reject_catalog BEFORE UPDATE ON sdk_catalog BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        let mut changed = old.clone();
        changed.sdk_download.kind = "tencent".into();
        assert!(store.save_sdk_catalog(Some(&changed), &[], &[]).is_err());
        assert_eq!(store.settings().sdk_download, old.sdk_download);
        assert_eq!(store.sdk_catalog().unwrap().0[0].id, package.id);
    }
    use super::*;
    #[test]
    fn task_search_reaches_older_history_and_treats_patterns_literally() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        let mut old = crate::regression_tests::task("old-adb-failure", "failed");
        old.title = "下载 SDK 100%_done".into();
        old.message = "ADB CONNECTION REFUSED".into();
        old.operation
            .args
            .insert("address".into(), "192.168.1.20:5555".into());
        store.save_task(&old).unwrap();
        for index in 0..220 {
            store
                .save_task(&crate::regression_tests::task(
                    &format!("recent-{index}"),
                    "succeeded",
                ))
                .unwrap();
        }
        assert!(!store.tasks().unwrap().iter().any(|task| task.id == old.id));
        for keyword in [
            " adb connection ",
            "100%_done",
            "192.168.1.20",
            "old-adb-failure",
        ] {
            let page = store
                .task_page(&TaskQuery {
                    keyword: keyword.into(),
                    state: "failed".into(),
                    offset: 90,
                })
                .unwrap();
            assert_eq!(page.total, 1, "{keyword}");
            assert_eq!(page.offset, 0);
            assert_eq!(page.tasks[0].id, old.id);
        }
        for keyword in ["100%Xdone", "' OR 1=1 --"] {
            assert_eq!(
                store
                    .task_page(&TaskQuery {
                        keyword: keyword.into(),
                        ..Default::default()
                    })
                    .unwrap()
                    .total,
                0
            );
        }
    }
    #[test]
    fn task_pages_keep_status_counts_and_active_work_consistent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        for state in [
            "queued",
            "running",
            "waiting_confirmation",
            "failed",
            "needs_review",
            "cancelled",
        ] {
            store
                .save_task(&crate::regression_tests::task(state, state))
                .unwrap();
        }
        for index in 0..65 {
            store
                .save_task(&crate::regression_tests::task(
                    &format!("done-{index}"),
                    "succeeded",
                ))
                .unwrap();
        }
        let first = store.task_page(&TaskQuery::default()).unwrap();
        assert_eq!(first.total, 71);
        assert_eq!(first.tasks.len(), 30);
        assert!(first.tasks[..3].iter().all(Task::active));
        assert_eq!(first.counts["succeeded"], 65);
        let second = store
            .task_page(&TaskQuery {
                offset: 30,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(second.tasks.len(), 30);
        assert!(!second
            .tasks
            .iter()
            .any(|task| first.tasks.iter().any(|previous| previous.id == task.id)));
        let active = store
            .task_page(&TaskQuery {
                state: "active".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(active.total, 3);
        assert!(active.tasks.iter().all(Task::active));
        let last = store
            .task_page(&TaskQuery {
                state: "succeeded".into(),
                offset: u32::MAX,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(last.offset, 60);
        assert_eq!(last.tasks.len(), 5);
        assert!(last.tasks.iter().all(|task| task.state == "succeeded"));
        for state in [
            "failed",
            "needs_review",
            "cancelled",
            "waiting_confirmation",
        ] {
            let page = store
                .task_page(&TaskQuery {
                    state: state.into(),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(page.total, 1);
            assert_eq!(page.tasks[0].state, state);
            assert_eq!(page.counts["succeeded"], 65);
        }
        assert_eq!(
            store
                .task_page(&TaskQuery {
                    state: "invalid".into(),
                    ..Default::default()
                })
                .unwrap_err()
                .code,
            "INVALID_TASK_STATE"
        );
    }
    #[test]
    fn persistence_and_interrupted_task_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let s = Store::open(&path).unwrap();
        let settings = Settings {
            sdk_root: "X:\\custom".into(),
            ..Settings::default()
        };
        s.save_settings(&settings).unwrap();
        let t = Task {
            id: "t".into(),
            request_id: "r".into(),
            title: "x".into(),
            operation: Operation {
                kind: "capture".into(),
                args: Default::default(),
                target: None,
            },
            state: "running".into(),
            phase: "capture".into(),
            progress: None,
            message: "".into(),
            log: vec![],
            created_at: 1,
            updated_at: 1,
            cancel_requested: false,
            cancellable: true,
            artifact: "".into(),
            license_prompt: "secret".into(),
            confirmed_licenses: vec![],
        };
        s.save_task(&t).unwrap();
        for i in 0..220 {
            let mut old = t.clone();
            old.id = format!("past-{i}");
            old.request_id = old.id.clone();
            old.state = "succeeded".into();
            s.save_task(&old).unwrap();
        }
        assert_eq!(s.active_tasks().unwrap().len(), 1);
        assert_eq!(s.tasks().unwrap().first().unwrap().id, "t");
        drop(s);
        let s = Store::open(&path).unwrap();
        assert_eq!(s.settings().sdk_root, "X:\\custom");
        assert_eq!(s.task("t").unwrap().state, "needs_review");
        assert!(s.task("t").unwrap().license_prompt.is_empty());
        assert_eq!(s.request("r").unwrap().id, "t");
        // A v1 database is backed up before schema migration; data remains readable.
        s.connection
            .lock()
            .unwrap()
            .execute_batch("PRAGMA user_version=1")
            .unwrap();
        drop(s);
        let upgraded = Store::open(&path).unwrap();
        assert_eq!(upgraded.request("r").unwrap().id, "t");
        assert!(std::fs::read_dir(dir.path()).unwrap().flatten().any(|p| p
            .file_name()
            .to_string_lossy()
            .starts_with("devkit.pre-v2-")));
    }
}
