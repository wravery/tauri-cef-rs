use cef::*;

pub struct TempWindowInstance {
    window: WindowHandle,
}

impl TempWindowInstance {
    pub fn new() -> Self {
        todo!("implement TempWindowInstance::new for macOS")
    }

    pub fn window_handle(&self) -> WindowHandle {
        self.window
    }
}

impl Drop for TempWindowInstance {
    fn drop(&mut self) {
        todo!("implement Drop for TempWindowInstance on macOS")
    }
}
