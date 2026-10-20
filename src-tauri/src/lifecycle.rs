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
    let mut builder = TrayIconBuilder::with_id("devkit")
        .menu(&menu)
        .tooltip("Android DevKit")
        .show_menu_on_left_click(false);
    if let Some(icon) = handle.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder
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
    tray.set_visible(handle.state::<Arc<App>>().store.settings().tray_enabled)
        .map_err(window_error)?;
    Ok(())
}

fn window_error(e: impl std::fmt::Display) -> AppError {
    AppError::new("WINDOW", e.to_string())
}
pub fn show_window(h: &tauri::AppHandle) {
    if let Some(w) = h.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
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
            tray_enabled: self.store.settings().tray_enabled,
        })
    }
    pub fn request_close(&self, force_exit: bool) {
        let Some(h) = self.handle.lock().unwrap().clone() else {
            return;
        };
        if self.store.settings().tray_enabled && !force_exit {
            if let Some(w) = h.get_webview_window("main") {
                let _ = w.hide();
            }
            return;
        }
        match self.close_status() {
            Ok(status) if status.tasks.is_empty() && status.unsaved.is_empty() => {
                self.stop_observers();
                h.exit(0);
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
        if mode == "return" {
            self.draining.store(false, Ordering::SeqCst);
            return Ok(());
        }
        if mode == "hide" {
            if !self.store.settings().tray_enabled {
                return Err(AppError::new("TRAY_DISABLED", "请先显式启用托盘"));
            }
            if let Some(h) = self.handle.lock().unwrap().as_ref() {
                if let Some(w) = h.get_webview_window("main") {
                    w.hide().map_err(window_error)?;
                }
            }
            return Ok(());
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
                                h.exit(0);
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
            "TRAY_DISABLED"
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
