//! Real tools, generated disposable keys, fixture APK only. Never uses release credentials.
use crate::{app::App, domain::*, signing::*, signing_tasks};
use base64::Engine;
use std::{path::Path, sync::Arc, time::Duration};

async fn wait(app: &App, id: &str) -> Task {
    for _ in 0..1200 {
        let task = app.store.task(id).unwrap();
        if !task.active() {
            return task;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("Signing fixture timed out");
}
fn secret(store: &str, key: &str) -> Passwords {
    Passwords {
        store_password: store.into(),
        key_password: key.into(),
    }
}
async fn submit(app: &Arc<App>, request: SigningRequest, passwords: Passwords) -> Task {
    let plan = signing_tasks::prepare(app, request, passwords)
        .await
        .unwrap();
    let request_id = uuid::Uuid::new_v4().to_string();
    assert!(signing_tasks::submit(app, &plan.id, &plan.hash, "wrong", &request_id).is_err());
    let first = signing_tasks::submit(
        app,
        &plan.id,
        &plan.hash,
        &plan.confirmation_token,
        &request_id,
    )
    .unwrap();
    let second = signing_tasks::submit(
        app,
        &plan.id,
        &plan.hash,
        &plan.confirmation_token,
        &request_id,
    )
    .unwrap();
    assert_eq!(first.id, second.id);
    let task = wait(app, &first.id).await;
    assert_eq!(
        task.state,
        "succeeded",
        "{} {} {:?} {}",
        task.title,
        task.message,
        task.log,
        task.operation.optional("request")
    );
    task
}
#[tokio::test]
#[ignore = "explicit local Java/SDK/fixture APK; generates disposable signing keys"]
async fn signing_real_tools_roundtrip_and_recovery() {
    let java = std::env::var("ANDROID_DEVKIT_SIGNING_TEST_JAVA").expect("Set signing test Java");
    let sdk = std::env::var("ANDROID_DEVKIT_SIGNING_TEST_SDK").expect("Set signing test SDK");
    let fixture =
        std::env::var("ANDROID_DEVKIT_SIGNING_TEST_APK").expect("Set signing fixture APK");
    let root = tempfile::tempdir().unwrap();
    let app = App::new(root.path().join("工作空间 with spaces")).unwrap();
    app.store
        .save_settings(&Settings {
            java_path: java,
            sdk_root: sdk,
            avd_roots: vec![root.path().join("avd").to_string_lossy().into()],
            boot_timeout_secs: 180,
            ..Settings::default()
        })
        .unwrap();
    let password = "Disposable-fixture-9382!";
    let unicode_password = "临时口令-Only-9382!";
    let key_password = "Different-private-7319!";
    let key = root.path().join("密钥 with spaces.p12");
    let first = submit(
        &app,
        SigningRequest {
            mode: "create".into(),
            name: "Disposable upload".into(),
            format: "PKCS12".into(),
            alias: "upload".into(),
            subject: "CN=Disposable Test, O=DevKit, C=CN".into(),
            output: key.to_string_lossy().into(),
            ..Default::default()
        },
        secret(password, ""),
    )
    .await;
    assert!(key.exists());
    let hash = digest_file(&key).unwrap();
    let record = app.store.signing_records().unwrap().remove(0);
    assert_eq!(record.entries[0].alias, "upload");
    assert_eq!(record.format, "PKCS12");
    assert_eq!(record.sha256, hash);
    assert!(inspect(&app, &key, &secret("incorrect", ""), None)
        .await
        .is_err());
    let info = inspect(&app, &key, &secret(password, ""), Some("upload"))
        .await
        .unwrap();
    assert_eq!(info.entries[0].certificate.bits, 3072);
    let cert = &info.entries[0].certificate;
    assert_eq!(cert.sha256.len(), 64);
    assert_eq!(cert.sha1.len(), 40);
    let certificate = root.path().join("public.cer");
    std::fs::write(
        &certificate,
        base64::engine::general_purpose::STANDARD
            .decode(&cert.der)
            .unwrap(),
    )
    .unwrap();
    let decoded: CertificateInfo = bridge(
        &app,
        None,
        "certificate",
        &certificate,
        "",
        zeroize::Zeroizing::new(vec![]),
    )
    .await
    .unwrap();
    assert_eq!(decoded.sha256, cert.sha256);
    let duplicate = register(
        &app,
        KeystoreRecord {
            name: "duplicate".into(),
            path: key.to_string_lossy().into(),
            role: "upload".into(),
            ..Default::default()
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(duplicate.id, record.id);
    let unsigned = inspect_apk(&app, Path::new(&fixture)).await.unwrap();
    assert!(!unsigned.verified);
    assert!(unsigned.standalone);
    assert!(unsigned.unsigned, "{:?}", unsigned.errors);
    let original_hash = digest_file(Path::new(&fixture)).unwrap();
    let output = root.path().join("签名 output.apk");
    let signed = submit(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: fixture.clone(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            output: output.to_string_lossy().into(),
            ..Default::default()
        },
        secret(password, ""),
    )
    .await;
    let verification = inspect_apk(&app, &output).await.unwrap();
    assert!(verification.verified);
    assert_eq!(verification.signers[0].sha256, cert.sha256);
    assert!(verification.schemes.contains(&"v2".into()));
    assert!(!output.with_extension("apk.idsig").exists());
    assert_eq!(digest_file(Path::new(&fixture)).unwrap(), original_hash);
    assert_eq!(digest_file(&key).unwrap(), hash);
    // Independent official CLI verification, outside the bridge's report parser.
    let env = environment(&app);
    let verified = crate::process::run(crate::process::Spec::new(
        &env.java,
        vec![
            "-jar".into(),
            env.apksigner.clone(),
            "verify".into(),
            "--verbose".into(),
            "--print-certs".into(),
            output.to_string_lossy().into(),
        ],
    ))
    .await
    .unwrap();
    assert_eq!(verified.code, 0, "{}", verified.text());
    assert!(verified.text().contains(&cert.sha256));
    if std::env::var("ANDROID_DEVKIT_SIGNING_TEST_INSTALL").as_deref() == Ok("1") {
        install_on_disposable_avd(&app, &output).await;
    }
    assert!(signing_tasks::prepare(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: output.to_string_lossy().into(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            output: root
                .path()
                .join("should-not-exist.apk")
                .to_string_lossy()
                .into(),
            ..Default::default()
        },
        secret(password, "")
    )
    .await
    .is_err());
    let resigned = root.path().join("resigned.apk");
    submit(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: output.to_string_lossy().into(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            output: resigned.to_string_lossy().into(),
            replace_signature: true,
            ..Default::default()
        },
        secret(password, ""),
    )
    .await;
    // A selected existing output is replaced after verification, without touching the input.
    let existing = root.path().join("existing-output.apk");
    std::fs::write(&existing, b"previous output").unwrap();
    let replaced_output = submit(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: fixture.clone(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            output: existing.to_string_lossy().into(),
            ..Default::default()
        },
        secret(password, ""),
    )
    .await;
    assert!(inspect_apk(&app, &existing).await.unwrap().verified);
    assert_eq!(
        replaced_output.operation.optional("overwriteOutput"),
        "true"
    );
    assert_eq!(
        replaced_output.operation.optional("sourceBackupResolved"),
        "true"
    );
    assert_eq!(digest_file(Path::new(&fixture)).unwrap(), original_hash);
    // Saving a cached signed result can also replace an empty existing destination.
    let cached_output = app
        .store
        .artifacts()
        .unwrap()
        .into_iter()
        .find(|artifact| artifact.task_id == replaced_output.id)
        .unwrap();
    let empty = root.path().join("existing-empty.apk");
    std::fs::write(&empty, b"").unwrap();
    signing_tasks::save_result_unlocked(&app, &cached_output.id, &empty).unwrap();
    assert_eq!(digest_file(&empty).unwrap(), cached_output.sha256);
    assert!(inspect_apk(&app, &empty).await.unwrap().verified);
    // Editing the destination after confirmation must not clobber the newer contents.
    let plan = signing_tasks::prepare(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: fixture.clone(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            output: existing.to_string_lossy().into(),
            ..Default::default()
        },
        secret(password, ""),
    )
    .await
    .unwrap();
    std::fs::write(&existing, b"external output edit").unwrap();
    let conflict = signing_tasks::submit(
        &app,
        &plan.id,
        &plan.hash,
        &plan.confirmation_token,
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    assert_ne!(wait(&app, &conflict.id).await.state, "succeeded");
    assert_eq!(std::fs::read(&existing).unwrap(), b"external output edit");
    let retained = app
        .store
        .artifacts()
        .unwrap()
        .into_iter()
        .find(|artifact| artifact.task_id == conflict.id)
        .unwrap();
    signing_tasks::save_result_unlocked(
        &app,
        &retained.id,
        &root.path().join("recovered-output.apk"),
    )
    .unwrap();
    let jks = root.path().join("双密码.jks");
    // Overwriting is opt-in, skips output selection, and never touches the fixture.
    let overwrite = root.path().join("覆盖 源文件.apk");
    std::fs::copy(&fixture, &overwrite).unwrap();
    let replaced = submit(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: overwrite.to_string_lossy().into(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            overwrite_source: true,
            ..Default::default()
        },
        secret(password, ""),
    )
    .await;
    assert!(inspect_apk(&app, &overwrite).await.unwrap().verified);
    assert_eq!(replaced.operation.optional("sourceBackupResolved"), "true");
    assert_eq!(
        std::fs::canonicalize(replaced.operation.optional("savedOutput")).unwrap(),
        std::fs::canonicalize(&overwrite).unwrap()
    );
    assert_eq!(digest_file(Path::new(&fixture)).unwrap(), original_hash);
    submit(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: overwrite.to_string_lossy().into(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            overwrite_source: true,
            replace_signature: true,
            ..Default::default()
        },
        secret(password, ""),
    )
    .await;
    let changed = root.path().join("changed-after-plan.apk");
    std::fs::copy(&fixture, &changed).unwrap();
    let plan = signing_tasks::prepare(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: changed.to_string_lossy().into(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            overwrite_source: true,
            ..Default::default()
        },
        secret(password, ""),
    )
    .await
    .unwrap();
    std::fs::write(&changed, b"external edit after confirmation").unwrap();
    let conflict = signing_tasks::submit(
        &app,
        &plan.id,
        &plan.hash,
        &plan.confirmation_token,
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    assert_ne!(wait(&app, &conflict.id).await.state, "succeeded");
    assert_eq!(
        std::fs::read(&changed).unwrap(),
        b"external edit after confirmation"
    );
    assert!(std::fs::read_dir(root.path()).unwrap().flatten().all(|f| !f
        .file_name()
        .to_string_lossy()
        .starts_with(".devkit-signing-")));
    submit(
        &app,
        SigningRequest {
            mode: "create".into(),
            name: "Unicode JKS".into(),
            format: "JKS".into(),
            alias: "中文 alias".into(),
            subject: "CN=Unicode Test".into(),
            output: jks.to_string_lossy().into(),
            ..Default::default()
        },
        secret(unicode_password, key_password),
    )
    .await;
    let jks_info = inspect(
        &app,
        &jks,
        &secret(unicode_password, key_password),
        Some("中文 alias"),
    )
    .await
    .unwrap();
    assert_eq!(jks_info.format, "JKS");
    let strict: KeystoreInfo = bridge(
        &app,
        None,
        "strict-keycheck",
        &jks,
        "中文 alias",
        secret(unicode_password, key_password).bridge_input(true),
    )
    .await
    .unwrap();
    assert_eq!(
        strict.entries[0].certificate.sha256,
        jks_info.entries[0].certificate.sha256
    );
    assert!(inspect(
        &app,
        &jks,
        &secret(unicode_password, "wrong-key"),
        Some("中文 alias")
    )
    .await
    .is_err());
    let jks_record = app
        .store
        .signing_records()
        .unwrap()
        .into_iter()
        .find(|r| r.path.ends_with("双密码.jks"))
        .unwrap();
    submit(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: fixture.clone(),
            keystore_id: jks_record.id.clone(),
            alias: "中文 alias".into(),
            output: root
                .path()
                .join("unicode-password.apk")
                .to_string_lossy()
                .into(),
            ..Default::default()
        },
        secret(unicode_password, key_password),
    )
    .await;
    let corrupt = root.path().join("tampered.apk");
    std::fs::copy(&output, &corrupt).unwrap();
    {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new_append(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&corrupt)
                .unwrap(),
        )
        .unwrap();
        zip.start_file(
            "unsigned-extra.txt",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"tampered fixture").unwrap();
        zip.finish().unwrap();
    }
    let invalid = inspect_apk(&app, &corrupt).await.unwrap();
    assert!(!invalid.verified);
    assert!(!invalid.unsigned);
    assert!(signing_tasks::prepare(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: corrupt.to_string_lossy().into(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            output: root.path().join("must-refuse.apk").to_string_lossy().into(),
            replace_signature: true,
            ..Default::default()
        },
        secret(password, "")
    )
    .await
    .is_err());
    // Two signers are readable but cannot enter the single-key re-signing path.
    let second_key = root.path().join("second.p12");
    submit(
        &app,
        SigningRequest {
            mode: "create".into(),
            name: "Second test key".into(),
            format: "PKCS12".into(),
            alias: "second".into(),
            subject: "CN=Second Test".into(),
            output: second_key.to_string_lossy().into(),
            ..Default::default()
        },
        secret(password, ""),
    )
    .await;
    let env = environment(&app);
    let multiple = root.path().join("multiple-signers.apk");
    let mut args: Vec<String> = vec![
        "-jar".into(),
        env.apksigner.clone(),
        "sign".into(),
        "--v3-signing-enabled".into(),
        "false".into(),
        "--v4-signing-enabled".into(),
        "false".into(),
        "--out".into(),
        multiple.to_string_lossy().into(),
    ];
    for (index, (store, alias)) in [(&key, "upload"), (&second_key, "second")]
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            args.push("--next-signer".into());
        }
        args.extend([
            "--v1-signer-name".into(),
            format!("DEVKIT{index}"),
            "--ks".into(),
            store.to_string_lossy().into(),
            "--ks-key-alias".into(),
            alias.into(),
            "--ks-pass".into(),
            "stdin".into(),
            "--key-pass".into(),
            "stdin".into(),
        ]);
    }
    args.push(fixture.clone());
    let mut input = secret(password, "").lines(false, "");
    input.extend_from_slice(&secret(password, "").lines(false, ""));
    let multi_result =
        crate::process::run_sensitive(crate::process::Spec::new(&env.java, args), input)
            .await
            .unwrap();
    assert_eq!(multi_result.code, 0);
    let multi_report = inspect_apk(&app, &multiple).await.unwrap();
    assert!(multi_report.verified);
    assert_eq!(multi_report.signers.len(), 2);
    assert!(signing_tasks::prepare(
        &app,
        SigningRequest {
            mode: "sign".into(),
            input: multiple.to_string_lossy().into(),
            keystore_id: record.id.clone(),
            alias: "upload".into(),
            output: root
                .path()
                .join("must-refuse-multiple.apk")
                .to_string_lossy()
                .into(),
            replace_signature: true,
            ..Default::default()
        },
        secret(password, "")
    )
    .await
    .is_err());
    // Persistence never serializes either password, including request IDs and retry records.
    let persisted = serde_json::to_string(&app.store.tasks().unwrap()).unwrap();
    for value in [password, unicode_password, key_password] {
        assert!(!persisted.contains(value));
    }
    let database = std::fs::read(app.data.join("state/devkit.db")).unwrap();
    for value in [password, unicode_password, key_password] {
        assert!(!database.windows(value.len()).any(|v| v == value.as_bytes()));
    }
    // Input snapshots are removed after every job. Only non-sensitive journals remain.
    assert!(std::fs::read_dir(app.data.join("signing-jobs"))
        .unwrap()
        .flatten()
        .all(|file| file.path().extension().is_some_and(|v| v == "json")));
    assert!(app
        .store
        .artifacts()
        .unwrap()
        .iter()
        .all(|a| a.acknowledged));
    app.store.forget_signing_record(&record.id).unwrap();
    assert!(key.exists());
    assert_eq!(digest_file(&key).unwrap(), hash);
    // Re-opening the same state restores public records and does not replay writes.
    let reopened = App::new(app.data.clone()).unwrap();
    signing_tasks::recover(&reopened).unwrap();
    assert!(!reopened
        .store
        .signing_records()
        .unwrap()
        .iter()
        .any(|r| r.id == record.id));
    assert!(reopened
        .store
        .task(&first.id)
        .unwrap()
        .operation
        .optional("savedOutput")
        .ends_with(".p12"));
    assert_eq!(reopened.store.task(&signed.id).unwrap().state, "succeeded");
    // Clearing an acknowledged encrypted copy preserves the exported file and record.
    let cached = reopened
        .store
        .artifacts()
        .unwrap()
        .into_iter()
        .find(|a| a.task_id == first.id)
        .unwrap();
    signing_tasks::discard_result(&reopened, &cached.id, &cached.sha256).unwrap();
    assert_eq!(digest_file(&key).unwrap(), hash);
    assert!(reopened
        .store
        .task(&first.id)
        .unwrap()
        .operation
        .optional("savedOutput")
        .ends_with(".p12"));
    // Recovery of an interrupted generated key is tested at the journal boundary below.
    signing_tasks::test_generated_key_recovery(&reopened, &key, password).await;
}

// Opt-in end-to-end check. The dedicated AVD lives only under this test's temp root.
async fn install_on_disposable_avd(app: &Arc<App>, apk_path: &Path) {
    use std::{collections::BTreeMap, sync::atomic::Ordering};
    async fn execute(
        app: &Arc<App>,
        kind: &str,
        pairs: &[(&str, &str)],
        target: Option<Target>,
    ) -> Task {
        let args: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
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
        for _ in 0..2400 {
            let task = app.store.task(&task.id).unwrap();
            if !task.active() {
                assert_eq!(
                    task.state, "succeeded",
                    "{kind}: {} {:?}",
                    task.message, task.log
                );
                return task;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("Disposable AVD task timed out: {kind}");
    }
    let settings = app.store.settings();
    let name = format!("DevKit_Signing_{}", uuid::Uuid::new_v4().simple());
    execute(
        app,
        "avd_create",
        &[
            ("name", &name),
            ("displayName", "Disposable signing QA"),
            ("profile", "pixel_2"),
            ("image", "system-images;android-29;default;x86_64"),
            ("memory", "1536"),
            ("storage", "2"),
            ("gpu", "auto"),
            ("cpuCores", "2"),
            ("skinMode", "none"),
        ],
        None,
    )
    .await;
    let avd = crate::android::avds(&settings)
        .into_iter()
        .find(|a| a.name == name)
        .unwrap();
    assert!(Path::new(&avd.data_path).starts_with(Path::new(&settings.avd_roots[0])));
    let log = std::fs::File::create(app.data.join("signing-emulator.log")).unwrap();
    let mut spec = crate::process::Spec::new(
        Path::new(&settings.sdk_root).join("emulator/emulator.exe"),
        vec![
            "-avd".into(),
            name.clone(),
            "-no-window".into(),
            "-no-snapshot".into(),
            "-no-audio".into(),
            "-gpu".into(),
            "host".into(),
        ],
    );
    spec.env
        .insert("ANDROID_AVD_HOME".into(), settings.avd_roots[0].clone());
    spec.env
        .insert("ANDROID_HOME".into(), settings.sdk_root.clone());
    let mut command = spec.command();
    command
        .stdout(std::process::Stdio::from(log.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(log))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    let owned_app = app.clone();
    let owned_id = avd.id.clone();
    let owned_name = name.clone();
    let owned_settings = settings.clone();
    let owned_apk = apk_path.to_path_buf();
    let result = tokio::spawn(async move {
        execute(&owned_app, "avd_wait", &[("avdId", &owned_id)], None).await;
        let devices =
            crate::android::devices(&owned_settings, owned_app.epoch.load(Ordering::SeqCst)).await;
        let device = devices
            .iter()
            .find(|d| d.avd_name == owned_name && d.state == "ready")
            .unwrap();
        let target = Target::from(device);
        let apk = owned_app.prepare_apk(&owned_apk).unwrap();
        let task = execute(
            &owned_app,
            "apk_install",
            &[("apkId", &apk.id)],
            Some(target.clone()),
        )
        .await;
        assert_eq!(task.operation.target, Some(target.clone()));
        let installed = owned_app
            .remote_shell(
                &owned_settings,
                &target,
                format!("pm path {}", apk.package),
                15,
            )
            .await
            .unwrap();
        assert!(installed.text().contains("package:"));
        println!(
            "Signing APK installed through existing frozen-target workflow: {} / {}",
            target.serial, apk.package
        );
    })
    .await;
    // The launcher may exit before its QEMU child. Stop via the exact owned AVD first.
    if crate::app::avd_locked(Path::new(&avd.data_path)) {
        execute(app, "avd_stop", &[("avdId", &avd.id)], None).await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
    for _ in 0..100 {
        if !crate::app::avd_locked(Path::new(&avd.data_path)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !crate::app::avd_locked(Path::new(&avd.data_path)),
        "Owned AVD did not release its files"
    );
    execute(app, "avd_delete", &[("avdId", &avd.id)], None).await;
    result.unwrap();
}
