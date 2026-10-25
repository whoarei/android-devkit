use crate::domain::*;
use ts_rs::TS;
fn source() -> String {
    let declarations = [
        AppError::decl(),
        Settings::decl(),
        crate::preferences::WindowPlacement::decl(),
        crate::preferences::UserPreferences::decl(),
        SdkDownloadSource::decl(),
        SdkSourceCheck::decl(),
        SdkSite::decl(),
        SdkSiteStatus::decl(),
        Environment::decl(),
        crate::capacity::Usage::decl(),
        crate::java::JavaCheck::decl(),
        crate::java::JavaEnvironment::decl(),
        SdkPackage::decl(),
        Avd::decl(),
        Device::decl(),
        Target::decl(),
        Operation::decl(),
        License::decl(),
        Plan::decl(),
        Task::decl(),
        TaskQuery::decl(),
        TaskPage::decl(),
        TaskDeletionSummary::decl(),
        TaskDeleteRequest::decl(),
        TaskDeleteIssue::decl(),
        TaskDeleteResult::decl(),
        ApkInfo::decl(),
        crate::phone_apk::ApkMember::decl(),
        crate::phone_apk::ApkSet::decl(),
        crate::phone_files::RemoteEntry::decl(),
        crate::phone_files::TransferItem::decl(),
        crate::phone_files::TransferSet::decl(),
        Workspace::decl(),
        LogFilter::decl(),
        LogEntry::decl(),
        LogSnapshot::decl(),
        Artifact::decl(),
        ExportPreview::decl(),
        CloseStatus::decl(),
        crate::updater::AppUpdateInfo::decl(),
        crate::updater::AppUpdateProgress::decl(),
        crate::terminal::TerminalInfo::decl(),
        crate::terminal::TerminalEvent::decl(),
    ];
    format!(
        "// Generated from Rust DTOs. Run pnpm bindings to update.\n{}\nexport const defaultUserPreferences: UserPreferences = {};\n",
        declarations
            .iter()
            .map(|d| format!("export {d}"))
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::to_string_pretty(&crate::preferences::UserPreferences::default()).unwrap()
    )
}
fn destination() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src/bindings.ts")
}
#[test]
fn typescript_contract_is_current() {
    assert_eq!(
        std::fs::read_to_string(destination()).unwrap(),
        source(),
        "Rust IPC DTO changed; run pnpm bindings and review generated changes"
    );
}
#[test]
#[ignore = "explicit code generation"]
fn export_typescript_contract() {
    std::fs::write(destination(), source()).unwrap();
}
