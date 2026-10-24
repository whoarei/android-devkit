use crate::domain::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

#[derive(Clone)]
pub struct Spec {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub timeout: u64,
    pub stdin: Option<String>,
    pub cwd: Option<PathBuf>,
}
impl Spec {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self {
            executable: program.into(),
            args,
            env: Default::default(),
            timeout: 30,
            stdin: None,
            cwd: None,
        }
    }
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.executable);
        if let Some(cwd) = &self.cwd {
            c.current_dir(cwd);
        }
        c.args(&self.args)
            .envs(&self.env)
            .env_remove("ANDROID_SERIAL")
            .env_remove("ADB_SERVER_SOCKET")
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        c.creation_flags(0x08000000);
        c
    }
}
#[derive(Debug)]
pub struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}
impl Output {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }
    pub fn success(self) -> Result<Self> {
        if self.code == 0 {
            Ok(self)
        } else {
            Err(
                AppError::new("TOOL_FAILED", format!("工具返回 {}", self.code)).detail(format!(
                    "{}\n{}",
                    self.text(),
                    self.stderr
                )),
            )
        }
    }
}
async fn bounded_read(
    mut stream: impl AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    let mut result = Vec::new();
    // Keep pipe buffers out of nested async frames on small Windows stacks.
    let mut buffer = vec![0; 16384];
    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        result.extend_from_slice(&buffer[..n]);
        if result.len() > limit {
            result.drain(..result.len() - limit);
        }
    }
    Ok(result)
}
pub async fn run(spec: Spec) -> Result<Output> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(spec.timeout);
    let mut child = spec.command().kill_on_drop(true).spawn()?;
    let stdin = child.stdin.take();
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let result = tokio::time::timeout_at(deadline, async {
        let input = async {
            if let Some(mut stdin) = stdin {
                if let Some(input) = &spec.stdin {
                    stdin.write_all(input.as_bytes()).await?;
                }
            }
            std::io::Result::Ok(())
        };
        // Keep the pipe readers in this future, so timeout or cancellation
        // closes them even when a descendant inherited the write handles.
        let (status, stdout, stderr, ()) = tokio::try_join!(
            child.wait(),
            bounded_read(out, 32 * 1024 * 1024),
            bounded_read(err, 2 * 1024 * 1024),
            input
        )?;
        Ok(Output {
            code: status.code().unwrap_or(-1),
            stdout,
            stderr: String::from_utf8_lossy(&stderr).into(),
        })
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            let _ = child.start_kill();
            Err(AppError::new(
                "TOOL_TIMEOUT",
                "工具响应超时；写操作结果需要检查",
            ))
        }
    }
}

pub async fn streaming(
    spec: Spec,
    on_line: Arc<dyn Fn(String) + Send + Sync>,
    answer: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<String>>>,
) -> Result<Output> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(spec.timeout);
    let mut child = spec.command().kill_on_drop(true).spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let result = tokio::time::timeout_at(deadline, async {
        if let Some(input) = &spec.stdin {
            stdin.write_all(input.as_bytes()).await?;
        }
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut pending = String::new();
        let mut receiver = answer.lock().await;
        let mut answers_open = true;
        let (mut out_open, mut err_open) = (true, true);
        let (mut out_bytes, mut err_bytes) = ([0; 4096], [0; 4096]);
        while out_open || err_open {
            let (is_error, n) = tokio::select! {
                result = out.read(&mut out_bytes), if out_open => (false, result?),
                result = err.read(&mut err_bytes), if err_open => (true, result?),
                response = receiver.recv(), if answers_open => {
                    if let Some(value) = response {
                        stdin.write_all(value.as_bytes()).await?;
                        stdin.flush().await?;
                    } else {
                        answers_open = false;
                    }
                    continue;
                }
            };
            if n == 0 {
                if is_error {
                    err_open = false;
                } else {
                    out_open = false;
                }
                continue;
            }
            let bytes = if is_error {
                &err_bytes[..n]
            } else {
                &out_bytes[..n]
            };
            let dest = if is_error { &mut stderr } else { &mut stdout };
            dest.extend_from_slice(bytes);
            if dest.len() > 2 * 1024 * 1024 {
                dest.drain(..dest.len() - 2 * 1024 * 1024);
            }
            pending.push_str(&String::from_utf8_lossy(bytes));
            while let Some(pos) = pending.find(['\n', '\r']) {
                let line = pending[..pos].trim().to_string();
                pending.drain(..=pos);
                if !line.is_empty() {
                    on_line(line);
                }
            }
            if pending.len() > 300 || pending.contains("(y/N)") || pending.contains("(y/n)") {
                on_line(std::mem::take(&mut pending));
            }
        }
        if !pending.trim().is_empty() {
            on_line(pending);
        }
        let status = child.wait().await?;
        Ok(Output {
            code: status.code().unwrap_or(-1),
            stdout,
            stderr: String::from_utf8_lossy(&stderr).into(),
        })
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            let _ = child.start_kill();
            Err(AppError::new("TOOL_TIMEOUT", "工具超时，结果需复查"))
        }
    }
}

pub fn java_tool(settings: &Settings, tool: &str, args: Vec<String>) -> Result<Spec> {
    let root = Path::new(&settings.sdk_root).join("cmdline-tools/latest");
    let jar = root.join(format!("lib/{tool}-classpath.jar"));
    if !jar.is_file() {
        return Err(AppError::new(
            "MISSING_COMMAND_TOOLS",
            "缺少受支持的 Command-Line Tools，请先准备环境",
        ));
    }
    let source = std::fs::read_to_string(root.join("source.properties"))?;
    let major = source
        .lines()
        .find_map(|line| line.strip_prefix("Pkg.Revision="))
        .and_then(|v| v.split('.').next())
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    if !(12..=22).contains(&major) {
        return Err(AppError::new(
            "UNSUPPORTED_TOOL_VERSION",
            "当前 Command-Line Tools 不在已支持的 12～22 版本范围",
        ));
    }
    if !Path::new(&settings.java_path).is_file() {
        return Err(AppError::new(
            "JAVA_MISSING",
            "请选择兼容的 java.exe 或准备应用私有 Java 运行时",
        ));
    }
    let class = match tool {
        "sdkmanager" => "com.android.sdklib.tool.sdkmanager.SdkManagerCli",
        "avdmanager" => "com.android.sdklib.tool.AvdManagerCli",
        "apkanalyzer" => "com.android.tools.apk.analyzer.ApkAnalyzerCli",
        _ => return Err(AppError::new("UNSUPPORTED_TOOL", "未知工具")),
    };
    let property = if tool == "avdmanager" {
        "-Dcom.android.sdkmanager.toolsdir="
    } else {
        "-Dcom.android.sdklib.toolsdir="
    };
    let mut all = vec![
        property.to_owned() + &root.to_string_lossy(),
        "-classpath".into(),
        jar.to_string_lossy().into(),
        class.into(),
    ];
    all.extend(args);
    let mut spec = Spec::new(&settings.java_path, all);
    spec.env
        .insert("ANDROID_HOME".into(), settings.sdk_root.clone());
    spec.env
        .insert("ANDROID_SDK_ROOT".into(), settings.sdk_root.clone());
    spec.timeout = 3600;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_spec(mode: &str, root: &Path) -> Spec {
        let mut spec = Spec::new(
            std::env::current_exe().unwrap(),
            vec![
                "--exact".into(),
                "process::tests::process_fixture".into(),
                "--nocapture".into(),
            ],
        );
        spec.env
            .insert("ANDROID_DEVKIT_PROCESS_FIXTURE_MODE".into(), mode.into());
        spec.env.insert(
            "ANDROID_DEVKIT_PROCESS_FIXTURE_ROOT".into(),
            root.to_string_lossy().into(),
        );
        spec.timeout = 1;
        spec
    }
    // This test also supplies finite, isolated child processes for the runner
    // tests. It does nothing when invoked by the normal test suite.
    #[test]
    #[allow(
        clippy::zombie_processes,
        reason = "The fixture parent must exit first; the test waits for its finite descendant's completion marker"
    )]
    fn process_fixture() {
        let Ok(mode) = std::env::var("ANDROID_DEVKIT_PROCESS_FIXTURE_MODE") else {
            return;
        };
        let root = PathBuf::from(std::env::var("ANDROID_DEVKIT_PROCESS_FIXTURE_ROOT").unwrap());
        match mode.as_str() {
            "inherit" => {
                let spec = fixture_spec("descendant", &root);
                let mut command = std::process::Command::new(&spec.executable);
                command
                    .args(&spec.args)
                    .envs(&spec.env)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                command.spawn().unwrap();
            }
            "descendant" => {
                std::thread::sleep(Duration::from_secs(4));
                std::fs::write(root.join("descendant-finished"), b"done").unwrap();
            }
            #[cfg(windows)]
            "closed-pipes" => {
                use std::os::windows::io::AsRawHandle;
                unsafe {
                    windows_sys::Win32::Foundation::CloseHandle(std::io::stdout().as_raw_handle());
                    windows_sys::Win32::Foundation::CloseHandle(std::io::stderr().as_raw_handle());
                }
                std::thread::sleep(Duration::from_secs(4));
            }
            "answer" => {
                println!("Accept license (y/N)");
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).unwrap();
                println!("answer={}", line.trim());
            }
            "stdin-eof" => {
                use std::io::Read;
                let mut data = Vec::new();
                std::io::stdin().read_to_end(&mut data).unwrap();
                println!("input-bytes={}", data.len());
            }
            _ => panic!("unknown fixture mode"),
        }
        std::process::exit(0);
    }
    #[tokio::test]
    async fn deadline_covers_inherited_pipes_and_closes_local_readers() {
        for stream in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let spec = fixture_spec("inherit", dir.path());
            let at = tokio::time::Instant::now();
            let result = tokio::time::timeout(Duration::from_secs(3), async {
                if stream {
                    let (_tx, rx) = tokio::sync::mpsc::channel(1);
                    streaming(
                        spec,
                        Arc::new(|_| {}),
                        Arc::new(tokio::sync::Mutex::new(rx)),
                    )
                    .await
                } else {
                    run(spec).await
                }
            })
            .await;
            let elapsed = at.elapsed();
            // Let the finite descendant exit before its temporary directory is
            // removed. No shared Android process is involved or terminated.
            let finished = dir.path().join("descendant-finished");
            let wait = tokio::time::timeout(Duration::from_secs(5), async {
                while !finished.exists() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await;
            wait.unwrap();
            assert_eq!(
                result.expect("runner exceeded deadline").unwrap_err().code,
                "TOOL_TIMEOUT"
            );
            assert!(elapsed < Duration::from_millis(2500), "{elapsed:?}");
        }
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn streaming_deadline_still_applies_after_both_pipes_close() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(tx);
        let at = tokio::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            streaming(
                fixture_spec("closed-pipes", dir.path()),
                Arc::new(|_| {}),
                Arc::new(tokio::sync::Mutex::new(rx)),
            ),
        )
        .await;
        assert_eq!(
            result
                .expect("wait after EOF exceeded deadline")
                .unwrap_err()
                .code,
            "TOOL_TIMEOUT"
        );
        assert!(at.elapsed() < Duration::from_millis(2500));
    }
    #[tokio::test]
    async fn streaming_accepts_prompt_answers_and_run_closes_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let mut spec = fixture_spec("stdin-eof", dir.path());
        spec.stdin = Some("fixture input".into());
        let output = run(spec).await.unwrap().success().unwrap();
        assert!(output.text().contains("input-bytes=13"));
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let output = streaming(
            fixture_spec("answer", dir.path()),
            Arc::new(move |line| {
                if line.contains("Accept license") {
                    tx.try_send("y\n".into()).unwrap();
                }
            }),
            Arc::new(tokio::sync::Mutex::new(rx)),
        )
        .await
        .unwrap()
        .success()
        .unwrap();
        assert!(output.text().contains("answer=y"));
    }
    #[tokio::test]
    async fn drains_both_streams_without_shell_interpolation() {
        let out = run(Spec::new("C:\\Windows\\System32\\whoami.exe", vec![]))
            .await
            .unwrap();
        assert_eq!(out.code, 0);
        assert!(!out.stdout.is_empty());
    }
}
