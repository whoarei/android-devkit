use crate::{
    domain::*,
    process::{self, Spec},
};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

pub fn properties(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with(['#', ';']))
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().into(), v.trim().into()))
        .collect()
}
pub fn read_properties(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .map(|v| properties(&v))
        .unwrap_or_default()
}
pub fn write_properties_preserving(
    path: &Path,
    changes: &BTreeMap<String, String>,
    expected: &str,
) -> Result<()> {
    let original = std::fs::read(path)?;
    if digest(&original) != expected {
        return Err(AppError::new(
            "EXTERNAL_CHANGE",
            "配置已被外部修改，保留输入并重新载入",
        ));
    }
    let text = String::from_utf8_lossy(&original);
    let mut pending = changes.clone();
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some((key, _)) = line.split_once('=') {
            if let Some(value) = pending.remove(key.trim()) {
                lines.push(format!("{}={value}", key.trim()));
                continue;
            }
        }
        lines.push(line.into());
    }
    for (k, v) in pending {
        lines.push(format!("{k}={v}"));
    }
    let temp = path.with_extension("ini.devkit-tmp");
    std::fs::write(&temp, lines.join("\r\n") + "\r\n")?;
    if digest(&std::fs::read(path)?) != expected {
        let _ = std::fs::remove_file(temp);
        return Err(AppError::new("EXTERNAL_CHANGE", "保存时发现外部更改"));
    }
    std::fs::rename(&temp, path)?;
    Ok(())
}
pub fn environment(settings: &Settings) -> Environment {
    let root = Path::new(&settings.sdk_root);
    let mut warnings = Vec::new();
    if let (Ok(a), Ok(b)) = (
        std::env::var("ANDROID_HOME"),
        std::env::var("ANDROID_SDK_ROOT"),
    ) {
        if a != b {
            warnings
                .push("系统 ANDROID_HOME 与 ANDROID_SDK_ROOT 不一致；本应用使用显式路径".into());
        }
    }
    let tool = |relative: &str| {
        let p = root.join(relative);
        if p.is_file() {
            p.to_string_lossy().into()
        } else {
            String::new()
        }
    };
    Environment {
        path: settings.sdk_root.clone(),
        source: "用户指定".into(),
        adb: tool("platform-tools/adb.exe"),
        emulator: tool("emulator/emulator.exe"),
        command_tools: tool("cmdline-tools/latest/lib/sdkmanager-classpath.jar"),
        java: settings.java_path.clone(),
        writable: root.is_dir()
            && !root
                .metadata()
                .map(|m| m.permissions().readonly())
                .unwrap_or(true),
        warnings,
    }
}
pub fn scan(settings: &Settings) -> Vec<Environment> {
    let mut paths = vec![settings.sdk_root.clone(), "E:\\Android\\sdk".into()];
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(v) = std::env::var(key) {
            paths.push(v);
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        paths.push(format!("{local}\\Android\\Sdk"));
    }
    let mut result = Vec::new();
    for p in paths {
        if result
            .iter()
            .any(|e: &Environment| e.path.eq_ignore_ascii_case(&p))
        {
            continue;
        }
        if Path::new(&p).is_dir() {
            let mut s = settings.clone();
            s.sdk_root = p;
            result.push(environment(&s));
        }
    }
    result
}
fn child_text<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> String {
    node.children()
        .find(|n| n.has_tag_name(name))
        .and_then(|n| n.text())
        .unwrap_or("")
        .into()
}
pub fn parse_catalog(xml: &str, base: &str, installed: bool) -> Result<Vec<SdkPackage>> {
    if xml.len() > 24 * 1024 * 1024 {
        return Err(AppError::new("CATALOG_TOO_LARGE", "仓库目录超出大小上限"));
    }
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| AppError::new("CATALOG_FORMAT", e.to_string()))?;
    let licenses: BTreeMap<String, String> = doc
        .descendants()
        .filter(|n| n.has_tag_name("license"))
        .filter_map(|n| Some((n.attribute("id")?.into(), n.text().unwrap_or("").into())))
        .collect();
    let mut packages = Vec::new();
    for node in doc.descendants().filter(|n| {
        n.has_tag_name(if installed {
            "localPackage"
        } else {
            "remotePackage"
        })
    }) {
        let id = node.attribute("path").unwrap_or("").to_string();
        if !valid_package(&id) {
            continue;
        }
        let revision = node.children().find(|n| n.has_tag_name("revision"));
        let version = revision
            .map(|r| {
                ["major", "minor", "micro"]
                    .into_iter()
                    .map(|k| {
                        let v = child_text(r, k);
                        if v.is_empty() {
                            "0".into()
                        } else {
                            v
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_default();
        let archive = node.descendants().find(|n| {
            n.has_tag_name("archive") && {
                let host = child_text(*n, "host-os");
                host == "windows" || host.is_empty()
            }
        });
        if !installed && archive.is_none() {
            continue;
        }
        let complete = archive.and_then(|a| a.children().find(|n| n.has_tag_name("complete")));
        let url = complete.map(|n| child_text(n, "url")).unwrap_or_default();
        let checksum_node =
            complete.and_then(|n| n.children().find(|c| c.has_tag_name("checksum")));
        let license_id = node
            .children()
            .find(|n| n.has_tag_name("uses-license"))
            .and_then(|n| n.attribute("ref"))
            .unwrap_or("")
            .to_string();
        let tag = node
            .descendants()
            .find(|n| n.has_tag_name("tag"))
            .map(|n| child_text(n, "id"))
            .unwrap_or_default();
        let api = node
            .descendants()
            .find(|n| n.has_tag_name("api-level"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .into();
        let abi = node
            .descendants()
            .find(|n| n.has_tag_name("abi"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .into();
        let category = if id.starts_with("system-images;") {
            "image"
        } else if id.starts_with("platforms;") {
            "platform"
        } else {
            "tool"
        };
        let channel = node
            .children()
            .find(|n| n.has_tag_name("channelRef"))
            .and_then(|n| n.attribute("ref"))
            .unwrap_or("channel-0");
        if !installed && channel != "channel-0" {
            continue;
        }
        packages.push(SdkPackage {
            id: id.clone(),
            name: child_text(node, "display-name"),
            version,
            category: category.into(),
            installed,
            installed_version: String::new(),
            api,
            abi,
            tag,
            size: complete
                .map(|n| child_text(n, "size").parse().unwrap_or(0))
                .unwrap_or(0),
            url: if url.starts_with("https://") || url.is_empty() {
                url
            } else {
                format!("{base}{url}")
            },
            checksum: checksum_node.and_then(|n| n.text()).unwrap_or("").into(),
            checksum_type: checksum_node
                .and_then(|n| n.attribute("type"))
                .unwrap_or("sha1")
                .into(),
            license: licenses.get(&license_id).cloned().unwrap_or_default(),
            license_id,
            dependencies: node
                .descendants()
                .filter(|n| n.has_tag_name("dependency"))
                .filter_map(|n| n.attribute("path").map(String::from))
                .collect(),
            dependency_min_versions: node
                .descendants()
                .filter(|n| n.has_tag_name("dependency"))
                .filter_map(|n| {
                    let path = n.attribute("path")?;
                    let revision = n.children().find(|c| c.has_tag_name("min-revision"))?;
                    Some((
                        path.to_owned(),
                        ["major", "minor", "micro"]
                            .iter()
                            .map(|key| {
                                let value = child_text(revision, key);
                                if value.is_empty() {
                                    "0".into()
                                } else {
                                    value
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("."),
                    ))
                })
                .collect(),
            obsolete: node.attribute("obsolete") == Some("true"),
        });
    }
    Ok(packages)
}
pub fn installed(settings: &Settings) -> Vec<SdkPackage> {
    fn walk(dir: &Path, depth: usize, result: &mut Vec<SdkPackage>) {
        if depth > 5 {
            return;
        }
        if dir.join("package.xml").is_file() {
            if let Ok(xml) = std::fs::read_to_string(dir.join("package.xml")) {
                if let Ok(mut p) = parse_catalog(&xml, "", true) {
                    result.append(&mut p);
                    return;
                }
            }
        }
        if dir.join("source.properties").is_file() {
            let p = read_properties(&dir.join("source.properties"));
            if let Some(id) = p.get("Pkg.Path") {
                result.push(SdkPackage {
                    id: id.into(),
                    name: p.get("Pkg.Desc").cloned().unwrap_or_else(|| id.into()),
                    version: p.get("Pkg.Revision").cloned().unwrap_or_default(),
                    category: if id.starts_with("system-images;") {
                        "image"
                    } else if id.starts_with("platforms;") {
                        "platform"
                    } else {
                        "tool"
                    }
                    .into(),
                    installed: true,
                    ..Default::default()
                });
                return;
            }
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && !entry.file_name().to_string_lossy().starts_with('.')
                {
                    walk(&entry.path(), depth + 1, result);
                }
            }
        }
    }
    let mut result = Vec::new();
    walk(Path::new(&settings.sdk_root), 0, &mut result);
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}
pub fn avds(settings: &Settings) -> Vec<Avd> {
    let mut result = Vec::new();
    for root in &settings.avd_roots {
        if let Ok(entries) = std::fs::read_dir(root) {
            for e in entries.flatten() {
                let ini = e.path();
                if ini.extension().and_then(|e| e.to_str()) != Some("ini") {
                    continue;
                }
                let index = read_properties(&ini);
                let data = index.get("path").map(PathBuf::from).unwrap_or_else(|| {
                    Path::new(root).join(format!(
                        "{}.avd",
                        ini.file_stem().unwrap().to_string_lossy()
                    ))
                });
                let config_path = data.join("config.ini");
                let content = std::fs::read(&config_path).unwrap_or_default();
                let p = properties(&String::from_utf8_lossy(&content));
                let image = p.get("image.sysdir.1").cloned().unwrap_or_default();
                let image_path = if Path::new(&image).is_absolute() {
                    PathBuf::from(&image)
                } else {
                    Path::new(&settings.sdk_root).join(&image)
                };
                let error = if content.is_empty() {
                    "配置缺失"
                } else if !image_path.join("system.img").is_file() {
                    "缺少系统镜像"
                } else {
                    ""
                };
                let name = ini.file_stem().unwrap().to_string_lossy().to_string();
                let get = |key: &str| p.get(key).cloned().unwrap_or_default();
                let canonical = std::fs::canonicalize(&ini).unwrap_or_else(|_| ini.clone());
                if result
                    .iter()
                    .any(|a: &Avd| a.id == canonical.to_string_lossy())
                {
                    continue;
                }
                result.push(Avd {
                    id: canonical.to_string_lossy().into(),
                    name: name.clone(),
                    display_name: p
                        .get("avd.ini.displayname")
                        .cloned()
                        .unwrap_or_else(|| name.clone()),
                    ini_path: ini.to_string_lossy().into(),
                    data_path: data.to_string_lossy().into(),
                    image,
                    api: get("target").replace("android-", ""),
                    abi: get("abi.type"),
                    memory: get("hw.ramSize"),
                    disk: get("disk.dataPartition.size"),
                    gpu: get("hw.gpu.mode"),
                    fingerprint: digest(&content),
                    status: if error.is_empty() {
                        "stopped"
                    } else {
                        "missing_dependency"
                    }
                    .into(),
                    error: error.into(),
                    serial: String::new(),
                    owned: data.join(".devkit-owned").is_file(),
                });
            }
        }
    }
    result
}

pub fn host_request(request: &str) -> Result<Vec<u8>> {
    let mut socket = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], 5037)),
        Duration::from_millis(600),
    )
    .map_err(|_| AppError::new("ADB_NOT_RUNNING", "ADB 服务尚未启动"))?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.write_all(format!("{:04x}{request}", request.len()).as_bytes())?;
    let mut status = [0; 4];
    socket.read_exact(&mut status)?;
    let mut size = [0; 4];
    socket.read_exact(&mut size)?;
    let len = usize::from_str_radix(std::str::from_utf8(&size).unwrap_or(""), 16)
        .map_err(|_| AppError::new("ADB_PROTOCOL", "无效的 ADB 帧长度"))?;
    if len > 1024 * 1024 {
        return Err(AppError::new("ADB_PROTOCOL", "ADB 响应过大"));
    }
    let mut bytes = vec![0; len];
    socket.read_exact(&mut bytes)?;
    if &status != b"OKAY" {
        return Err(AppError::new(
            "ADB_REQUEST_FAILED",
            String::from_utf8_lossy(&bytes),
        ));
    }
    Ok(bytes)
}
pub fn parse_devices(text: &str, epoch: u64) -> Vec<Device> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let raw = parts.next()?;
            let fields: BTreeMap<_, _> = parts.filter_map(|part| part.split_once(':')).collect();
            let transport_id = fields.get("transport_id").unwrap_or(&"").to_string();
            Some(Device {
                connection: if serial.starts_with("emulator-") {
                    "emulator"
                } else if serial.contains("._adb-tls-") {
                    "wireless"
                } else if serial.contains(':') {
                    "network"
                } else {
                    "usb"
                }
                .into(),
                serial,
                transport_id,
                server_epoch: epoch,
                model: fields
                    .get("model")
                    .unwrap_or(&"Android Device")
                    .replace('_', " "),
                state: match raw {
                    "device" => "ready",
                    "unauthorized" => "unauthorized",
                    "offline" => "offline",
                    _ => raw,
                }
                .into(),
                ..Default::default()
            })
        })
        .collect()
}
pub async fn compatible_adb(settings: &Settings) -> Result<PathBuf> {
    let path = Path::new(&settings.sdk_root).join("platform-tools/adb.exe");
    if !path.is_file() {
        return Err(AppError::new("ADB_MISSING", "当前 SDK 缺少 platform-tools"));
    }
    let output = process::run(Spec::new(&path, vec!["version".into()]))
        .await?
        .success()?;
    let client = output
        .text()
        .lines()
        .find_map(|l| l.strip_prefix("Android Debug Bridge version 1.0."))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .ok_or_else(|| AppError::new("ADB_VERSION", "无法验证 ADB 客户端版本"))?;
    match host_request("host:version") {
        Ok(raw) => {
            let server = u32::from_str_radix(String::from_utf8_lossy(&raw).trim(), 16).unwrap_or(0);
            if client != server {
                return Err(AppError::new(
                    "ADB_VERSION_CONFLICT",
                    format!(
                    "ADB 客户端协议 {client} 与共享服务 {server} 不兼容；请在诊断中明确选择重启"
                ),
                ));
            }
        }
        Err(error) if error.code == "ADB_NOT_RUNNING" => {}
        Err(error) => {
            return Err(AppError::new(
                "ADB_SERVER_UNVERIFIED",
                "无法验证共享 ADB 服务，未启动可能替换服务的客户端",
            )
            .detail(error.to_string()))
        }
    }
    Ok(path)
}
pub async fn adb(
    settings: &Settings,
    target: Option<&Target>,
    args: Vec<String>,
    timeout: u64,
) -> Result<process::Output> {
    let path = compatible_adb(settings).await?;
    let mut all = vec!["-H".into(), "127.0.0.1".into(), "-P".into(), "5037".into()];
    if let Some(t) = target {
        if t.transport_id.parse::<u64>().is_err() {
            return Err(AppError::new("INVALID_TARGET", "缺少有效 transportId"));
        }
        all.extend(["-t".into(), t.transport_id.clone()]);
    }
    all.extend(args);
    let mut s = Spec::new(path, all);
    s.timeout = timeout;
    process::run(s).await
}
pub async fn devices(settings: &Settings, epoch: u64) -> Vec<Device> {
    let raw = host_request("host:devices-l").unwrap_or_default();
    let mut devices = parse_devices(&String::from_utf8_lossy(&raw), epoch);
    for d in &mut devices {
        if d.state == "ready" {
            let target = Target::from(&*d);
            if let Ok(out) = adb(
                settings,
                Some(&target),
                vec!["shell".into(), "getprop".into()],
                5,
            )
            .await
            .and_then(process::Output::success)
            {
                let props = out.text();
                for (key, dest) in [
                    ("ro.build.version.sdk", &mut d.api),
                    ("ro.product.cpu.abilist", &mut d.abi),
                ] {
                    *dest = props
                        .lines()
                        .find_map(|l| l.strip_prefix(&format!("[{key}]: [")))
                        .unwrap_or("")
                        .trim_end_matches(']')
                        .into();
                }
                if d.connection == "emulator" {
                    let boot = props.lines().any(|l| l == "[sys.boot_completed]: [1]");
                    if !boot {
                        d.state = "booting".into();
                    }
                    if let Ok(out) = adb(
                        settings,
                        Some(&target),
                        vec!["emu".into(), "avd".into(), "name".into()],
                        5,
                    )
                    .await
                    {
                        d.avd_name = out
                            .text()
                            .lines()
                            .find(|l| *l != "OK")
                            .unwrap_or("")
                            .trim()
                            .into();
                    }
                }
            } else {
                d.state = "offline".into();
            }
        }
    }
    devices
}

pub fn normalize_endpoint(value: &str) -> Result<String> {
    value
        .parse::<SocketAddr>()
        .ok()
        .filter(|address| address.port() > 0)
        .map(|address| address.to_string())
        .ok_or_else(|| {
            AppError::new(
                "INVALID_ADDRESS",
                "请输入有效 IP 和 1～65535 端口；IPv6 地址应使用 [地址]:端口",
            )
        })
}
pub fn safe_avd_path(avd: &Avd, settings: &Settings) -> Result<PathBuf> {
    let path = std::fs::canonicalize(&avd.data_path)?;
    let meta = std::fs::symlink_metadata(&avd.data_path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(AppError::new(
                "UNSAFE_PATH",
                "AVD 目录为重解析点，拒绝删除或清理",
            ));
        }
    }
    let _ = meta;
    if path == std::fs::canonicalize(&settings.sdk_root)?
        || settings
            .avd_roots
            .iter()
            .any(|root| std::fs::canonicalize(root).ok().as_ref() == Some(&path))
    {
        return Err(AppError::new("UNSAFE_PATH", "拒绝清理共享根目录"));
    }
    if path.file_name().and_then(|n| n.to_str()) != Some(&format!("{}.avd", avd.name)) {
        return Err(AppError::new(
            "UNSAFE_PATH",
            "AVD 数据目录名称与注册信息不匹配，需手动处理",
        ));
    }
    for other in avds(settings) {
        if other.id != avd.id && std::fs::canonicalize(other.data_path).ok().as_ref() == Some(&path)
        {
            return Err(AppError::new(
                "SHARED_AVD_DATA",
                "另一个 AVD 引用相同数据目录",
            ));
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_unknown_ini_fields_and_detects_conflict() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("config.ini");
        std::fs::write(&f, "# keep\r\nunknown=x\r\nhw.ramSize=512\r\n").unwrap();
        let hash = digest(&std::fs::read(&f).unwrap());
        write_properties_preserving(
            &f,
            &BTreeMap::from([("hw.ramSize".into(), "2048".into())]),
            &hash,
        )
        .unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("unknown=x"));
        assert!(text.contains("hw.ramSize=2048"));
        assert!(write_properties_preserving(&f, &BTreeMap::new(), &hash).is_err());
    }
    #[test]
    fn unauthorized_is_not_ready() {
        let d = parse_devices(
            "A unauthorized usb:1 transport_id:5\nB device model:Pixel_8 transport_id:6",
            7,
        );
        assert_eq!(d[0].state, "unauthorized");
        assert_eq!(d[1].model, "Pixel 8");
        assert_eq!(d[1].transport_id, "6");
    }
    #[test]
    fn parses_catalog_metadata_without_installing() {
        let x = r#"<repository><license id="l">Terms</license><remotePackage path="platform-tools"><revision><major>36</major></revision><display-name>Platform Tools</display-name><uses-license ref="l"/><archives><archive><host-os>windows</host-os><complete><size>123</size><checksum type="sha256">abc</checksum><url>x.zip</url></complete></archive></archives></remotePackage></repository>"#;
        let p = parse_catalog(x, "https://dl.google.com/android/repository/", false).unwrap();
        assert_eq!(p[0].license, "Terms");
        assert_eq!(p[0].size, 123);
        assert!(p[0].url.ends_with("/x.zip"));
    }
    #[test]
    fn validates_network_addresses() {
        assert_eq!(
            normalize_endpoint("192.168.1.2:5555").unwrap(),
            "192.168.1.2:5555"
        );
        assert_eq!(
            normalize_endpoint("[2001:0db8::1]:5555").unwrap(),
            "[2001:db8::1]:5555"
        );
        for x in [
            "a;calc:4",
            "1.1.1.1:0",
            "1.1.1.1:99999",
            "192.168.1.2",
            "2001:db8::1:5555",
        ] {
            assert!(
                normalize_endpoint(x).is_err(),
                "accepted invalid endpoint {x}"
            );
        }
    }
    #[test]
    fn network_sessions_preserve_endpoint_and_authorization_state() {
        let devices = parse_devices(
            "USB123 device usb:1 transport_id:1\n192.168.1.2:5555 unauthorized transport_id:2\n[2001:db8::1]:5555 offline transport_id:3\nadb-phone._adb-tls-connect._tcp device transport_id:4\nemulator-5554 device transport_id:5",
            9,
        );
        let connections: Vec<_> = devices.iter().map(|d| d.connection.as_str()).collect();
        assert_eq!(
            connections,
            ["usb", "network", "network", "wireless", "emulator"]
        );
        assert_eq!(devices[1].serial, "192.168.1.2:5555");
        assert_eq!(devices[1].state, "unauthorized");
        assert_eq!(devices[2].state, "offline");
        assert_eq!(devices[1].server_epoch, 9);
        assert_eq!(devices[1].transport_id, "2");
    }
}
