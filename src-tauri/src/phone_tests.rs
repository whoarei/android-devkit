use crate::{
    app::App,
    domain::*,
    phone_apk::{ApkMember, ApkSet},
    phone_files,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn member(split: &str) -> ApkMember {
    ApkMember {
        file_name: format!("{split}.apk"),
        split: split.into(),
        sha256: format!("hash-{split}"),
        bytes: 1,
        min_sdk: 21,
        abis: vec![],
        requires: vec![],
    }
}
fn set() -> ApkSet {
    ApkSet {
        id: uuid::Uuid::new_v4().to_string(),
        package: "dev.androiddevkit.fixture".into(),
        version: "1".into(),
        version_code: "1".into(),
        signers: vec!["a".repeat(64)],
        members: vec![member(""), member("config.zh")],
    }
}
#[test]
fn apk_set_rejects_missing_base_duplicate_and_missing_dependencies() {
    let mut apk = set();
    crate::phone_apk::validate_set(&apk).unwrap();
    apk.members.remove(0);
    assert_eq!(
        crate::phone_apk::validate_set(&apk).unwrap_err().code,
        "APK_BASE_REQUIRED"
    );
    apk = set();
    apk.members.push(member("config.zh"));
    assert_eq!(
        crate::phone_apk::validate_set(&apk).unwrap_err().code,
        "APK_DUPLICATE"
    );
    apk = set();
    apk.members[1].requires.push("feature.missing".into());
    assert_eq!(
        crate::phone_apk::validate_set(&apk).unwrap_err().code,
        "APK_SPLIT_MISSING"
    );
    apk = set();
    apk.members[1].split = "../escape".into();
    assert!(crate::phone_apk::validate_set(&apk).is_err());
}
#[test]
fn installed_set_requires_exact_version_and_split_set() {
    let apk = set();
    crate::phone_apk::verify_installed(
        &apk,
        "  versionCode=1 minSdk=21\n  splits=[base, config.zh]\n",
    )
    .unwrap();
    for text in [
        "versionCode=2\nsplits=[base, config.zh]",
        "versionCode=1\nsplits=[base]",
        "versionCode=1\nsplits=[base, config.zh, config.en]",
    ] {
        assert_eq!(
            crate::phone_apk::verify_installed(&apk, text)
                .unwrap_err()
                .code,
            "RESULT_UNKNOWN"
        );
    }
}
#[test]
fn incompatible_api_and_abi_are_rejected_before_an_install_can_be_built() {
    let mut member = member("config.arm64_v8a");
    member.min_sdk = 23;
    member.abis = vec!["arm64-v8a".into()];
    assert_eq!(
        crate::phone_apk::compatible_member(&member, 22, "arm64-v8a")
            .unwrap_err()
            .code,
        "SDK_MISMATCH"
    );
    assert_eq!(
        crate::phone_apk::compatible_member(&member, 29, "x86_64,x86")
            .unwrap_err()
            .code,
        "ABI_MISMATCH"
    );
    crate::phone_apk::compatible_member(&member, 29, "arm64-v8a,armeabi-v7a").unwrap();
    member.abis.clear();
    crate::phone_apk::compatible_member(&member, 29, "x86_64").unwrap();
    member.min_sdk = 1;
    assert_eq!(
        crate::phone_apk::compatible_member(&member, 20, "x86")
            .unwrap_err()
            .code,
        "SDK_MISMATCH"
    );
}
#[test]
fn shared_storage_and_windows_names_reject_traversal_and_aliases() {
    for path in [
        "/data/data/pkg",
        "/sdcard/Download/../DCIM",
        "/sdcard/Movies//x",
        "/sdcard/Download/a\\b",
        "/sdcard/Download/CON.txt",
        "/sdcard/Download/a.",
        "/storage/emulated/0/Download",
    ] {
        assert!(phone_files::remote_path(path).is_err(), "{path}");
    }
    for path in [
        "/sdcard/Download",
        "/sdcard/Download/中文 空格/quote'$().txt",
        "/sdcard/Pictures/.hidden",
    ] {
        assert!(phone_files::remote_path(path).is_ok(), "{path}");
    }
    assert_eq!(phone_files::quote("a'b;$()"), "'a'\\''b;$()'");
    assert!(phone_files::owned_remote("/sdcard/Download", "../bad").is_err());
}
#[test]
fn publish_race_preserves_existing_file_and_directory() {
    let temp = tempfile::tempdir().unwrap();
    for directory in [false, true] {
        let source = temp.path().join(if directory {
            "source-dir"
        } else {
            "source-file"
        });
        let destination = temp
            .path()
            .join(if directory { "dest-dir" } else { "dest-file" });
        if directory {
            std::fs::create_dir(&source).unwrap();
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(destination.join("original"), b"keep").unwrap();
        } else {
            std::fs::write(&source, b"new").unwrap();
            std::fs::write(&destination, b"keep").unwrap();
        }
        assert!(crate::recovery::publish_new_file(&source, &destination).is_err());
        assert!(source.exists());
        assert_eq!(
            std::fs::read(if directory {
                destination.join("original")
            } else {
                destination
            })
            .unwrap(),
            b"keep"
        );
    }
}
#[test]
fn mp4_truncation_and_missing_finalization_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("video.mp4");
    fn atom(kind: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut b = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        b.extend(kind);
        b.extend(payload);
        b
    }
    fn movie(samples: u32, duration: u32) -> Vec<u8> {
        let mut hdlr = vec![0; 8];
        hdlr.extend(b"vide");
        let mut mdhd = vec![0; 12];
        mdhd.extend(1000u32.to_be_bytes());
        mdhd.extend(duration.to_be_bytes());
        let mut stsz = vec![0; 4];
        stsz.extend(4u32.to_be_bytes());
        stsz.extend(samples.to_be_bytes());
        let mut mdia = atom(b"mdhd", &mdhd);
        mdia.extend(atom(b"hdlr", &hdlr));
        mdia.extend(atom(b"minf", &atom(b"stbl", &atom(b"stsz", &stsz))));
        let mut bytes = atom(b"ftyp", &[0; 4]);
        bytes.extend(atom(b"mdat", &[0; 4]));
        bytes.extend(atom(b"moov", &atom(b"trak", &atom(b"mdia", &mdia))));
        bytes
    }
    let bytes = movie(1, 1000);
    std::fs::write(&path, &bytes).unwrap();
    crate::phone_record::validate_mp4(&path).unwrap();
    for bad in [&bytes[..24], &bytes[..35]] {
        std::fs::write(&path, bad).unwrap();
        assert!(crate::phone_record::validate_mp4(&path).is_err());
    }
    for bad in [movie(0, 0), movie(1, 0), movie(100, 1000)] {
        std::fs::write(&path, bad).unwrap();
        assert!(crate::phone_record::validate_mp4(&path).is_err());
    }
}
#[test]
fn interrupted_recording_retry_recovers_and_transfers_require_review() {
    let temp = tempfile::tempdir().unwrap();
    let app = App::new(temp.path().into()).unwrap();
    let mut task = crate::regression_tests::task("transfer", "needs_review");
    task.operation.kind = "file_upload".into();
    app.store.save_task(&task).unwrap();
    assert_eq!(
        app.retry_plan("transfer").unwrap_err().code,
        "TRANSFER_REVIEW_REQUIRED"
    );
}
async fn submit(
    app: &Arc<App>,
    kind: &str,
    arguments: BTreeMap<String, String>,
    target: Option<Target>,
) -> Task {
    let mut operation = Operation {
        kind: kind.into(),
        args: arguments,
        target,
    };
    if matches!(kind, "file_upload" | "file_download") {
        app.prepare_transfer_destinations(&mut operation)
            .await
            .unwrap();
    }
    let plan = app.prepare_plan(operation).unwrap();
    app.submit(
        &plan.id,
        &plan.hash,
        &uuid::Uuid::new_v4().to_string(),
        &plan.confirmation_token,
        &[],
    )
    .unwrap()
}
async fn finish(app: &App, id: &str) -> Task {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(350);
    let mut last = String::new();
    loop {
        let task = app.store.task(id).unwrap();
        if task.phase != last {
            println!("{}: {} · {}", task.operation.kind, task.phase, task.message);
            last = task.phase.clone();
        }
        if !task.active() {
            return task;
        }
        assert!(tokio::time::Instant::now() < deadline, "{}", task.message);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
async fn execute(
    app: &Arc<App>,
    kind: &str,
    arguments: BTreeMap<String, String>,
    target: Option<Target>,
) -> Task {
    let task = submit(app, kind, arguments, target).await;
    let task = finish(app, &task.id).await;
    assert_eq!(task.state, "succeeded", "{}\n{:?}", task.message, task.log);
    task
}
fn compare_tree(source: &Path, destination: &Path) {
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = destination.join(entry.file_name());
        if entry.path().is_dir() {
            assert!(path.is_dir());
            compare_tree(&entry.path(), &path);
        } else {
            assert_eq!(
                digest_file(&entry.path()).unwrap(),
                digest_file(&path).unwrap()
            );
        }
    }
}
#[tokio::test(flavor = "multi_thread")]
async fn phone_crash_worker() {
    let Ok(profile) = std::env::var("ANDROID_DEVKIT_PHONE_CRASH_PROFILE") else {
        return;
    };
    let profile = PathBuf::from(profile);
    let qa = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".qa/runtime")
        .canonicalize()
        .unwrap();
    assert!(profile.canonicalize().unwrap().starts_with(qa));
    assert!(profile.join(".phase2-owned").is_file());
    let mut target: Target =
        serde_json::from_slice(&std::fs::read(profile.join("target.json")).unwrap()).unwrap();
    let app = App::new(profile.join("app")).unwrap();
    target.server_epoch = app.epoch.load(std::sync::atomic::Ordering::SeqCst);
    if let Ok(direction) = std::env::var("ANDROID_DEVKIT_PHONE_TRANSFER_CRASH") {
        let paths: Vec<String> =
            serde_json::from_slice(&std::fs::read(profile.join("paths.json")).unwrap()).unwrap();
        let destination = std::fs::read_to_string(profile.join("destination.txt")).unwrap();
        let set = app
            .prepare_transfer(&direction, paths, target.clone(), destination)
            .await
            .unwrap();
        let task = submit(
            &app,
            &format!("file_{direction}"),
            args(&[("transferId", &set.id), ("conflicts", "replace")]),
            Some(target),
        )
        .await;
        std::fs::write(profile.join("ready-task-id"), &task.id).unwrap();
        tokio::time::sleep(Duration::from_secs(300)).await;
        return;
    }
    let task = submit(
        &app,
        "screen_record",
        args(&[("seconds", "10"), ("resolution", "720p")]),
        Some(target),
    )
    .await;
    loop {
        let current = app.store.task(&task.id).unwrap();
        if current.phase == "recording" {
            break;
        }
        assert!(current.active(), "{}", current.message);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    std::fs::write(profile.join("ready-task-id"), &task.id).unwrap();
    tokio::time::sleep(Duration::from_secs(300)).await;
}
async fn crash_recording_roundtrip(root: &Path, settings: &Settings, target: &Target) -> String {
    let profile = root.join("crash-profile");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join(".phase2-owned"), b"disposable phase2 test").unwrap();
    std::fs::write(
        profile.join("target.json"),
        serde_json::to_vec(target).unwrap(),
    )
    .unwrap();
    {
        let app = App::new(profile.join("app")).unwrap();
        app.store.save_settings(settings).unwrap();
    }
    let log = std::fs::File::create(profile.join("worker.log")).unwrap();
    let mut spec = crate::process::Spec::new(
        std::env::current_exe().unwrap(),
        vec![
            "--exact".into(),
            "phone_tests::phone_crash_worker".into(),
            "--nocapture".into(),
        ],
    );
    spec.env.insert(
        "ANDROID_DEVKIT_PHONE_CRASH_PROFILE".into(),
        profile.to_string_lossy().into(),
    );
    let mut child = spec
        .command()
        .stdout(std::process::Stdio::from(log.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(log))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !profile.join("ready-task-id").is_file() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "crash worker did not start; inspect {}",
            profile.display()
        );
        assert!(child.try_wait().unwrap().is_none(), "crash worker exited");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let task_id = std::fs::read_to_string(profile.join("ready-task-id")).unwrap();
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    let recovered = App::new(profile.join("app")).unwrap();
    let current_target = Target {
        server_epoch: recovered.epoch.load(std::sync::atomic::Ordering::SeqCst),
        ..target.clone()
    };
    assert_eq!(
        recovered.store.task(&task_id).unwrap().state,
        "needs_review"
    );
    tokio::time::sleep(Duration::from_secs(10)).await;
    let result = execute(
        &recovered,
        "screen_recover",
        args(&[("sourceTaskId", &task_id)]),
        Some(current_target),
    )
    .await;
    crate::phone_record::validate_mp4(Path::new(&result.artifact)).unwrap();
    assert_eq!(recovered.store.task(&task_id).unwrap().state, "succeeded");
    result.artifact
}
async fn transfer_crash_recovery(
    root: &Path,
    settings: &Settings,
    target: &Target,
    direction: &str,
) {
    let profile = root.join(format!("crash-{direction}"));
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join(".phase2-owned"), b"owned transfer test").unwrap();
    std::fs::write(profile.join(".fault-sandbox"), b"owned fault sandbox").unwrap();
    std::fs::write(
        profile.join("target.json"),
        serde_json::to_vec(target).unwrap(),
    )
    .unwrap();
    let name = format!("DevKit_P2_{direction}_{}.txt", uuid::Uuid::new_v4());
    let old = b"original before replacement";
    let new = b"new verified replacement";
    let seed = App::new(profile.join("seed")).unwrap();
    seed.store.save_settings(settings).unwrap();
    let seed_target = Target {
        server_epoch: seed.epoch.load(std::sync::atomic::Ordering::SeqCst),
        ..target.clone()
    };
    let seed_path = profile.join(&name);
    std::fs::write(
        &seed_path,
        if direction == "upload" {
            old.as_slice()
        } else {
            new.as_slice()
        },
    )
    .unwrap();
    let seed_set = seed
        .prepare_transfer(
            "upload",
            vec![seed_path.to_string_lossy().into()],
            seed_target.clone(),
            "/sdcard/Download".into(),
        )
        .await
        .unwrap();
    execute(
        &seed,
        "file_upload",
        args(&[("transferId", &seed_set.id)]),
        Some(seed_target),
    )
    .await;
    let (paths, destination) = if direction == "upload" {
        std::fs::write(&seed_path, new).unwrap();
        (
            vec![seed_path.to_string_lossy().into_owned()],
            "/sdcard/Download".to_string(),
        )
    } else {
        let destination = profile.join("local-destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join(&name), old).unwrap();
        (
            vec![format!("/sdcard/Download/{name}")],
            destination.to_string_lossy().into_owned(),
        )
    };
    std::fs::write(
        profile.join("paths.json"),
        serde_json::to_vec(&paths).unwrap(),
    )
    .unwrap();
    std::fs::write(profile.join("destination.txt"), &destination).unwrap();
    {
        let app = App::new(profile.join("app")).unwrap();
        app.store.save_settings(settings).unwrap();
    }
    let log = std::fs::File::create(profile.join("worker.log")).unwrap();
    let mut spec = crate::process::Spec::new(
        std::env::current_exe().unwrap(),
        vec![
            "--exact".into(),
            "phone_tests::phone_crash_worker".into(),
            "--nocapture".into(),
        ],
    );
    for (key, value) in [
        (
            "ANDROID_DEVKIT_PHONE_CRASH_PROFILE",
            profile.to_string_lossy().into_owned(),
        ),
        ("ANDROID_DEVKIT_PHONE_TRANSFER_CRASH", direction.into()),
        (
            "DEVKIT_FAULT_WORKER_ROOT",
            profile.to_string_lossy().into_owned(),
        ),
        (
            "DEVKIT_FAULT_CHECKPOINT",
            "phone.transfer.after_backup".into(),
        ),
        ("DEVKIT_FAULT_MODE", "kill".into()),
    ] {
        spec.env.insert(key.into(), value);
    }
    let mut child = spec
        .command()
        .stdout(std::process::Stdio::from(log.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(log))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !profile.join("reached").exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "transfer crash boundary timeout: {}",
            profile.display()
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "transfer worker exited: {}",
            std::fs::read_to_string(profile.join("worker.log")).unwrap()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let task_id = std::fs::read_to_string(profile.join("ready-task-id")).unwrap();
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    let app = App::new(profile.join("app")).unwrap();
    assert_eq!(app.store.task(&task_id).unwrap().state, "needs_review");
    let current = Target {
        server_epoch: app.epoch.load(std::sync::atomic::Ordering::SeqCst),
        ..target.clone()
    };
    // Simulate a missing marker: the retained original itself must still block cleanup.
    if direction == "upload" {
        let stage = phone_files::owned_remote(&destination, &task_id).unwrap();
        app.remote_shell(
            settings,
            &current,
            format!("rm -f -- {}/.devkit-protected", phone_files::quote(&stage)),
            15,
        )
        .await
        .unwrap();
    } else {
        std::fs::remove_file(
            Path::new(&destination).join(format!(".devkit-transfer-{task_id}/.devkit-protected")),
        )
        .unwrap();
    }
    let cleanup = submit(
        &app,
        "file_cleanup",
        args(&[("sourceTaskId", &task_id)]),
        Some(current.clone()),
    )
    .await;
    let cleanup = finish(&app, &cleanup.id).await;
    assert!(
        cleanup.message.contains("BACKUPS_REVIEW_REQUIRED"),
        "{}",
        cleanup.message
    );
    execute(
        &app,
        "file_restore",
        args(&[("sourceTaskId", &task_id)]),
        Some(current.clone()),
    )
    .await;
    if direction == "upload" {
        assert_eq!(
            app.remote_hash(settings, &current, &format!("/sdcard/Download/{name}"))
                .await
                .unwrap(),
            digest(old)
        );
    } else {
        assert_eq!(
            std::fs::read(Path::new(&destination).join(&name)).unwrap(),
            old
        );
    }
    assert_eq!(app.store.task(&task_id).unwrap().state, "cancelled");
    std::fs::write(profile.join("recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({"direction":direction,"sourceTaskId":task_id,"killedAfterBackup":true,"originalRestored":true,"backupProtectedFromCleanup":true})).unwrap()).unwrap();
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "real SDK; creates two disposable isolated AVDs and writes only their shared storage and .qa/runtime"]
async fn phase2_real_sdk_workflows() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".qa/runtime")
        .join(format!("phase2-{}", now()));
    std::fs::create_dir_all(&root).unwrap();
    let app = App::new(root.join("state")).unwrap();
    let settings = Settings {
        sdk_root: "E:\\Android\\sdk".into(),
        java_path: "C:\\Program Files\\Android\\Android Studio\\jbr\\bin\\java.exe".into(),
        avd_roots: vec![root.join("avd").to_string_lossy().into()],
        boot_timeout_secs: 240,
        ..Default::default()
    };
    app.store.save_settings(&settings).unwrap();
    let owned_prefix = format!("DevKit_P2_{}", now());
    let test_app = app.clone();
    let test_root = root.clone();
    let prefix = owned_prefix.clone();
    let test_settings = settings.clone();
    let result=tokio::spawn(async move {
        let app=test_app;let root=test_root;let settings=test_settings;
        let fixture=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join(".qa/runtime/fixture/splits");
        let base=fixture.join("base.apk");let split=fixture.join("config.zh.apk");
        let set=app.prepare_apk_set(vec![base.clone(),split.clone()]).await.unwrap();assert_eq!(set.members.len(),2);
        for (paths,code) in [(vec![split.clone()],"APK_BASE_REQUIRED"),(vec![base.clone(),fixture.join("other-signer.apk")],"APK_SET_MISMATCH"),(vec![fixture.join("unsigned.apk")],"APK_SIGNATURE"),(vec![base.clone(),base.clone()],"APK_BASE_REQUIRED"),(vec![base.clone(),fixture.join("wrong.version.apk")],"APK_SET_MISMATCH"),(vec![base.clone(),fixture.join("wrong.package.apk")],"APK_SET_MISMATCH"),(vec![base.clone(),fixture.join("feature.extra.apk")],"APK_SPLIT_MISSING")] {
            assert_eq!(app.prepare_apk_set(paths).await.unwrap_err().code,code);
        }
        let mut targets=vec![];
        for index in 0..2 {
            let name=format!("{prefix}_{index}");
            execute(&app,"avd_create",args(&[("name",&name),("displayName","Phase 2 disposable QA"),("profile","pixel_2"),("image","system-images;android-29;default;x86_64"),("memory","1536"),("storage","2"),("gpu","auto"),("cpuCores","2"),("skinMode","none"),("screenWidth","720"),("screenHeight","1280")]),None).await;
            let avd=crate::android::avds(&settings).into_iter().find(|a|a.name==name).unwrap();
            let log=std::fs::File::create(root.join(format!("{name}.emulator.log"))).unwrap();
            let mut spec=crate::process::Spec::new(Path::new(&settings.sdk_root).join("emulator/emulator.exe"),vec!["-avd".into(),name.clone(),"-no-window".into(),"-no-snapshot".into(),"-gpu".into(),"host".into()]);
            spec.env.insert("ANDROID_AVD_HOME".into(),Path::new(&avd.ini_path).parent().unwrap().to_string_lossy().into());
            spec.env.insert("ANDROID_HOME".into(),settings.sdk_root.clone());
            let mut command=spec.command();command.stdout(std::process::Stdio::from(log.try_clone().unwrap())).stderr(std::process::Stdio::from(log)).stdin(std::process::Stdio::null());
            let _child=command.spawn().unwrap();
            execute(&app,"avd_wait",args(&[("avdId",&avd.id)]),None).await;
            let devices=crate::android::devices(&settings,app.epoch.load(std::sync::atomic::Ordering::SeqCst)).await;
            targets.push(Target::from(devices.iter().find(|d|d.avd_name==name && d.state=="ready").unwrap()));
        }
        let target=&targets[0];
        execute(&app,"apk_set_install",args(&[("setId",&set.id)]),Some(target.clone())).await;
        app.remote_shell(&settings,target,"run-as dev.androiddevkit.fixture sh -c 'mkdir -p files; printf retained-phase2 > files/retained.txt'".into(),15).await.unwrap();
        let directory_set=app.prepare_apk_set(vec![fixture.join("valid")]).await.unwrap();assert_eq!(directory_set.members.len(),2);
        let upgraded=app.prepare_apk_set(vec![fixture.join("v2")]).await.unwrap();
        execute(&app,"apk_set_install",args(&[("setId",&upgraded.id)]),Some(target.clone())).await;
        let retained=app.remote_shell(&settings,target,"run-as dev.androiddevkit.fixture cat files/retained.txt".into(),15).await.unwrap();assert_eq!(retained.text().trim(),"retained-phase2");
        let downgrade=submit(&app,"apk_set_install",args(&[("setId",&set.id)]),Some(target.clone())).await;let downgrade=finish(&app,&downgrade.id).await;assert_eq!(downgrade.state,"failed");assert!(downgrade.message.contains("VERSION_DOWNGRADE"),"{}",downgrade.message);
        let conflicting=app.prepare_apk_set(vec![fixture.join("other-signer-v2.apk")]).await.unwrap();
        let rejected=submit(&app,"apk_set_install",args(&[("setId",&conflicting.id)]),Some(target.clone())).await;let rejected=finish(&app,&rejected.id).await;assert_eq!(rejected.state,"failed");assert!(rejected.message.contains("SIGNATURE_CONFLICT"),"{}",rejected.message);
        let retained=app.remote_shell(&settings,target,"run-as dev.androiddevkit.fixture cat files/retained.txt".into(),15).await.unwrap();assert_eq!(retained.text().trim(),"retained-phase2");
        execute(&app,"app_launch",args(&[("package",&set.package)]),Some(target.clone())).await;
        let video=execute(&app,"screen_record",args(&[("seconds","5"),("resolution","720p")]),Some(target.clone())).await;
        crate::phone_record::validate_mp4(Path::new(&video.artifact)).unwrap();
        let stop=submit(&app,"screen_record",args(&[("seconds","30"),("resolution","original")]),Some(target.clone())).await;
        loop {let task=app.store.task(&stop.id).unwrap();if task.phase=="recording" {break;}assert!(task.active(),"{}",task.message);tokio::time::sleep(Duration::from_millis(100)).await;}
        tokio::time::sleep(Duration::from_secs(4)).await;app.cancel_task(&stop.id).unwrap();let stop=finish(&app,&stop.id).await;assert_eq!(stop.state,"succeeded","{}\n{:?}",stop.message,stop.log);crate::phone_record::validate_mp4(Path::new(&stop.artifact)).unwrap();
        let recovered_video=crash_recording_roundtrip(&root,&settings,target).await;
        let source=root.join("source");let nested=source.join("中文 空格'$()目录");std::fs::create_dir_all(nested.join("空目录")).unwrap();
        std::fs::write(nested.join("测试 空格'$();.txt"),"中文文件往返\nline two").unwrap();std::fs::write(nested.join(".hidden"),b"hidden").unwrap();
        let empty=source.join("empty.bin");std::fs::write(&empty,b"").unwrap();
        let upload=app.prepare_transfer("upload",vec![nested.to_string_lossy().into(),empty.to_string_lossy().into()],target.clone(),"/sdcard/Download".into()).await.unwrap();
        execute(&app,"file_upload",args(&[("transferId",&upload.id)]),Some(target.clone())).await;
        let destination=root.join("roundtrip");std::fs::create_dir_all(&destination).unwrap();
        let paths=upload.roots.iter().map(|r|format!("/sdcard/Download/{r}")).collect();
        let download=app.prepare_transfer("download",paths,target.clone(),destination.to_string_lossy().into()).await.unwrap();
        execute(&app,"file_download",args(&[("transferId",&download.id)]),Some(target.clone())).await;
        compare_tree(&source,&destination);
        let conflict=app.prepare_transfer("upload",vec![nested.to_string_lossy().into()],target.clone(),"/sdcard/Download".into()).await.unwrap();assert_eq!(conflict.conflicts.len(),1);
        assert_eq!(app.prepare_plan(Operation{kind:"file_upload".into(),args:args(&[("transferId",&conflict.id)]),target:Some(target.clone())}).unwrap_err().code,"FILE_CONFLICT");
        let wrong=Target{serial:targets[1].serial.clone(),..target.clone()};
        assert!(app.remote_entries(&wrong,"/sdcard/Download").await.is_err());
        assert!(app.remote_entries(&targets[1],"/sdcard/Download").await.unwrap().iter().all(|e|!upload.roots.contains(&e.name)));
        let tamper_source=source.join("tamper.txt");std::fs::write(&tamper_source,b"original").unwrap();
        let tamper=app.prepare_transfer("upload",vec![tamper_source.to_string_lossy().into()],target.clone(),"/sdcard/Download".into()).await.unwrap();
        std::fs::write(phone_files::transfer_dir(&app,&tamper.id).unwrap().join("payload/tamper.txt"),b"changed").unwrap();
        let task=submit(&app,"file_upload",args(&[("transferId",&tamper.id)]),Some(target.clone())).await;let task=finish(&app,&task.id).await;assert_eq!(task.state,"failed");assert!(task.message.contains("SOURCE_CHANGED"));
        transfer_crash_recovery(&root,&settings,target,"upload").await;
        transfer_crash_recovery(&root,&settings,target,"download").await;
        let maximum=execute(&app,"screen_record",args(&[("seconds","180"),("resolution","720p")]),Some(target.clone())).await;
        crate::phone_record::validate_mp4(Path::new(&maximum.artifact)).unwrap();
        std::fs::write(root.join("acceptance.json"),serde_json::to_vec_pretty(&serde_json::json!({"apkSet":set,"upgradeVersionCode":upgraded.version_code,"upgradeRetainsData":true,"downgradeAndSignatureConflictPreserveData":true,"targets":targets,"video":video.artifact,"earlyStoppedVideo":stop.artifact,"recoveredAfterProcessKill":recovered_video,"maximum180SecondVideo":maximum.artifact,"upload":upload,"download":download,"roundtripSha256":true,"conflictPreserved":true,"frozenTarget":true,"tamperRejected":true,"uploadAndDownloadReplacementCrashRecovery":true})).unwrap()).unwrap();
    }).await;
    // Even an assertion failure only stops/deletes AVDs owned by this isolated profile and exact prefix.
    for avd in crate::android::avds(&settings)
        .into_iter()
        .filter(|a| a.name.starts_with(&format!("{owned_prefix}_")))
    {
        if crate::app::avd_locked(Path::new(&avd.data_path)) {
            let task = submit(&app, "avd_stop", args(&[("avdId", &avd.id)]), None).await;
            let _ = finish(&app, &task.id).await;
        }
        if !crate::app::avd_locked(Path::new(&avd.data_path)) {
            let task = submit(&app, "avd_delete", args(&[("avdId", &avd.id)]), None).await;
            let _ = finish(&app, &task.id).await;
        }
    }
    println!("Phase 2 evidence: {}", root.display());
    result.unwrap();
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "targeted backup/record-protection regression; one disposable headless AVD under .qa/runtime"]
async fn phase2_replacement_recovery_regression() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".qa/runtime")
        .join(format!("phase2-recovery-{}", now()));
    std::fs::create_dir_all(&root).unwrap();
    let app = App::new(root.join("app")).unwrap();
    let settings = Settings {
        sdk_root: "E:\\Android\\sdk".into(),
        java_path: "C:\\Program Files\\Android\\Android Studio\\jbr\\bin\\java.exe".into(),
        avd_roots: vec![root.join("avd").to_string_lossy().into()],
        boot_timeout_secs: 120,
        ..Default::default()
    };
    app.store.save_settings(&settings).unwrap();
    let name = format!("DevKit_P2_Recovery_{}", now());
    let test_app = app.clone();
    let test_settings = settings.clone();
    let test_root = root.clone();
    let test_name = name.clone();
    let result = tokio::spawn(async move {
        let app = test_app;
        let settings = test_settings;
        let root = test_root;
        let name = test_name;
        execute(
            &app,
            "avd_create",
            args(&[
                ("name", &name),
                ("profile", "pixel_2"),
                ("image", "system-images;android-29;default;x86_64"),
                ("memory", "1536"),
                ("storage", "2"),
                ("gpu", "auto"),
            ]),
            None,
        )
        .await;
        let avd = crate::android::avds(&settings)
            .into_iter()
            .find(|a| a.name == name)
            .unwrap();
        let log = std::fs::File::create(root.join("emulator.log")).unwrap();
        let mut spec = crate::process::Spec::new(
            Path::new(&settings.sdk_root).join("emulator/emulator.exe"),
            vec![
                "-avd".into(),
                name.clone(),
                "-no-window".into(),
                "-no-snapshot".into(),
                "-gpu".into(),
                "host".into(),
            ],
        );
        spec.env.insert(
            "ANDROID_AVD_HOME".into(),
            Path::new(&avd.ini_path)
                .parent()
                .unwrap()
                .to_string_lossy()
                .into(),
        );
        spec.env
            .insert("ANDROID_HOME".into(), settings.sdk_root.clone());
        let _child = spec
            .command()
            .stdout(std::process::Stdio::from(log.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(log))
            .stdin(std::process::Stdio::null())
            .spawn()
            .unwrap();
        execute(&app, "avd_wait", args(&[("avdId", &avd.id)]), None).await;
        let devices = crate::android::devices(
            &settings,
            app.epoch.load(std::sync::atomic::Ordering::SeqCst),
        )
        .await;
        let target = Target::from(
            devices
                .iter()
                .find(|d| d.avd_name == name && d.state == "ready")
                .unwrap(),
        );
        // Package-manager boot readiness can precede the shared-storage mount on a fresh AVD.
        let storage_deadline = tokio::time::Instant::now() + Duration::from_secs(45);
        loop {
            match app.remote_entries(&target, "/sdcard/Download").await {
                Ok(_) => break,
                Err(e) => {
                    assert!(
                        tokio::time::Instant::now() < storage_deadline,
                        "shared storage did not become ready: {e:?}"
                    );
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        transfer_crash_recovery(&root, &settings, &target, "upload").await;
        transfer_crash_recovery(&root, &settings, &target, "download").await;
    })
    .await;
    for avd in crate::android::avds(&settings)
        .into_iter()
        .filter(|a| a.name == name)
    {
        if crate::app::avd_locked(Path::new(&avd.data_path)) {
            let task = submit(&app, "avd_stop", args(&[("avdId", &avd.id)]), None).await;
            finish(&app, &task.id).await;
        }
        if !crate::app::avd_locked(Path::new(&avd.data_path)) {
            execute(&app, "avd_delete", args(&[("avdId", &avd.id)]), None).await;
        }
    }
    println!("Replacement recovery evidence: {}", root.display());
    result.unwrap();
}
