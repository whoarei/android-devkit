//! The registry of durable user preferences. Operational Settings remain
//! separately validated so presentation changes cannot invalidate task plans.
use crate::domain::*;
use serde::{Deserialize, Serialize};

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    // Normal outer size in logical pixels; position is in physical screen pixels.
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
    pub monitor: String,
    pub maximized: bool,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct UserPreferences {
    #[cfg_attr(test, ts(type = "'light' | 'dark'"))]
    pub theme: String,
    #[cfg_attr(test, ts(type = "'zh-CN' | 'en-US'"))]
    pub locale: String,
    pub default_target: String,
    pub auto_check_updates: bool,
    pub zoom_factor: f64,
    pub window: Option<WindowPlacement>,
    #[cfg_attr(
        test,
        ts(type = "'devices' | 'terminal' | 'sdk' | 'tasks' | 'diagnostics' | 'settings'")
    )]
    pub last_page: String,
    #[cfg_attr(test, ts(type = "'all' | 'usb' | 'emulator'"))]
    pub device_filter: String,
    #[cfg_attr(test, ts(type = "'platforms' | 'tools' | 'sites'"))]
    pub sdk_tab: String,
    #[cfg_attr(test, ts(type = "'all' | 'installed' | 'missing' | 'updates'"))]
    pub sdk_status: String,
    pub hide_obsolete: bool,
    #[cfg_attr(
        test,
        ts(
            type = "'all' | 'active' | 'succeeded' | 'failed' | 'waiting_confirmation' | 'needs_review' | 'cancelled'"
        )
    )]
    pub task_state: String,
    pub recording_seconds: u32,
    #[cfg_attr(test, ts(type = "'original' | '720p'"))]
    pub recording_resolution: String,
    pub log_quota_mib: u32,
    pub log_minutes: u32,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            locale: "zh-CN".into(),
            default_target: String::new(),
            auto_check_updates: true,
            zoom_factor: 1.0,
            window: None,
            last_page: "devices".into(),
            device_filter: "all".into(),
            sdk_tab: "platforms".into(),
            sdk_status: "all".into(),
            hide_obsolete: true,
            task_state: "all".into(),
            recording_seconds: 30,
            recording_resolution: "original".into(),
            log_quota_mib: 64,
            log_minutes: 60,
        }
    }
}

impl UserPreferences {
    pub fn patched(&self, patch: &serde_json::Value) -> Result<Self> {
        let invalid = || AppError::new("INVALID_PREFERENCE", "用户配置无效");
        let patch = patch.as_object().ok_or_else(invalid)?;
        let mut value = serde_json::to_value(self)?;
        let object = value.as_object_mut().unwrap();
        for (key, value) in patch {
            if !object.contains_key(key) {
                return Err(invalid().detail(format!("Unknown preference: {key}")));
            }
            object.insert(key.clone(), value.clone());
        }
        let next: Self =
            serde_json::from_value(value).map_err(|error| invalid().detail(error.to_string()))?;
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<()> {
        let choices = [
            (self.theme.as_str(), &["light", "dark"][..]),
            (self.locale.as_str(), &["zh-CN", "en-US"][..]),
            (
                self.last_page.as_str(),
                &[
                    "devices",
                    "terminal",
                    "sdk",
                    "tasks",
                    "diagnostics",
                    "settings",
                ][..],
            ),
            (self.device_filter.as_str(), &["all", "usb", "emulator"][..]),
            (self.sdk_tab.as_str(), &["platforms", "tools", "sites"][..]),
            (
                self.sdk_status.as_str(),
                &["all", "installed", "missing", "updates"][..],
            ),
            (
                self.task_state.as_str(),
                &[
                    "all",
                    "active",
                    "succeeded",
                    "failed",
                    "waiting_confirmation",
                    "needs_review",
                    "cancelled",
                ][..],
            ),
            (
                self.recording_resolution.as_str(),
                &["original", "720p"][..],
            ),
        ];
        let valid_target = self.default_target.is_empty()
            || self.default_target.starts_with("avd:")
            || self.default_target.starts_with("device:");
        let valid_window = self.window.as_ref().is_none_or(|window| {
            window.width.is_finite()
                && window.height.is_finite()
                && (100.0..=20000.0).contains(&window.width)
                && (100.0..=20000.0).contains(&window.height)
                && (0.25..=8.0).contains(&window.scale_factor)
                && window.monitor.len() <= 256
        });
        if choices
            .iter()
            .any(|(value, options)| !options.contains(value))
            || !valid_target
            || self.default_target.len() > 2048
            || !self.zoom_factor.is_finite()
            || self.zoom_factor <= 0.0
            || !valid_window
            || !(1..=180).contains(&self.recording_seconds)
            || !(8..=4096).contains(&self.log_quota_mib)
            || !(1..=1440).contains(&self.log_minutes)
        {
            return Err(AppError::new("INVALID_PREFERENCE", "用户配置无效"));
        }
        Ok(())
    }
}

pub const LEGACY_FIELDS: &[&str] = &["theme", "locale", "defaultTarget", "autoCheckUpdates"];

#[tauri::command]
pub fn preferences_load(
    app: tauri::State<'_, std::sync::Arc<crate::app::App>>,
    legacy: Option<serde_json::Value>,
) -> Result<UserPreferences> {
    app.store.import_legacy_preferences(legacy.as_ref())
}

#[tauri::command]
pub fn preferences_patch(
    app: tauri::State<'_, std::sync::Arc<crate::app::App>>,
    patch: serde_json::Value,
) -> Result<UserPreferences> {
    // The frontend cannot supply geometry or zoom; those come from native events.
    if patch
        .as_object()
        .is_some_and(|object| object.contains_key("window") || object.contains_key("zoomFactor"))
    {
        return Err(AppError::new("INVALID_PREFERENCE", "用户配置无效"));
    }
    app.store.patch_preferences(&patch)
}
