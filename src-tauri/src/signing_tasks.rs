//! Explicit, idempotent signing submissions. Persistent journals contain public data only.
use crate::{app::App, domain::*, process::Spec, signing::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    task_id: String,
    artifact_id: String,
    kind: String,
    extension: String,
    request: SigningRequest,
    sha256: String,
    verified: bool,
    discarded: bool,
    record: Option<KeystoreRecord>,
    #[serde(default)]
    original_sha256: String,
    #[serde(default)]
    replaced_output_sha256: String,
}
fn replacement_digest(journal: &Journal) -> Option<&str> {
    if journal.request.overwrite_source {
        Some(&journal.original_sha256)
    } else if !journal.replaced_output_sha256.is_empty() {
        Some(&journal.replaced_output_sha256)
    } else {
        None
    }
}
fn existing_output_digest(path: &Path) -> Result<String> {
    if !path.try_exists()? {
        return Ok(String::new());
    }
    let metadata = path.metadata()?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 * 1024 {
        return Err(AppError::new(
            "SIGNING_OUTPUT",
            "输出目标不是普通文件或超过大小限制",
        ));
    }
    digest_file(path)
}
fn journal_path(app: &App, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("SIGNING_ID", "签名任务标识无效"))?;
    Ok(owned_root(app, "signing-jobs")?.join(format!("{id}.json")))
}
fn result_path(app: &App, journal: &Journal) -> Result<PathBuf> {
    if journal.artifact_id != format!("signing-{}", journal.task_id)
        || !matches!(
            (journal.kind.as_str(), journal.extension.as_str()),
            ("keystore", "p12" | "jks") | ("signed_apk", "apk")
        )
    {
        return Err(AppError::new("SIGNING_JOURNAL", "签名结果归属不匹配"));
    }
    journal_path(app, &journal.task_id)?;
    Ok(
        owned_root(app, "artifacts")?
            .join(format!("{}.{}", journal.artifact_id, journal.extension)),
    )
}
fn write_journal(app: &App, journal: &Journal) -> Result<()> {
    crate::recovery::write_commit_journal(
        &journal_path(app, &journal.task_id)?,
        &serde_json::to_value(journal)?,
    )
}
fn read_journal(app: &App, id: &str) -> Result<Journal> {
    let file = bounded_file(&journal_path(app, id)?, 2 * 1024 * 1024)?;
    let journal: Journal = serde_json::from_slice(&std::fs::read(file)?)?;
    if journal.task_id != id {
        return Err(AppError::new("SIGNING_JOURNAL", "签名恢复记录不匹配"));
    }
    result_path(app, &journal)?;
    Ok(journal)
}
fn tool_fingerprint(app: &App) -> Result<String> {
    let env = environment(app);
    let mut bytes = serde_json::to_vec(&env)?;
    for file in [&env.java, &env.keytool, &env.apksigner, &env.zipalign] {
        if !file.is_empty() {
            bytes.extend(digest_file(Path::new(file))?.bytes());
        }
    }
    Ok(digest(&bytes))
}
pub async fn prepare(
    app: &Arc<App>,
    mut request: SigningRequest,
    secrets: Passwords,
) -> Result<Plan> {
    let _update = app.allow_update_sensitive_work()?;
    let _sdk = app.sdk_gate.read().await;
    if app.draining.load(Ordering::SeqCst) {
        return Err(AppError::new("APP_CLOSING", "应用正在退出"));
    }
    let creating = request.mode == "create";
    if !creating && request.mode != "sign" {
        return Err(AppError::new("SIGNING_MODE", "签名操作无效"));
    }
    secrets.validate(creating)?;
    if request.alias.trim().is_empty()
        || request.alias.len() > 200
        || request.alias.starts_with('-')
        || request.alias.contains(['\0', '\r', '\n'])
    {
        return Err(AppError::new("SIGNING_ALIAS", "请填写有效的密钥 alias"));
    }
    if request.overwrite_source {
        if creating {
            return Err(AppError::new("SIGNING_OUTPUT", "新建密钥不能覆盖源文件"));
        }
        request.input = bounded_file(Path::new(&request.input), 1024 * 1024 * 1024)?
            .to_string_lossy()
            .into();
        request.output = request.input.clone();
    } else {
        request.output = output_path(&request.output)?.to_string_lossy().into();
    }
    if creating && Path::new(&request.output).try_exists()? {
        return Err(AppError::new(
            "SIGNING_OUTPUT",
            "新建 Keystore 请选择未使用的文件名",
        ));
    }
    let destination_sha256 = if !creating && !request.overwrite_source {
        existing_output_digest(Path::new(&request.output))?
    } else {
        String::new()
    };
    let environment = environment(app);
    if environment.java.is_empty()
        || (creating && environment.keytool.is_empty())
        || (!creating && (environment.apksigner.is_empty() || environment.zipalign.is_empty()))
    {
        return Err(AppError::new(
            "SIGNING_DEPENDENCY",
            "缺少签名所需工具，请检查 Java、keytool 和 SDK Build-Tools",
        ));
    }
    let mut args = BTreeMap::new();
    let _: serde_json::Value = bridge(
        app,
        None,
        if creating { "probe" } else { "signing-probe" },
        Path::new(""),
        "",
        Zeroizing::new(vec![]),
    )
    .await
    .map_err(|_| {
        AppError::new(
            "SIGNING_DEPENDENCY",
            "请配置 Java 17 或更高版本；APK 签名需支持 16 KiB 对齐的较新 Build-Tools",
        )
    })?;
    let mut effects = vec![format!(
        "{}：{}",
        if request.overwrite_source {
            "覆盖源文件"
        } else if !destination_sha256.is_empty() {
            "覆盖输出文件"
        } else {
            "输出到新文件"
        },
        request.output
    )];
    let mut replacing = false;
    let mut warnings = vec![if request.overwrite_source {
        "签名验证通过后覆盖源 APK；请确认覆盖目标。"
    } else if !destination_sha256.is_empty() {
        "密码仅用于本次操作；验证通过后替换所选输出文件。"
    } else {
        "密码仅用于本次操作，不写入配置、任务或日志。输入原文件保持不变。"
    }
    .into()];
    if creating {
        if request.format == "PKCS12" && !secrets.store_password.is_ascii() {
            return Err(AppError::new(
                "SIGNING_PASSWORD_ENCODING",
                "当前 Java 的 PKCS12 新建密码需使用 ASCII 字符；需要中文密码时请选择 JKS",
            ));
        }
        if !matches!(request.format.as_str(), "JKS" | "PKCS12")
            || request.subject.is_empty()
            || request.subject.len() > 1000
            || request.subject.contains(['\0', '\r', '\n'])
            || request.name.is_empty()
            || request.name.len() > 200
        {
            return Err(AppError::new(
                "SIGNING_INPUT",
                "请检查新密钥名称、格式及证书信息",
            ));
        }
        if request.format == "PKCS12" && secrets.key() != secrets.store_password {
            return Err(AppError::new(
                "SIGNING_PASSWORD",
                "新建 PKCS12 的容器与私钥使用相同密码",
            ));
        }
        let ext = if request.format == "JKS" {
            "jks"
        } else {
            "p12"
        };
        if !Path::new(&request.output)
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case(ext))
        {
            return Err(AppError::new(
                "SIGNING_OUTPUT",
                "输出扩展名必须与 Keystore 格式一致",
            ));
        }
        request.keystore_id = uuid::Uuid::new_v4().to_string();
        request.input.clear();
        effects.push(format!(
            "{} · {} · RSA 3072 / SHA256withRSA · 10950 天",
            request.format, request.alias
        ));
        effects.push(format!("证书主体：{}", request.subject));
        warnings.push(
            "生成后请另行备份文件与密码；尚未保存的新密钥受保护，不能被普通缓存清理丢弃。".into(),
        );
    } else {
        let report = inspect_apk(app, Path::new(&request.input)).await?;
        let unsigned = report.unsigned;
        if !report.standalone
            || report.lineage
            || report.signers.len() > 1
            || (!report.verified && !unsigned)
        {
            return Err(AppError::new(
                "SIGNING_APK",
                "请选择完整单 APK；分包、多签名者、轮换谱系及无效签名不能在本轮重签",
            ));
        }
        if report.verified && !request.replace_signature {
            return Err(AppError::new(
                "SIGNING_REPLACE",
                "该 APK 已签名，请明确确认替换新文件中的原签名",
            ));
        }
        if Path::new(&request.output)
            .extension()
            .is_none_or(|v| !v.eq_ignore_ascii_case("apk"))
        {
            return Err(AppError::new("APK_ONLY", "输出文件应使用 .apk 扩展名"));
        }
        let record = select_record(app, &request.keystore_id)?;
        let hash = digest_file(&bounded_file(Path::new(&record.path), MAX_KEYSTORE)?)?;
        let info = inspect(app, Path::new(&record.path), &secrets, Some(&request.alias)).await?;
        let entry = info
            .entries
            .iter()
            .find(|e| e.alias == request.alias && e.private_key)
            .ok_or_else(|| AppError::new("SIGNING_ALIAS", "所选 alias 不是可用私钥条目"))?;
        if digest_file(Path::new(&record.path))? != hash {
            return Err(AppError::new("SIGNING_CHANGED", "密钥文件在检查期间变化"));
        }
        request.input = report.path;
        replacing = report.verified;
        args.insert("inputSha256".into(), report.sha256);
        args.insert("keystorePath".into(), record.path);
        args.insert("keystoreSha256".into(), hash);
        args.insert("certificateSha256".into(), entry.certificate.sha256.clone());
        args.insert("package".into(), report.package.clone());
        args.insert("version".into(), report.version.clone());
        args.insert(
            "inputBytes".into(),
            std::fs::metadata(&request.input)?.len().to_string(),
        );
        args.insert("keystoreName".into(), record.name.clone());
        request.format = info.format;
        effects.push(format!("应用：{} · {}", report.package, report.version));
        effects.push(format!(
            "输入 APK：{}（{} bytes）",
            request.input,
            std::fs::metadata(&request.input)?.len()
        ));
        if report.verified {
            effects.push(format!("原证书 SHA-256：{}", report.signers[0].sha256));
            effects.push(format!("原签名方案：{}", report.schemes.join(", ")));
        } else {
            effects.push("输入 APK 尚未签名".into());
        }
        effects.push(format!("密钥：{} / {}", record.name, request.alias));
        effects.push(format!("证书 SHA-256：{}", entry.certificate.sha256));
        effects.push("ZIP 对齐 → 签名 → 独立验证 → 无覆盖发布；不生成 v4 .idsig".into());
        if report.verified {
            warnings.push(
                "新输出将替换原有签名；证书不同通常无法覆盖原安装，签名成功不代表应用已验证运行。"
                    .into(),
            );
        }
    }
    let bytes = if creating {
        1024 * 1024
    } else {
        std::fs::metadata(&request.input)?.len().saturating_mul(4)
    };
    crate::capacity::budget(&[
        (app.data.clone(), bytes, "签名暂存"),
        (PathBuf::from(&request.output), bytes, "签名结果"),
    ])?;
    args.insert("request".into(), serde_json::to_string(&request)?);
    args.insert("destinationSha256".into(), destination_sha256.clone());
    args.insert(
        "overwriteOutput".into(),
        (!destination_sha256.is_empty()).to_string(),
    );
    args.insert(
        "overwriteSource".into(),
        request.overwrite_source.to_string(),
    );
    args.insert("toolFingerprint".into(), tool_fingerprint(app)?);
    if request.overwrite_source || !destination_sha256.is_empty() {
        args.insert("sourceBackupResolved".into(), "false".into());
    }
    let operation = Operation {
        kind: if creating {
            "keystore_create"
        } else {
            "apk_sign"
        }
        .into(),
        args,
        target: None,
    };
    let plan = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        hash: digest(&serde_json::to_vec(&operation)?),
        operation,
        title: if creating {
            "新建 Keystore"
        } else if replacing {
            "使用所选密钥重新签名"
        } else {
            "签名 APK"
        }
        .into(),
        effects,
        warnings,
        licenses: vec![],
        created_at: now(),
        settings_hash: crate::plans::fingerprint(app, &app.store.settings())?,
        confirmation_token: uuid::Uuid::new_v4().to_string(),
    };
    let mut pending = app.signing.pending.lock().unwrap();
    pending.retain(|_, p| now().saturating_sub(p.plan.created_at) <= 300);
    if pending.len() >= 16 {
        return Err(AppError::new(
            "SIGNING_BUSY",
            "待确认签名计划过多，请关闭旧计划",
        ));
    }
    pending.insert(
        plan.id.clone(),
        Pending {
            plan: plan.clone(),
            request,
            secrets,
            expires_at: tokio::time::Instant::now() + Duration::from_secs(300),
        },
    );
    drop(pending);
    let weak = Arc::downgrade(app);
    let id = plan.id.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(301)).await;
        if let Some(app) = weak.upgrade() {
            app.signing.pending.lock().unwrap().remove(&id);
        }
    });
    Ok(plan)
}
pub fn submit(app: &Arc<App>, id: &str, hash: &str, token: &str, request_id: &str) -> Result<Task> {
    let _configuration = app.configuration_gate.lock().unwrap();
    let _update = app.allow_update_sensitive_work()?;
    uuid::Uuid::parse_str(request_id)
        .map_err(|_| AppError::new("SIGNING_REQUEST", "提交标识无效"))?;
    if let Some(task) = app.store.request(request_id) {
        if task.operation.optional("signingPlanHash") == hash {
            return Ok(task);
        }
        return Err(AppError::new("SIGNING_REQUEST", "提交标识与计划不匹配"));
    }
    if app.draining.load(Ordering::SeqCst) {
        return Err(AppError::new("APP_CLOSING", "应用正在退出"));
    }
    let mut pending = app.signing.pending.lock().unwrap();
    let value = pending
        .get(id)
        .ok_or_else(|| AppError::new("SIGNING_EXPIRED", "签名授权已过期，请重新提供密码"))?;
    if tokio::time::Instant::now() >= value.expires_at
        || value.plan.hash != hash
        || value.plan.confirmation_token != token
        || value.plan.settings_hash != crate::plans::fingerprint(app, &app.store.settings())?
    {
        return Err(AppError::new(
            "PLAN_CHANGED",
            "签名计划或环境已变化，请重新检查并提供密码",
        ));
    }
    let mut operation = value.plan.operation.clone();
    operation.args.insert("signingPlanHash".into(), hash.into());
    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        request_id: request_id.into(),
        title: value.plan.title.clone(),
        operation,
        state: "queued".into(),
        phase: "queued".into(),
        progress: None,
        message: "等待签名工具".into(),
        log: vec![],
        created_at: now(),
        updated_at: now(),
        cancel_requested: false,
        cancellable: true,
        artifact: String::new(),
        license_prompt: String::new(),
        confirmed_licenses: vec![],
    };
    app.store.save_task(&task)?;
    let value = pending.remove(id).unwrap();
    drop(pending);
    let _ = app.store.audit(&task.id, "signing-confirmed");
    let runner = app.clone();
    let task_id = task.id.clone();
    tauri::async_runtime::spawn(async move {
        let app = runner;
        let result = run(&app, &task_id, value).await;
        let _ = recover_one(&app, &task_id);
        let _ = app.update(&task_id, |t| {
            t.cancellable = false;
            match result {
                Ok(()) => {
                    t.state = "succeeded".into();
                    t.phase = "complete".into();
                    t.message = "签名结果已验证并保存".into();
                    t.progress = Some(100.0);
                }
                Err(ref error) => {
                    if !error.detail.is_empty() {
                        t.log.push(error.detail.clone());
                    }
                    t.state = if !t.artifact.is_empty() {
                        "needs_review"
                    } else if error.code == "CANCELLED" {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    t.message = format!("{}：{}", error.code, error.message);
                }
            }
        });
        let _ = app.store.audit(&task_id, "signing-finished");
        app.notify();
    });
    app.notify();
    Ok(task)
}
async fn run(app: &Arc<App>, id: &str, pending: Pending) -> Result<()> {
    let _use = app.use_task_record(id, false)?;
    let lock = app.lock(&format!(
        "signing:{}",
        pending.request.output.to_lowercase()
    ));
    let waiting = async {
        (
            app.signing.slots.acquire().await,
            app.sdk_gate.read().await,
            lock.lock().await,
        )
    };
    let cancellation = async {
        loop {
            if app.cancelled(id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    };
    let (_slot, _sdk, _lock) = tokio::select! {
        result = tokio::time::timeout_at(pending.expires_at, waiting) => result.map_err(|_| AppError::new("SIGNING_EXPIRED", "签名授权已过期，请重新提供密码"))?,
        _ = cancellation => return Err(AppError::new("CANCELLED", "已取消")),
    };
    if now().saturating_sub(pending.plan.created_at) > 300 {
        return Err(AppError::new(
            "SIGNING_EXPIRED",
            "签名授权已过期，请重新提供密码",
        ));
    }
    if tool_fingerprint(app)? != pending.plan.operation.optional("toolFingerprint")
        || pending.plan.settings_hash != crate::plans::fingerprint(app, &app.store.settings())?
    {
        return Err(AppError::new(
            "PLAN_CHANGED",
            "执行前签名工具环境变化，请重新确认",
        ));
    }
    if app.cancelled(id) {
        return Err(AppError::new("CANCELLED", "已取消"));
    }
    app.update(id, |t| {
        t.state = "running".into();
        t.phase = "preparing".into();
        t.cancellable = true;
    })?;
    let request = pending.request;
    let creating = request.mode == "create";
    let mut journal = Journal {
        task_id: id.into(),
        artifact_id: format!("signing-{id}"),
        kind: if creating { "keystore" } else { "signed_apk" }.into(),
        extension: if creating {
            if request.format == "JKS" {
                "jks"
            } else {
                "p12"
            }
        } else {
            "apk"
        }
        .into(),
        request: request.clone(),
        sha256: String::new(),
        verified: false,
        discarded: false,
        record: None,
        original_sha256: pending.plan.operation.optional("inputSha256").into(),
        replaced_output_sha256: pending.plan.operation.optional("destinationSha256").into(),
    };
    let result = result_path(app, &journal)?;
    write_journal(app, &journal)?;
    let env = environment(app);
    let jobs = owned_root(app, "signing-jobs")?;
    let key_copy = jobs.join(format!("{id}.key"));
    let input_copy = jobs.join(format!("{id}.apk"));
    let aligned = jobs.join(format!("{id}.aligned.apk"));
    let work = async {
        if creating {
            app.phase(id, "generating", "生成新密钥；生成后的文件受保护")?;
            let mut spec = Spec::new(
                &env.keytool,
                vec![
                    "-J-Duser.language=en".into(),
                    "-J-Duser.country=US".into(),
                    "-genkeypair".into(),
                    "-noprompt".into(),
                    "-keystore".into(),
                    result.to_string_lossy().into(),
                    "-storetype".into(),
                    request.format.clone(),
                    "-alias".into(),
                    request.alias.clone(),
                    "-keyalg".into(),
                    "RSA".into(),
                    "-keysize".into(),
                    "3072".into(),
                    "-sigalg".into(),
                    "SHA256withRSA".into(),
                    "-validity".into(),
                    "10950".into(),
                    "-dname".into(),
                    request.subject.clone(),
                ],
            );
            spec.timeout = 120;
            let output = sensitive_tool(
                app,
                Some(id),
                spec,
                pending.secrets.lines(true, &request.format),
            )
            .await?;
            if output.code != 0 {
                return Err(AppError::new(
                    "SIGNING_GENERATE",
                    "密钥生成未完成，请检查证书信息、密码及文件权限",
                ));
            }
            let info = inspect(app, &result, &pending.secrets, Some(&request.alias)).await?;
            if info.format == "JKS" {
                normalize_generated_key(app, Some(id), &result, &request.alias, &pending.secrets)
                    .await?;
            }
            journal.record = Some(KeystoreRecord {
                id: request.keystore_id.clone(),
                name: request.name.clone(),
                path: request.output.clone(),
                role: "unknown".into(),
                package: String::new(),
                notes: String::new(),
                sha256: digest_file(&result)?,
                format: info.format,
                entries: info.entries,
                checked_at: now(),
                status: "ready".into(),
            });
        } else {
            let op = &pending.plan.operation;
            new_copy(
                Path::new(&request.input),
                &input_copy,
                op.arg("inputSha256")?,
            )?;
            new_copy(
                Path::new(op.arg("keystorePath")?),
                &key_copy,
                op.arg("keystoreSha256")?,
            )?;
            app.phase(id, "aligning", "对齐 APK 内容")?;
            let output = sensitive_tool(
                app,
                Some(id),
                Spec::new(
                    &env.zipalign,
                    vec![
                        "-P".into(),
                        "16".into(),
                        "-f".into(),
                        "4".into(),
                        input_copy.to_string_lossy().into(),
                        aligned.to_string_lossy().into(),
                    ],
                ),
                Zeroizing::new(vec![]),
            )
            .await?;
            if output.code != 0 {
                return Err(AppError::new(
                    "SIGNING_ALIGN",
                    "APK 对齐失败，请检查 Build-Tools 版本与输入文件",
                ));
            }
            app.phase(id, "signing", "使用所选密钥签名")?;
            sign_apk_with_key(
                app,
                id,
                &key_copy,
                &request.alias,
                &aligned,
                &result,
                &pending.secrets,
            )
            .await?;
            app.phase(id, "verifying", "验证新签名、证书与 ZIP 对齐")?;
            let verification = inspect_apk(app, &result).await?;
            if !verification.verified
                || verification.signers.len() != 1
                || verification.signers[0].sha256 != op.arg("certificateSha256")?
            {
                return Err(AppError::new(
                    "SIGNING_VERIFY",
                    "新 APK 签名或证书不匹配，未发布结果",
                ));
            }
            let output = sensitive_tool(
                app,
                Some(id),
                Spec::new(
                    &env.zipalign,
                    vec![
                        "-c".into(),
                        "-P".into(),
                        "16".into(),
                        "4".into(),
                        result.to_string_lossy().into(),
                    ],
                ),
                Zeroizing::new(vec![]),
            )
            .await?;
            if output.code != 0 {
                return Err(AppError::new(
                    "SIGNING_VERIFY",
                    "签名后 ZIP 对齐校验失败，未发布结果",
                ));
            }
            app.update(id, |t| {
                t.log = vec![
                    format!("SHA-256: {}", verification.signers[0].sha256),
                    format!("Schemes: {}", verification.schemes.join(", ")),
                    format!("Warnings: {}", verification.warnings.join(", ")),
                ]
            })?;
        }
        journal.sha256 = digest_file(&result)?;
        journal.verified = true;
        write_journal(app, &journal)?;
        register_result(app, &journal)?;
        if app.cancelled(id) {
            return Err(AppError::new(
                "CANCELLED",
                "已生成结果并保留，请另存或明确处理",
            ));
        }
        app.update(id, |t| {
            t.phase = "publishing".into();
            t.cancellable = false;
            t.message = "保存已验证结果".into();
        })?;
        if let Some(old) = replacement_digest(&journal) {
            crate::signing_publish::replace_verified(
                &result,
                Path::new(&request.output),
                old,
                &journal.sha256,
                id,
            )?;
        } else {
            publish_copy(&result, Path::new(&request.output), &journal.sha256)?;
        }
        complete_saved(app, &journal, &request.output)?;
        if let Some(old) = replacement_digest(&journal) {
            crate::signing_publish::cleanup(Path::new(&request.output), old, &journal.sha256, id)?;
        }
        Ok(())
    }
    .await;
    // These are task-owned input snapshots, never original user files or generated keys.
    for file in [key_copy, input_copy, aligned] {
        let _ = std::fs::remove_file(file);
    }
    work
}
fn register_result(app: &App, journal: &Journal) -> Result<()> {
    let file = result_path(app, journal)?;
    let artifact = app.store.register_artifact(
        &journal.artifact_id,
        &file,
        &journal.kind,
        if journal.kind == "keystore" {
            if journal.verified {
                "新建 Keystore（请备份）"
            } else {
                "中断的 Keystore（待检查）"
            }
        } else {
            "签名 APK"
        },
        &journal.task_id,
    )?;
    app.update(&journal.task_id, |t| {
        t.artifact = file.to_string_lossy().into()
    })?;
    if !journal.sha256.is_empty() && artifact.sha256 != journal.sha256 {
        return Err(AppError::new("SIGNING_CHANGED", "待保存结果内容已变化"));
    }
    Ok(())
}
fn complete_saved(app: &App, journal: &Journal, output: &str) -> Result<()> {
    if let Some(mut record) = journal.record.clone() {
        record.path = output.into();
        app.store.save_signing_record(&record)?;
    }
    app.update(&journal.task_id, |t| {
        t.operation.args.insert("savedOutput".into(), output.into());
        if !t.active() {
            t.state = "succeeded".into();
            t.phase = "complete".into();
            t.message = "签名结果已验证并保存".into();
        }
    })?;
    app.store.acknowledge(&journal.artifact_id)?;
    Ok(())
}
fn recover_one(app: &App, id: &str) -> Result<()> {
    let journal = match read_journal(app, id) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    if let Some(old) = replacement_digest(&journal).filter(|_| app.store.task(id).is_ok()) {
        if journal.verified {
            crate::signing_publish::recover(
                Path::new(&journal.request.output),
                old,
                &journal.sha256,
                id,
            )?;
        }
        app.update(id, |t| {
            t.operation
                .args
                .insert("sourceBackupResolved".into(), "true".into());
        })?;
    }
    if journal.discarded {
        let file = result_path(app, &journal)?;
        if file.try_exists()? {
            if digest_file(&file)? != journal.sha256 {
                return Err(AppError::new("SIGNING_CHANGED", "放弃结果前文件已变化"));
            }
            std::fs::remove_file(&file)?;
        }
        app.store.forget_artifact(&journal.artifact_id, &file)?;
        if journal.kind == "keystore" {
            let secondary = file.with_extension("normalizing");
            if secondary.try_exists()? {
                std::fs::remove_file(secondary)?;
            }
        }
        return Ok(());
    }
    // A deleted history row must not be resurrected. Acknowledged results must
    // never re-import a forgotten key or overwrite later metadata edits.
    let task = match app.store.task(id) {
        Ok(task) => task,
        Err(_) => return Ok(()),
    };
    if app
        .store
        .artifact(&journal.artifact_id)
        .is_ok_and(|(artifact, _)| artifact.acknowledged)
    {
        return Ok(());
    }
    let file = result_path(app, &journal)?;
    if !file.exists() && journal.kind == "keystore" && file.with_extension("normalizing").is_file()
    {
        crate::recovery::publish_new_file(&file.with_extension("normalizing"), &file)?;
    }
    if !file.is_file() {
        return Ok(());
    }
    if journal.kind != "keystore" && !journal.verified {
        return Ok(());
    }
    bounded_file(
        &file,
        if journal.kind == "keystore" {
            MAX_KEYSTORE
        } else {
            1024 * 1024 * 1024
        },
    )?;
    if journal.verified && digest_file(&file)? != journal.sha256 {
        return Err(AppError::new(
            "SIGNING_CHANGED",
            "签名结果内容已变化，已保留文件供检查",
        ));
    }
    register_result(app, &journal)?;
    let saved = task.operation.optional("savedOutput");
    let output = if saved.is_empty() {
        &journal.request.output
    } else {
        saved
    };
    if journal.verified && digest_file(Path::new(output)).ok().as_deref() == Some(&journal.sha256) {
        complete_saved(app, &journal, output)?;
    }
    Ok(())
}
pub fn recover(app: &App) -> Result<()> {
    let root = owned_root(app, "signing-jobs")?;
    for file in std::fs::read_dir(root)?.take(10_000).flatten() {
        if file.path().extension().is_some_and(|v| v == "json") {
            if let Some(id) = file.path().file_stem().and_then(|v| v.to_str()) {
                if uuid::Uuid::parse_str(id).is_ok() {
                    if let Err(error) = recover_one(app, id) {
                        let _ = app.update(id, |t| {
                            t.state = "needs_review".into();
                            t.message = error.message.clone();
                        });
                    }
                    for extension in ["key", "apk", "aligned.apk"] {
                        let _ = std::fs::remove_file(
                            file.path().with_file_name(format!("{id}.{extension}")),
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
pub fn save_result_unlocked(app: &App, artifact_id: &str, destination: &Path) -> Result<String> {
    let (artifact, source) = app.checked_artifact(artifact_id)?;
    if !matches!(artifact.kind.as_str(), "keystore" | "signed_apk") {
        return Err(AppError::new("SIGNING_RESULT", "请选择签名工具生成的结果"));
    }
    recover_one(app, &artifact.task_id)?;
    let mut journal = read_journal(app, &artifact.task_id)?;
    if source != result_path(app, &journal)? || journal.discarded {
        return Err(AppError::new("SIGNING_RESULT", "签名结果归属不匹配"));
    }
    if !journal.verified {
        return Err(AppError::new(
            "SIGNING_RECOVERY",
            "中断的密钥需要在签名工具中重新提供密码核验后保存",
        ));
    }
    if destination
        .extension()
        .is_none_or(|v| !v.eq_ignore_ascii_case(&journal.extension))
    {
        return Err(AppError::new(
            "SIGNING_OUTPUT",
            "输出文件扩展名与结果类型不一致",
        ));
    }
    let destination = output_path(&destination.to_string_lossy())?;
    if artifact.kind == "signed_apk" {
        journal.request.overwrite_source = false;
        journal.request.output = destination.to_string_lossy().into();
        journal.replaced_output_sha256 = existing_output_digest(&destination)?;
        write_journal(app, &journal)?;
        app.update(&artifact.task_id, |task| {
            task.operation.args.insert(
                "overwriteOutput".into(),
                (!journal.replaced_output_sha256.is_empty()).to_string(),
            );
            task.operation.args.insert(
                "sourceBackupResolved".into(),
                journal.replaced_output_sha256.is_empty().to_string(),
            );
        })?;
    }
    let publication = if let Some(old) = replacement_digest(&journal) {
        crate::signing_publish::replace_verified(
            &source,
            &destination,
            old,
            &artifact.sha256,
            &artifact.task_id,
        )
    } else {
        publish_copy(&source, &destination, &artifact.sha256)
    };
    if let Err(error) = publication {
        let _ = recover_one(app, &artifact.task_id);
        return Err(error);
    }
    complete_saved(app, &journal, &destination.to_string_lossy())?;
    recover_one(app, &artifact.task_id)?;
    app.notify();
    Ok(destination.to_string_lossy().into())
}
pub async fn check_recovered_key(
    app: &Arc<App>,
    artifact_id: &str,
    secrets: Passwords,
) -> Result<()> {
    let (artifact, path) = app.store.artifact(artifact_id)?;
    let _use = app.use_task_record(&artifact.task_id, true)?;
    let mut journal = read_journal(app, &artifact.task_id)?;
    if journal.kind != "keystore" || path != result_path(app, &journal)? {
        return Err(AppError::new("SIGNING_RESULT", "请选择待恢复的密钥"));
    }
    bounded_file(&path, MAX_KEYSTORE)?;
    let current_sha = digest_file(&path)?;
    if journal.verified && current_sha != artifact.sha256 {
        return Err(AppError::new("SIGNING_CHANGED", "待保存结果内容已变化"));
    }
    let info = inspect(app, &path, &secrets, Some(&journal.request.alias)).await?;
    if digest_file(&path)? != current_sha {
        return Err(AppError::new("SIGNING_CHANGED", "密钥文件在检查期间变化"));
    }
    if !journal.verified && info.format == "JKS" {
        normalize_generated_key(app, None, &path, &journal.request.alias, &secrets).await?;
    }
    journal.verified = true;
    journal.sha256 = digest_file(&path)?;
    journal.record = Some(KeystoreRecord {
        id: journal.request.keystore_id.clone(),
        name: journal.request.name.clone(),
        path: journal.request.output.clone(),
        role: "unknown".into(),
        package: String::new(),
        notes: String::new(),
        sha256: journal.sha256.clone(),
        format: info.format,
        entries: info.entries,
        checked_at: now(),
        status: "ready".into(),
    });
    app.store.refresh_signing_key_artifact(artifact_id, &path)?;
    write_journal(app, &journal)?;
    app.notify();
    Ok(())
}
pub fn discard_result(app: &App, artifact_id: &str, expected_sha256: &str) -> Result<()> {
    let (artifact, source) = app.checked_artifact(artifact_id)?;
    let _use = app.use_task_record(&artifact.task_id, true)?;
    if artifact.sha256 != expected_sha256 {
        return Err(AppError::new("SIGNING_CHANGED", "放弃结果前文件已变化"));
    }
    let mut journal = read_journal(app, &artifact.task_id)?;
    if source != result_path(app, &journal)? {
        return Err(AppError::new("SIGNING_RESULT", "签名结果归属不匹配"));
    }
    // The tombstone must be durable before deleting. Startup never resurrects this result.
    journal.discarded = true;
    journal.sha256 = artifact.sha256;
    write_journal(app, &journal)?;
    std::fs::remove_file(&source)?;
    if journal.kind == "keystore" {
        let secondary = source.with_extension("normalizing");
        if secondary.try_exists()? {
            std::fs::remove_file(secondary)?;
        }
    }
    app.store.forget_artifact(artifact_id, &source)?;
    app.update(&journal.task_id, |t| {
        t.message = "签名结果缓存已清理，已另存文件和任务记录保留".into()
    })?;
    app.notify();
    Ok(())
}

#[cfg(test)]
pub(crate) async fn test_generated_key_recovery(app: &Arc<App>, source: &Path, password: &str) {
    let original = app
        .store
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.operation.kind == "keystore_create")
        .unwrap();
    let mut task = original.clone();
    task.id = uuid::Uuid::new_v4().to_string();
    task.request_id = uuid::Uuid::new_v4().to_string();
    task.state = "needs_review".into();
    task.artifact.clear();
    task.operation.args.remove("savedOutput");
    app.store.save_task(&task).unwrap();
    let destination = source.parent().unwrap().join("recovered.p12");
    let journal = Journal {
        task_id: task.id.clone(),
        artifact_id: format!("signing-{}", task.id),
        kind: "keystore".into(),
        extension: "p12".into(),
        request: SigningRequest {
            mode: "create".into(),
            keystore_id: uuid::Uuid::new_v4().to_string(),
            name: "Recovered key".into(),
            alias: "upload".into(),
            output: destination.to_string_lossy().into(),
            ..Default::default()
        },
        sha256: String::new(),
        verified: false,
        discarded: false,
        record: None,
        original_sha256: String::new(),
        replaced_output_sha256: String::new(),
    };
    let file = result_path(app, &journal).unwrap();
    new_copy(source, &file, &digest_file(source).unwrap()).unwrap();
    write_journal(app, &journal).unwrap();
    recover(app).unwrap();
    recover(app).unwrap();
    assert_eq!(
        app.store
            .artifacts()
            .unwrap()
            .iter()
            .filter(|a| a.id == journal.artifact_id)
            .count(),
        1
    );
    assert!(app
        .discard_artifacts(std::slice::from_ref(&journal.artifact_id))
        .is_err());
    assert!(save_result_unlocked(app, &journal.artifact_id, &destination).is_err());
    check_recovered_key(
        app,
        &journal.artifact_id,
        Passwords {
            store_password: password.into(),
            key_password: String::new(),
        },
    )
    .await
    .unwrap();
    save_result_unlocked(app, &journal.artifact_id, &destination).unwrap();
    assert_eq!(
        digest_file(&destination).unwrap(),
        digest_file(source).unwrap()
    );
    let artifact = app.store.artifact(&journal.artifact_id).unwrap().0;
    discard_result(app, &journal.artifact_id, &artifact.sha256).unwrap();
    recover(app).unwrap();
    assert!(app.store.artifact(&journal.artifact_id).is_err());
    assert!(destination.exists());
    assert!(source.exists());
}
