//! Persist native WebView zoom, including keyboard and Ctrl+wheel changes.
use crate::{app::App, domain::*};
use std::sync::{Arc, Weak};
use tauri::Emitter;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, Microsoft::Web::WebView2::Win32::*,
    ZoomFactorChangedEventHandler,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_MENU, VK_NUMPAD0, VK_SHIFT,
};

fn report_error(state: &Weak<App>, error: impl std::fmt::Display) {
    eprintln!("UI zoom preference: {error}");
    if let Some(state) = state.upgrade() {
        if let Some(handle) = state.handle.lock().unwrap().as_ref() {
            let _ = handle.emit_to(
                "main",
                "task-error",
                AppError::new("UI_PREFERENCE", "界面缩放设置保存或恢复失败")
                    .detail(error.to_string()),
            );
        }
    }
}

fn save(state: &Weak<App>, factor: f64) {
    if let Some(app) = state.upgrade() {
        if let Err(error) = app.store.save_zoom_factor(factor) {
            report_error(state, error);
        }
    }
}

fn configure(
    controller: ICoreWebView2Controller,
    factor: f64,
    state: Weak<App>,
) -> webview2_com::Result<()> {
    let zoom_state = state.clone();
    let changed = ZoomFactorChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(controller) = sender {
            let mut factor = 1.0;
            unsafe { controller.ZoomFactor(&mut factor)? };
            save(&zoom_state, factor);
        }
        Ok(())
    }));
    // SetZoomFactor makes the restored value WebView2's new Ctrl+0 default.
    // Handle reset explicitly so Ctrl+0 always means 100%, even after restart.
    let reset = AcceleratorKeyPressedEventHandler::create(Box::new(move |sender, args| {
        if let (Some(controller), Some(args)) = (sender, args) {
            let mut key = 0;
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
            unsafe {
                args.VirtualKey(&mut key)?;
                args.KeyEventKind(&mut kind)?;
                if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                    && (key == 0x30 || key == VK_NUMPAD0 as u32)
                    && GetKeyState(VK_CONTROL as i32) < 0
                    && GetKeyState(VK_MENU as i32) >= 0
                    && GetKeyState(VK_SHIFT as i32) >= 0
                {
                    args.SetHandled(true)?;
                    controller.SetZoomFactor(1.0)?;
                    // Programmatic zoom changes do not emit ZoomFactorChanged.
                    save(&state, 1.0);
                }
            }
        }
        Ok(())
    }));
    // WebView2 retains both handlers for this controller's lifetime. Neither
    // handler owns the controller or App, so they do not form reference cycles.
    let mut token = 0;
    unsafe {
        controller.add_ZoomFactorChanged(&changed, &mut token)?;
        controller.add_AcceleratorKeyPressed(&reset, &mut token)?;
        controller.SetZoomFactor(factor)?;
    }
    Ok(())
}

pub fn install(window: &tauri::WebviewWindow, state: &Arc<App>) -> Result<()> {
    let factor = state.store.zoom_factor()?;
    let state = Arc::downgrade(state);
    window
        .with_webview(move |webview| {
            if let Err(error) = configure(webview.controller(), factor, state.clone()) {
                report_error(&state, error);
            }
        })
        .map_err(|error| AppError::new("UI_PREFERENCE", error.to_string()))
}
