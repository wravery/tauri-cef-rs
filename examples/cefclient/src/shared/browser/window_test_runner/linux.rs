use super::*;
use cef::*;

struct WindowTestRunnerLinux {}

impl WindowTestRunnerLinux {
    pub fn new(_browser: &Browser) -> Self {
        WindowTestRunnerLinux {}
    }
}

impl WindowTestRunner for WindowTestRunnerLinux {
    fn set_pos(&mut self, _x: i32, _y: i32, _width: i32, _height: i32) {}

    fn minimize(&mut self) {}

    fn maximize(&mut self) {}

    fn restore(&mut self) {}

    fn fullscreen(&mut self) {}

    fn set_titlebar_height(&mut self, _height: f32) {}
}

pub fn create_native_window_test_runner(browser: &Browser) -> Box<dyn WindowTestRunner> {
    Box::new(WindowTestRunnerLinux::new(browser))
}
