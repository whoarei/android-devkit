use crate::domain::*;
use serde::Deserialize;
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Regions {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::sync::{Mutex, Once, OnceLock};
    use tauri::Emitter;
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::ScreenToClient,
        System::Threading::*,
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    };
    static REGION: OnceLock<Mutex<Regions>> = OnceLock::new();
    static OVERLAY: OnceLock<Mutex<isize>> = OnceLock::new();
    static OWNER: OnceLock<Mutex<Option<tauri::WebviewWindow>>> = OnceLock::new();
    static REGISTER: Once = Once::new();
    fn hover(active: bool) {
        if let Some(owner) = OWNER.get() {
            if let Some(window) = owner.lock().unwrap().as_ref() {
                let _ = window.emit("native-maximize-hover", active);
            }
        }
    }
    unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        let parent = GetParent(hwnd);
        match msg {
            WM_NCHITTEST => HTMAXBUTTON as isize,
            WM_ERASEBKGND => 1,
            WM_NCMOUSEMOVE => {
                hover(true);
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_NONCLIENT | TME_HOVER | TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 100,
                };
                TrackMouseEvent(&mut tracking);
                DefWindowProcW(parent, msg, HTMAXBUTTON as usize, l)
            }
            WM_NCMOUSEHOVER => DefWindowProcW(parent, msg, HTMAXBUTTON as usize, l),
            WM_NCMOUSELEAVE => {
                hover(false);
                DefWindowProcW(parent, msg, w, l)
            }
            WM_NCLBUTTONDOWN => {
                hover(false);
                ShowWindow(
                    parent,
                    if IsZoomed(parent) != 0 {
                        SW_RESTORE
                    } else {
                        SW_MAXIMIZE
                    },
                );
                0
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }
    fn overlay(window: &tauri::WebviewWindow, r: Regions) {
        *OWNER.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(window.clone());
        let Ok(parent) = window.hwnd() else { return };
        let class: Vec<u16> = "AndroidDevKitSnapOverlay\0".encode_utf16().collect();
        unsafe {
            let instance =
                windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
            REGISTER.call_once(|| {
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(overlay_proc),
                    hInstance: instance,
                    lpszClassName: class.as_ptr(),
                    ..Default::default()
                };
                RegisterClassW(&wc);
            });
            let mut stored = OVERLAY.get_or_init(|| Mutex::new(0)).lock().unwrap();
            if *stored == 0 || IsWindow(*stored as HWND) == 0 {
                let hwnd = CreateWindowExW(
                    WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_CHILD | WS_VISIBLE,
                    0,
                    0,
                    1,
                    1,
                    parent.0,
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                );
                *stored = hwnd as isize;
            }
            if *stored != 0 {
                SetWindowPos(
                    *stored as HWND,
                    HWND_TOP,
                    (r.left as f64 * r.scale).round() as i32,
                    (r.top as f64 * r.scale).round() as i32,
                    (r.width as f64 * r.scale).round() as i32,
                    (r.height as f64 * r.scale).round() as i32,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    }
    unsafe extern "system" fn hook(
        hwnd: HWND,
        msg: u32,
        w: WPARAM,
        l: LPARAM,
        _id: usize,
        parent: usize,
    ) -> LRESULT {
        if msg == WM_NCHITTEST {
            let root = parent as HWND;
            let mut point = POINT {
                x: (l as i16) as i32,
                y: ((l >> 16) as i16) as i32,
            };
            ScreenToClient(root, &mut point);
            let scale = GetDpiForWindow(root) as f64 / 96.0;
            let region = REGION.get().map(|r| *r.lock().unwrap());
            if let Some(r) = region {
                let rect = (
                    (r.left as f64 * r.scale) as i32,
                    (r.top as f64 * r.scale) as i32,
                    (r.width as f64 * r.scale) as i32,
                    (r.height as f64 * r.scale) as i32,
                );
                if point.x >= rect.0
                    && point.x < rect.0 + rect.2
                    && point.y >= rect.1
                    && point.y < rect.1 + rect.3
                {
                    return if hwnd == root {
                        HTMAXBUTTON as isize
                    } else {
                        HTTRANSPARENT as isize
                    };
                }
            }
            if hwnd == root {
                let mut rect = RECT::default();
                GetClientRect(root, &mut rect);
                let edge = (6.0 * scale) as i32;
                let left = point.x < edge;
                let right = point.x >= rect.right - edge;
                let top = point.y < edge;
                let bottom = point.y >= rect.bottom - edge;
                if IsZoomed(root) == 0 {
                    let hit = match (left, right, top, bottom) {
                        (true, _, true, _) => HTTOPLEFT,
                        (_, true, true, _) => HTTOPRIGHT,
                        (true, _, _, true) => HTBOTTOMLEFT,
                        (_, true, _, true) => HTBOTTOMRIGHT,
                        (true, _, _, _) => HTLEFT,
                        (_, true, _, _) => HTRIGHT,
                        (_, _, true, _) => HTTOP,
                        (_, _, _, true) => HTBOTTOM,
                        _ => 0,
                    };
                    if hit != 0 {
                        return hit as isize;
                    }
                }
            }
        }
        if hwnd != parent as HWND
            && matches!(
                msg,
                WM_NCMOUSEMOVE | WM_NCMOUSEHOVER | WM_NCLBUTTONDOWN | WM_NCLBUTTONUP
            )
            && w == HTMAXBUTTON as usize
        {
            return SendMessageW(parent as HWND, msg, w, l);
        }
        DefSubclassProc(hwnd, msg, w, l)
    }
    pub fn install(window: &tauri::WebviewWindow) {
        if let Ok(h) = window.hwnd() {
            unsafe {
                SetWindowSubclass(h.0, Some(hook), 0x444b, h.0 as usize);
            }
        }
    }
    pub fn regions(window: &tauri::WebviewWindow, r: Regions) {
        *REGION.get_or_init(|| Mutex::new(r)).lock().unwrap() = r;
        install(window);
        overlay(window, r);
    }
    pub fn identity(pid: u32) -> Result<(u64, String)> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return Err(AppError::new("PROCESS_UNAVAILABLE", "进程不可查询"));
            }
            let mut creation = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            let ok = GetProcessTimes(h, &mut creation, &mut exit, &mut kernel, &mut user);
            let mut path = [0u16; 32768];
            let mut length = path.len() as u32;
            let queried = QueryFullProcessImageNameW(h, 0, path.as_mut_ptr(), &mut length);
            CloseHandle(h);
            if ok == 0 || queried == 0 {
                return Err(AppError::new("PROCESS_UNAVAILABLE", "无法验证进程身份"));
            }
            Ok((
                ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64,
                String::from_utf16_lossy(&path[..length as usize]),
            ))
        }
    }
    pub fn running(pid: u32) -> bool {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return GetLastError() != 87;
            }
            let mut exit = 0;
            let ok = GetExitCodeProcess(handle, &mut exit);
            CloseHandle(handle);
            ok == 0 || exit == 259
        }
    }
    pub fn terminate(pid: u32, created: u64, path: &str) -> Result<()> {
        let actual = identity(pid)?;
        if actual.0 != created
            || !actual.1.eq_ignore_ascii_case(path)
            || !actual.1.to_lowercase().contains("emulator")
        {
            return Err(AppError::new(
                "PROCESS_IDENTITY_CHANGED",
                "进程身份已变化，拒绝强停",
            ));
        }
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                return Err(AppError::new("PROCESS_PERMISSION", "不能打开目标进程"));
            }
            let result = TerminateProcess(handle, 1);
            CloseHandle(handle);
            if result == 0 {
                return Err(AppError::new("PROCESS_STOP_FAILED", "停止失败"));
            }
        }
        Ok(())
    }
    unsafe extern "system" fn focus(hwnd: HWND, param: LPARAM) -> i32 {
        let search = &mut *(param as *mut (u32, bool));
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == search.0 && IsWindowVisible(hwnd) != 0 {
            search.1 = true;
            ShowWindow(hwnd, SW_RESTORE);
            if SetForegroundWindow(hwnd) == 0 {
                FlashWindow(hwnd, 1);
            }
            return 0;
        }
        1
    }
    pub fn focus_pid(pid: u32) -> Result<()> {
        identity(pid)?;
        let mut search = (pid, false);
        unsafe {
            EnumWindows(Some(focus), &mut search as *mut (u32, bool) as isize);
        }
        if search.1 {
            Ok(())
        } else {
            Err(AppError::new(
                "NO_WINDOW",
                "未找到该进程的可见窗口，请检查模拟器是否正在退出",
            ))
        }
    }
}
#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow) {
    win::install(window)
}
#[cfg(windows)]
pub fn set_regions(window: &tauri::WebviewWindow, r: Regions) {
    let owned = window.clone();
    let _ = window.run_on_main_thread(move || win::regions(&owned, r));
}
#[cfg(windows)]
pub fn process_identity(pid: u32) -> Result<(u64, String)> {
    win::identity(pid)
}
#[cfg(windows)]
pub fn process_running(pid: u32) -> bool {
    win::running(pid)
}
#[cfg(windows)]
pub fn avd_lock_owner_running(pid: u32, pid_path: &std::path::Path) -> bool {
    if !process_running(pid) {
        return false;
    }
    let Ok((created, executable)) = process_identity(pid) else {
        // Keep an unqueryable live process busy rather than assume the lock is stale.
        return true;
    };
    avd_lock_matches_process(
        created,
        std::path::Path::new(&executable),
        std::fs::metadata(pid_path).and_then(|m| m.modified()).ok(),
    )
}
#[cfg(windows)]
fn avd_lock_matches_process(
    created: u64,
    executable: &std::path::Path,
    modified: Option<std::time::SystemTime>,
) -> bool {
    let name = executable
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if !(matches!(name.as_str(), "emulator" | "emulator.exe")
        || (name.starts_with("qemu-system-")
            && executable
                .extension()
                .is_none_or(|ext| ext.eq_ignore_ascii_case("exe"))))
    {
        return false;
    }
    // Windows can reuse even an emulator PID. Its new process cannot own an older PID file.
    modified
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .is_none_or(|elapsed| {
            let written = 116_444_736_000_000_000u128 + elapsed.as_nanos() / 100;
            u128::from(created) <= written
        })
}
#[cfg(windows)]
pub fn terminate_verified(pid: u32, created: u64, path: &str) -> Result<()> {
    win::terminate(pid, created, path)
}
#[cfg(windows)]
pub fn focus_process(pid: u32) -> Result<()> {
    win::focus_pid(pid)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{path::Path, time::Duration};

    #[test]
    fn avd_pid_lock_checks_executable_and_process_creation_time() {
        let created = 116_444_736_000_000_000 + 100 * 10_000_000;
        let written = std::time::UNIX_EPOCH + Duration::from_secs(101);
        for executable in [
            r"E:\Android\sdk\emulator\emulator.exe",
            r"E:\other-sdk\emulator\qemu\windows-x86_64\QEMU-SYSTEM-X86_64.EXE",
            r"E:\Android\sdk\emulator\qemu-system-aarch64.exe",
        ] {
            assert!(avd_lock_matches_process(
                created,
                Path::new(executable),
                Some(written)
            ));
            assert!(!avd_lock_matches_process(
                created + 20_000_000,
                Path::new(executable),
                Some(written)
            ));
            assert!(avd_lock_matches_process(
                created,
                Path::new(executable),
                None
            ));
        }
        for executable in [
            r"C:\Windows\System32\RuntimeBroker.exe",
            r"E:\Android\sdk\emulator\emulator-check.exe",
            r"E:\Android\sdk\emulator\adb.exe",
        ] {
            assert!(!avd_lock_matches_process(
                created,
                Path::new(executable),
                Some(written)
            ));
        }
    }
}
