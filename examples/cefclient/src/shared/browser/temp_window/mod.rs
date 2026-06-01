use cef::*;
use std::sync::OnceLock;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac::TempWindowInstance;

#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "windows")]
use win::TempWindowInstance;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::TempWindowInstance;

static TEMP_WINDOW: OnceLock<Option<TempWindowInstance>> = OnceLock::new();

#[derive(Default)]
pub struct TempWindow;

impl TempWindow {
    pub fn window_handle() -> WindowHandle {
        TEMP_WINDOW
            .get_or_init(|| Some(TempWindowInstance::new()))
            .as_ref()
            .map(|instance| instance.window_handle())
            .unwrap_or_default()
    }
}

impl Drop for TempWindow {
    fn drop(&mut self) {
        let _ = TEMP_WINDOW.set(None);
    }
}
