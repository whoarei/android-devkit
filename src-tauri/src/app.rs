use crate::{android, bootstrap, domain::*, storage::Store};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::Emitter;

pub struct App {
    pub store: Store,
    pub data: PathBuf,
    pub handle: Mutex<Option<tauri::AppHandle>>,
    pub plans: Mutex<HashMap<String, Plan>>,
    pub apks: Mutex<HashMap<String, ApkInfo>>,
    pub catalog: Mutex<Vec<SdkPackage>>,
    pub catalog_gate: tokio::sync::Mutex<()>,
    pub epoch: AtomicU64,
    pub draining: AtomicBool,
    pub update_gate: tokio::sync::RwLock<()>,
    pub updating: AtomicBool,
    pub app_update: crate::updater::UpdateState,
    pub english: AtomicBool,
    pub locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub sdk_gate: tokio::sync::RwLock<()>,
    pub device_slots: tokio::sync::Semaphore,
    pub answers: Mutex<HashMap<String, tokio::sync::mpsc::Sender<String>>>,
    pub logs: Mutex<HashMap<String, crate::log_buffer::LogBuffer>>,
    pub log_children: Mutex<HashMap<String, tokio::sync::mpsc::Sender<()>>>,
    pub log_views: Mutex<HashSet<String>>,
    pub process_cache: Mutex<HashMap<String, crate::observe::ProcessCache>>,
}
impl App {
    pub fn new(data: PathBuf) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&data)?;
        let catalog = std::fs::read(data.join("catalog.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut apks = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(data.join("apks")) {
            for file in entries.flatten().take(200) {
                if file.path().extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(raw) = std::fs::read(file.path()) {
                        if let Ok(apk) = serde_json::from_slice::<ApkInfo>(&raw) {
                            apks.insert(apk.id.clone(), apk);
                        }
                    }
                }
            }
        }
        Ok(Arc::new(Self {
            store: Store::open(&data.join("state/devkit.db"))?,
            data,
            handle: Mutex::new(None),
            plans: Mutex::new(HashMap::new()),
            apks: Mutex::new(apks),
            catalog: Mutex::new(catalog),
            catalog_gate: tokio::sync::Mutex::new(()),
            epoch: AtomicU64::new(now()),
            draining: AtomicBool::new(false),
            update_gate: tokio::sync::RwLock::new(()),
            updating: AtomicBool::new(false),
            app_update: crate::updater::UpdateState::default(),
            english: AtomicBool::new(false),
            locks: Mutex::new(HashMap::new()),
            sdk_gate: tokio::sync::RwLock::new(()),
            device_slots: tokio::sync::Semaphore::new(2),
            answers: Mutex::new(HashMap::new()),
            logs: Mutex::new(HashMap::new()),
            log_children: Mutex::new(HashMap::new()),
            log_views: Mutex::new(HashSet::new()),
            process_cache: Mutex::new(HashMap::new()),
        }))
    }
    pub fn notify(&self) {
        let handle = self.handle.lock().unwrap().clone();
        if let Some(h) = handle {
            let _ = h.emit_to("main", "workspace-changed", now());
            crate::lifecycle::update_tray(&h, self);
        }
    }
    pub fn update(&self, id: &str, edit: impl FnOnce(&mut Task)) -> Result<()> {
        self.store.update_task(id, edit)?;
        self.notify();
        Ok(())
    }
    pub fn phase(&self, id: &str, phase: &str, message: &str) -> Result<()> {
        self.update(id, |t| {
            if t.phase != phase {
                t.progress = None;
            }
            t.phase = phase.into();
            t.message = message.into();
        })
    }
    pub fn line(&self, id: &str, line: String) -> Result<()> {
        self.update(id, |t| {
            let line = line.chars().take(2000).collect::<String>();
            t.message = line.clone();
            t.log.push(line);
            if t.log.len() > 100 {
                t.log.drain(..t.log.len() - 100);
            }
        })
    }
    pub fn cancelled(&self, id: &str) -> bool {
        self.store
            .task(id)
            .map(|t| t.cancel_requested)
            .unwrap_or(true)
    }
    pub fn lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(key.into())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
    pub async fn snapshot(&self) -> Result<Workspace> {
        let settings = self.store.settings();
        let epoch = self.epoch.load(Ordering::SeqCst);
        let devices = android::devices(&settings, epoch).await;
        let mut avds = android::avds(&settings);
        let mut name_counts: HashMap<String, usize> = HashMap::new();
        for avd in &avds {
            *name_counts.entry(avd.name.clone()).or_default() += 1;
        }
        for a in &mut avds {
            if let Some(d) = devices
                .iter()
                .find(|d| d.avd_name == a.name && name_counts.get(&a.name) == Some(&1))
            {
                a.serial = d.serial.clone();
                a.status = if d.state == "ready" {
                    "ready"
                } else {
                    "booting"
                }
                .into();
            } else if a.status == "stopped" && avd_locked(Path::new(&a.data_path)) {
                a.status = "busy".into();
                a.error = "检测到外部进程锁，先检查运行实例".into();
            }
        }
        let local = android::installed(&settings);
        let mut packages = self.catalog.lock().unwrap().clone();
        packages.retain(|package| crate::repository::package_enabled(package, &settings));
        for p in local {
            if let Some(remote) = packages.iter_mut().find(|r| r.id == p.id) {
                remote.installed = true;
                remote.installed_version = p.version;
            } else {
                packages.push(p);
            }
        }
        let server = android::host_request("host:version")
            .map(|v| format!("共享 ADB · 协议 {}", String::from_utf8_lossy(&v)))
            .unwrap_or_else(|_| "ADB 服务未运行".into());
        self.reconcile(&packages, &avds, &devices)?;
        Ok(Workspace {
            environment: android::environment(&settings),
            free_bytes: bootstrap::free_space(Path::new(&settings.sdk_root)),
            settings,
            packages,
            avds,
            devices,
            tasks: self.store.tasks()?,
            server,
            server_epoch: epoch,
        })
    }
    pub async fn refresh_catalog(&self) -> Result<Vec<String>> {
        let _guard = self.catalog_gate.lock().await;
        let settings = self.store.settings();
        let (p, statuses) = crate::repository::fetch(&settings).await?;
        if serde_json::to_vec(&settings)? != serde_json::to_vec(&self.store.settings())? {
            return Err(AppError::new(
                "CATALOG_SETTINGS_CHANGED",
                "刷新期间设置发生变化，请按当前设置重新刷新目录",
            ));
        }
        std::fs::write(
            self.data.join("sdk-site-status.json"),
            serde_json::to_vec(&statuses)?,
        )?;
        let w: Vec<_> = statuses
            .iter()
            .filter(|status| !status.error.is_empty())
            .map(|status| format!("{}: {}", status.name, status.error))
            .collect();
        if p.is_empty() {
            self.notify();
            return Err(AppError::new(
                "CATALOG_UNAVAILABLE",
                if w.is_empty() {
                    "所选更新通道暂无可用组件".into()
                } else {
                    w.join("\n")
                },
            ));
        }
        std::fs::write(self.data.join("catalog.json"), serde_json::to_vec(&p)?)?;
        *self.catalog.lock().unwrap() = p;
        self.notify();
        Ok(w)
    }
    pub fn prepare_plan(&self, operation: Operation) -> Result<Plan> {
        crate::plans::build(self, operation)
    }
    pub fn monitor(self: &Arc<Self>) {
        let app = self.clone();
        std::thread::spawn(move || loop {
            use std::io::{Read, Write};
            if let Ok(mut socket) = std::net::TcpStream::connect_timeout(
                &"127.0.0.1:5037".parse().unwrap(),
                std::time::Duration::from_secs(1),
            ) {
                let _ = socket.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                let request = "host:track-devices";
                if socket
                    .write_all(format!("{:04x}{request}", request.len()).as_bytes())
                    .is_ok()
                {
                    let mut status = [0; 4];
                    if socket.read_exact(&mut status).is_ok() && &status == b"OKAY" {
                        app.epoch.fetch_add(1, Ordering::SeqCst);
                        app.prune_logs();
                        app.notify();
                        loop {
                            let mut length = [0; 4];
                            if read_tracking_frame(&mut socket, &mut length).is_err() {
                                break;
                            }
                            let len = usize::from_str_radix(
                                std::str::from_utf8(&length).unwrap_or(""),
                                16,
                            )
                            .unwrap_or(usize::MAX);
                            if len > 1024 * 1024 {
                                break;
                            }
                            let mut buf = vec![0; len];
                            if read_tracking_frame(&mut socket, &mut buf).is_err() {
                                break;
                            }
                            app.notify();
                        }
                        app.epoch.fetch_add(1, Ordering::SeqCst);
                        app.prune_logs();
                        app.notify();
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        });
    }
}
// A tracking connection is normally silent until the device list changes. Keep
// partial frame bytes across idle timeouts; only EOF or a real I/O error ends it.
fn read_tracking_frame(reader: &mut impl std::io::Read, bytes: &mut [u8]) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        match reader.read(&mut bytes[offset..]) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => offset += n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
pub fn validate_app_package(value: &str) -> Result<()> {
    if !value.contains('.')
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
    {
        return Err(AppError::new("INVALID_PACKAGE_NAME", "应用包名无效"));
    }
    Ok(())
}
pub fn validate_avd_values(op: &Operation) -> Result<()> {
    if !op.optional("memory").is_empty()
        && !op
            .optional("memory")
            .parse::<u32>()
            .ok()
            .is_some_and(|v| (512..=16384).contains(&v))
    {
        return Err(AppError::new("INVALID_MEMORY", "内存范围为 512～16384 MiB"));
    }
    if !op.optional("storage").is_empty()
        && !op
            .optional("storage")
            .parse::<u32>()
            .ok()
            .is_some_and(|v| (2..=128).contains(&v))
    {
        return Err(AppError::new("INVALID_STORAGE", "存储范围为 2～128 GiB"));
    }
    if !op.optional("gpu").is_empty() && !matches!(op.optional("gpu"), "auto" | "host" | "software")
    {
        return Err(AppError::new("INVALID_GPU", "不支持的图形模式"));
    }
    crate::avd_settings::validate(op)
}
pub fn avd_locked(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|entries| {
            entries.flatten().any(|entry| {
                if !entry.file_name().to_string_lossy().ends_with(".lock") {
                    return false;
                }
                if entry.path().is_dir() {
                    if let Ok(pid) = std::fs::read_to_string(entry.path().join("pid")) {
                        if let Ok(pid) = pid.trim().parse::<u32>() {
                            return crate::platform::process_running(pid);
                        }
                    }
                    return std::fs::read_dir(entry.path())
                        .map(|mut children| children.next().is_some())
                        .unwrap_or(true);
                }
                let mut options = std::fs::OpenOptions::new();
                options.read(true).write(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.share_mode(0);
                }
                match options.open(entry.path()) {
                    Ok(file) => match fs2::FileExt::try_lock_exclusive(&file) {
                        Ok(()) => {
                            let _ = fs2::FileExt::unlock(&file);
                            false
                        }
                        Err(_) => true,
                    },
                    Err(error) => error.kind() != std::io::ErrorKind::NotFound,
                }
            })
        })
        .unwrap_or(false)
}
pub fn assert_avd_stopped(avd: &Avd) -> Result<()> {
    if avd_locked(Path::new(&avd.data_path)) {
        Err(AppError::new(
            "AVD_IN_USE",
            "检测到模拟器锁；请先停止实例，不会删除外部锁文件",
        ))
    } else {
        Ok(())
    }
}
pub fn assert_package_unused(id: &str, settings: &Settings, avds: &[Avd]) -> Result<()> {
    for a in avds {
        if image_references_package(&a.image, id, settings)
            || (id == "emulator" && avd_locked(Path::new(&a.data_path)))
        {
            return Err(AppError::new(
                "SDK_IN_USE",
                format!("{} 正在引用该组件", a.display_name),
            ));
        }
    }
    if id == "platform-tools" && android::host_request("host:version").is_ok() {
        return Err(AppError::new(
            "SDK_IN_USE",
            "共享 ADB 服务正在运行，拒绝替换或卸载 platform-tools",
        ));
    }
    if (id == "emulator" || id.starts_with("system-images;"))
        && android::host_request("host:devices-l").is_ok_and(|raw| {
            String::from_utf8_lossy(&raw)
                .lines()
                .any(|line| line.starts_with("emulator-"))
        })
    {
        return Err(AppError::new(
            "SDK_IN_USE",
            "ADB 中仍有运行中的模拟器；请先停止实例再替换或卸载共享组件",
        ));
    }
    let _ = settings;
    Ok(())
}
fn image_references_package(image: &str, id: &str, settings: &Settings) -> bool {
    if image.is_empty() || !id.starts_with("system-images;") {
        return false;
    }
    let image_path = if Path::new(image).is_absolute() {
        PathBuf::from(image)
    } else {
        Path::new(&settings.sdk_root).join(image)
    };
    let package_path = id
        .split(';')
        .fold(PathBuf::from(&settings.sdk_root), |p, component| {
            p.join(component)
        });
    if let (Ok(image), Ok(package)) = (
        std::fs::canonicalize(image_path),
        std::fs::canonicalize(package_path),
    ) {
        #[cfg(windows)]
        return image
            .to_string_lossy()
            .eq_ignore_ascii_case(&package.to_string_lossy());
        #[cfg(not(windows))]
        return image == package;
    }
    image.replace(['\\', '/'], ";").trim_matches(';') == id
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracking_frames_survive_idle_timeouts_and_partial_reads() {
        use std::{
            collections::VecDeque,
            io::{self, Read},
        };
        struct Tracking(VecDeque<io::Result<Vec<u8>>>);
        impl Read for Tracking {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                let chunk = self.0.pop_front().unwrap_or(Ok(vec![]))?;
                assert!(chunk.len() <= bytes.len());
                bytes[..chunk.len()].copy_from_slice(&chunk);
                Ok(chunk.len())
            }
        }
        let mut stream = Tracking(VecDeque::from([
            Err(io::ErrorKind::TimedOut.into()),
            Ok(b"00".to_vec()),
            Err(io::ErrorKind::WouldBlock.into()),
            Ok(b"05".to_vec()),
            Ok(b"he".to_vec()),
            Err(io::ErrorKind::TimedOut.into()),
            Err(io::ErrorKind::Interrupted.into()),
            Ok(b"llo".to_vec()),
        ]));
        let mut length = [0; 4];
        read_tracking_frame(&mut stream, &mut length).unwrap();
        assert_eq!(&length, b"0005");
        let mut body = [0; 5];
        read_tracking_frame(&mut stream, &mut body).unwrap();
        assert_eq!(&body, b"hello");
        assert_eq!(
            read_tracking_frame(&mut stream, &mut length)
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
    #[test]
    fn absolute_and_relative_image_paths_block_uninstall() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            sdk_root: dir.path().join("sdk").to_string_lossy().into(),
            ..Default::default()
        };
        let image =
            Path::new(&settings.sdk_root).join("system-images/android-35/google_apis/x86_64");
        std::fs::create_dir_all(&image).unwrap();
        let id = "system-images;android-35;google_apis;x86_64";
        assert!(image_references_package(
            &image.to_string_lossy(),
            id,
            &settings
        ));
        assert!(image_references_package(
            "system-images/android-35/google_apis/x86_64/.",
            id,
            &settings
        ));
        let other = dir
            .path()
            .join("other-sdk/system-images/android-35/google_apis/x86_64");
        std::fs::create_dir_all(&other).unwrap();
        assert!(!image_references_package(
            &other.to_string_lossy(),
            id,
            &settings
        ));
        let avd: Avd = serde_json::from_value(serde_json::json!({"id":"fixture", "name":"fixture", "displayName":"fixture", "iniPath":"", "dataPath":"", "image":image, "api":"35", "abi":"x86_64", "memory":"", "disk":"", "gpu":"", "fingerprint":"", "status":"stopped", "error":"", "serial":"", "owned":false})).unwrap();
        assert_eq!(
            assert_package_unused(id, &settings, &[avd])
                .unwrap_err()
                .code,
            "SDK_IN_USE"
        );
    }
}
