use crate::{app::App, domain::*, signing::*, signing_tasks};
use base64::Engine;
use std::{path::Path, sync::Arc};
use tauri::State;
type Service<'a> = State<'a, Arc<App>>;

#[tauri::command]
pub fn signing_environment(app: Service<'_>) -> SigningEnvironment {
    environment(&app)
}
#[tauri::command]
pub async fn signing_records(app: Service<'_>) -> Result<Vec<KeystoreRecord>> {
    let app = app.inner().clone();
    tokio::task::spawn_blocking(move || records(&app))
        .await
        .map_err(|_| AppError::new("SIGNING_RECORD", "读取密钥登记失败"))?
}
#[tauri::command]
pub async fn keystore_register(
    app: Service<'_>,
    record: KeystoreRecord,
    secrets: Option<Passwords>,
) -> Result<KeystoreRecord> {
    let _update = app.allow_update_sensitive_work()?;
    register(app.inner(), record, secrets).await
}
#[tauri::command]
pub fn keystore_forget(app: Service<'_>, record_id: String) -> Result<()> {
    app.store.forget_signing_record(&record_id)?;
    app.notify();
    Ok(())
}
#[tauri::command]
pub async fn keystore_copy_base64(
    app: Service<'_>,
    record_id: String,
    secrets: Passwords,
) -> Result<()> {
    let content = zeroize::Zeroizing::new(keystore_base64(app.inner(), &record_id, secrets).await?);
    tokio::task::spawn_blocking(move || {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(content.as_str()))
            .map_err(|_| AppError::new("CLIPBOARD_FAILED", "复制失败，请重试"))
    })
    .await
    .map_err(|_| AppError::new("CLIPBOARD_FAILED", "复制失败，请重试"))?
}
#[tauri::command]
pub async fn apk_signature_inspect(app: Service<'_>, path: String) -> Result<ApkSignatureReport> {
    let _sdk = app.sdk_gate.read().await;
    inspect_apk(app.inner(), Path::new(&path)).await
}
#[tauri::command]
pub async fn signing_certificate_inspect(
    app: Service<'_>,
    path: String,
) -> Result<CertificateInfo> {
    let path = bounded_file(Path::new(&path), 1024 * 1024)?;
    bridge(
        app.inner(),
        None,
        "certificate",
        &path,
        "",
        zeroize::Zeroizing::new(vec![]),
    )
    .await
}
#[tauri::command]
pub async fn signing_prepare(
    app: Service<'_>,
    request: SigningRequest,
    secrets: Passwords,
) -> Result<Plan> {
    signing_tasks::prepare(app.inner(), request, secrets).await
}
#[tauri::command]
pub fn signing_plan_release(app: Service<'_>, plan_id: String) {
    app.signing.pending.lock().unwrap().remove(&plan_id);
}
#[tauri::command]
pub fn signing_submit(
    app: Service<'_>,
    plan_id: String,
    plan_hash: String,
    confirmation_token: String,
    request_id: String,
) -> Result<Task> {
    signing_tasks::submit(
        app.inner(),
        &plan_id,
        &plan_hash,
        &confirmation_token,
        &request_id,
    )
}
#[tauri::command]
pub async fn signing_choose(kind: String, input: Option<String>) -> Result<Option<String>> {
    tokio::task::spawn_blocking(move || {
        let dialog = rfd::FileDialog::new();
        let apk_name = input
            .as_deref()
            .and_then(|path| Path::new(path).file_stem())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(|name| format!("{name}-signed.apk"))
            .unwrap_or_else(|| "app-signed.apk".into());
        let path = match kind.as_str() {
            "keystore" => dialog
                .add_filter("Keystore", &["jks", "keystore", "p12", "pfx"])
                .pick_file(),
            "apk" => dialog.add_filter("Android APK", &["apk"]).pick_file(),
            "certificate" => dialog
                .add_filter("X.509", &["pem", "cer", "der"])
                .pick_file(),
            "output-apk" => dialog
                .add_filter("Android APK", &["apk"])
                .set_file_name(apk_name)
                .save_file(),
            "output-jks" => dialog
                .add_filter("JKS", &["jks"])
                .set_file_name("release.jks")
                .save_file(),
            "output-p12" => dialog
                .add_filter("PKCS12", &["p12"])
                .set_file_name("release.p12")
                .save_file(),
            _ => return Err(AppError::new("SIGNING_INPUT", "文件选择类型无效")),
        };
        Ok(path.map(|p| p.to_string_lossy().into()))
    })
    .await
    .map_err(|_| AppError::new("DIALOG", "文件选择未完成"))?
}
#[tauri::command]
pub async fn signing_result_save(app: Service<'_>, artifact_id: String) -> Result<Option<String>> {
    let (artifact, source) = app.checked_artifact(&artifact_id)?;
    let _use = app.use_task_record(&artifact.task_id, true)?;
    let app = app.inner().clone();
    tokio::task::spawn_blocking(move || {
        let extension = source.extension().and_then(|v| v.to_str()).unwrap_or("apk");
        let Some(destination) = rfd::FileDialog::new()
            .add_filter("Signing result", &[extension])
            .set_file_name(format!("android-result.{extension}"))
            .save_file()
        else {
            return Ok(None);
        };
        // The outer record guard already prevents deletion; saving verifies the source again.
        signing_tasks::save_result_unlocked(&app, &artifact_id, &destination).map(Some)
    })
    .await
    .map_err(|_| AppError::new("SAVE_FAILED", "保存签名结果未完成"))?
}
#[tauri::command]
pub async fn signing_result_check(
    app: Service<'_>,
    artifact_id: String,
    secrets: Passwords,
) -> Result<()> {
    signing_tasks::check_recovered_key(app.inner(), &artifact_id, secrets).await
}
#[tauri::command]
pub fn signing_result_discard(
    app: Service<'_>,
    artifact_id: String,
    expected_sha256: String,
) -> Result<()> {
    signing_tasks::discard_result(&app, &artifact_id, &expected_sha256)
}
#[tauri::command]
pub async fn signing_certificate_export(
    app: Service<'_>,
    record_id: String,
    alias: String,
    expected_sha256: String,
    format: String,
) -> Result<Option<String>> {
    let record = select_record(&app, &record_id)?;
    if digest_file(&bounded_file(Path::new(&record.path), MAX_KEYSTORE)?)? != record.sha256 {
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "密钥文件已变化，请重新检查",
        ));
    }
    let entry = record
        .entries
        .iter()
        .find(|e| e.alias == alias && e.certificate.sha256 == expected_sha256)
        .ok_or_else(|| AppError::new("SIGNING_CHANGED", "证书信息已变化，请重新检查"))?;
    let der = base64::engine::general_purpose::STANDARD
        .decode(&entry.certificate.der)
        .map_err(|_| AppError::new("SIGNING_CERTIFICATE", "证书编码无效"))?;
    let (extension, bytes) = match format.as_str() {
        "PEM" => {
            let base = base64::engine::general_purpose::STANDARD.encode(&der);
            let body = base
                .as_bytes()
                .chunks(64)
                .map(|line| std::str::from_utf8(line).unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            (
                "pem",
                format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----\n")
                    .into_bytes(),
            )
        }
        "DER" => ("cer", der),
        _ => return Err(AppError::new("SIGNING_CERTIFICATE", "证书格式无效")),
    };
    let app = app.inner().clone();
    tokio::task::spawn_blocking(move || {
        let Some(destination) = rfd::FileDialog::new()
            .add_filter("X.509", &[extension])
            .set_file_name(format!("certificate.{extension}"))
            .save_file()
        else {
            return Ok(None);
        };
        let root = owned_root(&app, "artifacts")?;
        let temporary = root.join(format!("certificate-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<Option<String>> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            publish_copy(&temporary, &destination, &digest(&bytes))?;
            Ok(Some(destination.to_string_lossy().into()))
        })();
        let _ = std::fs::remove_file(temporary);
        result
    })
    .await
    .map_err(|_| AppError::new("SAVE_FAILED", "证书导出未完成"))?
}
#[tauri::command]
pub fn signing_open_file(app: Service<'_>, path: String) -> Result<()> {
    let allowed = app.store.signing_records()?.iter().any(|r| r.path == path)
        || app.store.tasks()?.iter().any(|t| {
            matches!(t.operation.kind.as_str(), "keystore_create" | "apk_sign")
                && t.operation.optional("savedOutput") == path
        });
    if !allowed {
        return Err(AppError::new(
            "SIGNING_PATH",
            "文件不属于已登记或已保存的签名结果",
        ));
    }
    let path = bounded_file(Path::new(&path), 1024 * 1024 * 1024)?;
    // Explorer expects ordinary drive/UNC paths rather than canonical verbatim prefixes.
    let canonical = path.to_string_lossy();
    let shell_path = if let Some(unc) = canonical.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        canonical
            .strip_prefix(r"\\?\")
            .unwrap_or(&canonical)
            .to_string()
    };
    let mut command =
        crate::process::Spec::new("explorer.exe", vec![format!("/select,{shell_path}")]).command();
    command.spawn()?;
    Ok(())
}
