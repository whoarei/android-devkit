use super::*;

#[test]
fn discovery_handles_types_ipv6_duplicates_and_malformed_rows() {
    let services = parse_services(
        "List of discovered mdns services\n\
        adb-one _adb-tls-pairing._tcp 192.0.2.10:37123\n\
        adb-one _adb-tls-pairing._tcp 192.0.2.10:37123\n\
        adb-two._adb-tls-connect._tcp.local. _adb-tls-connect._tcp. [2001:db8::1]:40123\n\
        adb-old _adb._tcp 192.0.2.10:5555\n\
        malformed _adb-tls-pairing._tcp invalid:0\n\
        studio-current _adb-tls-pairing._tcp 192.0.2.11:33333\n",
    );
    assert_eq!(services.len(), 3);
    assert_eq!(services[1].name, "adb-two");
    assert_eq!(services[1].address, "[2001:db8::1]:40123");
    assert!(validate_code_service("studio-another-workstation").is_err());
    assert!(validate_code_service("adb-name\nother").is_err());
    assert!(validate_code_service("adb-phone (2)").is_ok());
    assert!(validate_code_service("adb-phone (evil)").is_err());
    assert_eq!(
        parse_services("adb-phone (2) _adb-tls-connect._tcp 192.0.2.1:12345")[0].name,
        "adb-phone (2)"
    );
    assert_eq!(
        normalize_disconnect("adb-one._adb-tls-connect._tcp").unwrap(),
        "adb-one._adb-tls-connect._tcp"
    );
    for address in [
        "",
        "--all",
        "adb-one",
        "adb-one._adb-tls-pairing._tcp",
        "adb-one._adb-tls-connect._tcp:extra",
    ] {
        assert!(normalize_disconnect(address).is_err());
    }
}

#[test]
fn connection_identity_does_not_accept_an_unrelated_online_phone() {
    let devices = android::parse_devices(
        "adb-other._adb-tls-connect._tcp device transport_id:1\n\
        adb-target._adb-tls-connect._tcp offline transport_id:2\n",
        1,
    );
    assert!(matching_device(&devices, "adb-target", &HashSet::new()).is_none());
    let devices =
        android::parse_devices("adb-target._adb-tls-connect._tcp device transport_id:3", 1);
    assert_eq!(
        matching_device(&devices, "adb-target", &HashSet::new())
            .unwrap()
            .transport_id,
        "3"
    );
    assert_eq!(
        paired_guid("Enter pairing code: Successfully paired to 192.0.2.1:1234 [guid=adb-target]"),
        Some("adb-target".into())
    );
    assert!(
        paired_guid("Failed: Successfully paired but server returned unknown response=2").is_none()
    );
}

async fn fixture(root: &std::path::Path, binary: &std::path::Path, mode: &str) -> Arc<App> {
    let sdk = root.join("sdk");
    let tools = sdk.join("platform-tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::copy(binary, tools.join("adb.exe")).unwrap();
    // Read-only protocol check; every executed command uses the isolated fixture executable.
    let protocol = android::host_request("host:version")
        .ok()
        .and_then(|raw| u32::from_str_radix(String::from_utf8_lossy(&raw).trim(), 16).ok())
        .unwrap_or(41);
    std::fs::write(tools.join("protocol"), protocol.to_string()).unwrap();
    std::fs::write(tools.join("mode"), mode).unwrap();
    let app = App::new(root.join("data")).unwrap();
    app.wireless.native.disable_for_fixture();
    app.store
        .save_settings(&Settings {
            sdk_root: sdk.to_string_lossy().into(),
            avd_roots: vec![],
            ..Default::default()
        })
        .unwrap();
    app
}

async fn finished(app: &App, id: &str) -> Task {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let task = app.store.task(id).unwrap();
        if !task.active() {
            return task;
        }
        assert!(
            Instant::now() < deadline,
            "task did not finish: {}",
            task.message
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn inactive_connection_ports_are_skipped_before_calling_shared_adb() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    assert!(endpoint_reachable(&address).await);
    drop(listener);
    assert!(!endpoint_reachable(&address).await);
}

#[tokio::test]
async fn expired_qr_drops_its_secret_without_executing_adb() {
    let temp = tempfile::tempdir().unwrap();
    let app = App::new(temp.path().join("data")).unwrap();
    let operation = Operation {
        kind: "wireless_qr_pair".into(),
        target: None,
        args: BTreeMap::from([("sessionId".into(), "expired-session".into())]),
    };
    let task = Task {
        id: "expired-task".into(),
        request_id: "expired-request".into(),
        title: "QR".into(),
        operation: operation.clone(),
        state: "running".into(),
        phase: "preparing".into(),
        message: String::new(),
        log: vec![],
        created_at: now(),
        updated_at: now(),
        cancel_requested: false,
        cancellable: true,
        artifact: String::new(),
        license_prompt: String::new(),
        confirmed_licenses: vec![],
        progress: None,
    };
    app.store.save_task(&task).unwrap();
    app.wireless.sessions.lock().unwrap().insert(
        "expired-session".into(),
        QrSession {
            name: "studio-expired".into(),
            secret: Zeroizing::new("never-use-this-secret".into()),
            deadline: Instant::now() - Duration::from_secs(1),
        },
    );
    assert_eq!(
        app.wireless
            .validate_session("expired-session")
            .unwrap_err()
            .code,
        "QR_EXPIRED"
    );
    assert_eq!(
        app.pair_wireless_qr(&task.id, &operation, &app.store.settings())
            .await
            .unwrap_err()
            .code,
        "QR_EXPIRED"
    );
    assert!(app.wireless.sessions.lock().unwrap().is_empty());
    assert!(!serde_json::to_string(&app.store.task(&task.id).unwrap())
        .unwrap()
        .contains("never-use-this-secret"));
}

#[tokio::test(flavor = "multi_thread")]
async fn wireless_workflows_keep_target_identity_and_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("fixture.exe");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wireless_adb.rs");
    let mut compiler = process::Spec::new(
        "rustc",
        vec![
            source.to_string_lossy().into(),
            "-o".into(),
            binary.to_string_lossy().into(),
            "--edition=2021".into(),
        ],
    );
    compiler.timeout = 60;
    process::run(compiler).await.unwrap().success().unwrap();
    let service = WirelessService {
        name: "adb-fixture-guid".into(),
        service_type: PAIRING.into(),
        address: "192.0.2.10:37123".into(),
    };
    for mode in ["ip", "service", "auto", "renamed"] {
        let app = fixture(&temp.path().join(mode), &binary, mode).await;
        let task = start_code(
            &app,
            service.clone(),
            "123456".into(),
            format!("request-{mode}"),
        )
        .await
        .unwrap();
        let repeated = start_code(
            &app,
            service.clone(),
            "123456".into(),
            format!("request-{mode}"),
        )
        .await
        .unwrap();
        assert_eq!(task.id, repeated.id);
        let task = finished(&app, &task.id).await;
        assert_eq!(task.state, "succeeded", "{}: {}", mode, task.message);
        assert_eq!(task.operation.target.as_ref().unwrap().transport_id, "5");
        let saved = serde_json::to_string(&task).unwrap();
        assert!(!saved.contains("123456"));
        assert!(!saved.contains("\"code\""));
        let calls = std::fs::read_to_string(
            std::path::Path::new(&app.store.settings().sdk_root).join("platform-tools/calls"),
        )
        .unwrap();
        assert!(!calls.contains("123456"));
        assert!(!calls.contains("connect 192.0.2.10:49999"));
        if mode == "auto" {
            assert!(!calls.contains("connect "));
        }
        let mut devices = android::parse_devices(
            "192.0.2.10:40123 device transport_id:5",
            app.epoch.load(Ordering::SeqCst),
        );
        if mode == "ip" {
            app.wireless.classify(&mut devices);
            assert_eq!(devices[0].connection, "wireless");
        }
        assert_eq!(
            app.retry_plan(&task.id).unwrap_err().code,
            "PAIR_CODE_REQUIRED"
        );
        let disconnect = submit_pair(
            &app,
            Operation {
                kind: "wireless_disconnect".into(),
                target: None,
                args: BTreeMap::from([(
                    "address".into(),
                    task.operation.target.as_ref().unwrap().serial.clone(),
                )]),
            },
            &format!("disconnect-{mode}"),
        )
        .unwrap();
        assert_eq!(finished(&app, &disconnect.id).await.state, "succeeded");
    }
    let app = fixture(&temp.path().join("reject"), &binary, "service").await;
    let task = start_code(&app, service.clone(), "000000".into(), "reject".into())
        .await
        .unwrap();
    let task = finished(&app, &task.id).await;
    assert_eq!(task.state, "failed");
    assert!(!serde_json::to_string(&task).unwrap().contains("000000"));

    let app = fixture(&temp.path().join("disconnect"), &binary, "disconnect").await;
    let task = start_code(&app, service.clone(), "123456".into(), "disconnect".into())
        .await
        .unwrap();
    let task = finished(&app, &task.id).await;
    assert_eq!(task.state, "failed");
    assert!(
        task.message.starts_with("PAIRED_NOT_CONNECTED"),
        "{}",
        task.message
    );

    let app = fixture(&temp.path().join("slow"), &binary, "slow").await;
    let task = start_code(&app, service.clone(), "123456".into(), "slow".into())
        .await
        .unwrap();
    let tools = std::path::Path::new(&app.store.settings().sdk_root).join("platform-tools");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !tools.join("pair-started").exists() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    app.cancel_task(&task.id).unwrap();
    assert_eq!(finished(&app, &task.id).await.state, "cancelled");
    assert!(!tools.join("paired").exists());

    let app = fixture(&temp.path().join("qr"), &binary, "service").await;
    let qr = start_qr(&app).await.unwrap();
    assert!(qr.image.starts_with("data:image/svg+xml;base64,"));
    assert_eq!(
        start_qr(&app).await.err().unwrap().code,
        "PAIR_SESSION_ACTIVE"
    );
    let tools = std::path::Path::new(&app.store.settings().sdk_root).join("platform-tools");
    std::fs::write(
        tools.join("qr-name"),
        &qr.task.operation.args["serviceName"],
    )
    .unwrap();
    let task = finished(&app, &qr.task.id).await;
    assert_eq!(task.state, "succeeded", "{}", task.message);
    assert_eq!(
        std::fs::read_to_string(tools.join("stdin-size")).unwrap(),
        "33"
    );
    assert!(!serde_json::to_string(&task).unwrap().contains("WIFI:T:ADB"));
    assert!(app.wireless.sessions.lock().unwrap().is_empty());

    std::fs::remove_file(tools.join("qr-name")).unwrap();
    let qr = start_qr(&app).await.unwrap();
    app.cancel_task(&qr.task.id).unwrap();
    assert_eq!(finished(&app, &qr.task.id).await.state, "cancelled");
    assert!(app.wireless.sessions.lock().unwrap().is_empty());

    let app = fixture(&temp.path().join("no-mdns"), &binary, "mdns-unavailable").await;
    assert_eq!(start_qr(&app).await.err().unwrap().code, "MDNS_UNAVAILABLE");
    let disabled = fixture(&temp.path().join("disabled-mdns"), &binary, "mdns-disabled").await;
    assert_eq!(
        start_qr(&disabled).await.err().unwrap().code,
        "MDNS_UNAVAILABLE"
    );
    std::fs::write(
        std::path::Path::new(&app.store.settings().sdk_root).join("platform-tools/mode"),
        "service",
    )
    .unwrap();
    let mut gone = service;
    gone.address = "192.0.2.10:37124".into();
    assert_eq!(
        start_code(&app, gone, "123456".into(), "gone".into())
            .await
            .unwrap_err()
            .code,
        "PAIR_SERVICE_GONE"
    );
}
