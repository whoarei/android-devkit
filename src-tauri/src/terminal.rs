//! Persistent, byte-oriented PTYs. Only this module owns terminal processes.
use crate::{android, app::App, domain::*, process};
use base64::Engine;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc, Condvar, Mutex, Weak,
    },
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, State};

const WINDOW_BYTES: u64 = 128 * 1024;
const INPUT_BYTES: usize = 16 * 1024;
const SESSION_LIMIT: usize = 8;
type Sink = Arc<dyn Fn(TerminalEvent) -> bool + Send + Sync>;

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    pub id: String,
    pub generation: String,
    pub kind: String,
    pub label: String,
    pub target: Option<Target>,
    pub sdk_root: String,
    pub state: String,
    pub reason: String,
    pub exit_code: Option<u32>,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEvent {
    pub generation: String,
    pub kind: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub offset: u64,
    pub data: Vec<u8>,
    pub state: String,
    pub reason: String,
    pub exit_code: Option<u32>,
}

#[derive(Default)]
struct Flow {
    sent: u64,
    acked: u64,
}
struct Input {
    sender: Option<SyncSender<Vec<u8>>>,
    sequence: u32,
}
struct Session {
    owner: String,
    info: Mutex<TerminalInfo>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
    input: Mutex<Input>,
    flow: Mutex<Flow>,
    credit: Condvar,
    stopping: AtomicBool,
    reader_done: AtomicBool,
    done: AtomicBool,
    stop_reason: Mutex<Option<(String, String)>>,
    sink: Mutex<Option<Sink>>,
}
impl Session {
    fn write(&self, sequence: u32, data: Vec<u8>) -> Result<()> {
        if data.is_empty() || data.len() > INPUT_BYTES {
            return Err(AppError::new("TERMINAL_INPUT", "终端输入过长或为空"));
        }
        if self.stopping.load(Ordering::SeqCst) || self.done.load(Ordering::SeqCst) {
            return Err(AppError::new("TERMINAL_CLOSED", "终端会话已关闭"));
        }
        let mut input = self.input.lock().unwrap();
        if sequence != input.sequence + 1 {
            return Err(AppError::new("TERMINAL_SEQUENCE", "终端输入顺序已变化"));
        }
        input
            .sender
            .as_ref()
            .ok_or_else(|| AppError::new("TERMINAL_CLOSED", "终端会话已关闭"))?
            .try_send(data)
            .map_err(|_| AppError::new("TERMINAL_INPUT_BUSY", "终端输入队列已满，请稍后重试"))?;
        input.sequence = sequence;
        Ok(())
    }
    fn ack(&self, offset: u64) -> Result<()> {
        let mut flow = self.flow.lock().unwrap();
        if offset > flow.sent {
            return Err(AppError::new("TERMINAL_ACK", "终端输出确认无效"));
        }
        flow.acked = flow.acked.max(offset);
        drop(flow);
        self.credit.notify_all();
        Ok(())
    }
    fn info(&self) -> TerminalInfo {
        self.info.lock().unwrap().clone()
    }
    fn stop(&self, state: &str, reason: &str) {
        {
            let mut saved = self.stop_reason.lock().unwrap();
            if saved.is_none() {
                *saved = Some((state.into(), reason.into()));
            }
        }
        self.stopping.store(true, Ordering::SeqCst);
        self.credit.notify_all();
        self.input.lock().unwrap().sender.take();
        if let Some(killer) = self.killer.lock().unwrap().as_mut() {
            let _ = killer.kill();
        }
    }
    fn send_data(&self, data: Vec<u8>) {
        let mut flow = self.flow.lock().unwrap();
        while flow.sent - flow.acked >= WINDOW_BYTES && !self.stopping.load(Ordering::SeqCst) {
            flow = self.credit.wait(flow).unwrap();
        }
        if self.stopping.load(Ordering::SeqCst) {
            return; // Continue draining the OS pipe during ConPTY shutdown.
        }
        flow.sent += data.len() as u64;
        let offset = flow.sent;
        drop(flow);
        let generation = self.info.lock().unwrap().generation.clone();
        let sink = self.sink.lock().unwrap().clone();
        if sink.is_some_and(|sink| {
            !sink(TerminalEvent {
                generation,
                kind: "data".into(),
                offset,
                data,
                state: String::new(),
                reason: String::new(),
                exit_code: None,
            })
        }) {
            self.stop("closed", "终端视图已关闭");
        }
    }
    fn finish(&self, code: Option<u32>) {
        let (state, reason) = self
            .stop_reason
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| ("exited".into(), String::new()));
        let mut info = self.info.lock().unwrap();
        info.state = state;
        info.reason = reason;
        info.exit_code = code;
        let event = TerminalEvent {
            generation: info.generation.clone(),
            kind: "state".into(),
            offset: self.flow.lock().unwrap().sent,
            data: vec![],
            state: info.state.clone(),
            reason: info.reason.clone(),
            exit_code: code,
        };
        drop(info);
        if let Some(sink) = self.sink.lock().unwrap().take() {
            let _ = sink(event);
        }
        self.done.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    retiring: Mutex<Vec<Arc<Session>>>,
}
impl TerminalManager {
    fn start(
        &self,
        owner: String,
        mut info: TerminalInfo,
        command: CommandBuilder,
        size: PtySize,
        sink: Sink,
        app: Weak<App>,
    ) -> Result<TerminalInfo> {
        let mut sessions = self.sessions.lock().unwrap();
        self.retiring
            .lock()
            .unwrap()
            .retain(|s| !s.done.load(Ordering::SeqCst));
        if sessions.len() >= SESSION_LIMIT {
            return Err(AppError::new(
                "TERMINAL_LIMIT",
                "最多保留 8 个终端会话，请先关闭一个会话",
            ));
        }
        if sessions.contains_key(&info.id) {
            return Err(AppError::new("TERMINAL_EXISTS", "终端会话已存在"));
        }
        let pair = native_pty_system().openpty(size).map_err(pty_error)?;
        let mut child = pair.slave.spawn_command(command).map_err(pty_error)?;
        drop(pair.slave);
        // Take both endpoints before transferring ownership to the worker threads.
        let mut reader = match pair.master.try_clone_reader() {
            Ok(value) => value,
            Err(error) => {
                let _ = child.kill();
                return Err(pty_error(error));
            }
        };
        let mut writer = match pair.master.take_writer() {
            Ok(value) => value,
            Err(error) => {
                let _ = child.kill();
                return Err(pty_error(error));
            }
        };
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(32);
        info.state = "active".into();
        let session = Arc::new(Session {
            owner,
            info: Mutex::new(info.clone()),
            master: Mutex::new(Some(pair.master)),
            killer: Mutex::new(Some(child.clone_killer())),
            input: Mutex::new(Input {
                sender: Some(sender),
                sequence: 0,
            }),
            flow: Mutex::new(Flow::default()),
            credit: Condvar::new(),
            stopping: AtomicBool::new(false),
            reader_done: AtomicBool::new(false),
            done: AtomicBool::new(false),
            stop_reason: Mutex::new(None),
            sink: Mutex::new(Some(sink)),
        });
        sessions.insert(info.id.clone(), session.clone());
        drop(sessions);
        let reading = session.clone();
        std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => reading.send_data(bytes[..n].to_vec()),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            reading.reader_done.store(true, Ordering::SeqCst);
        });
        let writing = session.clone();
        std::thread::spawn(move || {
            while let Ok(bytes) = receiver.recv() {
                if writing.stopping.load(Ordering::SeqCst) {
                    break;
                }
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    writing.stop("failed", "终端输入已断开");
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let target = session.info().target;
            let mut checked = Instant::now();
            let code = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status.exit_code()),
                    Err(_) => {
                        session.stop("failed", "无法读取终端进程状态");
                        break None;
                    }
                    Ok(None) => {}
                }
                if checked.elapsed() >= Duration::from_secs(1)
                    && !session.stopping.load(Ordering::SeqCst)
                {
                    checked = Instant::now();
                    if let Some(target) = &target {
                        let valid = app.upgrade().is_some_and(|app| {
                            let epoch = app.epoch.load(Ordering::SeqCst);
                            android::host_request("host:devices-l").is_ok_and(|raw| {
                                checked_target(
                                    target,
                                    &android::parse_devices(&String::from_utf8_lossy(&raw), epoch),
                                    epoch,
                                )
                                .is_ok()
                            })
                        });
                        if !valid {
                            session.stop("disconnected", "原设备连接已断开或变化，请重新连接");
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            session.input.lock().unwrap().sender.take();
            // ClosePseudoConsole may block on older Windows. Its output pipe is
            // drained by the independent reader, never by this teardown thread.
            let master = session.master.lock().unwrap().take();
            drop(master);
            while !session.reader_done.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            session.killer.lock().unwrap().take();
            session.finish(code);
        });
        Ok(info)
    }
    fn get(&self, id: &str, generation: &str) -> Result<Arc<Session>> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::new("TERMINAL_CLOSED", "终端会话已关闭"))?;
        if session.info().generation != generation {
            return Err(AppError::new(
                "TERMINAL_STALE",
                "终端连接已经变化，请重新输入",
            ));
        }
        Ok(session)
    }
    pub fn active_count(&self) -> usize {
        let active = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| !s.done.load(Ordering::SeqCst))
            .count();
        active
            + self
                .retiring
                .lock()
                .unwrap()
                .iter()
                .filter(|s| !s.done.load(Ordering::SeqCst))
                .count()
    }
    pub fn check_sdk_unused(&self, root: &str) -> Result<()> {
        let normalized = normalize_path(root);
        let sessions = self.sessions.lock().unwrap();
        let retiring = self.retiring.lock().unwrap();
        if sessions.values().chain(retiring.iter()).any(|s| {
            !s.done.load(Ordering::SeqCst) && normalize_path(&s.info().sdk_root) == normalized
        }) {
            return Err(AppError::new(
                "TERMINAL_SDK_BUSY",
                "该 SDK 仍被终端会话使用，请先关闭相关会话",
            ));
        }
        Ok(())
    }
    pub fn close(&self, id: &str, generation: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get(id) {
            if session.info().generation != generation {
                return Err(AppError::new(
                    "TERMINAL_STALE",
                    "终端连接已经变化，请重新输入",
                ));
            }
        }
        if let Some(session) = sessions.remove(id) {
            session.stop("closed", "");
            let mut retiring = self.retiring.lock().unwrap();
            retiring.retain(|s| !s.done.load(Ordering::SeqCst));
            retiring.push(session);
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        let mut sessions: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .drain()
            .map(|(_, s)| s)
            .collect();
        sessions.extend(self.retiring.lock().unwrap().drain(..));
        for session in &sessions {
            session.stop("closed", "");
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while sessions.iter().any(|s| !s.done.load(Ordering::SeqCst)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn list(&self, owner: &str) -> Vec<TerminalInfo> {
        // A WebView reload starts a new owner. Reap its previous orphan PTYs;
        // ordinary React navigation keeps the same owner and terminal objects.
        let mut sessions = self.sessions.lock().unwrap();
        let mut retiring = self.retiring.lock().unwrap();
        retiring.retain(|s| !s.done.load(Ordering::SeqCst));
        sessions.retain(|_, s| {
            if s.owner != owner {
                s.stop("closed", "终端视图已重新载入");
                retiring.push(s.clone());
                false
            } else {
                true
            }
        });
        sessions.values().map(|s| s.info()).collect()
    }
}
fn normalize_path(root: &str) -> String {
    std::fs::canonicalize(root)
        .unwrap_or_else(|_| root.into())
        .to_string_lossy()
        .replace('/', "\\")
        .to_lowercase()
}
fn pty_error(error: impl std::fmt::Display) -> AppError {
    AppError::new("TERMINAL_FAILED", "无法启动交互终端").detail(error.to_string())
}
fn pty_size(cols: u16, rows: u16) -> Result<PtySize> {
    if !(2..=500).contains(&cols) || !(1..=250).contains(&rows) {
        return Err(AppError::new("TERMINAL_SIZE", "终端尺寸无效"));
    }
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}
fn terminal_command(
    settings: &Settings,
    adb: &Path,
    target: Option<&Target>,
) -> Result<CommandBuilder> {
    let windows = std::env::var_os("SystemRoot").ok_or_else(|| pty_error("SystemRoot missing"))?;
    let powershell = Path::new(&windows).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    if !powershell.is_file() {
        return Err(pty_error("PowerShell missing"));
    }
    let mut command = CommandBuilder::new(powershell);
    if let Some(target) = target {
        // Windows adb reads console input using ReadConsoleInputA. Set the console
        // code page before launching it so ConPTY's UTF-8 input survives intact.
        // The script is fixed; executable paths and transport IDs stay data.
        let script = "$OutputEncoding = [Console]::InputEncoding = [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; & $env:ANDROID_DEVKIT_ADB_EXE -H 127.0.0.1 -P 5037 -t $env:ANDROID_DEVKIT_TRANSPORT_ID shell -tt; exit $LASTEXITCODE";
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        command.args(["-NoLogo", "-NoProfile", "-EncodedCommand", &encoded]);
        command.env("ANDROID_DEVKIT_ADB_EXE", adb);
        command.env("ANDROID_DEVKIT_TRANSPORT_ID", &target.transport_id);
    } else {
        // Static bootstrap only: paths are passed as environment values.
        command.args(["-NoLogo", "-NoProfile", "-NoExit", "-Command", "$OutputEncoding = [Console]::InputEncoding = [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false"]);
    }
    command.cwd(&settings.sdk_root);
    let mut paths = vec![Path::new(&settings.sdk_root).join("platform-tools")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    command.env("PATH", std::env::join_paths(paths).map_err(pty_error)?);
    command.env("ANDROID_HOME", &settings.sdk_root);
    command.env("ANDROID_SDK_ROOT", &settings.sdk_root);
    command.env("ANDROID_ADB_SERVER_ADDRESS", "127.0.0.1");
    command.env("ANDROID_ADB_SERVER_PORT", "5037");
    command.env("TERM", "xterm-256color");
    for name in [
        "ANDROID_SERIAL",
        "ADB_SERVER_SOCKET",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
    ] {
        command.env_remove(name);
    }
    Ok(command)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn terminal_create(
    app: State<'_, Arc<App>>,
    id: String,
    owner: String,
    kind: String,
    target: Option<Target>,
    cols: u16,
    rows: u16,
    on_event: Channel<TerminalEvent>,
) -> Result<TerminalInfo> {
    let _update = app.allow_update_sensitive_work()?;
    let _sdk = app.sdk_gate.read().await;
    if app.draining.load(Ordering::SeqCst) {
        return Err(AppError::new("APP_CLOSING", "应用正在退出"));
    }
    if uuid::Uuid::parse_str(&id).is_err()
        || uuid::Uuid::parse_str(&owner).is_err()
        || !matches!(kind.as_str(), "device" | "host")
        || (kind == "device") != target.is_some()
    {
        return Err(AppError::new("INVALID_TERMINAL", "终端会话参数无效"));
    }
    let size = pty_size(cols, rows)?;
    let settings = app.store.settings();
    let adb = android::compatible_adb(&settings).await?;
    // The shared daemon must be started outside the session's pseudoconsole.
    if android::host_request("host:version").is_err() {
        process::run(process::Spec::new(&adb, vec!["start-server".into()]))
            .await?
            .success()?;
    }
    let label = if let Some(target) = &target {
        let epoch = app.epoch.load(Ordering::SeqCst);
        let raw = android::host_request("host:devices-l")?;
        let devices = android::parse_devices(&String::from_utf8_lossy(&raw), epoch);
        let device = checked_target(target, &devices, epoch)?;
        if device.model.is_empty() {
            device.serial.clone()
        } else {
            device.model.clone()
        }
    } else {
        "通用命令行".into()
    };
    let command = terminal_command(&settings, &adb, target.as_ref())?;
    let info = TerminalInfo {
        id,
        generation: uuid::Uuid::new_v4().to_string(),
        kind,
        label,
        target,
        sdk_root: settings.sdk_root,
        state: "starting".into(),
        reason: String::new(),
        exit_code: None,
    };
    let service = app.inner().clone();
    let weak = Arc::downgrade(&service);
    tokio::task::spawn_blocking(move || {
        service.terminals.start(
            owner,
            info,
            command,
            size,
            Arc::new(move |event| on_event.send(event).is_ok()),
            weak,
        )
    })
    .await
    .map_err(pty_error)?
}
#[tauri::command]
pub fn terminal_list(app: State<'_, Arc<App>>, owner: String) -> Result<Vec<TerminalInfo>> {
    if uuid::Uuid::parse_str(&owner).is_err() {
        return Err(AppError::new("INVALID_TERMINAL", "终端会话参数无效"));
    }
    Ok(app.terminals.list(&owner))
}
#[tauri::command]
pub fn terminal_write(
    app: State<'_, Arc<App>>,
    id: String,
    generation: String,
    sequence: u32,
    data: Vec<u8>,
) -> Result<()> {
    app.terminals.get(&id, &generation)?.write(sequence, data)
}
#[tauri::command]
pub fn terminal_resize(
    app: State<'_, Arc<App>>,
    id: String,
    generation: String,
    cols: u16,
    rows: u16,
) -> Result<()> {
    let size = pty_size(cols, rows)?;
    let session = app.terminals.get(&id, &generation)?;
    if let Some(master) = session.master.lock().unwrap().as_ref() {
        master.resize(size).map_err(pty_error)?;
    }
    Ok(())
}
#[tauri::command]
pub fn terminal_ack(
    app: State<'_, Arc<App>>,
    id: String,
    generation: String,
    offset: u64,
) -> Result<()> {
    app.terminals.get(&id, &generation)?.ack(offset)
}
#[tauri::command]
pub fn terminal_close(app: State<'_, Arc<App>>, id: String, generation: String) -> Result<()> {
    app.terminals.close(&id, &generation)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    struct Fixture {
        manager: Arc<TerminalManager>,
        session: Arc<Session>,
        events: mpsc::Receiver<TerminalEvent>,
        output: Vec<u8>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.manager.shutdown();
        }
    }
    impl Fixture {
        fn start(
            command: CommandBuilder,
            root: &str,
            target: Option<Target>,
            app: Weak<App>,
        ) -> Self {
            Self::start_with_ack(command, root, target, app, true)
        }
        fn start_with_ack(
            command: CommandBuilder,
            root: &str,
            target: Option<Target>,
            app: Weak<App>,
            auto_ack: bool,
        ) -> Self {
            let manager = Arc::new(TerminalManager::default());
            let weak = Arc::downgrade(&manager);
            let id = uuid::Uuid::new_v4().to_string();
            let callback_id = id.clone();
            let (sender, events) = mpsc::channel::<TerminalEvent>();
            let sink: Sink = Arc::new(move |event| {
                if let Some(manager) = weak.upgrade() {
                    if let Ok(session) = manager.get(&callback_id, &event.generation) {
                        if event.kind == "data" {
                            if auto_ack {
                                let _ = session.ack(event.offset);
                            }
                            if event.data.windows(4).any(|s| s == b"\x1b[6n") {
                                if let Some(sender) = session.input.lock().unwrap().sender.as_ref()
                                {
                                    let _ = sender.try_send(b"\x1b[1;1R".to_vec());
                                }
                            }
                        }
                    }
                }
                sender.send(event).is_ok()
            });
            let info = manager
                .start(
                    uuid::Uuid::new_v4().to_string(),
                    TerminalInfo {
                        id,
                        generation: uuid::Uuid::new_v4().to_string(),
                        kind: if target.is_some() { "device" } else { "host" }.into(),
                        label: "fixture".into(),
                        target,
                        sdk_root: root.into(),
                        state: "starting".into(),
                        reason: String::new(),
                        exit_code: None,
                    },
                    command,
                    pty_size(80, 24).unwrap(),
                    sink,
                    app,
                )
                .unwrap();
            let session = manager.get(&info.id, &info.generation).unwrap();
            Self {
                manager,
                session,
                events,
                output: vec![],
            }
        }
        fn send(&self, input: &str) {
            let sequence = self.session.input.lock().unwrap().sequence + 1;
            self.session
                .write(sequence, input.as_bytes().to_vec())
                .unwrap();
        }
        fn until(&mut self, text: &str) {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !String::from_utf8_lossy(&self.output).contains(text) {
                assert!(
                    Instant::now() < deadline,
                    "missing {text:?}: {}",
                    String::from_utf8_lossy(&self.output)
                );
                if let Ok(event) = self.events.recv_timeout(Duration::from_millis(50)) {
                    self.output.extend(event.data);
                }
            }
        }
    }

    #[test]
    fn terminal_pty_powershell_unicode_edit_interrupt_resize_and_close() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("SDK 中文 ' $()");
        std::fs::create_dir(&root).unwrap();
        let settings = Settings {
            sdk_root: root.to_string_lossy().into(),
            ..Default::default()
        };
        let command = terminal_command(&settings, Path::new("unused"), None).unwrap();
        assert_eq!(command.get_env("ANDROID_HOME"), Some(root.as_os_str()));
        assert!(command.get_env("ANDROID_SERIAL").is_none());
        let mut fixture = Fixture::start(command, &settings.sdk_root, None, Weak::new());
        fixture.until("PS ");
        assert!(fixture
            .manager
            .check_sdk_unused(&settings.sdk_root)
            .is_err());
        fixture.send("Write-Output ('输入'+'中文'); Write-Output ('UTF8'+'-READY')\r");
        fixture.until("UTF8-READY");
        assert!(String::from_utf8_lossy(&fixture.output).contains("输入中文"));
        fixture.send("Write-Output ('EDIT'+'-OXX\x7f\x7fK')\r");
        fixture.until("EDIT-OK");
        fixture
            .session
            .master
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .resize(pty_size(100, 32).unwrap())
            .unwrap();
        assert_eq!(
            fixture
                .session
                .master
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .get_size()
                .unwrap()
                .rows,
            32
        );
        fixture.send("while($true){Start-Sleep -Milliseconds 10}\r");
        std::thread::sleep(Duration::from_millis(150));
        fixture.until("Start-Sleep");
        while fixture.events.try_recv().is_ok() {}
        fixture.output.clear();
        fixture.send("\x03");
        fixture.until("PS ");
        fixture.send("Write-Output ('INT'+'-OK')\r");
        fixture.until("INT-OK");
        let sequence = fixture.session.input.lock().unwrap().sequence;
        assert_eq!(
            fixture
                .session
                .write(sequence, b"x".to_vec())
                .unwrap_err()
                .code,
            "TERMINAL_SEQUENCE"
        );
        assert!(fixture.session.ack(u64::MAX).is_err());
        let info = fixture.session.info();
        fixture.manager.close(&info.id, &info.generation).unwrap();
        fixture.manager.close(&info.id, &info.generation).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fixture.session.done.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(fixture.session.done.load(Ordering::SeqCst));
        assert!(fixture.session.sink.lock().unwrap().is_none());
        assert_eq!(fixture.manager.active_count(), 0);
        fixture
            .manager
            .check_sdk_unused(&settings.sdk_root)
            .unwrap();
    }

    #[test]
    fn terminal_backpressure_is_bounded_and_close_unblocks_reader() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            sdk_root: dir.path().to_string_lossy().into(),
            ..Default::default()
        };
        let command = terminal_command(&settings, Path::new("unused"), None).unwrap();
        let mut fixture =
            Fixture::start_with_ack(command, &settings.sdk_root, None, Weak::new(), false);
        fixture.until("PS ");
        fixture.send("1..30000 | ForEach-Object { [Console]::WriteLine('pressure-buffer-test-0123456789') }\r");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let flow = fixture.session.flow.lock().unwrap();
            if flow.sent - flow.acked >= WINDOW_BYTES {
                assert!(flow.sent - flow.acked <= WINDOW_BYTES + 8192);
                break;
            }
            drop(flow);
            assert!(
                Instant::now() < deadline,
                "producer did not reach the flow-control limit"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let info = fixture.session.info();
        assert_eq!(
            fixture
                .manager
                .get(&info.id, "old-generation")
                .err()
                .unwrap()
                .code,
            "TERMINAL_STALE"
        );
        fixture.manager.close(&info.id, &info.generation).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fixture.session.done.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            fixture.session.done.load(Ordering::SeqCst),
            "closing must unblock a paused PTY reader"
        );
    }

    #[test]
    fn terminal_device_command_binds_transport_without_shell_interpolation() {
        let settings = Settings {
            sdk_root: "E:\\SDK 中文 ' & $()".into(),
            ..Default::default()
        };
        let target = Target {
            serial: "pixel".into(),
            transport_id: "13".into(),
            server_epoch: 7,
        };
        let adb = Path::new(&settings.sdk_root).join("platform-tools/adb.exe");
        let command = terminal_command(&settings, &adb, Some(&target)).unwrap();
        let args: Vec<_> = command
            .get_argv()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args[0].ends_with("powershell.exe"));
        assert_eq!(&args[1..4], ["-NoLogo", "-NoProfile", "-EncodedCommand"]);
        assert_eq!(
            command.get_env("ANDROID_DEVKIT_ADB_EXE"),
            Some(adb.as_os_str())
        );
        assert_eq!(
            command.get_env("ANDROID_DEVKIT_TRANSPORT_ID"),
            Some(std::ffi::OsStr::new("13"))
        );
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&args[4])
            .unwrap();
        let script = String::from_utf16(
            &decoded
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(script.contains("$env:ANDROID_DEVKIT_ADB_EXE -H 127.0.0.1 -P 5037 -t $env:ANDROID_DEVKIT_TRANSPORT_ID shell -tt; exit $LASTEXITCODE"));
        assert!(!script.contains(&settings.sdk_root));
        assert!(!args.iter().any(|a| a == "-NoExit"));
        assert!(pty_size(0, 24).is_err());
    }

    #[test]
    #[ignore = "read-only integration with explicitly selected emulator in ANDROID_DEVKIT_TERMINAL_TEST_SERIAL"]
    fn terminal_real_device_shell() {
        let serial = std::env::var("ANDROID_DEVKIT_TERMINAL_TEST_SERIAL").unwrap();
        assert!(serial.starts_with("emulator-"));
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let mut settings = app.store.settings();
        if let Ok(root) = std::env::var("ANDROID_DEVKIT_TERMINAL_TEST_SDK") {
            settings.sdk_root = root;
            app.store.save_settings(&settings).unwrap();
        }
        let epoch = app.epoch.load(Ordering::SeqCst);
        let raw = android::host_request("host:devices-l").unwrap();
        let devices = android::parse_devices(&String::from_utf8_lossy(&raw), epoch);
        let target = Target::from(
            devices
                .iter()
                .find(|d| d.serial == serial && d.state == "ready")
                .unwrap(),
        );
        let adb = Path::new(&settings.sdk_root).join("platform-tools/adb.exe");
        let command = terminal_command(&settings, &adb, Some(&target)).unwrap();
        let mut fixture = Fixture::start(
            command,
            &settings.sdk_root,
            Some(target.clone()),
            Arc::downgrade(&app),
        );
        fixture.until(":/ ");
        fixture.send("echo '设备''终端'; echo 'SHELL''-READY'\r");
        fixture.until("SHELL-READY");
        assert!(
            String::from_utf8_lossy(&fixture.output).contains("设备终端"),
            "{}",
            String::from_utf8_lossy(&fixture.output)
        );
        fixture
            .session
            .master
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .resize(pty_size(100, 32).unwrap())
            .unwrap();
        fixture.send("stty size; echo 'SIZE''-READY'\r");
        fixture.until("SIZE-READY");
        assert!(
            String::from_utf8_lossy(&fixture.output).contains("32 100"),
            "{}",
            String::from_utf8_lossy(&fixture.output)
        );
        fixture.send("top\r");
        std::thread::sleep(Duration::from_millis(300));
        fixture.send("q");
        fixture.send("echo 'RAW''-READY'\r");
        fixture.until("RAW-READY");
        fixture.send("logcat\r");
        std::thread::sleep(Duration::from_millis(200));
        while fixture.events.try_recv().is_ok() {}
        fixture.output.clear();
        fixture.send("\x03");
        fixture.until(":/ ");
        fixture.send("echo 'CTRL''-READY'\r");
        fixture.until("CTRL-READY");
        fixture.send("exit\r");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fixture.session.done.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fixture.session.info().state, "exited");
        // A new ADB server identity must retire the original binding, even if
        // the same emulator serial is still present on the shared server.
        let command = terminal_command(&settings, &adb, Some(&target)).unwrap();
        let mut rebound = Fixture::start(
            command,
            &settings.sdk_root,
            Some(target),
            Arc::downgrade(&app),
        );
        rebound.until(":/ ");
        app.epoch.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !rebound.session.done.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(rebound.session.info().state, "disconnected");
        assert!(android::host_request("host:version").is_ok());
    }
}
