use cef::*;

pub struct TempWindowInstance {
    hwnd: WindowHandle,
}

impl TempWindowInstance {
    pub fn new() -> Self {
        todo!("implement TempWindowInstance::new for Windows")
    }

    pub fn window_handle(&self) -> WindowHandle {
        self.hwnd
    }
}

impl Drop for TempWindowInstance {
    fn drop(&mut self) {
        todo!("implement Drop for TempWindowInstance on Windows")
    }
}
