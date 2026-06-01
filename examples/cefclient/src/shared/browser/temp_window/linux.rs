use cef::*;
#[cfg(feature = "linux-x11")]
use x11_dl::xlib::*;

pub struct TempWindowInstance {
    x_window: WindowHandle,
}

impl TempWindowInstance {
    pub fn new() -> Self {
        #[cfg(feature = "linux-x11")]
        let x_window = Xlib::open()
            .ok()
            .and_then(|xlib| unsafe {
                let display = get_xdisplay();
                assert!(!display.is_null());
                let parent_window = (xlib.XDefaultRootWindow)(display as *mut _);
                let mut attributes = XSetWindowAttributes {
                    background_pixmap: 0,
                    override_redirect: 0,
                    ..std::mem::zeroed()
                };
                Some((xlib.XCreateWindow)(
                    display as *mut _,
                    parent_window,
                    0,                                // x
                    0,                                // y
                    1,                                // width
                    1,                                // height
                    0,                                // border width
                    CopyFromParent,                   // depth
                    InputOutput as _,                 // class
                    CopyFromParent as _,              // visual
                    CWBackPixel | CWOverrideRedirect, // value mask
                    &mut attributes,
                ))
            })
            .unwrap_or(0);
        #[cfg(not(feature = "linux-x11"))]
        let x_window = 0;

        Self { x_window }
    }

    pub fn window_handle(&self) -> WindowHandle {
        self.x_window
    }
}

#[cfg(feature = "linux-x11")]
impl Drop for TempWindowInstance {
    fn drop(&mut self) {
        if self.x_window != 0
            && let Ok(xlib) = Xlib::open()
        {
            let display = get_xdisplay();
            assert!(!display.is_null());
            unsafe {
                ((xlib.XDestroyWindow)(display as *mut _, self.x_window));
            }
        }
    }
}
