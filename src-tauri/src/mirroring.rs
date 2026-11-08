//! A scrcpy session owns its child and an ADB proxy pinned to one transport.
//! scrcpy's public selector accepts a serial only. The private proxy rewrites
//! that selector to transport-id, including ADB's tport response handshake.
use crate::{android, app::App, domain::*, process::Spec};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

#[derive(Debug, Clone, PartialEq)]
pub struct MirrorOptions {
    pub control: bool,
    pub max_size: u32,
    pub max_fps: u32,
}

pub fn validate_options(op: &Operation) -> Result<MirrorOptions> {
    let control = match op.optional("control") {
        "" | "true" => true,
        "false" => false,
        _ => return Err(AppError::new("MIRROR_OPTIONS", "投屏控制选项无效")),
    };
    let max_size = match op.optional("maxSize") {
        "" | "1280" => 1280,
        "0" => 0,
        "1920" => 1920,
        "2560" => 2560,
        _ => return Err(AppError::new("MIRROR_OPTIONS", "投屏分辨率无效")),
    };
    let max_fps = match op.optional("maxFps") {
        "" | "60" => 60,
        "30" => 30,
        _ => return Err(AppError::new("MIRROR_OPTIONS", "投屏帧率无效")),
    };
    let target = op
        .target
        .as_ref()
        .ok_or_else(|| AppError::new("TARGET_REQUIRED", "请选择投屏设备"))?;
    if target.serial.is_empty()
        || target.serial.len() > 256
        || target.serial.chars().any(char::is_control)
        || target
            .transport_id
            .parse::<u64>()
            .ok()
            .is_none_or(|id| id == 0)
    {
        return Err(AppError::new("INVALID_TARGET", "投屏目标无效"));
    }
    Ok(MirrorOptions {
        control,
        max_size,
        max_fps,
    })
}

pub fn plan(
    app: &App,
    op: &mut Operation,
    effects: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let options = validate_options(op)?;
    let target = op.target.as_ref().unwrap();
    if app.store.active_tasks()?.iter().any(|task| {
        task.operation.kind == "mirror_start"
            && task
                .operation
                .target
                .as_ref()
                .is_some_and(|other| same_target(other, target))
    }) {
        return Err(AppError::new(
            "MIRROR_ACTIVE",
            "该设备已有投屏会话，请先停止",
        ));
    }
    let tool = crate::device_tools::path(app, "scrcpy")?;
    op.args.insert("toolHash".into(), digest_file(&tool)?);
    op.args
        .insert("control".into(), options.control.to_string());
    op.args
        .insert("maxSize".into(), options.max_size.to_string());
    op.args.insert("maxFps".into(), options.max_fps.to_string());
    effects.push(
        if options.control {
            "打开投屏窗口并允许鼠标键盘控制"
        } else {
            "打开只读投屏窗口"
        }
        .into(),
    );
    effects.push(format!(
        "最大边长 {} · {} FPS",
        if options.max_size == 0 {
            "原始".into()
        } else {
            options.max_size.to_string()
        },
        options.max_fps
    ));
    warnings.push("关闭投屏窗口或停止任务即可结束；断线后不会自动重连。".into());
    warnings.push("本次不转发音频，不自动同步剪贴板。".into());
    Ok(())
}

pub fn same_target(a: &Target, b: &Target) -> bool {
    a.server_epoch == b.server_epoch && a.transport_id == b.transport_id && a.serial == b.serial
}

fn arguments(target: &Target, options: &MirrorOptions) -> Vec<String> {
    let mut args = vec![
        format!("--serial={}", target.serial),
        "--no-audio".into(),
        "--no-clipboard-autosync".into(),
        format!("--max-size={}", options.max_size),
        format!("--max-fps={}", options.max_fps),
        format!("--window-title=Android DevKit · {}", target.serial),
    ];
    if !options.control {
        args.push("--no-control".into());
    }
    args
}

/// One request is inspected before any bytes reach the real shared ADB server.
/// After a transport selection, the remaining stream stays on that transport.
#[derive(Debug, PartialEq)]
enum Request {
    Forward(String),
    Transport { tport: bool },
    Devices { long: bool },
}
fn rewrite(request: &str, target: &Target) -> Result<Request> {
    if matches!(request, "host:devices" | "host:devices-l") {
        return Ok(Request::Devices {
            long: request.ends_with("-l"),
        });
    }
    if matches!(request, "host:version" | "host:host-features") {
        return Ok(Request::Forward(request.into()));
    }
    if request == format!("host:tport:serial:{}", target.serial) {
        return Ok(Request::Transport { tport: true });
    }
    if request == format!("host:transport:{}", target.serial)
        || request == format!("host:transport-id:{}", target.transport_id)
    {
        return Ok(Request::Transport { tport: false });
    }
    let serial_prefix = format!("host-serial:{}:", target.serial);
    let id_prefix = format!("host-transport-id:{}:", target.transport_id);
    let suffix = request
        .strip_prefix(&serial_prefix)
        .or_else(|| request.strip_prefix(&id_prefix));
    if let Some(suffix) = suffix {
        if matches!(
            suffix,
            "get-state" | "get-serialno" | "get-devpath" | "features"
        ) || suffix.starts_with("forward:")
            || suffix.starts_with("killforward:")
        {
            return Ok(Request::Forward(format!("{id_prefix}{suffix}")));
        }
    }
    // No connect, disconnect, kill-server, arbitrary device or global forwarding.
    Err(AppError::new(
        "MIRROR_ADB_REQUEST",
        "投屏请求超出当前设备范围",
    ))
}

async fn read_frame(socket: &mut (impl AsyncRead + Unpin)) -> Result<String> {
    let mut length = [0; 4];
    socket.read_exact(&mut length).await?;
    let len = std::str::from_utf8(&length)
        .ok()
        .and_then(|s| usize::from_str_radix(s, 16).ok())
        .filter(|n| *n > 0 && *n <= 4096)
        .ok_or_else(|| AppError::new("MIRROR_ADB_PROTOCOL", "投屏 ADB 请求长度无效"))?;
    let mut bytes = vec![0; len];
    socket.read_exact(&mut bytes).await?;
    String::from_utf8(bytes)
        .map_err(|_| AppError::new("MIRROR_ADB_PROTOCOL", "投屏 ADB 请求编码无效"))
}
async fn write_frame(socket: &mut (impl AsyncWrite + Unpin), request: &str) -> Result<()> {
    socket
        .write_all(format!("{:04x}{request}", request.len()).as_bytes())
        .await?;
    Ok(())
}
async fn select_transport(
    client: &mut (impl AsyncWrite + Unpin),
    upstream: &mut (impl AsyncRead + AsyncWrite + Unpin),
    id: u64,
    tport: bool,
) -> Result<()> {
    write_frame(upstream, &format!("host:transport-id:{id}")).await?;
    let mut status = [0; 4];
    tokio::time::timeout(Duration::from_secs(5), upstream.read_exact(&mut status))
        .await
        .map_err(|_| AppError::new("MIRROR_ADB", "冻结设备连接超时"))??;
    client.write_all(&status).await?;
    if tport && &status == b"OKAY" {
        client.write_all(&id.to_le_bytes()).await?;
    }
    Ok(())
}
async fn fail(socket: &mut TcpStream, message: &str) {
    let _ = socket
        .write_all(format!("FAIL{:04x}{message}", message.len()).as_bytes())
        .await;
}
async fn current_target(app: &Arc<App>, target: &Target) -> Result<String> {
    let app = app.clone();
    let target = target.clone();
    tokio::task::spawn_blocking(move || {
        let epoch = app.epoch.load(Ordering::SeqCst);
        let raw = android::host_request("host:devices-l")?;
        let raw = String::from_utf8_lossy(&raw);
        let devices = android::parse_devices(&raw, epoch);
        checked_target(&target, &devices, epoch)?;
        raw.lines()
            .find(|line| {
                let mut fields = line.split_whitespace();
                fields.next() == Some(target.serial.as_str())
                    && fields.any(|field| field == format!("transport_id:{}", target.transport_id))
            })
            .map(str::to_owned)
            .ok_or_else(|| AppError::new("DEVICE_UNAVAILABLE", "原投屏设备已断开"))
    })
    .await
    .map_err(|e| AppError::new("MIRROR_ADB", e.to_string()))?
}
async fn proxy_connection(mut client: TcpStream, app: Arc<App>, target: Target) -> Result<()> {
    let request = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut client))
        .await
        .map_err(|_| AppError::new("MIRROR_ADB_PROTOCOL", "投屏 ADB 请求超时"))??;
    let result: Result<()> = async {
        let request = rewrite(&request, &target)?;
        let device = current_target(&app, &target).await?;
        if let Request::Devices { long } = request {
            let text = if long {
                format!("{device}\n")
            } else {
                format!("{}\tdevice\n", target.serial)
            };
            client.write_all(b"OKAY").await?;
            write_frame(&mut client, &text).await?;
            return Ok(());
        }
        let mut upstream = tokio::time::timeout(
            Duration::from_secs(3),
            TcpStream::connect(("127.0.0.1", 5037)),
        )
        .await
        .map_err(|_| AppError::new("MIRROR_ADB", "连接共享 ADB 超时"))??;
        match request {
            Request::Forward(request) => {
                write_frame(&mut upstream, &request).await?;
            }
            Request::Transport { tport } => {
                select_transport(
                    &mut client,
                    &mut upstream,
                    target.transport_id.parse::<u64>().unwrap(),
                    tport,
                )
                .await?;
            }
            Request::Devices { .. } => unreachable!(),
        }
        tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
        Ok(())
    }
    .await;
    if let Err(error) = &result {
        fail(&mut client, &error.message).await;
    }
    result
}
struct Proxy {
    task: tokio::task::JoinHandle<()>,
    port: u16,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Proxy {
    async fn start(app: Arc<App>, target: Target) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let task = tokio::spawn(async move {
            let mut clients = JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept() => {
                        let Ok((socket,_))=accepted else { break; };
                        if clients.len() >= 32 { continue; }
                        clients.spawn(proxy_connection(socket,app.clone(),target.clone()));
                    }
                    _=clients.join_next(), if !clients.is_empty() => {}
                }
            }
        });
        Ok(Self { task, port })
    }
}

async fn collect_tail(
    mut input: impl AsyncRead + Unpin,
    sender: tokio::sync::mpsc::Sender<String>,
) {
    let mut buffer = [0; 4096];
    let mut pending = Vec::new();
    loop {
        let count = match input.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for byte in &buffer[..count] {
            if *byte == b'\n' || pending.len() >= 2000 {
                let line = String::from_utf8_lossy(&pending).trim().to_string();
                pending.clear();
                if !line.is_empty() {
                    let _ = sender.try_send(line);
                }
            } else {
                pending.push(*byte);
            }
        }
    }
    if !pending.is_empty() {
        let _ = sender.try_send(String::from_utf8_lossy(&pending).trim().into());
    }
}

impl App {
    pub async fn mirror_screen(
        self: &Arc<Self>,
        id: &str,
        op: &Operation,
        settings: &Settings,
    ) -> Result<String> {
        let options = validate_options(op)?;
        let target = op.target.as_ref().unwrap();
        let tool = crate::device_tools::path(self, "scrcpy")?;
        if digest_file(&tool)? != op.arg("toolHash")? {
            return Err(AppError::new(
                "DEVICE_TOOL_CHANGED",
                "工具文件已变化，请重新准备工具",
            ));
        }
        // Resolves the compatible SDK adb without starting or replacing a shared server.
        let adb = self
            .bound_adb_spec(settings, target, vec!["get-state".into()], 10)
            .await?
            .executable
            .canonicalize()?;
        current_target(self, target).await?;
        if self.cancelled(id) {
            return Err(AppError::new("CANCELLED", "已取消投屏"));
        }
        let proxy = Proxy::start(self.clone(), target.clone()).await?;
        let mut spec = Spec::new(&tool, arguments(target, &options));
        spec.cwd = tool.parent().map(Path::to_path_buf);
        let mut command = spec.command();
        command
            .env("ADB", adb)
            .env("ADB_SERVER_SOCKET", format!("tcp:127.0.0.1:{}", proxy.port))
            .env_remove("ANDROID_ADB_SERVER_PORT")
            .env_remove("ADB_SERVER_PORT")
            .env_remove("SCRCPY_SERVER_PATH")
            .env_remove("SCRCPY_ICON_PATH")
            .env_remove("SDL_VIDEODRIVER")
            .env_remove("SDL_RENDER_DRIVER")
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| {
            AppError::new("MIRROR_START_FAILED", "无法启动投屏工具").detail(e.to_string())
        })?;
        drop(child.stdin.take());
        let (sender, mut lines) = tokio::sync::mpsc::channel(64);
        let mut readers = JoinSet::new();
        readers.spawn(collect_tail(child.stdout.take().unwrap(), sender.clone()));
        readers.spawn(collect_tail(child.stderr.take().unwrap(), sender));
        self.phase(
            id,
            "mirroring",
            "投屏会话正在运行，可关闭投屏窗口或停止任务",
        )?;
        self.update(id, |task| task.cancellable = true)?;
        let mut tick = tokio::time::interval(Duration::from_millis(200));
        let mut checks = 0u8;
        let mut tail = VecDeque::new();
        let outcome = loop {
            tokio::select! {
                status=child.wait() => break match status {
                    Ok(status) if status.success() => Ok("投屏窗口已关闭".into()),
                    Ok(status) => Err(AppError::new("MIRROR_EXIT","投屏工具异常退出").detail(format!("exit={}\n{}",status.code().unwrap_or(-1),tail.iter().cloned().collect::<Vec<_>>().join("\n")))),
                    Err(error) => Err(AppError::new("MIRROR_EXIT","无法读取投屏退出状态").detail(error.to_string())),
                },
                line=lines.recv(), if !lines.is_closed() || !lines.is_empty() => {
                    if let Some(line)=line {
                        tail.push_back(line.clone()); while tail.len()>30 { tail.pop_front(); }
                        if let Err(error)=self.line(id,line) { break Err(error); }
                    }
                }
                _=tick.tick() => {
                    if self.cancelled(id) { break Ok("已停止投屏".into()); }
                    checks+=1;
                    if checks>=5 {
                        checks=0;
                        if let Err(error)=current_target(self,target).await { break Err(error); }
                    }
                }
            }
        };
        // Kill only this owned process. Closing the proxy breaks inherited adb
        // client sockets; neither path terminates the shared ADB server.
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        drop(proxy);
        readers.abort_all();
        while readers.join_next().await.is_some() {}
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn target() -> Target {
        Target {
            serial: "192.168.1.2:4567".into(),
            transport_id: "19".into(),
            server_epoch: 7,
        }
    }
    fn operation() -> Operation {
        Operation {
            kind: "mirror_start".into(),
            args: BTreeMap::new(),
            target: Some(target()),
        }
    }
    #[test]
    fn options_reject_injection_and_arguments_bind_explicit_target() {
        let mut op = operation();
        let options = validate_options(&op).unwrap();
        assert_eq!(
            options,
            MirrorOptions {
                control: true,
                max_size: 1280,
                max_fps: 60
            }
        );
        for (key, value) in [
            ("control", "yes"),
            ("maxSize", "1280 --kill-adb-on-close"),
            ("maxFps", "0"),
        ] {
            op.args.insert(key.into(), value.into());
            assert!(validate_options(&op).is_err());
            op.args.clear();
        }
        let args = arguments(
            &target(),
            &MirrorOptions {
                control: false,
                max_size: 0,
                max_fps: 30,
            },
        );
        assert!(args.contains(&"--serial=192.168.1.2:4567".into()));
        assert!(args.contains(&"--no-control".into()));
        assert!(args.contains(&"--no-audio".into()));
        assert!(!args
            .iter()
            .any(|a| a.contains("kill-adb") || a.contains("tcpip")));
        op.target.as_mut().unwrap().transport_id = "0".into();
        assert!(validate_options(&op).is_err());
    }
    #[test]
    fn proxy_rewrites_serial_selectors_and_blocks_shared_service_actions() {
        let target = target();
        assert_eq!(
            rewrite("host:tport:serial:192.168.1.2:4567", &target).unwrap(),
            Request::Transport { tport: true }
        );
        assert_eq!(
            rewrite("host:transport:192.168.1.2:4567", &target).unwrap(),
            Request::Transport { tport: false }
        );
        assert_eq!(
            rewrite(
                "host-serial:192.168.1.2:4567:forward:tcp:123;localabstract:scrcpy",
                &target
            )
            .unwrap(),
            Request::Forward("host-transport-id:19:forward:tcp:123;localabstract:scrcpy".into())
        );
        for command in [
            "host:kill",
            "host:connect:other:5555",
            "host:transport:other",
            "host:tport:any",
            "host-transport-id:20:features",
            "host-serial:192.168.1.2:4567:killforward-all",
        ] {
            assert!(rewrite(command, &target).is_err(), "{command}");
        }
    }
    #[tokio::test]
    async fn pipe_reader_is_bounded_even_without_newlines() {
        let (mut writer, reader) = tokio::io::duplex(8192);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let writer = tokio::spawn(async move {
            writer.write_all(&vec![b'x'; 100_000]).await.unwrap();
        });
        collect_tail(reader, sender).await;
        writer.await.unwrap();
        let mut count = 0;
        while let Some(line) = receiver.recv().await {
            assert!(line.len() <= 2000);
            count += 1;
        }
        assert!(count <= 4);
        assert!(count > 0);
    }
    #[tokio::test]
    async fn tport_handshake_preserves_client_protocol_while_pinning_transport() {
        for tport in [true, false] {
            for success in [true, false] {
                let (mut proxy_upstream, mut server) = tokio::io::duplex(256);
                let (mut proxy_client, mut client) = tokio::io::duplex(256);
                let server = tokio::spawn(async move {
                    assert_eq!(
                        read_frame(&mut server).await.unwrap(),
                        "host:transport-id:72623859790382856"
                    );
                    server
                        .write_all(if success { b"OKAY" } else { b"FAIL" })
                        .await
                        .unwrap();
                });
                select_transport(
                    &mut proxy_client,
                    &mut proxy_upstream,
                    0x0102030405060708,
                    tport,
                )
                .await
                .unwrap();
                drop(proxy_client);
                let mut bytes = Vec::new();
                client.read_to_end(&mut bytes).await.unwrap();
                let mut expected = if success {
                    b"OKAY".to_vec()
                } else {
                    b"FAIL".to_vec()
                };
                if tport && success {
                    expected.extend_from_slice(&0x0102030405060708u64.to_le_bytes());
                }
                assert_eq!(bytes, expected);
                server.await.unwrap();
            }
        }
    }
    #[tokio::test]
    async fn request_frames_are_bounded_before_allocation() {
        for header in [b"0000", b"ffff", b"zzzz"] {
            let (mut writer, mut reader) = tokio::io::duplex(32);
            writer.write_all(header).await.unwrap();
            drop(writer);
            assert_eq!(
                read_frame(&mut reader).await.unwrap_err().code,
                "MIRROR_ADB_PROTOCOL"
            );
        }
    }
    #[test]
    fn duplicate_detection_is_scoped_to_the_frozen_connection() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(temp.path().into()).unwrap();
        let mut task = crate::regression_tests::task("active-mirror", "running");
        task.operation = operation();
        app.store.save_task(&task).unwrap();
        let mut op = operation();
        assert_eq!(
            plan(&app, &mut op, &mut vec![], &mut vec![])
                .unwrap_err()
                .code,
            "MIRROR_ACTIVE"
        );
        op.target.as_mut().unwrap().transport_id = "20".into();
        assert_eq!(
            plan(&app, &mut op, &mut vec![], &mut vec![])
                .unwrap_err()
                .code,
            "DEVICE_TOOL_MISSING"
        );
    }
}
