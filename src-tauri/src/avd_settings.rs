use crate::{
    domain::*,
    process::{self, Spec},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn numeric(op: &Operation, key: &str, min: u32, max: u32) -> Result<()> {
    if !op.optional(key).is_empty()
        && !op
            .optional(key)
            .parse::<u32>()
            .ok()
            .is_some_and(|v| (min..=max).contains(&v))
    {
        return Err(AppError::new(
            "INVALID_AVD_SETTING",
            format!("{key} 范围为 {min}～{max}"),
        ));
    }
    Ok(())
}
fn network(value: &str, latency: bool) -> bool {
    if matches!(
        value,
        "full"
            | "none"
            | "gsm"
            | "hscsd"
            | "gprs"
            | "edge"
            | "umts"
            | "hsdpa"
            | "lte"
            | "evdo"
            | "5g"
    ) {
        return (latency && value != "full") || (!latency && value != "none");
    }
    let values: Vec<_> = value.split(':').map(str::parse::<u32>).collect();
    !values.is_empty()
        && values.len() <= 2
        && values.iter().all(|v| {
            v.as_ref().is_ok_and(|n| {
                if latency {
                    *n <= 60000
                } else {
                    (1..=1_000_000).contains(n)
                }
            })
        })
        && (!latency
            || values.len() != 2
            || values[0].as_ref().unwrap() <= values[1].as_ref().unwrap())
}
pub fn validate(op: &Operation) -> Result<()> {
    for key in ["displayName", "skinPath", "sdCardPath"] {
        if op.optional(key).contains(['\n', '\r']) || op.optional(key).len() > 4096 {
            return Err(AppError::new(
                "INVALID_AVD_SETTING",
                "配置值不能含换行或超出长度限制",
            ));
        }
    }
    if op.optional("displayName").len() > 200 {
        return Err(AppError::new(
            "INVALID_DISPLAY_NAME",
            "显示名称不得超过 200 字节",
        ));
    }
    for (key, min, max) in [
        ("cpuCores", 1, 16),
        ("vmHeap", 16, 2048),
        ("screenWidth", 240, 8192),
        ("screenHeight", 240, 8192),
        ("density", 72, 1000),
        ("storageMiB", 2048, 131072),
        ("sdCardMiB", 64, 32768),
    ] {
        numeric(op, key, min, max)?;
    }
    for (key, options) in [
        ("orientation", &["portrait", "landscape"][..]),
        ("bootMode", &["quick", "cold"][..]),
        ("skinMode", &["keep", "none", "custom"][..]),
        ("sdCardMode", &["keep", "none", "size", "external"][..]),
        ("keyboard", &["yes", "no"][..]),
        ("audioInput", &["yes", "no"][..]),
        ("battery", &["yes", "no"][..]),
        ("gps", &["yes", "no"][..]),
        ("showDeviceFrame", &["yes", "no"][..]),
    ] {
        if !op.optional(key).is_empty() && !options.contains(&op.optional(key)) {
            return Err(AppError::new(
                "INVALID_AVD_SETTING",
                format!("不支持的 {key} 配置"),
            ));
        }
    }
    for key in ["cameraFront", "cameraBack"] {
        let value = op.optional(key);
        if key == "cameraFront" && value == "virtualscene" {
            return Err(AppError::new("INVALID_CAMERA", "虚拟场景仅支持后置摄像头"));
        }
        if !value.is_empty()
            && !matches!(value, "none" | "emulated" | "virtualscene")
            && !value.strip_prefix("webcam").is_some_and(|v| {
                !v.is_empty() && v.len() <= 2 && v.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return Err(AppError::new(
                "INVALID_CAMERA",
                "摄像头须为 none、emulated、virtualscene 或 webcam 编号",
            ));
        }
    }
    for (key, latency) in [("networkSpeed", false), ("networkLatency", true)] {
        if !op.optional(key).is_empty() && !network(op.optional(key), latency) {
            return Err(AppError::new(
                "INVALID_NETWORK_PROFILE",
                "不支持的网络速度或延迟，可使用预设或数值 / 范围",
            ));
        }
    }
    if op.optional("skinMode") == "custom" {
        let path = Path::new(op.arg("skinPath")?);
        if !path.is_absolute() || !path.join("layout").is_file() {
            return Err(AppError::new(
                "INVALID_SKIN",
                "请选择包含 layout 文件的完整皮肤目录",
            ));
        }
    }
    if op.optional("sdCardMode") == "size" {
        op.arg("sdCardMiB")?;
    }
    if op.optional("sdCardMode") == "external" {
        let path = Path::new(op.arg("sdCardPath")?);
        if !path.is_absolute()
            || !path.is_file()
            || path
                .extension()
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("img"))
        {
            return Err(AppError::new(
                "INVALID_SD_CARD",
                "请选择现有的绝对路径 .img 扩展存储镜像",
            ));
        }
    }
    Ok(())
}
pub fn changes(op: &Operation) -> Result<BTreeMap<String, String>> {
    validate(op)?;
    let mut changes = BTreeMap::new();
    for (arg, key) in [
        ("displayName", "avd.ini.displayname"),
        ("memory", "hw.ramSize"),
        ("gpu", "hw.gpu.mode"),
        ("cpuCores", "hw.cpu.ncore"),
        ("vmHeap", "vm.heapSize"),
        ("screenWidth", "hw.lcd.width"),
        ("screenHeight", "hw.lcd.height"),
        ("density", "hw.lcd.density"),
        ("orientation", "hw.initialOrientation"),
        ("cameraFront", "hw.camera.front"),
        ("cameraBack", "hw.camera.back"),
        ("networkSpeed", "runtime.network.speed"),
        ("networkLatency", "runtime.network.latency"),
        ("keyboard", "hw.keyboard"),
        ("audioInput", "hw.audioInput"),
        ("battery", "hw.battery"),
        ("gps", "hw.gps"),
        ("showDeviceFrame", "showDeviceFrame"),
    ] {
        if !op.optional(arg).is_empty() {
            changes.insert(key.into(), op.optional(arg).into());
        }
    }
    if !op.optional("gpu").is_empty() {
        changes.insert("hw.gpu.enabled".into(), "yes".into());
    }
    if !op.optional("storage").is_empty() {
        changes.insert(
            "disk.dataPartition.size".into(),
            format!("{}G", op.optional("storage")),
        );
    }
    if !op.optional("storageMiB").is_empty() {
        changes.insert(
            "disk.dataPartition.size".into(),
            format!("{}M", op.optional("storageMiB")),
        );
    }
    if !op.optional("bootMode").is_empty() {
        changes.insert(
            "fastboot.forceColdBoot".into(),
            if op.optional("bootMode") == "cold" {
                "yes"
            } else {
                "no"
            }
            .into(),
        );
        changes.insert(
            "fastboot.forceFastBoot".into(),
            if op.optional("bootMode") == "quick" {
                "yes"
            } else {
                "no"
            }
            .into(),
        );
        changes.insert("fastboot.forceChosenSnapshotBoot".into(), "no".into());
    }
    if op.optional("skinMode") == "none" {
        changes.insert("showDeviceFrame".into(), "no".into());
        changes.insert("skin.path".into(), String::new());
        if !op.optional("screenWidth").is_empty() && !op.optional("screenHeight").is_empty() {
            changes.insert(
                "skin.name".into(),
                format!(
                    "{}x{}",
                    op.optional("screenWidth"),
                    op.optional("screenHeight")
                ),
            );
        }
    } else if op.optional("skinMode") == "custom" {
        let path = Path::new(op.arg("skinPath")?);
        changes.insert("skin.path".into(), path.to_string_lossy().into());
        changes.insert(
            "skin.name".into(),
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
        );
    }
    match op.optional("sdCardMode") {
        "none" => {
            changes.insert("devkit.sdcard.managed".into(), "no".into());
            changes.insert("hw.sdCard".into(), "no".into());
            changes.insert("sdcard.path".into(), String::new());
            changes.insert("sdcard.size".into(), String::new());
        }
        "size" => {
            changes.insert("devkit.sdcard.managed".into(), "yes".into());
            changes.insert("hw.sdCard".into(), "yes".into());
            changes.insert("sdcard.size".into(), format!("{}M", op.arg("sdCardMiB")?));
        }
        "external" => {
            changes.insert("devkit.sdcard.managed".into(), "no".into());
            changes.insert("hw.sdCard".into(), "yes".into());
            changes.insert("sdcard.path".into(), op.arg("sdCardPath")?.into());
            changes.insert("sdcard.size".into(), String::new());
        }
        _ => {}
    }
    Ok(changes)
}
pub fn resolved_changes(
    op: &Operation,
    current: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut result = changes(op)?;
    let numeric_skin = current.get("skin.name").is_some_and(|name| {
        name.split_once('x').is_some_and(|(width, height)| {
            width.parse::<u32>().is_ok() && height.parse::<u32>().is_ok()
        })
    });
    let resize_numeric_skin = numeric_skin
        && matches!(op.optional("skinMode"), "" | "keep")
        && (!op.optional("screenWidth").is_empty() || !op.optional("screenHeight").is_empty());
    if op.optional("skinMode") == "none" || resize_numeric_skin {
        let dimension = |label: &str, key: &str| {
            result
                .get(key)
                .or_else(|| current.get(key))
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|value| (240..=8192).contains(value))
                .ok_or_else(|| {
                    AppError::new(
                        "INVALID_SKIN",
                        format!("无皮肤模式需要有效的{label}，请填写屏幕尺寸"),
                    )
                })
        };
        let width = dimension("屏幕宽度", "hw.lcd.width")?;
        let height = dimension("屏幕高度", "hw.lcd.height")?;
        result.insert("skin.name".into(), format!("{width}x{height}"));
    }
    Ok(result)
}
pub async fn create_sd_card(
    settings: &Settings,
    data: &Path,
    op: &Operation,
    task: &str,
) -> Result<Option<PathBuf>> {
    if op.optional("sdCardMode") != "size" {
        return Ok(None);
    }
    let tool = Path::new(&settings.sdk_root).join("emulator/mksdcard.exe");
    if !tool.is_file() {
        return Err(AppError::new(
            "MKSDCARD_MISSING",
            "SDK 缺少 emulator/mksdcard.exe",
        ));
    }
    let path = data.join(format!("devkit-sd-{}.img", digest(task.as_bytes())));
    if path.exists() {
        return Err(AppError::new(
            "SD_CARD_EXISTS",
            "新扩展存储镜像路径已存在，未覆盖",
        ));
    }
    let mut spec = Spec::new(
        tool,
        vec![
            format!("{}M", op.arg("sdCardMiB")?),
            path.to_string_lossy().into(),
        ],
    );
    spec.timeout = 90;
    process::run(spec).await?.success()?;
    if !path.is_file() {
        return Err(AppError::new("SD_CARD_FAILED", "扩展存储镜像未生成"));
    }
    Ok(Some(path))
}
pub fn launch_args(config: &BTreeMap<String, String>) -> Result<Vec<String>> {
    let mut args = Vec::new();
    for (key, flag, latency) in [
        ("runtime.network.speed", "-netspeed", false),
        ("runtime.network.latency", "-netdelay", true),
    ] {
        if let Some(value) = config.get(key).filter(|value| !value.is_empty()) {
            if !network(value, latency) {
                return Err(AppError::new(
                    "INVALID_NETWORK_PROFILE",
                    "AVD 网络配置无效，请在编辑面板修正",
                ));
            }
            args.extend([flag.into(), value.clone()]);
        }
    }
    Ok(args)
}
pub fn assert_no_shared_storage(avd: &Avd, settings: &Settings) -> Result<()> {
    let root = std::fs::canonicalize(&avd.data_path)?;
    for other in crate::android::avds(settings)
        .into_iter()
        .filter(|other| other.id != avd.id)
    {
        for key in [
            "sdcard.path",
            "disk.dataPartition.path",
            "disk.cachePartition.path",
        ] {
            if let Some(value) = other.config.get(key).filter(|value| !value.is_empty()) {
                let path = Path::new(value);
                let path = if path.is_absolute() {
                    path.to_owned()
                } else {
                    Path::new(&other.data_path).join(path)
                };
                if std::fs::canonicalize(path).is_ok_and(|path| path.starts_with(&root)) {
                    return Err(AppError::new(
                        "SHARED_AVD_STORAGE",
                        format!(
                            "{} 引用此 AVD 内的存储镜像，未清理共享文件",
                            other.display_name
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}
pub async fn reset_managed_sd(settings: &Settings, avd: &Avd) -> Result<()> {
    if avd
        .config
        .get("devkit.sdcard.managed")
        .is_none_or(|value| value != "yes")
    {
        return Ok(());
    }
    let Some(path) = avd
        .config
        .get("sdcard.path")
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let root = std::fs::canonicalize(&avd.data_path)?;
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    let canonical = std::fs::canonicalize(&path)?;
    if !canonical.starts_with(&root) {
        return Err(AppError::new(
            "UNSAFE_SD_CARD",
            "扩展存储镜像在 AVD 目录之外，未清除",
        ));
    }
    let size = avd.config.get("sdcard.size").cloned().unwrap_or_default();
    let number = size
        .strip_suffix('M')
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (64..=32768).contains(value))
        .ok_or_else(|| AppError::new("INVALID_SD_CARD", "扩展存储容量无效，未清除"))?;
    let tool = Path::new(&settings.sdk_root).join("emulator/mksdcard.exe");
    if !tool.is_file() {
        return Err(AppError::new(
            "MKSDCARD_MISSING",
            "缺少扩展存储格式化工具，未清除",
        ));
    }
    let overlay = PathBuf::from(format!("{}.qcow2", path.to_string_lossy()));
    if overlay.exists() && !std::fs::canonicalize(&overlay)?.starts_with(&root) {
        return Err(AppError::new(
            "UNSAFE_SD_CARD",
            "扩展存储覆盖层在 AVD 目录之外，未清除",
        ));
    }
    let reset = || -> std::io::Result<()> {
        if overlay.is_file() {
            std::fs::remove_file(&overlay)?;
        }
        std::fs::remove_file(&path)
    };
    reset().map_err(|error| {
        AppError::new("PARTIAL_COMPLETE", "扩展存储清理未完成，请检查原 AVD")
            .detail(error.to_string())
    })?;
    let mut spec = Spec::new(
        tool,
        vec![format!("{number}M"), path.to_string_lossy().into()],
    );
    spec.timeout = 90;
    process::run(spec)
        .await
        .and_then(process::Output::success)
        .map_err(|error| {
            AppError::new("PARTIAL_COMPLETE", "扩展存储重置未完成，请检查原 AVD")
                .detail(error.to_string())
        })?;
    Ok(())
}
pub fn summary(op: &Operation) -> Result<Vec<String>> {
    Ok(changes(op)?
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| format!("{key} = {value}"))
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn operation(values: &[(&str, &str)]) -> Operation {
        Operation {
            kind: "avd_edit".into(),
            target: None,
            args: values
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        }
    }
    #[test]
    fn resolution_skin_tracks_screen_edits_and_none_uses_existing_dimensions() {
        let current = BTreeMap::from([
            ("skin.name".into(), "1080x1920".into()),
            ("hw.lcd.width".into(), "1080".into()),
            ("hw.lcd.height".into(), "1920".into()),
        ]);
        let changes = resolved_changes(&operation(&[("screenWidth", "1440")]), &current).unwrap();
        assert_eq!(changes["skin.name"], "1440x1920");
        let changes = resolved_changes(&operation(&[("skinMode", "none")]), &current).unwrap();
        assert_eq!(changes["skin.name"], "1080x1920");
        assert!(changes["skin.path"].is_empty());
        assert!(resolved_changes(&operation(&[("skinMode", "none")]), &BTreeMap::new()).is_err());
    }
    #[test]
    fn advanced_settings_map_to_real_config_and_launch_options() {
        let op = operation(&[
            ("cpuCores", "4"),
            ("vmHeap", "256"),
            ("screenWidth", "1536"),
            ("screenHeight", "2048"),
            ("density", "320"),
            ("orientation", "landscape"),
            ("cameraFront", "none"),
            ("cameraBack", "virtualscene"),
            ("networkSpeed", "1024:2048"),
            ("networkLatency", "20:100"),
            ("bootMode", "cold"),
            ("storageMiB", "10240"),
            ("keyboard", "yes"),
            ("skinMode", "none"),
        ]);
        let config = changes(&op).unwrap();
        assert_eq!(config["hw.cpu.ncore"], "4");
        assert_eq!(config["hw.lcd.width"], "1536");
        assert_eq!(config["disk.dataPartition.size"], "10240M");
        assert_eq!(config["fastboot.forceColdBoot"], "yes");
        assert_eq!(config["hw.camera.back"], "virtualscene");
        assert_eq!(
            launch_args(&config).unwrap(),
            ["-netspeed", "1024:2048", "-netdelay", "20:100"]
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.ini");
        std::fs::write(
            &path,
            "# Keep external setting\nunknown.vendor.key=keep\nhw.cpu.ncore=2\n",
        )
        .unwrap();
        let fingerprint = digest(&std::fs::read(&path).unwrap());
        crate::android::write_properties_preserving(&path, &config, &fingerprint).unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("unknown.vendor.key=keep"));
        assert_eq!(
            crate::android::write_properties_preserving(&path, &config, &fingerprint)
                .unwrap_err()
                .code,
            "EXTERNAL_CHANGE"
        );
    }
    #[test]
    fn invalid_or_unsafe_advanced_values_never_reach_configuration() {
        for (key, value) in [
            ("cpuCores", "0"),
            ("cpuCores", "17"),
            ("vmHeap", "0"),
            ("screenWidth", "99999"),
            ("density", "0"),
            ("networkLatency", "100:20"),
            ("networkSpeed", "full;calc"),
            ("cameraFront", "webcam0\nhw.cpu.ncore=8"),
            ("cameraFront", "virtualscene"),
            ("displayName", "bad\nvalue"),
            ("skinMode", "unknown"),
            ("sdCardMiB", "1"),
            ("orientation", "sideways"),
        ] {
            assert!(
                changes(&operation(&[(key, value)])).is_err(),
                "{key}={value}"
            );
        }
        assert!(changes(&operation(&[
            ("skinMode", "custom"),
            ("skinPath", "relative")
        ]))
        .is_err());
        assert!(changes(&operation(&[
            ("sdCardMode", "external"),
            ("sdCardPath", "missing.img")
        ]))
        .is_err());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("layout"), "skin layout").unwrap();
        assert!(changes(&operation(&[
            ("skinMode", "custom"),
            ("skinPath", dir.path().to_str().unwrap())
        ]))
        .is_ok());
        let config = changes(&operation(&[("sdCardMode", "none")])).unwrap();
        assert_eq!(config["hw.sdCard"], "no");
        assert_eq!(config["sdcard.path"], "");
    }
    #[test]
    fn shared_storage_blocks_destructive_avd_operations() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("avds");
        let data = root.join("Primary.avd");
        let other = root.join("Secondary.avd");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let image = data.join("shared.img");
        std::fs::write(&image, "fixture").unwrap();
        std::fs::write(
            root.join("Primary.ini"),
            format!("path={}\n", data.display()),
        )
        .unwrap();
        std::fs::write(
            root.join("Secondary.ini"),
            format!("path={}\n", other.display()),
        )
        .unwrap();
        std::fs::write(data.join("config.ini"), "hw.ramSize=2048\n").unwrap();
        std::fs::write(
            other.join("config.ini"),
            format!("sdcard.path={}\n", image.display()),
        )
        .unwrap();
        let settings = Settings {
            avd_roots: vec![root.to_string_lossy().into()],
            ..Default::default()
        };
        let avd = crate::android::avds(&settings)
            .into_iter()
            .find(|avd| avd.name == "Primary")
            .unwrap();
        assert_eq!(
            assert_no_shared_storage(&avd, &settings).unwrap_err().code,
            "SHARED_AVD_STORAGE"
        );
        assert!(image.is_file());
    }
}
