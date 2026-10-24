//! Deterministic fault fixtures. Hooks and workers exist only in test builds.
//! Every worker owns a fresh sandbox; the parent kills only its own child.
use crate::{
    app::App,
    domain::*,
    recording::{Manifest, Recorder},
    regression_tests::{target, task},
};
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const ID: &str = "fault-task";
const PAYLOAD: &[u8] = b"complete verified artifact\n";

pub(crate) fn checkpoint(name: &str) -> std::io::Result<()> {
    if std::env::var("DEVKIT_FAULT_CHECKPOINT").ok().as_deref() != Some(name) {
        return Ok(());
    }
    let root = PathBuf::from(std::env::var_os("DEVKIT_FAULT_WORKER_ROOT").unwrap());
    assert!(root.join(".fault-sandbox").is_file());
    fs::write(root.join("reached"), name)?;
    match std::env::var("DEVKIT_FAULT_MODE").unwrap().as_str() {
        "kill" => {
            // Bound the worker lifetime even if the test runner is interrupted.
            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                std::thread::park_timeout(Duration::from_millis(100));
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "fault parent did not terminate its child",
            ))
        }
        "disk-full" => Err(std::io::Error::from_raw_os_error(if cfg!(windows) {
            112 // ERROR_DISK_FULL
        } else {
            28 // ENOSPC
        })),
        "denied" => Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        mode => panic!("unknown fault mode: {mode}"),
    }
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_worker(root: &Path, point: &str, action: &str, mode: &str) {
    fs::write(root.join(".fault-sandbox"), b"owned disposable test data").unwrap();
    let log = fs::File::create(root.join("worker.log")).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "fault_tests::crash_worker", "--nocapture"])
        .env("DEVKIT_FAULT_WORKER_ROOT", root)
        .env("DEVKIT_FAULT_CHECKPOINT", point)
        .env("DEVKIT_FAULT_ACTION", action)
        .env("DEVKIT_FAULT_MODE", mode)
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut worker = Worker(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if mode == "kill" && root.join("reached").is_file() {
            worker.0.kill().unwrap();
            let status = worker.0.wait().unwrap();
            assert!(!status.success());
            break;
        }
        if let Some(status) = worker.0.try_wait().unwrap() {
            assert!(
                mode != "kill" && status.success(),
                "{point}/{mode}: {}",
                fs::read_to_string(root.join("worker.log")).unwrap()
            );
            assert!(root.join("reached").is_file(), "hook not reached: {point}");
            let result: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("outcome.json")).unwrap()).unwrap();
            assert_eq!(result["error"]["code"], "IO_ERROR", "{result}");
            break;
        }
        assert!(Instant::now() < deadline, "fault worker timed out: {point}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn receipt(name: &str, details: serde_json::Value) {
    println!("fault-case passed: {name}");
    if let Some(directory) = std::env::var_os("DEVKIT_FAULT_REPORT_DIR") {
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&json!({"case":name,"passed":true,"details":details}))
                .unwrap(),
        )
        .unwrap();
    }
}

fn settings(root: &Path) -> Settings {
    Settings {
        sdk_root: root.join("sdk").to_string_lossy().into(),
        avd_roots: vec![root.join("avd").to_string_lossy().into()],
        ..Default::default()
    }
}

fn setup(root: &Path, kind: &str) -> std::sync::Arc<App> {
    let app = App::new(root.join("app")).unwrap();
    app.store.save_settings(&settings(root)).unwrap();
    let mut t = task(ID, "running");
    t.operation.kind = kind.into();
    t.operation.target = Some(target(7, 41));
    app.store.save_task(&t).unwrap();
    app
}

fn zip_fixture(root: &Path) -> PathBuf {
    let path = root.join("archive.zip");
    let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    archive
        .start_file(
            "platform-tools/source.properties",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive
        .write_all(b"Pkg.Path=platform-tools\nPkg.Revision=1\nPkg.Desc=Fault fixture\n")
        .unwrap();
    archive.finish().unwrap();
    path
}

fn package() -> SdkPackage {
    SdkPackage {
        url: "https://dl.google.com/android/repository/fault-fixture.zip".into(),
        checksum: digest(PAYLOAD),
        checksum_type: "sha256".into(),
        size: PAYLOAD.len() as u64,
        ..Default::default()
    }
}

async fn response(body: &[u8], declared: usize, status: u16) -> reqwest::Response {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let body = body.to_vec();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        socket.write_all(&body).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
        .get(format!("http://{address}/archive"))
        .send()
        .await
        .unwrap()
}

fn worker_action(root: &Path, action: &str) -> Result<()> {
    match action {
        "download" => tokio::runtime::Runtime::new().unwrap().block_on(async {
            crate::bootstrap::receive_archive(
                &package(),
                &root.join("cache"),
                response(PAYLOAD, PAYLOAD.len(), 200).await,
                |_, _| {},
            )
            .await
            .map(|_| ())
        }),
        "sdk" => {
            let app = setup(root, "sdk_install");
            app.update(ID, |t| {
                t.operation
                    .args
                    .insert("packages".into(), "platform-tools\nemulator".into());
            })?;
            let existing = root.join("sdk/emulator");
            fs::create_dir_all(&existing)?;
            fs::write(existing.join("sentinel"), b"existing component")?;
            crate::bootstrap::install_verified_archive(
                &zip_fixture(root),
                &root.join("sdk/platform-tools"),
                "platform-tools",
                ID,
            )
        }
        "avd" => {
            let app = setup(root, "avd_create");
            app.update(ID, |t| {
                t.operation.args.insert("name".into(), "FaultAVD".into());
                t.operation.args.insert(
                    "image".into(),
                    "system-images;android-35;default;x86_64".into(),
                );
            })?;
            let image = root.join("sdk/system-images/android-35/default/x86_64");
            fs::create_dir_all(&image)?;
            fs::write(image.join("system.img"), b"image fixture")?;
            let stage = root.join("avd/.devkit-stage-fault-task");
            let data = stage.join("data");
            fs::create_dir_all(&data)?;
            fs::write(data.join(".devkit-owned"), ID)?;
            fs::write(data.join("config.ini"), "target=android-35\nimage.sysdir.1=system-images/android-35/default/x86_64/\nabi.type=x86_64\n")?;
            crate::recovery::commit_avd(
                &data,
                &root.join("avd/FaultAVD.avd"),
                &root.join("avd/FaultAVD.ini"),
                &root.join("app/avd-commit-fault-task.json"),
                &stage,
                ID,
                "android-35",
            )
        }
        "apk" => {
            let _app = setup(root, "apk_install");
            // A fake transport records its one remote side effect before returning
            // the same response consumed by the real install-result boundary.
            fs::write(root.join("remote-install-count"), b"1")?;
            crate::engine::apk_install_result(0, "Success\n", "dev.fault.fixture").map(|_| ())
        }
        "export" => {
            let app = setup(root, "export");
            let source = root.join("source.txt");
            fs::write(&source, PAYLOAD)?;
            fs::write(root.join("saved.txt"), b"old user content")?;
            app.store
                .register_artifact(ID, &source, "export", "fixture", ID)?;
            crate::exports::atomic_copy_verified(&source, &root.join("saved.txt"), &digest(PAYLOAD))
        }
        "record" => {
            let _app = setup(root, "log_record");
            let mut recorder = Recorder::new(
                &root.join("app/recordings").join(ID),
                ID,
                target(7, 41),
                LogFilter::default(),
                4096,
                4096,
            )?;
            recorder.line("retained line")?;
            recorder.checkpoint()?;
            recorder.finish("completed", "fixture").map(|_| ())
        }
        _ => panic!("unknown action: {action}"),
    }
}

#[test]
fn crash_worker() {
    let Some(root) = std::env::var_os("DEVKIT_FAULT_WORKER_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(root.join(".fault-sandbox").is_file());
    let action = std::env::var("DEVKIT_FAULT_ACTION").unwrap();
    let result = worker_action(&root, &action);
    fs::write(
        root.join("outcome.json"),
        serde_json::to_vec(&json!({"error":result.err()})).unwrap(),
    )
    .unwrap();
}

fn reopen(root: &Path) -> std::sync::Arc<App> {
    let app = App::new(root.join("app")).unwrap();
    app.recover_commits().unwrap();
    app.recover_recordings().unwrap();
    let settings = app.store.settings();
    app.reconcile(
        &crate::android::installed(&settings),
        &crate::android::avds(&settings),
        &[],
    )
    .unwrap();
    app
}

fn assert_no_replay(app: &std::sync::Arc<App>) {
    let previous = app.store.task(ID).unwrap();
    let same_request = app.submit("unused", "unused", ID, "", &[]).unwrap();
    assert_eq!(same_request.id, previous.id);
    assert_eq!(same_request.operation.target, previous.operation.target);
    assert_eq!(app.store.tasks().unwrap().len(), 1);
}

#[tokio::test]
async fn network_disconnect_checksum_proxy_and_retry() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let package = package();
    for (name, body, length, status, expected) in [
        (
            "disconnect",
            &PAYLOAD[..5],
            PAYLOAD.len(),
            200,
            "DOWNLOAD_FAILED",
        ),
        ("checksum", b"corrupt".as_slice(), 7, 200, "CHECKSUM_FAILED"),
        ("proxy-407", b"".as_slice(), 0, 407, "DOWNLOAD_FAILED"),
    ] {
        let error = crate::bootstrap::receive_archive(
            &package,
            &cache,
            response(body, length, status).await,
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, expected, "{name}");
        assert!(!fs::read_dir(&cache)
            .unwrap()
            .flatten()
            .any(|p| p.path().extension().is_some_and(|e| e == "zip")));
        let path = crate::bootstrap::receive_archive(
            &package,
            &cache,
            response(PAYLOAD, PAYLOAD.len(), 200).await,
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), PAYLOAD);
        assert!(!path.with_extension("part").exists());
        fs::remove_file(path).unwrap();
        receipt(
            &format!("network-{name}-retry"),
            json!({"error":expected,"retry":"verified bytes only"}),
        );
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = listener.local_addr().unwrap();
    drop(listener);
    let settings = Settings {
        proxy: format!("http://{closed}"),
        ..Default::default()
    };
    let error = crate::bootstrap::download(&settings, &package, &cache, |_, _| {})
        .await
        .unwrap_err();
    assert_eq!(error.code, "DOWNLOAD_FAILED");
    receipt(
        "proxy-connection-refused",
        json!({"productionDownload":true,"error":error.code}),
    );
}

#[tokio::test]
async fn killed_download_never_publishes_partial_bytes() {
    for point in ["download.after_chunk", "download.after_verify"] {
        let dir = tempfile::tempdir().unwrap();
        run_worker(dir.path(), point, "download", "kill");
        let cache = dir.path().join("cache");
        assert!(!fs::read_dir(&cache)
            .unwrap()
            .flatten()
            .any(|p| p.path().extension().is_some_and(|e| e == "zip")));
        let path = crate::bootstrap::receive_archive(
            &package(),
            &cache,
            response(PAYLOAD, PAYLOAD.len(), 200).await,
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), PAYLOAD);
        receipt(
            point,
            json!({"termination":"OS child kill","retry":"verified archive"}),
        );
    }
}

#[test]
fn sdk_partial_commit_requires_all_components_and_is_idempotent() {
    for point in ["sdk.after_extract", "sdk.after_publish"] {
        let dir = tempfile::tempdir().unwrap();
        run_worker(dir.path(), point, "sdk", "kill");
        let app = reopen(dir.path());
        assert_eq!(app.store.task(ID).unwrap().state, "needs_review");
        assert_eq!(
            fs::read(dir.path().join("sdk/emulator/sentinel")).unwrap(),
            b"existing component"
        );
        let published = point == "sdk.after_publish";
        assert_eq!(
            dir.path()
                .join("sdk/platform-tools/source.properties")
                .exists(),
            published
        );
        if !published {
            // An explicit retry uses a new task staging ID, preserving interrupted evidence.
            crate::bootstrap::install_verified_archive(
                &dir.path().join("archive.zip"),
                &dir.path().join("sdk/platform-tools"),
                "platform-tools",
                "explicit-retry",
            )
            .unwrap();
        }
        fs::write(
            dir.path().join("sdk/emulator/source.properties"),
            b"Pkg.Path=emulator\nPkg.Revision=1\n",
        )
        .unwrap();
        for _ in 0..2 {
            app.reconcile(&crate::android::installed(&app.store.settings()), &[], &[])
                .unwrap();
            assert_eq!(app.store.task(ID).unwrap().state, "succeeded");
        }
        assert_no_replay(&app);
        receipt(
            point,
            json!({"termination":"OS child kill","partial":"needs_review","allComponents":"succeeded","existingComponent":"preserved"}),
        );
    }
}

#[test]
fn avd_commit_kill_matrix_recovers_only_owned_committed_data() {
    for point in [
        "avd.before_journal",
        "avd.after_journal",
        "avd.after_data",
        "avd.after_index",
    ] {
        let dir = tempfile::tempdir().unwrap();
        run_worker(dir.path(), point, "avd", "kill");
        let committed = matches!(point, "avd.after_data" | "avd.after_index");
        for _ in 0..2 {
            let app = reopen(dir.path());
            let state = if committed {
                "succeeded"
            } else {
                "needs_review"
            };
            assert_eq!(app.store.task(ID).unwrap().state, state, "{point}");
            let avds = crate::android::avds(&app.store.settings());
            assert_eq!(avds.len(), usize::from(committed), "{point}: {avds:?}");
            if committed {
                assert!(avds[0].error.is_empty(), "{:?}", avds[0]);
                assert!(!dir.path().join("app/avd-commit-fault-task.json").exists());
            }
            assert_no_replay(&app);
        }
        receipt(
            point,
            json!({"termination":"OS child kill","committed":committed,"repeatedRecovery":"stable","duplicateAvds":0}),
        );
    }
}

#[test]
fn apk_response_before_task_commit_is_uncertain_and_never_replayed() {
    let dir = tempfile::tempdir().unwrap();
    run_worker(dir.path(), "apk.after_response", "apk", "kill");
    for _ in 0..2 {
        let app = reopen(dir.path());
        let task = app.store.task(ID).unwrap();
        assert_eq!(task.state, "needs_review");
        assert_eq!(task.operation.target, Some(target(7, 41)));
        assert_eq!(
            fs::read(dir.path().join("remote-install-count")).unwrap(),
            b"1"
        );
        assert_no_replay(&app);
    }
    receipt(
        "apk.after_response",
        json!({"termination":"OS child kill","transport":"sandbox response fixture","frozenTarget":"epoch7/transport41","remoteSideEffects":1,"state":"needs_review"}),
    );
}

#[test]
fn export_kill_matrix_preserves_source_and_atomic_user_destination() {
    for point in [
        "export.after_copy",
        "export.after_verify",
        "export.after_rename",
    ] {
        let dir = tempfile::tempdir().unwrap();
        run_worker(dir.path(), point, "export", "kill");
        let app = reopen(dir.path());
        let expected = if point == "export.after_rename" {
            PAYLOAD
        } else {
            b"old user content"
        };
        assert_eq!(fs::read(dir.path().join("saved.txt")).unwrap(), expected);
        assert_eq!(fs::read(dir.path().join("source.txt")).unwrap(), PAYLOAD);
        assert!(!app.store.artifact(ID).unwrap().0.acknowledged);
        assert_ne!(app.store.task(ID).unwrap().state, "succeeded");
        crate::exports::atomic_copy(
            &dir.path().join("source.txt"),
            &dir.path().join("saved.txt"),
        )
        .unwrap();
        assert_eq!(fs::read(dir.path().join("saved.txt")).unwrap(), PAYLOAD);
        receipt(
            point,
            json!({"termination":"OS child kill","destination":"whole old or whole new","source":"preserved","acknowledged":false,"retry":"passed"}),
        );
    }
}

#[test]
fn recording_kill_matrix_seals_retained_bytes_with_stable_digest() {
    for point in ["record.after_flush", "record.after_rename"] {
        let dir = tempfile::tempdir().unwrap();
        run_worker(dir.path(), point, "record", "kill");
        let path = dir
            .path()
            .join("app/recordings")
            .join(ID)
            .join("manifest.json");
        let mut previous = None;
        for _ in 0..2 {
            let app = reopen(dir.path());
            assert_eq!(app.store.task(ID).unwrap().state, "needs_review");
            let bytes = fs::read(&path).unwrap();
            let manifest: Manifest = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(manifest.state, "interrupted");
            assert_eq!(manifest.gaps, 1);
            assert_eq!(manifest.files.len(), 1);
            let file = path.parent().unwrap().join(&manifest.files[0].name);
            assert_eq!(fs::read(&file).unwrap(), b"retained line\n");
            assert_eq!(manifest.files[0].sha256, digest_file(&file).unwrap());
            assert_eq!(manifest.bytes, 14);
            if let Some(previous) = previous {
                assert_eq!(bytes, previous);
            }
            previous = Some(bytes);
            assert_no_replay(&app);
        }
        receipt(
            point,
            json!({"termination":"OS child kill","retainedBytes":14,"state":"interrupted","repeatedRecovery":"byte-identical manifest"}),
        );
    }
}

#[test]
fn disk_full_and_permission_errors_do_not_commit_success() {
    for (point, action) in [
        ("download.before_create", "download"),
        ("download.after_chunk", "download"),
        ("sdk.after_extract", "sdk"),
        ("avd.after_journal", "avd"),
        ("export.before_copy", "export"),
        ("export.after_copy", "export"),
        ("record.before_write", "record"),
    ] {
        for mode in ["disk-full", "denied"] {
            let dir = tempfile::tempdir().unwrap();
            run_worker(dir.path(), point, action, mode);
            if action != "download" {
                let app = reopen(dir.path());
                assert_ne!(app.store.task(ID).unwrap().state, "succeeded");
                assert_no_replay(&app);
            }
            match action {
                "sdk" => assert!(!dir.path().join("sdk/platform-tools").exists()),
                "avd" => assert!(!dir.path().join("avd/FaultAVD.ini").exists()),
                "export" => {
                    assert_eq!(
                        fs::read(dir.path().join("saved.txt")).unwrap(),
                        b"old user content"
                    );
                    assert_eq!(fs::read(dir.path().join("source.txt")).unwrap(), PAYLOAD);
                    assert!(!fs::read_dir(dir.path())
                        .unwrap()
                        .flatten()
                        .any(|f| f.file_name().to_string_lossy().starts_with(".devkit-save-")));
                }
                "download" => assert!(!fs::read_dir(dir.path().join("cache"))
                    .unwrap()
                    .flatten()
                    .any(|f| f.path().extension().is_some_and(|e| e == "zip"))),
                "record" => {
                    let manifest: Manifest = serde_json::from_slice(
                        &fs::read(dir.path().join("app/recordings/fault-task/manifest.json"))
                            .unwrap(),
                    )
                    .unwrap();
                    assert_eq!(manifest.bytes, 0);
                    assert_eq!(manifest.state, "interrupted");
                }
                _ => unreachable!(),
            }
            receipt(
                &format!("{point}-{mode}"),
                json!({"injection":mode,"error":"IO_ERROR","falseSuccess":false}),
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn windows_file_lock_preserves_destination_and_cleans_export_temporary() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let destination = dir.path().join("saved");
    fs::write(&source, PAYLOAD).unwrap();
    fs::write(&destination, b"locked user data").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&destination)
        .unwrap();
    assert_eq!(
        crate::exports::atomic_copy(&source, &destination)
            .unwrap_err()
            .code,
        "IO_ERROR"
    );
    assert_eq!(fs::read(&destination).unwrap(), b"locked user data");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    drop(lock);
    crate::exports::atomic_copy(&source, &destination).unwrap();
    assert_eq!(fs::read(&destination).unwrap(), PAYLOAD);
    receipt(
        "windows-file-lock",
        json!({"realWindowsHandle":true,"lockedDestination":"preserved","temporary":"removed","retry":"passed"}),
    );
}
