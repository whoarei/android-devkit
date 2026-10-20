//! Opt-in integration test. Only creates/deletes its own AVD under .qa/runtime.
use crate::{app::App, domain::*};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
async fn execute(
    app: &Arc<App>,
    kind: &str,
    args: BTreeMap<String, String>,
    target: Option<Target>,
) -> Task {
    let plan = app
        .prepare_plan(Operation {
            kind: kind.into(),
            args,
            target,
        })
        .unwrap();
    let task = app
        .submit(
            &plan.id,
            &plan.hash,
            &uuid::Uuid::new_v4().to_string(),
            &plan.confirmation_token,
            &[],
        )
        .unwrap();
    let mut last = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(350);
    loop {
        let t = app.store.task(&task.id).unwrap();
        if t.phase != last {
            println!("{kind}: {} · {}", t.phase, t.message);
            last = t.phase.clone();
        }
        if !t.active() {
            assert_eq!(
                t.state,
                "succeeded",
                "{}: {}\n{}",
                kind,
                t.message,
                t.log.join("\n")
            );
            return t;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "task deadline: {kind}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
#[ignore = "read-only network check of official SDK feeds; writes evidence only under .qa/runtime"]
async fn sdk_repository_catalog() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".qa/runtime")
        .join(format!("sdk-catalog-{}", now()));
    std::fs::create_dir_all(&root).unwrap();
    for channel in [0, 3] {
        let settings = Settings {
            sdk_channel: channel,
            ..Default::default()
        };
        let (packages, statuses) = crate::repository::fetch(&settings).await.unwrap();
        assert!(
            statuses.iter().all(|site| site.error.is_empty()),
            "{statuses:?}"
        );
        assert!(packages
            .iter()
            .any(|package| package.id.starts_with("platforms;")));
        assert!(packages
            .iter()
            .any(|package| package.id == "platform-tools"));
        assert!(packages
            .iter()
            .any(|package| package.id.starts_with("system-images;")));
        assert!(packages.iter().all(|package| package.channel <= channel));
        if channel == 0 {
            assert!(packages
                .iter()
                .all(|package| !package.id.to_ascii_lowercase().contains("canary")));
        }
        if channel == 3 {
            assert!(packages.iter().any(|package| package.channel > 0));
        }
        std::fs::write(
            root.join(format!("channel-{channel}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({"packages":packages,"sites":statuses}))
                .unwrap(),
        )
        .unwrap();
        println!(
            "channel {channel}: {} packages / {} sources",
            packages.len(),
            statuses.len()
        );
    }
    println!("SDK catalog evidence: {}", root.display());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads official tools into an isolated empty SDK; never changes E:\\Android\\sdk"]
async fn m2_blank_environment() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".qa/runtime")
        .join(format!("m2-{}", now()));
    let app = App::new(root.join("state")).unwrap();
    let settings = Settings {
        sdk_root: root.join("sdk").to_string_lossy().into(),
        avd_roots: vec![root.join("avd").to_string_lossy().into()],
        java_path: String::new(),
        ..Default::default()
    };
    app.store.save_settings(&settings).unwrap();
    println!("M2 fetching official catalog");
    let warnings = app.refresh_catalog().await.unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let p = app
        .prepare_plan(Operation {
            kind: "bootstrap".into(),
            args: args(&[("purpose", "emulator")]),
            target: None,
        })
        .unwrap();
    assert!(app
        .submit(
            &p.id,
            &p.hash,
            "missing-consent",
            &p.confirmation_token,
            &[]
        )
        .is_err());
    // These hashes model the explicit checked consent boxes in the isolated test UI.
    let accepted: Vec<_> = p.licenses.iter().map(|l| l.hash.clone()).collect();
    let task = app
        .submit(
            &p.id,
            &p.hash,
            "sandbox-bootstrap",
            &p.confirmation_token,
            &accepted,
        )
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1200);
    let mut phase = String::new();
    loop {
        let t = app.store.task(&task.id).unwrap();
        if t.phase != phase {
            println!("M2 {}: {}", t.phase, t.message);
            phase = t.phase.clone();
        }
        if !t.license_prompt.is_empty() {
            assert!(
                t.license_prompt.contains("android-sdk-license"),
                "unexpected license requires explicit human review"
            );
            let tx = app.answers.lock().unwrap().get(&t.id).cloned().unwrap();
            tx.send("y\n".into()).await.unwrap();
            app.update(&t.id, |t| t.license_prompt.clear()).unwrap();
        }
        if !t.active() {
            assert_eq!(t.state, "succeeded", "{}\n{}", t.message, t.log.join("\n"));
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let s = app.store.settings();
    assert!(PathBuf::from(&s.java_path).is_file());
    let e = crate::android::environment(&s);
    assert!(!e.adb.is_empty());
    assert!(!e.emulator.is_empty());
    assert!(!e.command_tools.is_empty());
    std::fs::write(
        root.join("acceptance.json"),
        serde_json::to_vec_pretty(&app.store.tasks().unwrap()).unwrap(),
    )
    .unwrap();
    println!("M2 empty SDK evidence: {}", root.display());
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "uses E:\\Android\\sdk and creates an isolated disposable test AVD"]
async fn m3_real_sdk_lifecycle() {
    run_lifecycle(false).await;
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "uses E:\\Android\\sdk and disposable AVD for logging, PID restart and export acceptance"]
async fn m4_real_sdk_observability() {
    run_lifecycle(true).await;
}
async fn run_lifecycle(include_m4: bool) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".qa/runtime")
        .join(format!(
            "{}-{}",
            if include_m4 { "m4" } else { "m3" },
            now()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let app = App::new(root.join("state")).unwrap();
    let settings = Settings {
        sdk_root: "E:\\Android\\sdk".into(),
        avd_roots: vec![root.join("avd").to_string_lossy().into()],
        boot_timeout_secs: 240,
        ..Settings::default()
    };
    app.store.save_settings(&settings).unwrap();
    assert!(!crate::android::environment(&settings).adb.is_empty());
    let installed = crate::android::installed(&settings);
    let image = installed
        .iter()
        .find(|p| p.id == "system-images;android-29;default;x86_64")
        .expect("existing test image");
    let name = format!("DevKit_QA_{}", now());
    execute(
        &app,
        "avd_create",
        args(&[
            ("name", &name),
            ("displayName", "DevKit M3 disposable test"),
            ("profile", "pixel_2"),
            ("image", &image.id),
            ("memory", "2048"),
            ("storage", "2"),
            ("gpu", "auto"),
            ("cpuCores", "2"),
            ("vmHeap", "256"),
            ("cameraFront", "none"),
            ("cameraBack", "emulated"),
            ("sdCardMode", "size"),
            ("sdCardMiB", "64"),
        ]),
        None,
    )
    .await;
    let a = crate::android::avds(&settings)
        .into_iter()
        .find(|a| a.name == name)
        .unwrap();
    let before = std::fs::read_to_string(PathBuf::from(&a.data_path).join("config.ini")).unwrap();
    let sd = PathBuf::from(a.config.get("sdcard.path").expect("managed SD path"));
    assert!(sd.is_file());
    assert!(std::fs::metadata(&sd).unwrap().len() >= 64 * 1024 * 1024);
    execute(
        &app,
        "avd_edit",
        args(&[
            ("avdId", &a.id),
            ("fingerprint", &a.fingerprint),
            ("displayName", "DevKit M3 QA edited"),
            ("memory", "2048"),
            ("gpu", "auto"),
            ("cpuCores", "3"),
            ("vmHeap", "256"),
            ("networkSpeed", "lte"),
            ("networkLatency", "20:100"),
            ("skinMode", "none"),
            ("orientation", "portrait"),
        ]),
        None,
    )
    .await;
    assert!(before.contains("image.sysdir.1"));
    let edited = crate::android::read_properties(&PathBuf::from(&a.data_path).join("config.ini"));
    assert_eq!(edited["hw.cpu.ncore"], "3");
    assert_eq!(edited["runtime.network.speed"], "lte");
    assert_eq!(edited["runtime.network.latency"], "20:100");
    execute(&app, "avd_start", args(&[("avdId", &a.id)]), None).await;
    let snapshot = app.snapshot().await.unwrap();
    let device = snapshot
        .devices
        .iter()
        .find(|d| d.avd_name == name && d.state == "ready")
        .expect("new AVD ready");
    let target = Target::from(device);
    let screenshot = execute(&app, "capture", BTreeMap::new(), Some(target.clone())).await;
    assert!(PathBuf::from(&screenshot.artifact).is_file());
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".qa/runtime/fixture/fixture.apk");
    assert!(
        fixture.is_file(),
        "Run scripts/build-test-apk.ps1 before the real SDK acceptance test"
    );
    if fixture.is_file() {
        let apk = app.prepare_apk(&fixture).unwrap();
        assert_eq!(apk.package, "dev.androiddevkit.fixture");
        execute(
            &app,
            "apk_install",
            args(&[("apkId", &apk.id)]),
            Some(target.clone()),
        )
        .await;
        execute(
            &app,
            "app_launch",
            args(&[("package", &apk.package)]),
            Some(target.clone()),
        )
        .await;
        if include_m4 {
            verify_observability(&app, &settings, &target, &apk.package, &root).await;
        }
    }
    execute(&app, "avd_stop", args(&[("avdId", &a.id)]), None).await;
    execute(&app, "avd_cold", args(&[("avdId", &a.id)]), None).await;
    execute(&app, "avd_stop", args(&[("avdId", &a.id)]), None).await;
    // A stale overlay must not restore data after the managed SD is formatted.
    let overlay = PathBuf::from(format!("{}.qcow2", sd.to_string_lossy()));
    std::fs::write(&overlay, b"stale-overlay-marker").unwrap();
    execute(&app, "avd_wipe", args(&[("avdId", &a.id)]), None).await;
    if overlay.is_file() {
        assert!(!std::fs::read(&overlay)
            .unwrap()
            .starts_with(b"stale-overlay-marker"));
    }
    execute(&app, "avd_delete", args(&[("avdId", &a.id)]), None).await;
    assert!(!PathBuf::from(&a.data_path).exists());
    assert!(PathBuf::from(
        "E:\\Android\\sdk\\system-images\\android-29\\default\\x86_64\\system.img"
    )
    .is_file());
    let tablet_name = format!("DevKit_Tablet_QA_{}", now());
    execute(
        &app,
        "avd_create",
        args(&[
            ("name", &tablet_name),
            ("displayName", "Disposable tablet QA"),
            ("profile", "Nexus 9"),
            ("image", &image.id),
            ("memory", "2048"),
            ("storage", "2"),
            ("gpu", "auto"),
        ]),
        None,
    )
    .await;
    let tablet = crate::android::avds(&settings)
        .into_iter()
        .find(|a| a.name == tablet_name)
        .unwrap();
    execute(&app, "avd_start", args(&[("avdId", &tablet.id)]), None).await;
    execute(&app, "avd_focus", args(&[("avdId", &tablet.id)]), None).await;
    execute(&app, "avd_force_stop", args(&[("avdId", &tablet.id)]), None).await;
    execute(&app, "avd_delete", args(&[("avdId", &tablet.id)]), None).await;
    std::fs::write(
        root.join("acceptance.json"),
        serde_json::to_vec_pretty(&app.store.tasks().unwrap()).unwrap(),
    )
    .unwrap();
    println!("SDK lifecycle evidence: {}", root.display());
}

async fn verify_observability(
    app: &Arc<App>,
    settings: &Settings,
    target: &Target,
    package: &str,
    root: &std::path::Path,
) {
    let key = crate::observe::key(target);
    app.log_views.lock().unwrap().insert(key);
    app.log_start(target.clone()).await.unwrap();
    let filter = LogFilter {
        package: package.into(),
        tag: "DevKitFixture".into(),
        ..Default::default()
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let old = app.log_snapshot(target, &filter, 500, true).await.unwrap();
    assert!(!old.pids.is_empty());
    assert!(old.rows.iter().any(|r| r.raw.contains("heartbeat")));
    let task_app = app.clone();
    let task_target = target.clone();
    let json = serde_json::to_string(&filter).unwrap();
    let recording = tokio::spawn(async move {
        execute(
            &task_app,
            "log_record",
            args(&[("filter", &json), ("quotaMiB", "8"), ("seconds", "10")]),
            Some(task_target),
        )
        .await
    });
    for _ in 0..50 {
        if app
            .store
            .active_tasks()
            .unwrap()
            .iter()
            .any(|t| t.phase == "recording")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    crate::android::adb(
        settings,
        Some(target),
        vec![
            "shell".into(),
            "am".into(),
            "force-stop".into(),
            package.into(),
        ],
        10,
    )
    .await
    .unwrap()
    .success()
    .unwrap();
    tokio::time::sleep(Duration::from_millis(2300)).await;
    let absent = app.log_snapshot(target, &filter, 500, true).await.unwrap();
    assert!(absent.pids.is_empty());
    assert!(absent.process_status.contains("等待"));
    execute(
        app,
        "app_launch",
        args(&[("package", package)]),
        Some(target.clone()),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2300)).await;
    let current = app.log_snapshot(target, &filter, 500, true).await.unwrap();
    assert!(!current.pids.is_empty());
    assert_ne!(old.pids, current.pids);
    assert!(current
        .rows
        .iter()
        .filter(|r| r.level != "MARK")
        .all(|r| current.pids.contains(&r.pid)));
    let task = recording.await.unwrap();
    let manifest: crate::recording::Manifest =
        serde_json::from_slice(&std::fs::read(&task.artifact).unwrap()).unwrap();
    assert_eq!(manifest.state, "complete");
    assert!(manifest
        .files
        .iter()
        .all(|s| s.name.ends_with(".log") && !s.sha256.is_empty()));
    let selected: Vec<_> = (0..manifest.files.len()).collect();
    let exported = app.export_recording(&task.id, &selected, true).unwrap();
    assert!(exported.text.contains("heartbeat"));
    assert!(!exported.text.contains("fixture-secret") && !exported.text.contains("192.168.10.20"));
    let raw = app.export_recording(&task.id, &selected, false).unwrap();
    assert!(raw.text.contains("fixture-secret"));
    assert_eq!(
        app.resolve_close("exit", false, false, &[])
            .unwrap_err()
            .code,
        "UNSAVED_ARTIFACTS"
    );
    let cursor = current.latest;
    let cleared = app
        .log_snapshot(
            target,
            &LogFilter {
                after: cursor,
                ..filter.clone()
            },
            500,
            true,
        )
        .await
        .unwrap();
    assert!(cleared.rows.iter().all(|r| r.seq > cursor));
    app.log_release(target);
    std::fs::write(root.join("m4-observability.json"),serde_json::to_vec_pretty(&serde_json::json!({"package":package,"oldPids":old.pids,"newPids":current.pids,"waitingForProcess":true,"manifest":manifest,"redactedExportRows":exported.rows,"redactionVerified":true,"rawExportRequiresExplicitChoice":true,"unsavedExitBlocked":true,"clearUsesSequenceCursor":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "explicit CPU/memory throughput benchmark"]
fn log_throughput() {
    let start = std::time::Instant::now();
    let mut buffer = crate::log_buffer::LogBuffer::default();
    for i in 0..150_000 {
        buffer.push(&format!("10-18 01:23:45.123  123  124 I TestTag: row {i}"));
    }
    let seconds = start.elapsed().as_secs_f64();
    let per_second = 150000.0 / seconds;
    assert!(buffer.bytes <= 8 * 1024 * 1024);
    assert_eq!(buffer.lines.len(), 10_000);
    assert!(per_second >= 5000.0, "{per_second} lines/s");
    println!(
        "{}",
        serde_json::json!({"lines":150000,"seconds":seconds,"linesPerSecond":per_second,"bufferRows":buffer.lines.len(),"bufferRawBytes":buffer.bytes,"dropped":buffer.dropped})
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "checks real ADB connection failures against localhost:9; never pairs an external phone"]
async fn wireless_failure_and_secret_handling() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().into()).unwrap();
    for kind in ["wireless_pair", "wireless_connect", "network_connect"] {
        let op = Operation {
            kind: kind.into(),
            args: args(&[("address", "127.0.0.1:9"), ("code", "860731")]),
            target: None,
        };
        let p = app.prepare_plan(op).unwrap();
        let t = app
            .submit(
                &p.id,
                &p.hash,
                &uuid::Uuid::new_v4().to_string(),
                &p.confirmation_token,
                &[],
            )
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(75);
        loop {
            let task = app.store.task(&t.id).unwrap();
            assert!(!task.operation.args.contains_key("code"));
            assert!(!serde_json::to_string(&task).unwrap().contains("860731"));
            if !task.active() {
                assert_eq!(task.state, "failed");
                println!("{kind}: {}", task.message);
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }
}
