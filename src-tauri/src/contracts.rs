use crate::domain::*;
use ts_rs::TS;
fn source() -> String {
    let declarations = [
        AppError::decl(),
        Settings::decl(),
        SdkSite::decl(),
        SdkSiteStatus::decl(),
        Environment::decl(),
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
        ApkInfo::decl(),
        Workspace::decl(),
        LogFilter::decl(),
        LogEntry::decl(),
        LogSnapshot::decl(),
        Artifact::decl(),
        ExportPreview::decl(),
        CloseStatus::decl(),
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
