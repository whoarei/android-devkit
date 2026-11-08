use crate::{android, app::App, domain::*, process};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    future::Future,
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::State;
use zeroize::Zeroizing;

pub(super) const PAIRING: &str = "_adb-tls-pairing._tcp";
pub(super) const CONNECT: &str = "_adb-tls-connect._tcp";
const QR_LIFETIME: u64 = 120;

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WirelessService {
    pub name: String,
    pub service_type: String,
    pub address: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WirelessQr {
    pub task: Task,
    pub image: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub expires_at: u64,
}

struct QrSession {
    name: String,
    secret: Zeroizing<String>,
    deadline: Instant,
}

#[derive(Default)]
pub struct WirelessState {
    pub native: crate::wireless_discovery::NativeDiscovery,
    sessions: Mutex<HashMap<String, QrSession>>,
    start_gate: tokio::sync::Mutex<()>,
    connections: Mutex<HashSet<String>>,
}

impl WirelessState {
    pub fn validate_session(&self, id: &str) -> Result<()> {
        if self
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|s| Instant::now() < s.deadline)
        {
            Ok(())
        } else {
            Err(AppError::new(
                "QR_EXPIRED",
                "二维码已过期，请生成新的二维码",
            ))
        }
    }
    pub fn classify(&self, devices: &mut [Device]) {
        let mut connections = self.connections.lock().unwrap();
        let current: HashSet<_> = devices
            .iter()
            .map(|d| crate::observe::key(&Target::from(d)))
            .collect();
        connections.retain(|key| current.contains(key));
        for device in devices {
            if connections.contains(&crate::observe::key(&Target::from(&*device))) {
                device.connection = "wireless".into();
            }
        }
    }
    fn remember(&self, device: &Device) {
        self.connections
            .lock()
            .unwrap()
            .insert(crate::observe::key(&Target::from(device)));
    }
}

// Also releases unused secrets when a queued task is cancelled or its environment changes.
pub struct SessionCleanup<'a> {
    state: &'a WirelessState,
    id: String,
}
impl Drop for SessionCleanup<'_> {
    fn drop(&mut self) {
        self.state.sessions.lock().unwrap().remove(&self.id);
    }
}
pub fn cleanup<'a>(app: &'a App, op: &Operation) -> Option<SessionCleanup<'a>> {
    (op.kind == "wireless_qr_pair").then(|| SessionCleanup {
        state: &app.wireless,
        id: op.optional("sessionId").into(),
    })
}

fn instance_name(value: &str, service_type: &str) -> String {
    value
        .trim_end_matches('.')
        .trim_end_matches(".local")
        .trim_end_matches(&format!(".{service_type}"))
        .to_string()
}

pub fn parse_services(text: &str) -> Vec<WirelessService> {
    let mut services = Vec::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        let Some(index) = fields.iter().position(|field| {
            matches!(
                field.trim_end_matches('.').trim_end_matches(".local"),
                PAIRING | CONNECT
            )
        }) else {
            continue;
        };
        if index == 0 || index + 1 >= fields.len() {
            continue;
        }
        let service_type = fields[index]
            .trim_end_matches('.')
            .trim_end_matches(".local");
        let Ok(address) = android::normalize_endpoint(fields[index + 1]) else {
            continue;
        };
        let name = instance_name(&fields[..index].join(" "), service_type);
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            continue;
        }
        let service = WirelessService {
            name,
            service_type: service_type.into(),
            address,
        };
        if !services.contains(&service) {
            services.push(service);
        }
        if services.len() >= 256 {
            break;
        }
    }
    services.sort_by(|a, b| (&a.name, &a.address).cmp(&(&b.name, &b.address)));
    services
}

pub fn validate_code_service(name: &str) -> Result<()> {
    let base = canonical_service_name(name);
    if !base.starts_with("adb-")
        || name.len() > 255
        || !base
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return Err(AppError::new(
            "PAIR_SERVICE_INVALID",
            "配对服务无效，请重新选择发现的设备",
        ));
    }
    Ok(())
}

fn canonical_service_name(name: &str) -> &str {
    // DNS-SD responders rename colliding instances, e.g. "adb-guid (2)".
    if let Some((base, number)) = name.rsplit_once(" (") {
        if number
            .strip_suffix(')')
            .and_then(|s| s.parse::<u32>().ok())
            .is_some_and(|n| n >= 2)
        {
            return base;
        }
    }
    name
}

pub fn normalize_disconnect(value: &str) -> Result<String> {
    for suffix in [
        "._adb-tls-connect._tcp",
        "._adb-tls-connect._tcp.",
        "._adb-tls-connect._tcp.local",
        "._adb-tls-connect._tcp.local.",
    ] {
        if let Some(name) = value.strip_suffix(suffix) {
            validate_code_service(name)?;
            return Ok(value.into());
        }
    }
    android::normalize_endpoint(value)
}

async fn discover(app: &App, settings: &Settings) -> Result<Vec<WirelessService>> {
    if let Ok(services) = app.wireless.native.services() {
        return Ok(services);
    }
    let out = android::adb(settings, None, vec!["mdns".into(), "services".into()], 8).await?;
    if out.code != 0 || !out.text().contains("List of discovered mdns services") {
        return Err(AppError::new(
            "MDNS_UNAVAILABLE",
            "无法自动发现设备，请检查 ADB 的 mDNS 支持或使用手动输入",
        ));
    }
    let services = parse_services(&out.text());
    if services.is_empty() {
        // Old shared servers can return an empty list when their Bonjour daemon
        // is unavailable. Report that instead of letting a QR session time out.
        let health = android::adb(settings, None, vec!["mdns".into(), "check".into()], 3).await;
        if !health.is_ok_and(|out| {
            out.code == 0 && out.text().to_lowercase().contains("mdns daemon version")
        }) {
            return Err(AppError::new(
                "MDNS_UNAVAILABLE",
                "无法自动发现设备，请检查 ADB 的 mDNS 支持或使用手动输入",
            ));
        }
    }
    Ok(services)
}

#[tauri::command]
pub async fn wireless_discover(app: State<'_, Arc<App>>) -> Result<Vec<WirelessService>> {
    discover(&app, &app.store.settings()).await.map(|services| {
        services
            .into_iter()
            .filter(|s| s.service_type == PAIRING && s.name.starts_with("adb-"))
            .collect()
    })
}

fn submit_pair(app: &Arc<App>, operation: Operation, request: &str) -> Result<Task> {
    let plan = app.prepare_plan(operation)?;
    let result = app.submit(&plan.id, &plan.hash, request, &plan.confirmation_token, &[]);
    // A failed submission must not keep a six-digit code alive in an abandoned plan.
    app.plans.lock().unwrap().remove(&plan.id);
    result
}

#[tauri::command]
pub async fn wireless_pair_start(
    app: State<'_, Arc<App>>,
    service: WirelessService,
    code: String,
    request_id: String,
) -> Result<Task> {
    start_code(app.inner(), service, code, request_id).await
}

async fn start_code(
    app: &Arc<App>,
    service: WirelessService,
    code: String,
    request_id: String,
) -> Result<Task> {
    let code = Zeroizing::new(code);
    let _gate = app.wireless.start_gate.lock().await;
    if let Some(task) = app.store.request(&request_id) {
        return Ok(task);
    }
    validate_code_service(&service.name)?;
    if service.service_type != PAIRING
        || !discover(app, &app.store.settings())
            .await?
            .contains(&service)
    {
        return Err(AppError::new(
            "PAIR_SERVICE_GONE",
            "设备配对服务已变化，请重新打开手机配对码页面并选择设备",
        ));
    }
    submit_pair(
        app,
        Operation {
            kind: "wireless_pair".into(),
            target: None,
            args: BTreeMap::from([
                ("address".into(), service.address),
                ("serviceName".into(), service.name),
                ("code".into(), code.to_string()),
            ]),
        },
        &request_id,
    )
}

#[tauri::command]
pub async fn wireless_qr_start(app: State<'_, Arc<App>>) -> Result<WirelessQr> {
    start_qr(app.inner()).await
}

async fn start_qr(app: &Arc<App>) -> Result<WirelessQr> {
    let _gate = app.wireless.start_gate.lock().await;
    if app
        .store
        .active_tasks()?
        .iter()
        .any(|task| task.operation.kind == "wireless_qr_pair")
    {
        return Err(AppError::new(
            "PAIR_SESSION_ACTIVE",
            "已有扫码配对流程，请先取消或等待完成",
        ));
    }
    android::compatible_adb(&app.store.settings()).await?;
    discover(app, &app.store.settings()).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let name = format!(
        "studio-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10]
    );
    let secret = Zeroizing::new(uuid::Uuid::new_v4().simple().to_string());
    let payload = Zeroizing::new(format!("WIFI:T:ADB;S:{name};P:{};;", &*secret));
    let code = qrcode::QrCode::new(payload.as_bytes())
        .map_err(|_| AppError::new("QR_FAILED", "无法生成配对二维码，请重试"))?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .build();
    let image = format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg)
    );
    app.wireless.sessions.lock().unwrap().insert(
        id.clone(),
        QrSession {
            name: name.clone(),
            secret,
            deadline: Instant::now() + Duration::from_secs(QR_LIFETIME),
        },
    );
    let result = submit_pair(
        app,
        Operation {
            kind: "wireless_qr_pair".into(),
            target: None,
            args: BTreeMap::from([
                ("sessionId".into(), id.clone()),
                ("serviceName".into(), name),
            ]),
        },
        &id,
    );
    match result {
        Ok(task) => Ok(WirelessQr {
            task,
            image,
            expires_at: now() + QR_LIFETIME,
        }),
        Err(error) => {
            app.wireless.sessions.lock().unwrap().remove(&id);
            Err(error)
        }
    }
}

#[cfg(test)]
#[path = "wireless_tests.rs"]
mod tests;

async fn cancellable<T>(app: &App, id: &str, future: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::pin!(future);
    loop {
        if app.cancelled(id) {
            return Err(AppError::new(
                "CANCELLED",
                "已停止连接流程；已建立的配对授权请在手机中检查",
            ));
        }
        tokio::select! {
            result = &mut future => return result,
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
}

fn paired_guid(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|line| line.contains("Successfully paired to "))?;
    let guid = line.split_once("[guid=")?.1.split_once(']')?.0;
    validate_code_service(guid).ok()?;
    Some(guid.into())
}

fn matching_device<'a>(
    devices: &'a [Device],
    guid: &str,
    endpoints: &HashSet<String>,
) -> Option<&'a Device> {
    devices.iter().find(|device| {
        device.state == "ready"
            && ((!guid.is_empty()
                && canonical_service_name(&instance_name(&device.serial, CONNECT)) == guid)
                || endpoints.contains(&device.serial))
    })
}

async fn endpoint_reachable(address: &str) -> bool {
    tokio::time::timeout(
        Duration::from_millis(600),
        tokio::net::TcpStream::connect(address),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

impl App {
    pub async fn pair_wireless_qr(
        &self,
        id: &str,
        op: &Operation,
        settings: &Settings,
    ) -> Result<String> {
        let session = self
            .wireless
            .sessions
            .lock()
            .unwrap()
            .remove(op.arg("sessionId")?)
            .ok_or_else(|| AppError::new("QR_EXPIRED", "二维码已过期，请生成新的二维码"))?;
        self.update(id, |task| {
            task.phase = "waiting_scan".into();
            task.message = "请使用手机无线调试中的扫码入口扫描二维码".into();
            task.cancellable = true;
        })?;
        let service = cancellable(self, id, async {
            loop {
                if Instant::now() >= session.deadline {
                    return Err(AppError::new(
                        "QR_EXPIRED",
                        "二维码已过期，请生成新的二维码",
                    ));
                }
                let services = discover(self, settings).await?;
                let matches: Vec<_> = services
                    .into_iter()
                    .filter(|s| s.name == session.name && s.service_type == PAIRING)
                    .collect();
                if matches.len() > 1 {
                    return Err(AppError::new(
                        "PAIR_AMBIGUOUS",
                        "多个地址响应同一二维码，请刷新二维码并只扫描一台手机",
                    ));
                }
                if let Some(service) = matches.into_iter().next() {
                    if Instant::now() >= session.deadline {
                        continue;
                    }
                    break Ok(service);
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        })
        .await?;
        self.update(id, |task| {
            task.operation
                .args
                .insert("address".into(), service.address.clone());
        })?;
        self.pair_and_connect(id, settings, &service.address, &session.secret, "")
            .await
    }

    pub async fn pair_and_connect(
        &self,
        id: &str,
        settings: &Settings,
        address: &str,
        secret: &str,
        service_name: &str,
    ) -> Result<String> {
        self.update(id, |task| {
            task.phase = "pairing".into();
            task.message = "正在配对设备".into();
            task.cancellable = true;
        })?;
        if !service_name.is_empty() {
            let services = cancellable(self, id, discover(self, settings)).await?;
            if !services.iter().any(|s| {
                s.name == service_name && s.address == address && s.service_type == PAIRING
            }) {
                return Err(AppError::new(
                    "PAIR_SERVICE_GONE",
                    "设备配对服务已变化，请重新打开手机配对码页面并选择设备",
                ));
            }
        }
        let mut spec =
            android::adb_spec(settings, None, vec!["pair".into(), address.into()], 60).await?;
        spec.stdin = None;
        let input = Zeroizing::new(format!("{secret}\n").into_bytes());
        let result = cancellable(self, id, process::run_sensitive(spec, input)).await;
        // Pairing output is deliberately not attached to errors: tools may echo secrets.
        let out = result.map_err(|error| {
            if error.code == "CANCELLED" {
                error
            } else {
                AppError::new(
                    "PAIR_FAILED",
                    "配对未完成，请检查手机配对记录，并重新获取配对码或二维码",
                )
            }
        })?;
        if out.code != 0 || !out.text().contains("Successfully paired to ") {
            return Err(AppError::new(
                "PAIR_FAILED",
                "配对失败，请重新获取配对码，并核对配对地址、端口和网络",
            ));
        }
        let guid = paired_guid(&out.text()).ok_or_else(|| {
            AppError::new(
                "PAIR_IDENTITY_UNKNOWN",
                "配对成功，但未能确认设备身份；请使用手机无线调试页的连接端口手动连接",
            )
        })?;
        self.phase(id, "connecting", "配对成功，正在发现并验证无线连接")?;
        cancellable(
            self,
            id,
            async {
                tokio::time::timeout(Duration::from_secs(25), self.finish_wireless_connection(id, settings, &guid))
                    .await
                    .unwrap_or_else(|_| Err(AppError::new("PAIRED_NOT_CONNECTED", "配对成功，但无线连接未就绪；请检查网络，或使用手机无线调试页的连接端口手动连接")))
                    .map_err(|_| AppError::new("PAIRED_NOT_CONNECTED", "配对成功，但无线连接未就绪；请检查网络，或使用手机无线调试页的连接端口手动连接"))
            },
        )
        .await
    }

    async fn finish_wireless_connection(
        &self,
        id: &str,
        settings: &Settings,
        guid: &str,
    ) -> Result<String> {
        let mut attempts: HashMap<String, u8> = HashMap::new();
        loop {
            let services: Vec<_> = discover(self, settings)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|s| canonical_service_name(&s.name) == guid && s.service_type == CONNECT)
                .collect();
            let endpoints: HashSet<_> = services.iter().map(|s| s.address.clone()).collect();
            let devices = android::adb(settings, None, vec!["devices".into(), "-l".into()], 5)
                .await?
                .success()?;
            let devices =
                android::parse_devices(&devices.text(), self.epoch.load(Ordering::SeqCst));
            if let Some(device) = matching_device(&devices, guid, &endpoints) {
                self.wireless.remember(device);
                self.update(id, |task| {
                    task.operation.target = Some(Target::from(device));
                })?;
                return Ok("配对成功，无线连接已验证，设备可用".into());
            }
            // Only the GUID returned by authenticated pairing can select a connect service.
            // An IP match alone is insufficient on networks with multiple devices.
            let multiple_endpoints = services.len() > 1;
            for service in services {
                // A phone can advertise an old port alongside a renamed instance.
                // Avoid submitting dead endpoints to the shared server, where a
                // connect can keep running even after our client times out.
                if multiple_endpoints && !endpoint_reachable(&service.address).await {
                    continue;
                }
                let count = attempts.entry(service.address.clone()).or_default();
                if *count < 3 {
                    *count += 1;
                    let _ =
                        android::adb(settings, None, vec!["connect".into(), service.address], 5)
                            .await;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    pub async fn connect_wireless_endpoint(
        &self,
        id: &str,
        settings: &Settings,
        address: &str,
    ) -> Result<String> {
        self.update(id, |task| {
            task.cancellable = true;
        })?;
        cancellable(self, id, async {
            android::adb(settings, None, vec!["connect".into(), address.into()], 15)
                .await?
                .success()?;
            for _ in 0..8 {
                let services = discover(self, settings).await.unwrap_or_default();
                let names: HashSet<_> = services
                    .iter()
                    .filter(|s| s.address == address && s.service_type == CONNECT)
                    .map(|s| s.name.as_str())
                    .collect();
                let out = android::adb(settings, None, vec!["devices".into(), "-l".into()], 5)
                    .await?
                    .success()?;
                let devices =
                    android::parse_devices(&out.text(), self.epoch.load(Ordering::SeqCst));
                if let Some(device) = devices.iter().find(|d| {
                    d.state == "ready"
                        && (d.serial == address
                            || names.contains(instance_name(&d.serial, CONNECT).as_str()))
                }) {
                    self.wireless.remember(device);
                    self.update(id, |task| {
                        task.operation.target = Some(Target::from(device));
                    })?;
                    return Ok("无线连接已验证，设备可用".into());
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(AppError::new(
                "CONNECT_NOT_READY",
                "未验证可用连接；检查连接端口、网络隔离或重新配对",
            ))
        })
        .await
    }
}
