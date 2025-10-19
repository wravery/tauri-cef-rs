use cef::*;

pub struct OsrRendererSettings {
    /// If true draw a border around update rectangles.
    pub show_update_rect: bool,

    /// If true return real screen bounds from GetRootScreenRect/GetScreenInfo.
    /// - Allows window.outerWidth/Height and window.screenX/Y to return correct
    ///   values.
    /// - Allows JavaScript window.moveTo/By() and window.resizeTo/By() to provide
    ///   bounds that include the window frame.
    /// - Causes HTML select popups to be cropped (limitation of cefclient impl).
    pub real_screen_bounds: bool,

    /// Background color. Enables transparency if the alpha component is 0.
    pub background_color: Color,

    /// Render using shared textures. Supported on Windows only via D3D11.
    pub shared_texture_enabled: bool,

    /// Client implements a BeginFrame timer by calling
    /// CefBrowserHost::SendExternalBeginFrame at the specified frame rate.
    pub begin_frame_rate: Option<u32>,
}

impl Default for OsrRendererSettings {
    fn default() -> Self {
        Self {
            show_update_rect: false,
            real_screen_bounds: true,
            background_color: 0,
            shared_texture_enabled: false,
            begin_frame_rate: None,
        }
    }
}
