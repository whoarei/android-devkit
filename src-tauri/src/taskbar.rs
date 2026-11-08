//! Window icons are in-memory; pinned/grouped taskbar entries need a durable
//! ICO and Shell properties. Only shortcuts targeting this exact executable
//! with a compatible AppUserModelID are eligible for icon updates.
use crate::{app::App, domain::*};
use image::ImageEncoder;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use tauri::Manager;
use windows::{
    core::{Interface, BSTR, GUID, HSTRING, PCWSTR},
    Win32::{
        Foundation::{HWND, PROPERTYKEY, RPC_E_CHANGED_MODE},
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
                StructuredStorage::{PropVariantChangeType, PROPVARIANT, PVCHF_DEFAULT},
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READWRITE,
            },
            Variant::VT_LPWSTR,
        },
        UI::Shell::{
            FOLDERID_Programs, FOLDERID_UserPinned, IShellLinkW,
            PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
            SHChangeNotify, SHGetKnownFolderPath, SetCurrentProcessExplicitAppUserModelID,
            ShellLink, KF_FLAG_DEFAULT, SHCNE_UPDATEITEM, SHCNF_FLUSHNOWAIT, SHCNF_PATHW,
            SLGP_RAWPATH,
        },
    },
};

const fn property(pid: u32) -> PROPERTYKEY {
    PROPERTYKEY {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid,
    }
}
const APP_ID: PROPERTYKEY = property(5);
const RELAUNCH_COMMAND: PROPERTYKEY = property(2);
const RELAUNCH_ICON: PROPERTYKEY = property(3);
const RELAUNCH_NAME: PROPERTYKEY = property(4);

fn shell_error(error: impl std::fmt::Display) -> AppError {
    AppError::new("TASKBAR_ICON", "无法刷新任务栏图标").detail(error.to_string())
}

fn identity(identifier: &str) -> String {
    // Isolated QA profiles must not merge into the installed app's taskbar group.
    if let Some(data) = std::env::var_os("ANDROID_DEVKIT_DATA_DIR") {
        let digest = format!("{:x}", Sha256::digest(data.to_string_lossy().as_bytes()));
        format!("{identifier}.qa.{}", &digest[..16])
    } else {
        identifier.to_owned()
    }
}

pub fn initialize_identity(identifier: &str) {
    // Match the NSIS shortcut identity before presenting any windows. Never put
    // the theme or version in the production AppID: that would split grouping.
    if let Err(error) =
        unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(identity(identifier))) }
    {
        eprintln!("Taskbar identity: {error}");
    }
}

struct ComApartment(bool);
impl ComApartment {
    fn new() -> Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        result.ok().map_err(shell_error)?;
        Ok(Self(true))
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn set_string(store: &IPropertyStore, key: &PROPERTYKEY, text: &str) -> Result<()> {
    // Shell string properties use VT_LPWSTR. The windows crate's From<&str>
    // creates VT_BSTR, so convert explicitly instead of relying on coercion.
    let mut value = PROPVARIANT::default();
    unsafe {
        PropVariantChangeType(
            &mut value,
            &PROPVARIANT::from(text),
            PVCHF_DEFAULT,
            VT_LPWSTR,
        )
        .map_err(shell_error)?;
        store.SetValue(key, &value).map_err(shell_error)
    }
}

fn get_string(store: &IPropertyStore, key: &PROPERTYKEY) -> Result<String> {
    let value = unsafe { store.GetValue(key).map_err(shell_error)? };
    // An unset AppID is permitted for a legacy shortcut with the exact target.
    if unsafe { value.Anonymous.Anonymous.vt.0 } == 0 {
        return Ok(String::new());
    }
    BSTR::try_from(&value)
        .map(|value| value.to_string())
        .map_err(shell_error)
}

fn apply_window_properties(hwnd: HWND, app_id: &str, executable: &Path, icon: &Path) -> Result<()> {
    let _apartment = ComApartment::new()?;
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd).map_err(shell_error)? };
    set_string(
        &store,
        &RELAUNCH_COMMAND,
        &format!("\"{}\"", executable.display()),
    )?;
    set_string(&store, &RELAUNCH_NAME, "Android DevKit")?;
    set_string(&store, &RELAUNCH_ICON, &format!("{},0", icon.display()))?;
    set_string(&store, &APP_ID, app_id)?;
    // Clear Shell property values before HWND destruction to release resources.
    let installed = unsafe {
        windows_sys::Win32::UI::Shell::SetWindowSubclass(
            hwnd.0,
            Some(window_cleanup),
            0x41444b49,
            0,
        )
    };
    if installed == 0 {
        clear_window_properties(hwnd)?;
        return Err(shell_error(std::io::Error::last_os_error()));
    }
    // Window properties are applied immediately; this store does not require Commit.
    Ok(())
}

fn clear_window_properties(hwnd: HWND) -> Result<()> {
    let _apartment = ComApartment::new()?;
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd).map_err(shell_error)? };
    for key in [APP_ID, RELAUNCH_ICON, RELAUNCH_NAME, RELAUNCH_COMMAND] {
        unsafe {
            store
                .SetValue(&key, &PROPVARIANT::default())
                .map_err(shell_error)?;
        }
    }
    Ok(())
}

unsafe extern "system" fn window_cleanup(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    if message == windows_sys::Win32::UI::WindowsAndMessaging::WM_NCDESTROY {
        let _ = clear_window_properties(HWND(hwnd));
        windows_sys::Win32::UI::Shell::RemoveWindowSubclass(hwnd, Some(window_cleanup), 0x41444b49);
    }
    windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, message, wparam, lparam)
}

fn ico_bytes(icon: &tauri::image::Image<'_>) -> Result<Vec<u8>> {
    let original = image::RgbaImage::from_raw(icon.width(), icon.height(), icon.rgba().to_vec())
        .ok_or_else(|| shell_error("Invalid RGBA image"))?;
    let sizes = [16u32, 24, 32, 48, 64, 128];
    let mut frames = Vec::new();
    for size in sizes {
        let pixels =
            image::imageops::resize(&original, size, size, image::imageops::FilterType::Lanczos3);
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(pixels.as_raw(), size, size, image::ExtendedColorType::Rgba8)
            .map_err(shell_error)?;
        frames.push(png);
    }
    let mut bytes = vec![0, 0, 1, 0, sizes.len() as u8, 0];
    let mut offset = 6 + sizes.len() * 16;
    for (size, frame) in sizes.into_iter().zip(&frames) {
        bytes.extend_from_slice(&[size as u8, size as u8, 0, 0, 1, 0, 32, 0]);
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += frame.len();
    }
    for frame in frames {
        bytes.extend(frame);
    }
    Ok(bytes)
}

fn persist_icon(directory: &Path, icon: &tauri::image::Image<'_>) -> Result<PathBuf> {
    let bytes = ico_bytes(icon)?;
    let path = directory.join(format!("theme-{:x}.ico", Sha256::digest(&bytes)));
    fs::create_dir_all(directory)?;
    if fs::read(&path).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(path);
    }
    let temporary = directory.join(format!(".{}.part", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    // Keep old content-addressed icons: an existing pinned shortcut may still
    // reference one, including a read-only shortcut that could not be migrated.
    Ok(path)
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn notify(path: &Path) {
    let path = wide(path);
    unsafe {
        SHChangeNotify(
            SHCNE_UPDATEITEM,
            SHCNF_PATHW | SHCNF_FLUSHNOWAIT,
            Some(path.as_ptr().cast()),
            None,
        );
    }
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let result = path.to_string();
        CoTaskMemFree(Some(path.0.cast()));
        result.ok().map(PathBuf::from)
    }
}

fn shortcut_roots() -> Vec<PathBuf> {
    // QA executable/profile checks are always isolated from real user shortcuts.
    if std::env::var_os("ANDROID_DEVKIT_DATA_DIR").is_some() {
        return Vec::new();
    }
    let mut roots = Vec::new();
    if let Some(pinned) = known_folder(&FOLDERID_UserPinned) {
        roots.push(pinned.join("TaskBar"));
        roots.push(pinned.join("ImplicitAppShortcuts"));
    }
    if let Some(programs) = known_folder(&FOLDERID_Programs) {
        roots.push(programs);
    }
    roots
}

fn shortcuts(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack: Vec<_> = roots.iter().map(|path| (path.clone(), 0)).collect();
    while let Some((directory, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten().take(1024) {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && depth < 3 {
                stack.push((entry.path(), depth + 1));
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            {
                result.push(entry.path());
            }
            if result.len() >= 2048 {
                return result;
            }
        }
    }
    result
}

fn text(buffer: &[u16]) -> String {
    String::from_utf16_lossy(
        &buffer[..buffer
            .iter()
            .position(|&value| value == 0)
            .unwrap_or(buffer.len())],
    )
}

fn update_shortcut(path: &Path, executable: &Path, app_id: &str, icon: &Path) -> Result<bool> {
    let original = fs::read(path)?;
    let link: IShellLinkW =
        unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(shell_error)? };
    let persist: IPersistFile = link.cast().map_err(shell_error)?;
    let source = wide(path);
    unsafe {
        persist
            .Load(PCWSTR(source.as_ptr()), STGM_READWRITE)
            .map_err(shell_error)?;
    }
    let mut target = vec![0u16; 32768];
    unsafe {
        link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .map_err(shell_error)?;
    }
    let target = PathBuf::from(text(&target));
    if target
        .file_name()
        .is_none_or(|name| !name.eq_ignore_ascii_case(executable.file_name().unwrap_or_default()))
    {
        return Ok(false);
    }
    let (Ok(target), Ok(current)) = (fs::canonicalize(&target), fs::canonicalize(executable))
    else {
        return Ok(false);
    };
    if target != current || !executable.is_file() {
        return Ok(false);
    }
    let store: IPropertyStore = link.cast().map_err(shell_error)?;
    let owner = get_string(&store, &APP_ID)?;
    if !owner.is_empty() && owner != app_id {
        return Ok(false);
    }
    let mut old_icon = vec![0u16; 32768];
    let mut old_index = 0;
    unsafe {
        link.GetIconLocation(&mut old_icon, &mut old_index)
            .map_err(shell_error)?;
    }
    if Path::new(&text(&old_icon)) == icon && old_index == 0 && owner == app_id {
        return Ok(false);
    }
    let icon_path = wide(icon);
    unsafe {
        link.SetIconLocation(PCWSTR(icon_path.as_ptr()), 0)
            .map_err(shell_error)?;
    }
    if owner.is_empty() {
        set_string(&store, &APP_ID, app_id)?;
        unsafe {
            store.Commit().map_err(shell_error)?;
        }
    }
    // Preserve arguments, target, working directory and all unrelated metadata.
    // Publish only if no concurrent edit changed the shortcut since it was read.
    let temporary = path.with_file_name(format!(".devkit-{}.lnk", uuid::Uuid::new_v4()));
    let output = wide(&temporary);
    let result = (|| -> Result<()> {
        unsafe {
            persist
                .Save(PCWSTR(output.as_ptr()), true)
                .map_err(shell_error)?;
        }
        if fs::read(path)? != original {
            return Err(shell_error("Shortcut changed during icon update"));
        }
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result?;
    notify(path);
    Ok(true)
}

struct Request {
    handle: tauri::AppHandle,
    icon: tauri::image::Image<'static>,
    generation: u64,
}
struct Worker {
    sender: mpsc::Sender<Request>,
    generation: Arc<AtomicU64>,
}

pub fn install(handle: &tauri::AppHandle) -> Result<()> {
    let (sender, receiver) = mpsc::channel::<Request>();
    let generation = Arc::new(AtomicU64::new(0));
    let latest = generation.clone();
    std::thread::Builder::new()
        .name("devkit-taskbar-icons".into())
        .spawn(move || {
            while let Ok(mut request) = receiver.recv() {
                // Coalesce rapid color input without blocking the window/UI thread.
                while let Ok(next) = receiver.recv_timeout(Duration::from_millis(120)) {
                    request = next;
                }
                if let Err(error) = apply_request(request, &latest) {
                    eprintln!("Taskbar appearance: {error}");
                }
            }
        })
        .map_err(shell_error)?;
    handle.manage(Worker { sender, generation });
    Ok(())
}

pub fn refresh_shell(handle: &tauri::AppHandle, icon: tauri::image::Image<'static>) {
    if let Some(worker) = handle.try_state::<Worker>() {
        let generation = worker.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = worker.sender.send(Request {
            handle: handle.clone(),
            icon,
            generation,
        });
    }
}

fn apply_request(request: Request, latest: &Arc<AtomicU64>) -> Result<()> {
    let _apartment = ComApartment::new()?;
    let app = request.handle.state::<Arc<App>>();
    let icon = persist_icon(&app.data.join("state/taskbar-icons"), &request.icon)?;
    let executable = std::env::current_exe()?;
    let app_id = identity(&request.handle.config().identifier);
    for path in shortcuts(&shortcut_roots()) {
        if latest.load(Ordering::SeqCst) != request.generation {
            return Ok(());
        }
        if let Err(error) = update_shortcut(&path, &executable, &app_id, &icon) {
            eprintln!("Taskbar shortcut {}: {error}", path.display());
        }
    }
    let handle = request.handle.clone();
    let latest = latest.clone();
    request
        .handle
        .run_on_main_thread(move || {
            if latest.load(Ordering::SeqCst) != request.generation {
                return;
            }
            if let Some(window) = handle.get_webview_window("main") {
                let result = window.hwnd().map_err(shell_error).and_then(|hwnd| {
                    apply_window_properties(HWND(hwnd.0), &app_id, &executable, &icon)
                });
                if let Err(error) = result {
                    eprintln!("Taskbar properties: {error}");
                }
                notify(&icon);
            }
        })
        .map_err(shell_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::UserPreferences;

    fn create_link(path: &Path, executable: &Path, app_id: &str) {
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap() };
        unsafe {
            link.SetPath(&HSTRING::from(executable.as_os_str()))
                .unwrap();
            link.SetArguments(windows::core::w!("--keep \"draft with spaces\" & literal"))
                .unwrap();
            link.SetWorkingDirectory(&HSTRING::from(executable.parent().unwrap().as_os_str()))
                .unwrap();
        }
        if !app_id.is_empty() {
            let store: IPropertyStore = link.cast().unwrap();
            set_string(&store, &APP_ID, app_id).unwrap();
            unsafe {
                store.Commit().unwrap();
            }
        }
        let persist: IPersistFile = link.cast().unwrap();
        unsafe {
            persist
                .Save(&HSTRING::from(path.as_os_str()), true)
                .unwrap();
        }
    }

    #[test]
    fn durable_icons_cover_dpi_sizes_and_reuse_identical_content() {
        let dir = tempfile::tempdir().unwrap();
        let icon = crate::brand_icon::render(&UserPreferences::default(), false);
        let first = persist_icon(dir.path(), &icon).unwrap();
        assert_eq!(persist_icon(dir.path(), &icon).unwrap(), first);
        let bytes = fs::read(&first).unwrap();
        assert_eq!(&bytes[..4], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), 6);
        for index in 0..6 {
            let entry = 6 + index * 16;
            let size = bytes[entry] as u32;
            let length =
                u32::from_le_bytes(bytes[entry + 8..entry + 12].try_into().unwrap()) as usize;
            let offset =
                u32::from_le_bytes(bytes[entry + 12..entry + 16].try_into().unwrap()) as usize;
            let frame = image::load_from_memory(&bytes[offset..offset + length])
                .unwrap()
                .into_rgba8();
            assert_eq!(frame.dimensions(), (size, size));
            assert!(frame.pixels().any(|pixel| pixel[3] == 0));
            assert!(frame.pixels().any(|pixel| pixel[3] == 255));
        }
        let preferences = UserPreferences {
            color_theme: "purple".into(),
            ..UserPreferences::default()
        };
        let second =
            persist_icon(dir.path(), &crate::brand_icon::render(&preferences, true)).unwrap();
        assert_ne!(first, second);
        assert!(
            first.is_file(),
            "Existing pinned shortcuts must retain a valid icon path"
        );
    }

    #[test]
    fn shortcut_updates_preserve_launch_details_and_reject_other_owners() {
        let _apartment = ComApartment::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("应用 with spaces.exe");
        fs::write(&executable, b"isolated shortcut target").unwrap();
        let icon = persist_icon(
            dir.path(),
            &crate::brand_icon::render(&UserPreferences::default(), false),
        )
        .unwrap();
        let app_id = "dev.android-devkit.desktop.tests";
        let link_path = dir.path().join("renamed launcher.lnk");
        create_link(&link_path, &executable, "");
        assert!(update_shortcut(&link_path, &executable, app_id, &icon).unwrap());
        assert!(!update_shortcut(&link_path, &executable, app_id, &icon).unwrap());
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap() };
        let persist: IPersistFile = link.cast().unwrap();
        unsafe {
            persist
                .Load(&HSTRING::from(link_path.as_os_str()), STGM_READWRITE)
                .unwrap();
        }
        let mut arguments = vec![0u16; 1024];
        let mut working = vec![0u16; 32768];
        let mut actual_icon = vec![0u16; 32768];
        let mut icon_index = -1;
        unsafe {
            link.GetArguments(&mut arguments).unwrap();
            link.GetWorkingDirectory(&mut working).unwrap();
            link.GetIconLocation(&mut actual_icon, &mut icon_index)
                .unwrap();
        }
        assert_eq!(text(&arguments), "--keep \"draft with spaces\" & literal");
        assert_eq!(Path::new(&text(&working)), dir.path());
        assert_eq!(Path::new(&text(&actual_icon)), icon);
        assert_eq!(icon_index, 0);
        assert_eq!(get_string(&link.cast().unwrap(), &APP_ID).unwrap(), app_id);

        let foreign_id = dir.path().join("foreign identity.lnk");
        create_link(&foreign_id, &executable, "other.application");
        let before = fs::read(&foreign_id).unwrap();
        assert!(!update_shortcut(&foreign_id, &executable, app_id, &icon).unwrap());
        assert_eq!(fs::read(foreign_id).unwrap(), before);

        let other_directory = dir.path().join("other copy");
        fs::create_dir(&other_directory).unwrap();
        let other_executable = other_directory.join(executable.file_name().unwrap());
        fs::write(&other_executable, b"different copy").unwrap();
        let foreign_target = dir.path().join("same name different target.lnk");
        create_link(&foreign_target, &other_executable, app_id);
        let before = fs::read(&foreign_target).unwrap();
        assert!(!update_shortcut(&foreign_target, &executable, app_id, &icon).unwrap());
        assert_eq!(fs::read(foreign_target).unwrap(), before);
    }

    #[test]
    fn real_window_shell_properties_keep_identity_and_relaunch_icon_together() {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
        };
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                let _ = unsafe { DestroyWindow(self.0) };
            }
        }
        let _apartment = ComApartment::new().unwrap();
        let window = TestWindow(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::w!("STATIC"),
                windows::core::w!("DevKit isolated icon test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        });
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("程序 with spaces.exe");
        let icon = dir.path().join("主题图标.ico");
        let app_id = "dev.android-devkit.desktop.tests";
        apply_window_properties(window.0, app_id, &executable, &icon).unwrap();
        let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(window.0).unwrap() };
        assert_eq!(get_string(&store, &APP_ID).unwrap(), app_id);
        assert_eq!(
            get_string(&store, &RELAUNCH_COMMAND).unwrap(),
            format!("\"{}\"", executable.display())
        );
        assert_eq!(
            get_string(&store, &RELAUNCH_NAME).unwrap(),
            "Android DevKit"
        );
        assert_eq!(
            get_string(&store, &RELAUNCH_ICON).unwrap(),
            format!("{},0", icon.display())
        );
        clear_window_properties(window.0).unwrap();
        for key in [APP_ID, RELAUNCH_ICON, RELAUNCH_NAME, RELAUNCH_COMMAND] {
            assert_eq!(get_string(&store, &key).unwrap(), "");
        }
    }
}
