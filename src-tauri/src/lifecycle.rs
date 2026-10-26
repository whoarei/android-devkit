use crate::{app::App, domain::*};
use std::sync::{atomic::Ordering, Arc};
use tauri::{Emitter, Manager};
struct TrayItems {
    show: tauri::menu::MenuItem<tauri::Wry>,
    status: tauri::menu::MenuItem<tauri::Wry>,
    quit: tauri::menu::MenuItem<tauri::Wry>,
}
pub fn create_tray(handle: &tauri::AppHandle) -> Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let show = MenuItem::with_id(handle, "show", "打开工作台", true, None::<&str>)
        .map_err(window_error)?;
    let status = MenuItem::with_id(handle, "status", "任务状态", false, None::<&str>)
        .map_err(window_error)?;
    let quit =
        MenuItem::with_id(handle, "quit", "退出…", true, None::<&str>).map_err(window_error)?;
    let menu = Menu::with_items(handle, &[&show, &status, &quit]).map_err(window_error)?;
    handle.manage(TrayItems { show, status, quit });
    let icon = handle
        .default_window_icon()
        .ok_or_else(|| AppError::new("TRAY_UNAVAILABLE", "无法加载托盘图标"))?;
    TrayIconBuilder::with_id("devkit")
        .menu(&menu)
        .icon(icon.clone())
        .tooltip("Android DevKit")
        .show_menu_on_left_click(false)
        .on_menu_event(|h, event| {
            if event.id.as_ref() == "show" {
                show_window(h);
            } else if event.id.as_ref() == "quit" {
                h.state::<Arc<App>>().request_close(true);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_window(tray.app_handle());
            }
        })
        .build(handle)
        .map_err(window_error)?;
    update_tray(handle, &handle.state::<Arc<App>>());
    Ok(())
}

fn window_error(e: impl std::fmt::Display) -> AppError {
    AppError::new("WINDOW", e.to_string())
}
pub fn show_window(h: &tauri::AppHandle) {
    if let Some(w) = h.get_webview_window("main") {
        let _ = w.show();
        if w.is_minimized().unwrap_or(false) {
            let _ = w.unminimize();
        }
        let _ = w.set_focus();
    }
}

pub fn tray_available(h: &tauri::AppHandle) -> bool {
    h.tray_by_id("devkit").is_some()
}

pub fn hide_to_tray(h: &tauri::AppHandle) -> Result<()> {
    if !tray_available(h) {
        return Err(AppError::new(
            "TRAY_UNAVAILABLE",
            "托盘不可用，请保持窗口打开",
        ));
    }
    if let Some(window) = h.get_webview_window("main") {
        window.hide().map_err(window_error)?;
        #[cfg(windows)]
        crate::window_state::flush(h);
    }
    Ok(())
}

pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    // Also cover OS shortcuts and the native taskbar/system menu, not just our title bar.
    if window.label() == "main"
        && matches!(event, tauri::WindowEvent::Resized(_))
        && window.is_minimized().unwrap_or(false)
        && window.is_visible().unwrap_or(false)
        && tray_available(window.app_handle())
    {
        if let Err(error) = hide_to_tray(window.app_handle()) {
            eprintln!("Minimize to tray: {error}");
        }
    }
}
pub fn update_tray(h: &tauri::AppHandle, app: &App) {
    if let Some(tray) = h.tray_by_id("devkit") {
        let tasks = app.store.tasks().unwrap_or_default();
        let active = app.store.active_tasks().unwrap_or_default().len();
        let failed = tasks
            .iter()
            .filter(|t| matches!(t.state.as_str(), "failed" | "needs_review"))
            .count();
        let english = app.english.load(Ordering::Relaxed);
        let text = if english {
            format!("Android DevKit · {active} active · {failed} need review")
        } else {
            format!("Android DevKit · {active} 个进行中 · {failed} 个需检查")
        };
        let _ = tray.set_tooltip(Some(&text));
        if let Some(items) = h.try_state::<TrayItems>() {
            let _ = items.status.set_text(&text);
            let _ = items.show.set_text(if english {
                "Open workbench"
            } else {
                "打开工作台"
            });
            let _ = items.quit.set_text(if english { "Exit…" } else { "退出…" });
        }
    }
}
impl App {
    pub fn close_status(&self) -> Result<CloseStatus> {
        Ok(CloseStatus {
            tasks: self.store.active_tasks()?,
            unsaved: self
                .store
                .artifacts()?
                .into_iter()
                .filter(|a| !a.acknowledged)
                .collect(),
            draining: self.draining.load(Ordering::SeqCst),
            tray_enabled: self
                .handle
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(tray_available),
        })
    }
    pub fn request_close(&self, force_exit: bool) {
        if self.updating.load(Ordering::SeqCst) {
            return;
        }
        let Some(h) = self.handle.lock().unwrap().clone() else {
            return;
        };
        if self.store.settings().tray_enabled && !force_exit && tray_available(&h) {
            if let Err(error) = hide_to_tray(&h) {
                let _ = h.emit_to("main", "task-error", error);
            }
            return;
        }
        match self.close_status() {
            Ok(status) if status.tasks.is_empty() && status.unsaved.is_empty() => {
                self.stop_observers();
                exit_after_terminals(h);
            }
            _ => {
                show_window(&h);
                let _ = h.emit_to("main", "close-requested", ());
            }
        }
    }
    pub fn stop_observers(&self) {
        for tx in self.log_children.lock().unwrap().values() {
            let _ = tx.try_send(());
        }
        self.log_views.lock().unwrap().clear();
    }
    pub fn resolve_close(
        self: &Arc<Self>,
        mode: &str,
        discard: bool,
        stop_recordings: bool,
        discard_ids: &[String],
    ) -> Result<()> {
        let _update_guard = self.allow_update_sensitive_work()?;
        if mode == "return" {
            self.draining.store(false, Ordering::SeqCst);
            return Ok(());
        }
        if mode == "hide" {
            let handle = self.handle.lock().unwrap().clone();
            return match handle {
                Some(h) => hide_to_tray(&h),
                None => Err(AppError::new(
                    "TRAY_UNAVAILABLE",
                    "托盘不可用，请保持窗口打开",
                )),
            };
        }
        if !matches!(mode, "exit" | "wait" | "cancel") {
            return Err(AppError::new("INVALID_ACTION", "关闭动作无效"));
        }
        let status = self.close_status()?;
        if status
            .unsaved
            .iter()
            .any(|a| !discard || !discard_ids.contains(&a.id))
        {
            return Err(AppError::new(
                "UNSAVED_ARTIFACTS",
                "请保存产物或明确选择放弃未保存产物",
            ));
        }
        if mode == "exit" && !status.tasks.is_empty() {
            return Err(AppError::new("TASKS_ACTIVE", "仍有任务在执行"));
        }
        if status
            .tasks
            .iter()
            .any(|t| t.operation.kind == "log_record")
            && !stop_recordings
        {
            return Err(AppError::new(
                "RECORDING_ACTIVE",
                "请明确停止采集并完成文件封口后退出",
            ));
        }
        if discard {
            self.discard_artifacts(
                &status
                    .unsaved
                    .iter()
                    .map(|a| a.id.clone())
                    .collect::<Vec<_>>(),
            )?;
        }
        self.draining.store(true, Ordering::SeqCst);
        for t in status.tasks {
            if mode == "cancel" || t.operation.kind == "log_record" {
                if let Err(error) = self.cancel_task(&t.id) {
                    self.draining.store(false, Ordering::SeqCst);
                    return Err(error);
                }
            }
        }
        let app = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                if !app.draining.load(Ordering::SeqCst) {
                    break;
                }
                match app.close_status() {
                    Ok(status) if status.tasks.is_empty() => {
                        if status.unsaved.is_empty() {
                            app.stop_observers();
                            let handle = app.handle.lock().unwrap().clone();
                            if let Some(h) = handle {
                                exit_after_terminals(h);
                            }
                        } else {
                            app.draining.store(false, Ordering::SeqCst);
                            let handle = app.handle.lock().unwrap().clone();
                            if let Some(h) = handle {
                                show_window(&h);
                                let _ = h.emit_to("main", "close-requested", ());
                            }
                        }
                        break;
                    }
                    Err(_) => {
                        app.draining.store(false, Ordering::SeqCst);
                        break;
                    }
                    _ => {}
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        });
        Ok(())
    }
}

fn exit_after_terminals(handle: tauri::AppHandle) {
    #[cfg(windows)]
    crate::window_state::flush(&handle);
    let app = handle.state::<Arc<App>>().inner().clone();
    app.draining.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        let _gate = app.update_gate.write().await;
        let closing = app.clone();
        let _ = tokio::task::spawn_blocking(move || closing.terminals.shutdown()).await;
        handle.exit(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn unsaved_exit_is_bound_to_reviewed_artifact_ids() {
        let d = tempfile::tempdir().unwrap();
        let app = App::new(d.path().into()).unwrap();
        let p = app
            .prepare_text_export(
                "test",
                vec!["token=secret 192.168.1.2".into()],
                true,
                vec![],
            )
            .unwrap();
        assert_eq!(
            app.resolve_close("exit", false, false, &[])
                .unwrap_err()
                .code,
            "UNSAVED_ARTIFACTS"
        );
        assert_eq!(
            app.resolve_close("exit", true, false, &["other".into()])
                .unwrap_err()
                .code,
            "UNSAVED_ARTIFACTS"
        );
        assert!(!app.store.artifact(&p.artifact.id).unwrap().0.acknowledged);
        assert_eq!(
            app.resolve_close("hide", false, false, &[])
                .unwrap_err()
                .code,
            "TRAY_UNAVAILABLE"
        );
        app.resolve_close("exit", true, false, std::slice::from_ref(&p.artifact.id))
            .unwrap();
        assert!(app.store.artifact(&p.artifact.id).is_err());
        assert!(!app
            .data
            .join("exports")
            .join(format!("{}.txt", p.artifact.id))
            .exists());
        assert!(app.close_status().unwrap().unsaved.is_empty());
    }
}
