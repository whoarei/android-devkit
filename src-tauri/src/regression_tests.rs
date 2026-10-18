use crate::{
    app::App,
    domain::*,
    observe,
    recording::{Manifest, Recorder},
    recovery::write_commit_journal,
};
use std::{
    sync::{atomic::Ordering, mpsc},
    time::Duration,
};

pub(crate) fn task(id: &str, state: &str) -> Task {
    serde_json::from_value(serde_json::json!({
        "id":id,"requestId":id,"title":"regression fixture",
        "operation":{"kind":"log_record","args":{},"target":null},
        "state":state,"phase":"recording","progress":null,
        "message":"fixture","log":[],"createdAt":1,"updatedAt":1,
        "cancelRequested":false,"cancellable":true,"artifact":"",
        "licensePrompt":"","confirmedLicenses":[]
    }))
    .unwrap()
}
pub(crate) fn target(epoch: u64, transport: u64) -> Target {
    Target {
        serial: "fixture".into(),
        transport_id: transport.to_string(),
        server_epoch: epoch,
    }
}

#[test]
fn cancellation_survives_an_in_flight_progress_update() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().into()).unwrap();
    app.store.save_task(&task("race", "running")).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let progress_app = app.clone();
    let progress = std::thread::spawn(move || {
        progress_app.update("race", |t| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            t.message = "new progress".into();
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let cancel_app = app.clone();
    let cancellation = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        cancel_app.cancel_task("race").unwrap();
        done_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let completed_early = done_rx.recv_timeout(Duration::from_millis(100)).is_ok();
    release_tx.send(()).unwrap();
    progress.join().unwrap().unwrap();
    cancellation.join().unwrap();
    assert!(
        !completed_early,
        "cancellation must wait for the in-flight task transaction"
    );
    assert!(app.cancelled("race"));
}

#[test]
fn concurrent_task_updates_preserve_all_changes() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().into()).unwrap();
    app.store.save_task(&task("updates", "running")).unwrap();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let app = app.clone();
            std::thread::spawn(move || {
                for _ in 0..20 {
                    app.update("updates", |t| {
                        t.progress = Some(t.progress.unwrap_or(0.0) + 1.0)
                    })
                    .unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(app.store.task("updates").unwrap().progress, Some(80.0));
}

#[test]
fn corrupt_commit_is_quarantined_while_valid_commit_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().join("app")).unwrap();
    let avd_root = dir.path().join("avd");
    let data = avd_root.join("fixture.avd");
    std::fs::create_dir_all(&data).unwrap();
    let mut settings = app.store.settings();
    settings.avd_roots = vec![avd_root.to_string_lossy().into()];
    app.store.save_settings(&settings).unwrap();
    let mut create = task("owned-create", "needs_review");
    create.operation.kind = "avd_create".into();
    create
        .operation
        .args
        .insert("name".into(), "fixture".into());
    app.store.save_task(&create).unwrap();
    std::fs::write(data.join(".devkit-owned"), &create.id).unwrap();
    std::fs::write(data.join("config.ini"), "target=android-35\n").unwrap();
    let ini = avd_root.join("fixture.ini");
    let valid = app.data.join("avd-commit-valid.json");
    write_commit_journal(
        &valid,
        &serde_json::json!({"data":data,"ini":ini,"taskId":create.id}),
    )
    .unwrap();
    let broken = app.data.join("avd-commit-broken.json");
    let bytes = b"{\"taskId\":";
    std::fs::write(&broken, bytes).unwrap();
    // An interrupted atomic write is not a committed journal.
    std::fs::write(app.data.join("avd-commit-staged.tmp"), bytes).unwrap();
    app.recover_commits().unwrap();
    app.recover_commits().unwrap();
    assert!(!broken.exists());
    assert!(!valid.exists());
    assert!(std::fs::read_to_string(&ini)
        .unwrap()
        .contains("target=android-35"));
    let preserved: Vec<_> = std::fs::read_dir(app.data.join("recovery"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|s| s == "invalid"))
        .collect();
    assert_eq!(preserved.len(), 1);
    assert_eq!(std::fs::read(preserved[0].path()).unwrap(), bytes);
    assert!(std::fs::read_dir(app.data.join("recovery"))
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().ends_with(".error.json")));
}

#[test]
fn recording_recovery_exports_parts_and_already_renamed_segments() {
    for (renamed, state) in [
        (false, "recording"),
        (true, "recording"),
        (true, "interrupted"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(dir.path().into()).unwrap();
        let root = app.data.join("recordings/crash");
        let mut writer =
            Recorder::new(&root, "crash", target(1, 1), Default::default(), 1024, 512).unwrap();
        writer.line("valuable captured log").unwrap();
        writer.checkpoint().unwrap();
        drop(writer);
        if renamed {
            std::fs::rename(
                root.join("segment-0001.log.part"),
                root.join("segment-0001.log"),
            )
            .unwrap();
        }
        if state == "interrupted" {
            let mut m: Manifest =
                serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap())
                    .unwrap();
            m.state = state.into();
            m.gaps = 1;
            crate::recording::save_manifest(&root, &m).unwrap();
        }
        app.store.save_task(&task("crash", "needs_review")).unwrap();
        app.recover_recordings().unwrap();
        let recovered = std::fs::read(root.join("manifest.json")).unwrap();
        app.recover_recordings().unwrap();
        assert_eq!(
            std::fs::read(root.join("manifest.json")).unwrap(),
            recovered
        );
        let m: Manifest = serde_json::from_slice(&recovered).unwrap();
        assert_eq!(m.files[0].name, "segment-0001.log");
        assert_eq!(
            m.files[0].sha256,
            digest_file(&root.join("segment-0001.log")).unwrap()
        );
        assert_eq!(m.files[0].bytes, "valuable captured log\n".len() as u64);
        assert_eq!(m.bytes, m.files[0].bytes);
        assert_eq!(m.gaps, 1);
        assert!(app
            .export_recording("crash", &[0], true)
            .unwrap()
            .text
            .contains("valuable captured log"));
    }
}

#[test]
fn ambiguous_recording_recovery_preserves_both_files_and_starts_normally() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().into()).unwrap();
    let root = app.data.join("recordings/conflict");
    let mut writer = Recorder::new(
        &root,
        "conflict",
        target(1, 1),
        Default::default(),
        1024,
        512,
    )
    .unwrap();
    writer.line("part data").unwrap();
    writer.checkpoint().unwrap();
    drop(writer);
    std::fs::write(root.join("segment-0001.log"), "sealed data").unwrap();
    app.store
        .save_task(&task("conflict", "needs_review"))
        .unwrap();
    app.recover_recordings().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("segment-0001.log.part")).unwrap(),
        "part data\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("segment-0001.log")).unwrap(),
        "sealed data"
    );
    assert!(app
        .store
        .task("conflict")
        .unwrap()
        .message
        .contains("RECOVERY_CONFLICT"));
}

#[test]
fn log_cache_is_bounded_and_protects_consumers_until_release() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().into()).unwrap();
    app.epoch.store(1, Ordering::SeqCst);
    for transport in 1..=50 {
        let t = target(1, transport);
        app.logs
            .lock()
            .unwrap()
            .entry(observe::key(&t))
            .or_default()
            .push("cached log");
        app.log_release(&t);
        assert!(app.logs.lock().unwrap().len() <= 4);
    }
    let viewed = target(2, 101);
    let collecting = target(2, 102);
    let recording = target(2, 103);
    for t in [&viewed, &collecting, &recording] {
        app.logs
            .lock()
            .unwrap()
            .entry(observe::key(t))
            .or_default()
            .push("protected log");
    }
    app.log_views.lock().unwrap().insert(observe::key(&viewed));
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    app.log_children
        .lock()
        .unwrap()
        .insert(observe::key(&collecting), tx);
    let mut active = task("recording", "running");
    active.operation.target = Some(recording.clone());
    app.store.save_task(&active).unwrap();
    app.prune_logs();
    for t in [&viewed, &collecting, &recording] {
        assert!(app.logs.lock().unwrap().contains_key(&observe::key(t)));
    }
    app.log_release(&viewed);
    app.log_children
        .lock()
        .unwrap()
        .remove(&observe::key(&collecting));
    app.update("recording", |t| t.state = "succeeded".into())
        .unwrap();
    app.prune_logs();
    for t in [&viewed, &collecting, &recording] {
        assert!(!app.logs.lock().unwrap().contains_key(&observe::key(t)));
    }
    assert!(app.logs.lock().unwrap().len() <= 4);
}

#[test]
fn rejected_cancel_write_is_reported_and_does_not_leave_exit_draining() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().into()).unwrap();
    app.store
        .save_task(&task("cancel-fault", "running"))
        .unwrap();
    let database = rusqlite::Connection::open(app.data.join("state/devkit.db")).unwrap();
    database.execute_batch("CREATE TRIGGER reject_task_update BEFORE UPDATE ON tasks BEGIN SELECT RAISE(FAIL, 'injected persistence failure'); END;").unwrap();
    assert_eq!(
        app.cancel_task("cancel-fault").unwrap_err().code,
        "DATABASE_ERROR"
    );
    assert!(!app.cancelled("cancel-fault"));
    assert_eq!(
        app.resolve_close("cancel", false, true, &[])
            .unwrap_err()
            .code,
        "DATABASE_ERROR"
    );
    assert!(!app.draining.load(Ordering::SeqCst));
    assert_eq!(
        app.update("cancel-fault", |t| t.state = "succeeded".into())
            .unwrap_err()
            .code,
        "DATABASE_ERROR"
    );
    assert_eq!(app.store.task("cancel-fault").unwrap().state, "running");
}

fn avd_commit_fixture() -> (
    tempfile::TempDir,
    std::sync::Arc<App>,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().join("app")).unwrap();
    let avd_root = dir.path().join("avds");
    let data = avd_root.join("fixture.avd");
    let image = dir.path().join("image");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir(&image).unwrap();
    std::fs::write(image.join("system.img"), b"fixture").unwrap();
    let mut settings = app.store.settings();
    settings.sdk_root = dir.path().join("sdk").to_string_lossy().into();
    settings.avd_roots = vec![avd_root.to_string_lossy().into()];
    app.store.save_settings(&settings).unwrap();
    let mut create = task("create-index", "needs_review");
    create.operation.kind = "avd_create".into();
    create
        .operation
        .args
        .insert("name".into(), "fixture".into());
    create.operation.args.insert(
        "image".into(),
        "system-images;android-35;google_apis;x86_64".into(),
    );
    app.store.save_task(&create).unwrap();
    std::fs::write(data.join(".devkit-owned"), &create.id).unwrap();
    // The registry API target also has to be recoverable when config.ini has none.
    std::fs::write(
        data.join("config.ini"),
        format!("image.sysdir.1={}\n", image.display()),
    )
    .unwrap();
    let ini = avd_root.join("fixture.ini");
    write_commit_journal(
        &app.data.join("avd-commit-index.json"),
        &serde_json::json!({"data":data,"ini":ini,"taskId":create.id}),
    )
    .unwrap();
    (dir, app, ini, data)
}

#[test]
fn truncated_owned_index_is_backed_up_repaired_and_verified() {
    for cut in [0, 20, 50] {
        let (_dir, app, ini, data) = avd_commit_fixture();
        let full = format!(
            "avd.ini.encoding=UTF-8\npath={}\ntarget=android-35\n",
            data.display()
        );
        let partial = &full.as_bytes()[..cut];
        std::fs::write(&ini, partial).unwrap();
        app.recover_commits().unwrap();
        app.recover_commits().unwrap();
        assert_eq!(std::fs::read(&ini).unwrap(), full.as_bytes());
        assert!(!app.data.join("avd-commit-index.json").exists());
        let backups: Vec<_> = std::fs::read_dir(ini.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("fixture.ini.devkit-interrupted-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(backups[0].path()).unwrap(), partial);
        assert!(crate::android::avds(&app.store.settings())[0]
            .error
            .is_empty());
    }
}

#[test]
fn conflicting_index_keeps_original_and_commit_evidence() {
    let (_dir, app, ini, _data) = avd_commit_fixture();
    let external = b"avd.ini.encoding=UTF-8\npath=C:\\other\\external.avd\ntarget=android-34\n";
    std::fs::write(&ini, external).unwrap();
    app.recover_commits().unwrap();
    assert_eq!(std::fs::read(ini).unwrap(), external);
    assert!(app.data.join("avd-commit-index.json").is_file());
    let diagnostics: Vec<_> = std::fs::read_dir(app.data.join("recovery"))
        .unwrap()
        .flatten()
        .collect();
    assert!(diagnostics.iter().any(|e| std::fs::read_to_string(e.path())
        .unwrap()
        .contains("AVD_INDEX_CONFLICT")));
}

#[test]
fn valid_index_preserves_unknown_fields_and_atomic_create_never_clobbers() {
    let (_dir, app, ini, data) = avd_commit_fixture();
    crate::recovery::create_avd_index(&ini, &data, "android-35").unwrap();
    let mut bytes = std::fs::read(&ini).unwrap();
    bytes.extend_from_slice(b"external.setting=kept\n");
    std::fs::write(&ini, &bytes).unwrap();
    assert!(crate::recovery::create_avd_index(&ini, &data, "android-36").is_err());
    app.recover_commits().unwrap();
    assert_eq!(std::fs::read(&ini).unwrap(), bytes);
    assert!(!app.data.join("avd-commit-index.json").exists());
    assert!(!std::fs::read_dir(ini.parent().unwrap())
        .unwrap()
        .flatten()
        .any(|e| e.path().extension().is_some_and(|s| s == "tmp")));
}

#[test]
fn avd_commit_conflict_stays_pending_until_recovery_verifies_index() {
    let (_dir, app, ini, data) = avd_commit_fixture();
    std::fs::write(
        &ini,
        format!(
            "avd.ini.encoding=UTF-8\npath={}\ntarget=android-34\n",
            data.display()
        ),
    )
    .unwrap();
    app.recover_commits().unwrap();
    let avds = crate::android::avds(&app.store.settings());
    assert!(avds[0].error.is_empty());
    app.reconcile(&[], &avds, &[]).unwrap();
    assert_eq!(
        app.store.task("create-index").unwrap().state,
        "needs_review"
    );
    assert!(app
        .store
        .task("create-index")
        .unwrap()
        .message
        .contains("提交记录尚未恢复"));
    std::fs::write(
        &ini,
        format!(
            "avd.ini.encoding=UTF-8\npath={}\ntarget=android-35\n",
            data.display()
        ),
    )
    .unwrap();
    app.reconcile(&[], &crate::android::avds(&app.store.settings()), &[])
        .unwrap();
    assert_eq!(
        app.store.task("create-index").unwrap().state,
        "needs_review"
    );
    app.recover_commits().unwrap();
    app.reconcile(&[], &crate::android::avds(&app.store.settings()), &[])
        .unwrap();
    assert_eq!(app.store.task("create-index").unwrap().state, "succeeded");
}

#[test]
fn avd_reconcile_validates_index_even_without_a_journal() {
    let (_dir, app, ini, data) = avd_commit_fixture();
    std::fs::remove_file(app.data.join("avd-commit-index.json")).unwrap();
    std::fs::write(
        &ini,
        format!(
            "avd.ini.encoding=UTF-8\npath={}\ntarget=android-34\n",
            data.display()
        ),
    )
    .unwrap();
    app.reconcile(&[], &crate::android::avds(&app.store.settings()), &[])
        .unwrap();
    assert_eq!(
        app.store.task("create-index").unwrap().state,
        "needs_review"
    );
    std::fs::write(
        &ini,
        format!(
            "avd.ini.encoding=UTF-8\npath={}\ntarget=android-35\n",
            data.display()
        ),
    )
    .unwrap();
    app.reconcile(&[], &crate::android::avds(&app.store.settings()), &[])
        .unwrap();
    assert_eq!(app.store.task("create-index").unwrap().state, "succeeded");
}

#[test]
fn recovery_preserves_committed_segment_integrity_until_content_is_restored() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(dir.path().into()).unwrap();
    let root = app.data.join("recordings/integrity");
    let mut writer = Recorder::new(
        &root,
        "integrity",
        target(1, 1),
        Default::default(),
        8192,
        64,
    )
    .unwrap();
    writer
        .line("original recorded line before rotation")
        .unwrap();
    writer
        .line("next recorded line triggering rotation")
        .unwrap();
    writer.checkpoint().unwrap();
    drop(writer);
    app.store
        .save_task(&task("integrity", "needs_review"))
        .unwrap();
    let path = root.join("manifest.json");
    let original_manifest = std::fs::read(&path).unwrap();
    let manifest: Manifest = serde_json::from_slice(&original_manifest).unwrap();
    let sealed = root.join(&manifest.files[0].name);
    let original = std::fs::read(&sealed).unwrap();
    let mut modified = original.clone();
    modified[0] = b'X';
    std::fs::write(&sealed, modified).unwrap();
    app.recover_recordings().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original_manifest);
    assert!(root.join(&manifest.files[1].name).is_file());
    assert!(app
        .store
        .task("integrity")
        .unwrap()
        .message
        .contains("ARTIFACT_CHANGED"));
    assert_eq!(
        app.export_recording("integrity", &[0], false)
            .unwrap_err()
            .code,
        "ARTIFACT_CHANGED"
    );
    std::fs::write(&sealed, original).unwrap();
    app.recover_recordings().unwrap();
    let recovered: Manifest = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(recovered.files[0].sha256, manifest.files[0].sha256);
    assert_eq!(recovered.state, "interrupted");
    let exported = app.export_recording("integrity", &[0, 1], false).unwrap();
    assert!(exported.text.contains("original recorded line"));
    assert!(exported.text.contains("next recorded line"));
}
