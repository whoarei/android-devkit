use crate::domain::*;
use ts_rs::TS;
fn source() -> String {
    let declarations = [
        AppError::decl(),
        Settings::decl(),
        SdkDownloadSource::decl(),
        SdkSourceCheck::decl(),
        SdkSite::decl(),
        SdkSiteStatus::decl(),
        Environment::decl(),
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
        "// Generated from Rust DTOs. Run pnpm bindings to update.\n{}\n",
        declarations
            .iter()
            .map(|d| format!("export {d}"))
            .collect::<Vec<_>>()
            .join("\n")
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
