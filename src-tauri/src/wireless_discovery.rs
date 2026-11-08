use crate::{
    domain::*,
    wireless::{WirelessService, CONNECT, PAIRING},
};
use mdns_sd::{DaemonEvent, ServiceDaemon, ServiceEvent};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct NativeDiscovery {
    browser: Mutex<Weak<Browser>>,
    #[cfg(test)]
    disabled: std::sync::atomic::AtomicBool,
}
impl Drop for NativeDiscovery {
    fn drop(&mut self) {
        if let Some(browser) = self.browser.lock().unwrap().upgrade() {
            *browser.last_used.lock().unwrap() = None;
            let _ = browser.daemon.shutdown();
        }
    }
}

#[derive(Default)]
struct ServiceCache {
    services: HashMap<String, Vec<WirelessService>>,
    error: Option<String>,
}

impl ServiceCache {
    fn resolved(&mut self, fullname: &str, ty_domain: &str, addresses: Vec<String>, port: u16) {
        let service_type = ty_domain.trim_end_matches('.').trim_end_matches(".local");
        if !matches!(service_type, PAIRING | CONNECT) {
            return;
        }
        let Some(name) = fullname.strip_suffix(&format!(".{service_type}.local.")) else {
            return;
        };
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) || port == 0 {
            return;
        }
        let mut services: Vec<_> = addresses
            .into_iter()
            .filter_map(|ip| {
                let endpoint = if ip.contains(':') {
                    format!("[{ip}]:{port}")
                } else {
                    format!("{ip}:{port}")
                };
                crate::android::normalize_endpoint(&endpoint)
                    .ok()
                    .map(|address| WirelessService {
                        name: name.into(),
                        service_type: service_type.into(),
                        address,
                    })
            })
            .collect();
        services.sort_by(|a, b| a.address.cmp(&b.address));
        services.dedup();
        // Prefer IPv4 when available; one dual-stack phone should be one pairing choice.
        if services.iter().any(|s| !s.address.starts_with('[')) {
            services.retain(|s| !s.address.starts_with('['));
        }
        if self.services.len() < 256 || self.services.contains_key(fullname) {
            self.services.insert(fullname.into(), services);
            self.error = None;
        }
    }
    fn event(&mut self, event: ServiceEvent) {
        match event {
            ServiceEvent::ServiceResolved(service) => self.resolved(
                &service.fullname,
                &service.ty_domain,
                service.addresses.iter().map(ToString::to_string).collect(),
                service.port,
            ),
            ServiceEvent::ServiceRemoved(_, fullname) => {
                self.services.remove(&fullname);
            }
            _ => {}
        }
    }
    fn snapshot(&self) -> Vec<WirelessService> {
        let mut result: Vec<_> = self.services.values().flatten().cloned().collect();
        result.sort_by(|a, b| (&a.name, &a.address).cmp(&(&b.name, &b.address)));
        result
    }
}

struct Browser {
    daemon: ServiceDaemon,
    cache: Mutex<ServiceCache>,
    // None means the receiver loop is stopping and must not be reused.
    last_used: Mutex<Option<Instant>>,
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

impl NativeDiscovery {
    #[cfg(test)]
    pub fn disable_for_fixture(&self) {
        self.disabled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn services(&self) -> Result<Vec<WirelessService>> {
        #[cfg(test)]
        if self.disabled.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(AppError::new(
                "NATIVE_MDNS_UNAVAILABLE",
                "隔离测试使用模拟 ADB 发现结果",
            ));
        }
        let mut handle = self.browser.lock().unwrap();
        if let Some(browser) = handle.upgrade() {
            let mut used = browser.last_used.lock().unwrap();
            if used.is_some() {
                *used = Some(Instant::now());
                let cache = browser.cache.lock().unwrap();
                if let Some(error) = cache.error.as_ref().filter(|_| cache.services.is_empty()) {
                    return Err(AppError::new(
                        "NATIVE_MDNS_UNAVAILABLE",
                        "本机局域网设备发现暂不可用",
                    )
                    .detail(error));
                }
                return Ok(cache.snapshot());
            }
        }
        let daemon = ServiceDaemon::new().map_err(|e| {
            AppError::new("NATIVE_MDNS_UNAVAILABLE", "无法启动本机局域网设备发现")
                .detail(e.to_string())
        })?;
        let browser = Arc::new(Browser {
            daemon,
            cache: Mutex::new(ServiceCache::default()),
            last_used: Mutex::new(Some(Instant::now())),
        });
        let pair = browser
            .daemon
            .browse(&format!("{PAIRING}.local."))
            .map_err(|e| {
                AppError::new("NATIVE_MDNS_UNAVAILABLE", "无法监听手机配对服务")
                    .detail(e.to_string())
            })?;
        let connect = browser
            .daemon
            .browse(&format!("{CONNECT}.local."))
            .map_err(|e| {
                AppError::new("NATIVE_MDNS_UNAVAILABLE", "无法监听手机连接服务")
                    .detail(e.to_string())
            })?;
        let errors = browser.daemon.monitor().map_err(|e| {
            AppError::new("NATIVE_MDNS_UNAVAILABLE", "无法检查局域网设备发现状态")
                .detail(e.to_string())
        })?;
        *handle = Arc::downgrade(&browser);
        // The worker owns the browser only while a pairing UI/task keeps polling it.
        // No Bonjour installation or restart of the user's shared ADB server is needed.
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    event = pair.recv_async() => match event { Ok(event) => browser.cache.lock().unwrap().event(event), Err(_) => break },
                    event = connect.recv_async() => match event { Ok(event) => browser.cache.lock().unwrap().event(event), Err(_) => break },
                    event = errors.recv_async() => match event {
                        Ok(DaemonEvent::Error(error)) => browser.cache.lock().unwrap().error = Some(error.to_string()),
                        Err(_) => break,
                        _ => {}
                    },
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
                let mut used = browser.last_used.lock().unwrap();
                if used.is_some_and(|last| last.elapsed() >= Duration::from_secs(15)) {
                    *used = None;
                    break;
                }
            }
        });
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_events_replace_changed_ports_and_remove_stopped_services() {
        let mut cache = ServiceCache::default();
        let fullname = "adb-phone._adb-tls-pairing._tcp.local.";
        cache.resolved(
            fullname,
            "_adb-tls-pairing._tcp.local.",
            vec!["192.0.2.1".into(), "2001:db8::1".into()],
            12345,
        );
        assert_eq!(cache.snapshot().len(), 1);
        assert_eq!(cache.snapshot()[0].address, "192.0.2.1:12345");
        cache.resolved(
            fullname,
            "_adb-tls-pairing._tcp.local.",
            vec!["192.0.2.1".into()],
            23456,
        );
        assert_eq!(cache.snapshot()[0].address, "192.0.2.1:23456");
        cache.event(ServiceEvent::ServiceRemoved(
            "_adb-tls-pairing._tcp.local.".into(),
            fullname.into(),
        ));
        assert!(cache.snapshot().is_empty());
    }
    #[tokio::test]
    #[ignore = "read-only LAN discovery of physical phones; never pairs or restarts ADB"]
    async fn native_lan_discovery() {
        let discovery = NativeDiscovery::default();
        let mut found = Vec::new();
        for _ in 0..12 {
            let services = discovery.services().unwrap();
            if !services.is_empty() {
                found = services;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        println!("{}", serde_json::to_string(&found).unwrap());
        assert!(
            !found.is_empty(),
            "No phone wireless debugging services found on the LAN"
        );
    }
}
