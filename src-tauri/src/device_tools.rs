//! Pinned upstream tools, installed privately only after a reviewed task.
use crate::{app::App, domain::*};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
pub struct Tool {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub entry: String,
}
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceToolStatus {
    pub name: String,
    pub version: String,
    pub ready: bool,
}
pub fn definition(name: &str) -> Result<Tool> {
    let mut tools: BTreeMap<String, Tool> =
        serde_json::from_str(include_str!("../resources/device-tools.json"))?;
    tools
        .remove(name)
        .ok_or_else(|| AppError::new("DEVICE_TOOL", "工具类型无效"))
}
fn directory(app: &App, name: &str, tool: &Tool) -> PathBuf {
    app.data
        .join("tools")
        .join(format!("{name}-{}", tool.version))
}
pub fn path(app: &App, name: &str) -> Result<PathBuf> {
    let tool = definition(name)?;
    let root = directory(app, name, &tool);
    crate::phone_files::regular_local(&root, true)
        .map_err(|_| AppError::new("DEVICE_TOOL_MISSING", "请先准备所需工具"))?;
    let manifest = root.join("verified.json");
    crate::phone_files::regular_local(&manifest, false)
        .map_err(|_| AppError::new("DEVICE_TOOL_MISSING", "请先准备所需工具"))?;
    if manifest.metadata()?.len() > 1024 * 1024 {
        return Err(AppError::new(
            "DEVICE_TOOL_CHANGED",
            "工具文件已变化，请重新准备工具",
        ));
    }
    let files: BTreeMap<String, String> = std::fs::read(manifest)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| AppError::new("DEVICE_TOOL_MISSING", "请先准备所需工具"))?;
    if files.is_empty() || files.len() > 1000 || !files.contains_key(&tool.entry) {
        return Err(AppError::new("DEVICE_TOOL_MISSING", "请先准备所需工具"));
    }
    for (relative, expected) in files {
        if !safe_relative(&relative)
            || expected.len() != 64
            || !expected.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(AppError::new(
                "DEVICE_TOOL_CHANGED",
                "工具文件已变化，请重新准备工具",
            ));
        }
        let file = root.join(&relative);
        let unchanged = (|| -> Result<bool> {
            crate::phone_files::regular_local(&file, false)?;
            Ok(file.canonicalize()?.starts_with(root.canonicalize()?)
                && digest_file(&file)? == expected)
        })()
        .unwrap_or(false);
        if !unchanged {
            return Err(AppError::new(
                "DEVICE_TOOL_CHANGED",
                "工具文件已变化，请重新准备工具",
            ));
        }
    }
    Ok(root.join(tool.entry))
}
fn safe_relative(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && name.split('/').all(crate::phone_files::safe_component)
        && Path::new(name)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}
fn check_size(actual: u64, expected: u64, complete: bool) -> Result<()> {
    if actual > expected || (complete && actual != expected) {
        return Err(AppError::new(
            "DEVICE_TOOL_SIZE",
            "官方工具下载大小与固定版本不符",
        ));
    }
    Ok(())
}
async fn download_tool(
    settings: &Settings,
    tool: &Tool,
    destination: &Path,
    progress: impl Fn(u64, u64),
) -> Result<PathBuf> {
    let mut response = crate::bootstrap::client(settings)?
        .get(&tool.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| AppError::new("DOWNLOAD_FAILED", e.to_string()))?;
    if let Some(size) = response.content_length() {
        check_size(size, tool.size, true)?;
    }
    let archive = destination.join("official.download");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archive)?;
    let mut bytes = 0u64;
    let mut hash = sha2::Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::new("DOWNLOAD_FAILED", e.to_string()))?
    {
        bytes = bytes.saturating_add(chunk.len() as u64);
        check_size(bytes, tool.size, false)?;
        file.write_all(&chunk)?;
        hash.update(&chunk);
        progress(bytes, tool.size);
    }
    check_size(bytes, tool.size, true)?;
    if format!("{:x}", hash.finalize()) != tool.sha256 {
        return Err(AppError::new("CHECKSUM_FAILED", "归档校验失败，文件未安装"));
    }
    file.sync_all()?;
    drop(file);
    Ok(archive)
}
fn assert_unused(app: &App, name: &str) -> Result<()> {
    if name == "scrcpy"
        && app
            .store
            .active_tasks()?
            .iter()
            .any(|task| task.operation.kind == "mirror_start")
    {
        return Err(AppError::new(
            "MIRROR_ACTIVE",
            "请先停止投屏，再重新准备投屏工具",
        ));
    }
    Ok(())
}
pub fn statuses(app: &App) -> Result<Vec<DeviceToolStatus>> {
    ["scrcpy", "bundletool"]
        .into_iter()
        .map(|name| {
            Ok(DeviceToolStatus {
                name: name.into(),
                version: definition(name)?.version,
                ready: path(app, name).is_ok(),
            })
        })
        .collect()
}
impl App {
    pub async fn prepare_device_tool(
        &self,
        id: &str,
        name: &str,
        settings: &Settings,
    ) -> Result<String> {
        if path(self, name).is_ok() {
            return Ok("工具已就绪".into());
        }
        assert_unused(self, name)?;
        uuid::Uuid::parse_str(id)
            .map_err(|_| AppError::new("DEVICE_TOOL_TASK", "工具准备任务标识无效"))?;
        let tool = definition(name)?;
        let root = directory(self, name, &tool);
        let staging = self.data.join("tools").join(format!(".prepare-{id}"));
        std::fs::create_dir_all(&staging)?;
        let result = async {
            crate::capacity::budget(&[(staging.clone(), tool.size * 8, "工具准备")])?;
            self.phase(id, "downloading", "下载并校验官方工具")?;
            self.update(id, |t| t.cancellable = true)?;
            let download = download_tool(settings, &tool, &staging, |done, total| {
                let _ = self.update(id, |t| t.progress = (total > 0).then_some(done as f64 * 100.0 / total as f64));
            });
            tokio::pin!(download);
            let archive = loop {
                tokio::select! {
                    result = &mut download => break result?,
                    _ = tokio::time::sleep(Duration::from_millis(200)) => {
                        if self.cancelled(id) { return Err(AppError::new("CANCELLED", "已取消工具准备")); }
                    }
                }
            };
            self.phase(id, "verifying", "校验并安装私有工具")?;
            let unpacked = staging.join("unpacked");
            std::fs::create_dir(&unpacked)?;
            if name == "scrcpy" {
                crate::bootstrap::extract_zip(&archive, &unpacked)?;
            } else {
                std::fs::copy(&archive, unpacked.join("bundletool.jar"))?;
            }
            if !unpacked.join(&tool.entry).is_file() { return Err(AppError::new("DEVICE_TOOL_ARCHIVE", "官方工具归档缺少入口文件")); }
            fn manifest(root: &std::path::Path, dir: &std::path::Path, files: &mut BTreeMap<String, String>) -> Result<()> {
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    if entry.file_type()?.is_dir() { manifest(root, &entry.path(), files)?; }
                    else {
                        crate::phone_files::regular_local(&entry.path(), false)?;
                        files.insert(entry.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), digest_file(&entry.path())?);
                    }
                }
                Ok(())
            }
            let mut files = BTreeMap::new();
            manifest(&unpacked, &unpacked, &mut files)?;
            std::fs::write(unpacked.join("verified.json"), serde_json::to_vec(&files)?)?;
            if self.cancelled(id) { return Err(AppError::new("CANCELLED", "已取消工具准备")); }
            self.update(id, |t| t.cancellable = false)?;
            let _configuration_guard=self.configuration_gate.lock().unwrap();
            assert_unused(self,name)?;
            // Only this version's app-owned directory can be replaced; never user tools.
            if root.exists() { crate::phone_files::regular_local(&root, true)?; std::fs::remove_dir_all(&root)?; }
            std::fs::rename(&unpacked, &root)?;
            path(self, name)?;
            Ok("官方工具已校验并就绪".into())
        }.await;
        let _ = std::fs::remove_dir_all(staging);
        result
    }
}

#[tauri::command]
pub fn device_tools(app: tauri::State<'_, std::sync::Arc<App>>) -> Result<Vec<DeviceToolStatus>> {
    statuses(&app)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tools_are_pinned_and_unknown_names_cannot_choose_paths() {
        for name in ["scrcpy", "bundletool"] {
            let tool = definition(name).unwrap();
            assert_eq!(tool.sha256.len(), 64);
            assert!(tool.url.starts_with("https://github.com/"));
            assert!(!tool.entry.contains(".."));
        }
        assert!(definition("../../other").is_err());
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(temp.path().into()).unwrap();
        assert!(path(&app, "scrcpy").is_err());
    }
    #[test]
    fn pinned_download_sizes_and_manifest_paths_reject_incomplete_or_escaping_input() {
        check_size(10, 10, true).unwrap();
        check_size(9, 10, false).unwrap();
        for (actual, complete) in [(9, true), (11, true), (11, false)] {
            assert!(check_size(actual, 10, complete).is_err());
        }
        for path in [
            "../outside",
            "/absolute",
            "C:/windows",
            "a\\b",
            "a//b",
            "a/CON.txt",
            "a/./b",
        ] {
            assert!(!safe_relative(path), "{path}");
        }
        assert!(safe_relative("scrcpy-win64-v5.0.1/scrcpy.exe"));
    }
    #[test]
    fn missing_or_changed_dependencies_make_the_whole_tool_unavailable() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(temp.path().into()).unwrap();
        let tool = definition("scrcpy").unwrap();
        let root = directory(&app, "scrcpy", &tool);
        let entry = root.join(&tool.entry);
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, b"exe").unwrap();
        let dependency = entry.parent().unwrap().join("SDL3.dll");
        std::fs::write(&dependency, b"dll").unwrap();
        let files = BTreeMap::from([
            (tool.entry.clone(), digest_file(&entry).unwrap()),
            (
                "scrcpy-win64-v5.0.1/SDL3.dll".into(),
                digest_file(&dependency).unwrap(),
            ),
        ]);
        std::fs::write(
            root.join("verified.json"),
            serde_json::to_vec(&files).unwrap(),
        )
        .unwrap();
        assert_eq!(path(&app, "scrcpy").unwrap(), entry);
        std::fs::write(&dependency, b"changed").unwrap();
        assert_eq!(
            path(&app, "scrcpy").unwrap_err().code,
            "DEVICE_TOOL_CHANGED"
        );
        std::fs::remove_file(dependency).unwrap();
        assert!(path(&app, "scrcpy").is_err());
    }
    #[test]
    fn active_mirroring_prevents_runtime_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(temp.path().into()).unwrap();
        let mut task = crate::regression_tests::task("mirror", "running");
        task.operation.kind = "mirror_start".into();
        app.store.save_task(&task).unwrap();
        assert_eq!(
            assert_unused(&app, "scrcpy").unwrap_err().code,
            "MIRROR_ACTIVE"
        );
        assert_unused(&app, "bundletool").unwrap();
    }
}
