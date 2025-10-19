use cef::*;

pub trait WindowTestRunner {
    fn set_pos(&mut self, x: i32, y: i32, width: i32, height: i32);
    fn minimize(&mut self);
    fn maximize(&mut self);
    fn restore(&mut self);
    fn fullscreen(&mut self);
    fn set_titlebar_height(&mut self, height: f32);
}

pub fn create_window_test_runner(browser: &Browser) -> Box<dyn WindowTestRunner> {
    create_native_window_test_runner(browser)
}

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac::create_native_window_test_runner;

#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "windows")]
use win::create_native_window_test_runner;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::create_native_window_test_runner;
