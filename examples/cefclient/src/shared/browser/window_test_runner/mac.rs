use super::*;
use cef::*;

struct WindowTestRunnerMac {}

impl WindowTestRunnerMac {
    pub fn new(_browser: &Browser) -> Self {
        WindowTestRunnerMac {}
    }
}

impl WindowTestRunner for WindowTestRunnerMac {
    fn set_pos(&mut self, x: i32, y: i32, width: i32, height: i32) {}

    fn minimize(&mut self) {}

    fn maximize(&mut self) {}

    fn restore(&mut self) {}

    fn fullscreen(&mut self) {}

    fn set_titlebar_height(&mut self, height: f32) {}
}

pub fn create_native_window_test_runner(browser: &Browser) -> Box<dyn WindowTestRunner> {
    Box::new(WindowTestRunnerMac::new(browser))
}
