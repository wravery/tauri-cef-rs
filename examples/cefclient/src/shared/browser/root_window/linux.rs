use super::super::osr_renderer::OsrRendererSettings;
use super::*;
use cef::*;
use tests_shared::browser::main_message_loop::currently_on_main_thread;

/// The Linux client uses GTK instead of the underlying platform type (X11).
pub type ClientWindowHandle = sys::cef_window_handle_t;

struct InitialScaleFactor(f32);

impl Default for InitialScaleFactor {
    fn default() -> Self {
        Self(1.0)
    }
}

#[derive(Default)]
pub struct RootWindowGtk {
    // After initialization all members are only accessed on the main thread.
    // Members set during initialization.
    inner: RootWindowInner,
    with_controls: bool,
    always_on_top: bool,
    with_osr: bool,
    osr_settings: OsrRendererSettings,
    is_popup: bool,
    start_rect: Rect,
    initial_show_state: ShowState,
    initial_scale_factor: InitialScaleFactor,
    // browser_window: Option<Arc<dyn BrowserWindow>>,

    // Main window.
    window: Option<ClientWindowHandle>,
}

impl RootWindowGtk {
    pub fn create(use_alloy_style: bool) -> Arc<dyn RootWindow> {
        Arc::new(Self {
            inner: RootWindowInner::new(use_alloy_style),
            ..Default::default()
        })
    }
}

impl RootWindow for RootWindowGtk {
    /// Returns true if the RootWindow is Views-hosted.
    fn is_views_hosted(&self) -> bool {
        false
    }

    /// Returns true if the RootWindow is Alloy style, otherwise Chrome style.
    fn is_alloy_style(&self) -> bool {
        self.inner.use_alloy_style()
    }

    /// Initialize as a normal window. This will create and show a native window
    /// hosting a single browser instance. This method may be called on any thread.
    /// |delegate| must be non-nullptr and outlive this object.
    /// Use RootWindowManager::CreateRootWindow() instead of calling this method
    /// directly.
    fn initialize(
        &mut self,
        delegate: Arc<dyn RootWindowDelegate>,
        config: RootWindowConfig,
        settings: &BrowserSettings,
    ) {
        debug_assert!(!self.inner.initialized);
        self.inner.delegate = Some(delegate);
        self.with_controls = config.with_controls;
        self.always_on_top = config.always_on_top;
        self.with_osr = config.with_osr;

        todo!("Finish implementing initialize")
    }

    /// Initialize as a popup window. This is used to attach a new native window to
    /// a single browser instance that will be created later. The native window
    /// will be created and shown once the browser is available. This method may be
    /// called on any thread. |delegate| must be non-nullptr and outlive this
    /// object. Use RootWindowManager::CreateRootWindowAsPopup() instead of calling
    /// this method directly. Called on the UI thread.
    fn initialize_as_popup(
        &mut self,
        delegate: Arc<dyn RootWindowDelegate>,
        popup_config: &PopupWindowConfig,
        client: Option<&Client>,
        settings: &BrowserSettings,
    ) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        debug_assert!(!self.inner.initialized);
        self.inner.delegate = Some(delegate);
        self.with_controls = popup_config.with_controls;
        self.with_osr = popup_config.with_osr;
        self.is_popup = true;

        todo!("Finish implementing initialize_as_popup")
    }

    /// Show the window.
    fn show(&mut self, show_mode: RootWindowShowMode) {
        debug_assert!(currently_on_main_thread());
        todo!("Finish implementing show")
    }

    /// Hide the window.
    fn hide(&mut self) {
        debug_assert!(currently_on_main_thread());
        todo!("Finish implementing hide")
    }

    /// Set bounds in DIP screen coordinates. If |content_bounds| is true then the
    /// specified bounds are for the browser's content area and will be expanded to
    /// appropriate containing window bounds. Otherwise, the specified bounds are
    /// for the containing window directly. Bounds will be constrained to the
    /// containing display work area. Specific behavioral expectations depend on
    /// platform and run mode. See the https://tests/window example for details.
    fn set_bounds(&mut self, x: i32, y: i32, width: i32, height: i32, content_bounds: bool) {
        debug_assert!(currently_on_main_thread());
        todo!("Finish implementing set_bounds")
    }

    /// Returns true if this RootWindow should default to sizing by content bounds.
    fn default_to_content_counts(&self) -> bool {
        if !self.with_windowless_rendering() {
            // Root GtkWindow bounds are provided via GetRootWindowScreenRect.
            return false;
        }
        if self.osr_settings.real_screen_bounds {
            // Root GtkWindow bounds are provided via GetRootScreenRect.
            return false;
        }
        // The root GtkWindow will not be queried by default.
        true
    }

    /// Close the window. If |force| is true onunload handlers will not be
    /// executed.
    fn close(&mut self, force: bool) {
        debug_assert!(currently_on_main_thread());
        todo!("Finish implementing close")
    }

    /// Set the device scale factor. Only used in combination with off-screen
    /// rendering.
    fn set_device_scale_factor(&mut self, factor: f32) {
        debug_assert!(currently_on_main_thread());
        todo!("Finish implementing set_device_scale_factor (depends on BrowserWindow)")
        // if self.with_osr && let Some(browser_window) = &self.browser_window {
        //   browser_window.set_device_scale_factor(factor);
        // }
    }

    /// Returns the device scale factor. Only used in combination with off-screen
    /// rendering.
    fn device_scale_factor(&self) -> Option<f32> {
        debug_assert!(currently_on_main_thread());
        todo!("Finish implementing device_scale_factor (depends on BrowserWindow)")
        // if self.with_osr && let Some(browser_window) = &self.browser_window {
        //   Some(browser_window.device_scale_factor())
        // } else {
        //   None
        // }
    }

    /// Returns the browser that this window contains, if any.
    fn browser(&self) -> Option<&Browser> {
        debug_assert!(currently_on_main_thread());
        todo!("Finish implementing browser (depends on BrowserWindow)")
    }

    /// Returns the native handle for this window, if any.
    fn window_handle(&self) -> Option<ClientWindowHandle> {
        debug_assert!(currently_on_main_thread());
        self.window
    }

    /// Returns true if this window is using windowless rendering (osr).
    fn with_windowless_rendering(&self) -> bool {
        debug_assert!(currently_on_main_thread());
        debug_assert!(self.inner.initialized);
        self.with_osr
    }

    /// Returns true if this object has been initialized.
    fn is_initialized(&self) -> bool {
        self.inner.initialized
    }

    /// Returns true if the platform window has been created.
    fn is_window_created(&self) -> bool {
        debug_assert!(currently_on_main_thread());
        self.inner.window_created
    }

    // Used to uniquely identify popup windows.
    fn set_popup_id(&mut self, opener_browser_id: i32, popup_id: i32) {
        self.inner.set_popup_id(opener_browser_id, popup_id);
    }

    /// If |popup_id| is -1 only match |opener_browser_id|.
    fn is_popup_id_match(&self, opener_browser_id: i32, popup_id: i32) -> bool {
        self.inner.is_popup_id_match(opener_browser_id, popup_id)
    }

    fn opened_browser_id(&self) -> i32 {
        self.inner.opened_browser_id()
    }

    fn popup_id(&self) -> i32 {
        self.inner.popup_id()
    }
}
