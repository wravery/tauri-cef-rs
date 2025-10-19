use super::{image_cache::*, main_context::*, *};
use cef::*;
use tests_shared::common::client_switches;

pub enum WindowType {
    Normal,
    /// The window is a modal dialog.
    Dialog,
    /// The window is a DevTools popup.
    DevTools,
}

/// Used to configure how a RootWindow is created.
pub struct RootWindowConfig {
    /// Associated command-line.
    pub command_line: Option<CommandLine>,
    /// If true the Views framework will be used.
    pub use_views: bool,
    /// If true Alloy style will be used. Alloy style is optional. Windowless
    /// rendering requires Alloy style.
    pub use_alloy_style: bool,
    /// Configure the window type.
    pub window_type: WindowType,
    /// If true the window will always display above other windows.
    pub always_on_top: bool,
    /// If true the window will show controls.
    pub with_controls: bool,
    /// If true the window will use windowless (off-screen) rendering.
    pub with_osr: bool,
    /// If true the window will be created initially hidden.
    pub initially_hidden: bool,
    /// Requested window position. If |bounds| and |source_bounds| are empty the
    /// default window size and location will be used.
    pub bounds: Option<Rect>,
    /// Position of the UI element that triggered the window creation. If |bounds|
    /// is empty and |source_bounds| is non-empty the new window will be positioned
    /// relative to |source_bounds|. This is currently only implemented for Views-
    /// hosted windows when |initially_hidden| is also true.
    pub source_bounds: Option<Rect>,
    /// Requested window show state. Only used when |bounds| is non-empty and
    /// |initially_hidden| is false.
    pub show_state: ShowState,
    /// Parent window. Only used for Views-hosted windows.
    pub parent_window: Option<Window>,
    /// Callback to be executed when the window is closed. Will be executed on the
    /// main thread. This is currently only implemented for Views-hosted windows.
    pub on_close: Option<Box<dyn FnOnce()>>,
    /// Initial URL to load.
    pub url: String,
}

impl RootWindowConfig {
    pub fn new(command_line: Option<CommandLine>) -> Self {
        let main_context = get_main_context();
        let main_context = main_context
            .as_ref()
            .and_then(|context| context.lock().ok());
        let mut command_line = command_line.or_else(|| {
            main_context
                .as_ref()
                .and_then(|context| context.command_line())
        });
        let with_controls = command_line.as_ref().map_or(0, |command_line| {
            command_line.has_switch(Some(&CefString::from(client_switches::HIDE_CONTROLS)))
        }) == 0;
        let url = main_context
            .as_ref()
            .and_then(|context| context.main_url(command_line.as_mut()))
            .unwrap_or_else(|| DEFAULT_URL.to_string());
        Self {
            command_line,
            use_views: main_context
                .as_ref()
                .is_some_and(|context| context.use_views_global()),
            use_alloy_style: main_context
                .as_ref()
                .is_some_and(|context| context.use_alloy_style_global()),
            window_type: WindowType::Normal,
            always_on_top: false,
            with_controls,
            with_osr: false,
            initially_hidden: false,
            bounds: None,
            source_bounds: None,
            show_state: ShowState::NORMAL,
            parent_window: None,
            on_close: None,
            url,
        }
    }
}

pub type RequestContextCallback = Box<dyn Send + FnOnce(RequestContext)>;

pub trait RootWindowDelegate {
    /// Called to asynchronously retrieve the CefRequestContext for browser. Only
    /// called for non-popup browsers. Save to call on any thread. |callback|
    /// will be executed on the UI thread after the request context is
    /// initialized.
    fn request_context(&self, callback: RequestContextCallback);
    /// Returns the ImageCache.
    fn image_cache(&self) -> Option<&ImageCache>;
    /// Called to execute a test. See resource.h for |test_id| values.
    fn on_test(&self, root_window: &dyn RootWindow, test_id: i32);
    /// Called to exit the application.
    fn on_exit(&self, root_window: &dyn RootWindow);
    /// Called when the RootWindow has been destroyed.
    fn on_root_window_destroyed(&self, root_window: &dyn RootWindow);
    /// Called when the RootWindow is activated (becomes the foreground window).
    fn on_root_window_activated(&self, root_window: &dyn RootWindow);
}

pub enum RootWindowShowMode {
    Normal,
    Minimized,
    Maximized,
    NoActivate,
}

pub struct PopupWindowConfig {
    with_controls: bool,
    with_osr: bool,
    popup_features: PopupFeatures,
    window_info: WindowInfo,
}

pub trait RootWindow {
    /// Returns true if the RootWindow is Views-hosted.
    fn is_views_hosted(&self) -> bool;
    /// Returns true if the RootWindow is Alloy style, otherwise Chrome style.
    fn is_alloy_style(&self) -> bool;
    /// Initialize as a normal window. This will create and show a native window
    /// hosting a single browser instance. This method may be called on any thread.
    /// |delegate| must be non-nullptr and outlive this object.
    /// Use RootWindowManager::CreateRootWindow() instead of calling this method
    /// directly.
    fn initialize(
        &mut self,
        delegate: &dyn RootWindowDelegate,
        config: RootWindowConfig,
        settings: &BrowserSettings,
    );
    /// Initialize as a popup window. This is used to attach a new native window to
    /// a single browser instance that will be created later. The native window
    /// will be created and shown once the browser is available. This method may be
    /// called on any thread. |delegate| must be non-nullptr and outlive this
    /// object. Use RootWindowManager::CreateRootWindowAsPopup() instead of calling
    /// this method directly. Called on the UI thread.
    fn initialize_as_popup(
        &mut self,
        delegate: &dyn RootWindowDelegate,
        popup_config: &PopupWindowConfig,
        client: Option<&Client>,
        settings: &BrowserSettings,
    );
    /// Show the window.
    fn show(&mut self, show_mode: RootWindowShowMode);
    /// Hide the window.
    fn hide(&mut self);
    /// Set bounds in DIP screen coordinates. If |content_bounds| is true then the
    /// specified bounds are for the browser's content area and will be expanded to
    /// appropriate containing window bounds. Otherwise, the specified bounds are
    /// for the containing window directly. Bounds will be constrained to the
    /// containing display work area. Specific behavioral expectations depend on
    /// platform and run mode. See the https://tests/window example for details.
    fn set_bounds(&mut self, x: i32, y: i32, width: i32, height: i32, content_bounds: bool);
    fn set_bounds_rect(&mut self, bounds: &Rect, content_bounds: bool) {
        self.set_bounds(
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            content_bounds,
        );
    }
    /// Returns true if this RootWindow should default to sizing by content bounds.
    fn default_to_content_counts(&self) -> bool;
    /// Close the window. If |force| is true onunload handlers will not be
    /// executed.
    fn close(&mut self, force: bool);
    /// Set the device scale factor. Only used in combination with off-screen
    /// rendering.
    fn set_device_scale_factor(&mut self, factor: f32);
    /// Returns the device scale factor. Only used in combination with off-screen
    /// rendering.
    fn device_scale_factor(&self) -> f32;
    /// Returns the browser that this window contains, if any.
    fn browser(&self) -> Option<&Browser>;
    /// Returns the native handle for this window, if any.
    fn window_handle(&self) -> Option<ClientWindowHandle>;
    /// Returns true if this window is using windowless rendering (osr).
    fn with_windowless_rendering(&self) -> bool;
    /// Returns true if this object has been initialized.
    fn is_initialized(&self) -> bool;
    /// Returns true if the platform window has been created.
    fn is_window_created(&self) -> bool;
    // Used to uniquely identify popup windows.
    fn set_popup_id(&mut self, opener_browser_id: i32, popup_id: i32);
    // If |popup_id| is -1 only match |opener_browser_id|.
    fn is_popup_id_match(&self, opener_browser_id: i32, popup_id: i32) -> bool;
    fn opened_browser_id(&self) -> i32;
    fn popup_id(&self) -> i32;
}

pub struct RootWindowManager;

impl RootWindowManager {
    /// Track other browsers that are not directly associated with a RootWindow.
    /// This may be an overlay browser, a popup created with `--use-default-popup`,
    /// or a browser using default Chrome UI. |opener_browser_id| will be > 0 for
    /// popup browsers.
    pub fn other_browser_created(&self, _browser_id: i32, _opener_browser_id: Option<i32>) {
        todo!("Implement other_browser_created")
    }

    /// Track other browsers that are not directly associated with a RootWindow.
    /// This may be an overlay browser, a popup created with `--use-default-popup`,
    /// or a browser using default Chrome UI. |opener_browser_id| will be > 0 for
    /// popup browsers.
    pub fn other_browser_closed(&self, _browser_id: i32, _opener_browser_id: Option<i32>) {
        todo!("Implement other_browser_closed")
    }
}

impl RootWindowDelegate for RootWindowManager {
    /// Called to asynchronously retrieve the CefRequestContext for browser. Only
    /// called for non-popup browsers. Save to call on any thread. |callback|
    /// will be executed on the UI thread after the request context is
    /// initialized.
    fn request_context(&self, _callback: RequestContextCallback) {
        todo!("Implement request_context")
    }

    /// Returns the ImageCache.
    fn image_cache(&self) -> Option<&ImageCache> {
        todo!("Implement image_cache")
    }

    /// Called to execute a test. See resource.h for |test_id| values.
    fn on_test(&self, _root_window: &dyn RootWindow, _test_id: i32) {
        todo!("Implement on_test")
    }

    /// Called to exit the application.
    fn on_exit(&self, _root_window: &dyn RootWindow) {
        todo!("Implement on_exit")
    }

    /// Called when the RootWindow has been destroyed.
    fn on_root_window_destroyed(&self, _root_window: &dyn RootWindow) {
        todo!("Implement on_root_window_destroyed")
    }

    /// Called when the RootWindow is activated (becomes the foreground window).
    fn on_root_window_activated(&self, _root_window: &dyn RootWindow) {
        todo!("Implement on_root_window_activated")
    }
}

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac::*;

#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "windows")]
use win::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::*;
