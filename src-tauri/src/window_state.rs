//! Normal window geometry is independent of maximized/minimized geometry.
use crate::{app::App, domain::*, preferences::WindowPlacement};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use tauri::{Emitter, Manager};
use windows_sys::Win32::{
    Graphics::Gdi::*,
    UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
};

#[derive(Clone, Debug)]
struct DisplayArea {
    name: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}

fn fit(saved: &WindowPlacement, areas: &[DisplayArea]) -> Option<WindowPlacement> {
    let area = areas
        .iter()
        .find(|area| area.name == saved.monitor)
        .or_else(|| {
            areas.iter().find(|area| {
                saved.x >= area.x
                    && saved.y >= area.y
                    && i64::from(saved.x) < i64::from(area.x) + i64::from(area.width)
                    && i64::from(saved.y) < i64::from(area.y) + i64::from(area.height)
            })
        })
        .or_else(|| areas.first())?;
    let width = (saved.width * area.scale).round().clamp(
        (480.0 * area.scale).min(area.width as f64),
        area.width as f64,
    ) as i32;
    let height = (saved.height * area.scale).round().clamp(
        (640.0 * area.scale).min(area.height as f64),
        area.height as f64,
    ) as i32;
    let same_monitor = areas
        .iter()
        .any(|candidate| candidate.name == saved.monitor);
    let (x, y) = if same_monitor {
        (
            (saved.x as i64).clamp(
                area.x as i64,
                area.x as i64 + area.width as i64 - width as i64,
            ) as i32,
            (saved.y as i64).clamp(
                area.y as i64,
                area.y as i64 + area.height as i64 - height as i64,
            ) as i32,
        )
    } else {
        (
            area.x + (area.width as i32 - width) / 2,
            area.y + (area.height as i32 - height) / 2,
        )
    };
    Some(WindowPlacement {
        x,
        y,
        width: width as f64 / area.scale,
        height: height as f64 / area.scale,
        scale_factor: area.scale,
        monitor: area.name.clone(),
        maximized: saved.maximized,
    })
}

fn capture(window: &tauri::Window) -> Result<WindowPlacement> {
    let hwnd = window
        .hwnd()
        .map_err(|error| AppError::new("WINDOW", error.to_string()))?
        .0;
    unsafe {
        let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        let mut monitor = MONITORINFOEXW::default();
        monitor.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetWindowPlacement(hwnd, &mut placement) == 0
            || GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &mut monitor.monitorInfo,
            ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let scale = (GetDpiForWindow(hwnd) as f64 / 96.0).max(0.25);
        let normal = placement.rcNormalPosition;
        // GetWindowPlacement uses workspace coordinates for ordinary windows.
        // Translate to screen coordinates before persisting; never pass these
        // workspace coordinates directly to SetWindowPos.
        let work = monitor.monitorInfo.rcWork;
        let screen = monitor.monitorInfo.rcMonitor;
        let tool_window = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0;
        let (dx, dy) = if tool_window {
            (0, 0)
        } else {
            (work.left - screen.left, work.top - screen.top)
        };
        Ok(WindowPlacement {
            x: normal.left + dx,
            y: normal.top + dy,
            width: (normal.right - normal.left) as f64 / scale,
            height: (normal.bottom - normal.top) as f64 / scale,
            scale_factor: scale,
            monitor: String::from_utf16_lossy(
                &monitor.szDevice[..monitor
                    .szDevice
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(monitor.szDevice.len())],
            ),
            maximized: placement.showCmd == SW_SHOWMAXIMIZED as u32
                || (IsIconic(hwnd) != 0 && placement.flags & WPF_RESTORETOMAXIMIZED != 0),
        })
    }
}

#[derive(Default)]
struct Pending {
    latest: Option<WindowPlacement>,
    saved: Option<WindowPlacement>,
}
struct WindowPersistence {
    ready: AtomicBool,
    pending: Mutex<Pending>,
    signal: mpsc::Sender<()>,
}
impl WindowPersistence {
    fn flush(&self, app: &App) {
        let mut pending = self.pending.lock().unwrap();
        if pending.latest == pending.saved {
            return;
        }
        if let Some(placement) = pending.latest.as_ref() {
            match app.store.save_window_placement(placement) {
                Ok(()) => pending.saved = pending.latest.clone(),
                Err(error) => {
                    eprintln!("Window preferences: {error}");
                    if let Some(handle) = app.handle.lock().unwrap().as_ref() {
                        let _ = handle.emit_to(
                            "main",
                            "task-error",
                            AppError::new("UI_PREFERENCE", "窗口位置和大小保存失败，请重试。"),
                        );
                    }
                }
            }
        }
    }
}

pub fn install(window: &tauri::WebviewWindow, app: &Arc<App>) -> Result<()> {
    let preferences = app.store.preferences()?;
    let (sender, receiver) = mpsc::channel();
    let persistence = Arc::new(WindowPersistence {
        ready: AtomicBool::new(false),
        pending: Mutex::new(Pending {
            latest: None,
            saved: preferences.window.clone(),
        }),
        signal: sender,
    });
    window.manage(persistence.clone());
    let weak_state = Arc::downgrade(app);
    let weak_persistence = Arc::downgrade(&persistence);
    std::thread::spawn(move || {
        while receiver.recv().is_ok() {
            while receiver
                .recv_timeout(std::time::Duration::from_millis(250))
                .is_ok()
            {}
            let (Some(app), Some(persistence)) = (weak_state.upgrade(), weak_persistence.upgrade())
            else {
                break;
            };
            persistence.flush(&app);
        }
    });
    if let Some(saved) = preferences.window {
        let mut monitors = window
            .available_monitors()
            .map_err(|error| AppError::new("WINDOW", error.to_string()))?;
        // Put the primary display first for the disconnected-monitor fallback.
        let primary = window.primary_monitor().ok().flatten();
        monitors.sort_by_key(|monitor| {
            primary
                .as_ref()
                .is_none_or(|primary| primary.name() != monitor.name())
        });
        let areas: Vec<_> = monitors
            .iter()
            .map(|monitor| {
                let work = monitor.work_area();
                DisplayArea {
                    name: monitor.name().cloned().unwrap_or_default(),
                    x: work.position.x,
                    y: work.position.y,
                    width: work.size.width,
                    height: work.size.height,
                    scale: monitor.scale_factor(),
                }
            })
            .collect();
        if let Some(placement) = fit(&saved, &areas) {
            let area = areas
                .iter()
                .find(|area| area.name == placement.monitor)
                .unwrap();
            window
                .set_min_size(Some(tauri::LogicalSize::new(
                    480.0_f64.min(area.width as f64 / area.scale),
                    640.0_f64.min(area.height as f64 / area.scale),
                )))
                .map_err(|error| AppError::new("WINDOW", error.to_string()))?;
            let hwnd = window
                .hwnd()
                .map_err(|error| AppError::new("WINDOW", error.to_string()))?
                .0;
            unsafe {
                if SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    placement.x,
                    placement.y,
                    (placement.width * placement.scale_factor).round() as i32,
                    (placement.height * placement.scale_factor).round() as i32,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                ) == 0
                {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            if placement.maximized {
                window
                    .maximize()
                    .map_err(|error| AppError::new("WINDOW", error.to_string()))?;
            }
        }
    }
    persistence.ready.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn on_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let Some(persistence) = window.try_state::<Arc<WindowPersistence>>() else {
        return;
    };
    if !persistence.ready.load(Ordering::SeqCst) {
        return;
    }
    if matches!(
        event,
        tauri::WindowEvent::Moved(_)
            | tauri::WindowEvent::Resized(_)
            | tauri::WindowEvent::ScaleFactorChanged { .. }
            | tauri::WindowEvent::CloseRequested { .. }
            | tauri::WindowEvent::Focused(false)
    ) {
        if let Ok(placement) = capture(window) {
            persistence.pending.lock().unwrap().latest = Some(placement);
            let _ = persistence.signal.send(());
        }
    }
    if matches!(
        event,
        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
    ) {
        if let Some(app) = window.try_state::<Arc<App>>() {
            persistence.flush(app.inner());
        }
    }
}

pub fn flush(handle: &tauri::AppHandle) {
    if let (Some(persistence), Some(app)) = (
        handle.try_state::<Arc<WindowPersistence>>(),
        handle.try_state::<Arc<App>>(),
    ) {
        persistence.flush(app.inner());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn saved() -> WindowPlacement {
        WindowPlacement {
            x: -1800,
            y: 100,
            width: 1000.0,
            height: 700.0,
            scale_factor: 1.0,
            monitor: "secondary".into(),
            maximized: true,
        }
    }
    #[test]
    fn disconnected_display_is_centered_and_large_windows_fit_work_area() {
        let area = DisplayArea {
            name: "primary".into(),
            x: 0,
            y: 40,
            width: 1920,
            height: 1040,
            scale: 1.5,
        };
        let placement = fit(&saved(), &[area]).unwrap();
        assert_eq!((placement.x, placement.y), (210, 40));
        assert_eq!(placement.width, 1000.0);
        assert_eq!(placement.height * placement.scale_factor, 1040.0);
        assert!(placement.maximized);
    }
    #[test]
    fn negative_coordinates_and_changed_dpi_preserve_reachable_normal_bounds() {
        let area = DisplayArea {
            name: "secondary".into(),
            x: -2560,
            y: -200,
            width: 2560,
            height: 1400,
            scale: 2.0,
        };
        let placement = fit(&saved(), &[area]).unwrap();
        assert_eq!((placement.x, placement.y), (-2000, -200));
        assert_eq!((placement.width, placement.height), (1000.0, 700.0));
        assert_eq!(placement.scale_factor, 2.0);
        assert!(fit(&saved(), &[]).is_none());
    }
}
