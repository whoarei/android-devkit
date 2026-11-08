//! Real production workflows, with disposable AVDs and temporary application data.
use crate::{app::App, domain::*, process::Spec};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn submit(
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
    app.submit(
        &plan.id,
        &plan.hash,
        &uuid::Uuid::new_v4().to_string(),
        &plan.confirmation_token,
        &plan
            .licenses
            .iter()
            .map(|l| l.hash.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
async fn finish(app: &App, task: &Task) -> Task {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(400);
    let mut phase = String::new();
    loop {
        let current = app.store.task(&task.id).unwrap();
        if current.phase != phase {
            println!(
                "{}: {} {}",
                current.operation.kind, current.phase, current.message
            );
            phase = current.phase.clone();
        }
        if !current.active() {
            return current;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{}: {}",
            current.operation.kind,
            current.message
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
async fn execute(
    app: &Arc<App>,
    kind: &str,
    arguments: BTreeMap<String, String>,
    target: Option<Target>,
) -> Task {
    let task = finish(app, &submit(app, kind, arguments, target)).await;
    assert_eq!(task.state, "succeeded", "{}\n{:?}", task.message, task.log);
    task
}
async fn app_args(app: &App, target: &Target, package: &str) -> BTreeMap<String, String> {
    let d = app.device_app_detail(target, package).await.unwrap();
    args(&[
        ("package", package),
        ("userId", &d.user_id.to_string()),
        ("appFingerprint", &d.fingerprint),
        ("version", &d.version_name),
        ("system", &d.app.system.to_string()),
    ])
}
fn archive(path: &Path, fixture: &Path) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for name in ["base.apk", "config.zh.apk"] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        std::io::copy(
            &mut std::fs::File::open(fixture.join(name)).unwrap(),
            &mut zip,
        )
        .unwrap();
    }
    zip.finish().unwrap().sync_all().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "real SDK + official downloads; creates only disposable AVDs and isolated .qa/runtime data"]
async fn device_utilities_real_sdk_workflows() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".qa/runtime")
        .join(format!("device-utilities-{}", now()));
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
    let owned_prefix = format!("DevKit_Utilities_{}", now());
    let test_app = app.clone();
    let test_root = root.clone();
    let prefix = owned_prefix.clone();
    let test_settings = settings.clone();
    let result=tokio::spawn(async move {
        let app=test_app; let root=test_root; let settings=test_settings;
        for tool in ["bundletool","scrcpy"] { execute(&app,"device_tool_prepare",args(&[("tool",tool)]),None).await; }
        let fixture_script=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("scripts/build-container-fixtures.ps1");
        let jar=crate::device_tools::path(&app,"bundletool").unwrap();
        let fixture_build=crate::process::run(Spec::new("powershell.exe",vec!["-NoProfile".into(),"-File".into(),fixture_script.to_string_lossy().into(),"-BundletoolJar".into(),jar.to_string_lossy().into()])).await.unwrap();
        assert_eq!(fixture_build.code,0,"{}\n{}",fixture_build.text(),fixture_build.stderr);
        let fixture=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join(".qa/runtime/fixture/splits");
        let mut children=Vec::new(); let mut targets=Vec::new();
        for index in 0..2 {
            let name=format!("{prefix}_{index}");
            execute(&app,"avd_create",args(&[("name",&name),("displayName","Device utilities QA"),("profile","pixel_2"),("image","system-images;android-29;default;x86_64"),("memory","1536"),("storage","2"),("gpu","auto"),("cpuCores","2"),("skinMode","none"),("screenWidth","720"),("screenHeight","1280")]),None).await;
            let avd=crate::android::avds(&settings).into_iter().find(|a|a.name==name).unwrap();
            let log=std::fs::File::create(root.join(format!("{name}.log"))).unwrap();
            let mut spec=Spec::new(Path::new(&settings.sdk_root).join("emulator/emulator.exe"),vec!["-avd".into(),name.clone(),"-no-window".into(),"-no-snapshot".into(),"-gpu".into(),"host".into()]);
            spec.env.insert("ANDROID_AVD_HOME".into(),Path::new(&avd.ini_path).parent().unwrap().to_string_lossy().into());
            spec.env.insert("ANDROID_HOME".into(),settings.sdk_root.clone());
            let mut command=spec.command(); command.stdout(std::process::Stdio::from(log.try_clone().unwrap())).stderr(std::process::Stdio::from(log)).stdin(std::process::Stdio::null()).kill_on_drop(true);
            children.push(command.spawn().unwrap());
            execute(&app,"avd_wait",args(&[("avdId",&avd.id)]),None).await;
            let devices=crate::android::devices(&settings,app.epoch.load(Ordering::SeqCst)).await;
            targets.push(Target::from(devices.iter().find(|d|d.avd_name==name&&d.state=="ready").unwrap()));
        }
        let target=&targets[0]; let other=&targets[1];
        let set=app.prepare_apk_set(vec![fixture.join("valid")]).await.unwrap();
        for target in &targets { execute(&app,"apk_set_install",args(&[("setId",&set.id)]),Some(target.clone())).await; }
        let package=&set.package;
        let detail=app.device_app_detail(target,package).await.unwrap();
        assert!(!detail.app.system); assert_eq!(detail.paths.len(),2); assert_eq!(detail.app.version_code,"1");
        let system=app.device_app_detail(target,"com.android.settings").await.unwrap();
        assert!(system.app.system); assert!(crate::device_apps::check_action("uninstall",&system).is_err());
        for target in &targets { app.remote_shell(&settings,target,"run-as dev.androiddevkit.fixture sh -c 'mkdir -p files; printf original > files/utilities-marker'".into(),15).await.unwrap(); }
        execute(&app,"device_app_launch",app_args(&app,target,package).await,Some(target.clone())).await;
        execute(&app,"device_app_stop",app_args(&app,target,package).await,Some(target.clone())).await;
        let exported=execute(&app,"device_app_export",app_args(&app,target,package).await,Some(target.clone())).await;
        let saved=PathBuf::from(&exported.artifact); assert_eq!(saved.extension().unwrap(),"apks");
        let reimported=app.prepare_apk_container(&saved,target).await.unwrap(); assert_eq!(reimported.members.len(),2);
        assert!(crate::phone_apk::check_target(&reimported,other).is_err());
        assert!(app.prepare_plan(Operation { kind:"apk_set_install".into(),args:args(&[("setId",&reimported.id)]),target:Some(other.clone()) }).is_err());
        execute(&app,"apk_set_install",args(&[("setId",&reimported.id)]),Some(target.clone())).await;
        for extension in ["xapk","apkm"] {
            let source=root.join(format!("中文 安装包.{extension}")); archive(&source,&fixture.join("valid"));
            let imported=app.prepare_apk_container(&source,target).await.unwrap(); assert_eq!(imported.members.len(),2);
            execute(&app,"apk_set_install",args(&[("setId",&imported.id)]),Some(target.clone())).await;
        }
        // Also test the official table-of-contents APKS, generated by the fixture script.
        let official=fixture.parent().unwrap().join("containers/official.apks");
        assert!(official.is_file());
        let official_set=app.prepare_apk_container(&official,target).await.unwrap();
        execute(&app,"apk_set_install",args(&[("setId",&official_set.id)]),Some(target.clone())).await;
        execute(&app,"device_tool_prepare",args(&[("tool","scrcpy")]),None).await;
        let mirror=submit(&app,"mirror_start",args(&[("control","true"),("maxSize","1280"),("maxFps","30")]),Some(target.clone()));
        let deadline=tokio::time::Instant::now()+Duration::from_secs(45);
        loop {
            let m=app.store.task(&mirror.id).unwrap(); assert!(m.active(),"{}\n{:?}",m.message,m.log);
            if m.phase=="mirroring" && m.log.iter().any(|l|l.contains("Texture:")||l.contains("Renderer:")) { break; }
            assert!(tokio::time::Instant::now()<deadline,"mirror did not render: {:?}",m.log);
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        if let Ok(plan)=app.prepare_plan(Operation { kind:"mirror_start".into(),args:args(&[("control","false")]),target:Some(target.clone()) }) {
            assert!(app.submit(&plan.id,&plan.hash,&uuid::Uuid::new_v4().to_string(),&plan.confirmation_token,&[]).is_err());
        }
        execute(&app,"device_app_launch",app_args(&app,target,package).await,Some(target.clone())).await;
        app.cancel_task(&mirror.id).unwrap(); let mirror_result=finish(&app,&mirror).await; assert_eq!(mirror_result.state,"succeeded","{}",mirror_result.message);
        execute(&app,"device_app_clear",app_args(&app,target,package).await,Some(target.clone())).await;
        assert!(app.remote_shell(&settings,target,"run-as dev.androiddevkit.fixture cat files/utilities-marker".into(),15).await.is_err());
        assert_eq!(app.remote_shell(&settings,other,"run-as dev.androiddevkit.fixture cat files/utilities-marker".into(),15).await.unwrap().text(),"original");
        let stale=app.prepare_plan(Operation { kind:"device_app_uninstall".into(),args:app_args(&app,target,package).await,target:Some(target.clone()) }).unwrap();
        let upgraded=app.prepare_apk_set(vec![fixture.join("v2")]).await.unwrap();
        execute(&app,"apk_set_install",args(&[("setId",&upgraded.id)]),Some(target.clone())).await;
        let stale_task=app.submit(&stale.id,&stale.hash,&uuid::Uuid::new_v4().to_string(),&stale.confirmation_token,&[]).unwrap();
        let stale_result=finish(&app,&stale_task).await; assert_eq!(stale_result.state,"failed"); assert!(stale_result.message.contains("APP_CHANGED"));
        assert_eq!(app.device_app_detail(target,package).await.unwrap().app.version_code,"2");
        execute(&app,"device_app_uninstall",app_args(&app,target,package).await,Some(target.clone())).await;
        assert!(!app.list_device_apps(target).await.unwrap().apps.iter().any(|a|a.package==*package));
        assert!(app.list_device_apps(other).await.unwrap().apps.iter().any(|a|a.package==*package));
        let report=serde_json::json!({"status":"passed","isolatedAvds":2,"applicationManagement":true,"systemMutationBlocked":true,"staleVersionRefused":true,"exportReimport":true,"containerFormats":["raw-apks","xapk","apkm"],"officialApks":official.is_file(),"targetBinding":true,"scrcpyRendered":true,"scrcpyConcurrentAppOperation":true,"scrcpyCancelled":true,"userDevicesUsed":false});
        std::fs::write(root.join("report.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        // Leave cleanup to the production stop/delete plans below, including on panic.
        for child in &mut children { let _=child.start_kill(); }
        for child in &mut children { let _=child.wait().await; }
        root
    }).await;
    for avd in crate::android::avds(&settings)
        .into_iter()
        .filter(|a| a.name.starts_with(&owned_prefix))
    {
        if let Ok(plan) = app.prepare_plan(Operation {
            kind: "avd_stop".into(),
            args: args(&[("avdId", &avd.id)]),
            target: None,
        }) {
            if let Ok(task) = app.submit(
                &plan.id,
                &plan.hash,
                &uuid::Uuid::new_v4().to_string(),
                &plan.confirmation_token,
                &[],
            ) {
                let _ = finish(&app, &task).await;
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        let plan = app
            .prepare_plan(Operation {
                kind: "avd_delete".into(),
                args: args(&[("avdId", &avd.id)]),
                target: None,
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
        let deleted = finish(&app, &task).await;
        assert_eq!(deleted.state, "succeeded", "{}", deleted.message);
    }
    let evidence = result.unwrap();
    println!("Evidence: {}", evidence.join("report.json").display());
}
