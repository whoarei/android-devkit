use crate::domain::*;
use rusqlite::{params, Connection};
use std::{path::Path, sync::Mutex};

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
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.connection.lock().unwrap().execute("INSERT INTO settings(id,json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json",[serde_json::to_string(settings)?])?;
        Ok(())
    }
    pub fn tasks(&self) -> Result<Vec<Task>> {
        self.query_tasks("SELECT json FROM tasks ORDER BY CASE WHEN json_extract(json,'$.state') IN ('queued','running','waiting_confirmation') THEN 0 ELSE 1 END, rowid DESC LIMIT 200")
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
        self.query_tasks("SELECT json FROM tasks WHERE json_extract(json,'$.state')='needs_review' ORDER BY rowid")
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
    use super::*;
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
