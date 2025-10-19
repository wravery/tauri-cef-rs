use super::{
    osr_renderer::OsrRendererSettings, root_window::RootWindowManager, test_runner::get_test_url,
};
use cef::*;
use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use tests_shared::{
    browser::client_app_browser::ClientAppBrowser,
    common::client_switches::{
        self, CACHE_PATH, FAKE_SCREEN_BOUNDS, HIDE_CHROME_BUBBLES, SHOW_UPDATE_RECT,
    },
};

/// Used to store global context in the browser process. The methods of this
/// class are thread-safe unless otherwise indicated.
pub trait MainContext: Send + Sync {
    /// Returns the global command-line.
    fn command_line(&self) -> Option<CommandLine>;
    /// Returns the full path to the console log file.
    fn console_log_path(&self) -> Option<PathBuf>;
    /// Returns the full path to |file_name|.
    fn download_path(&self, file_name: &str) -> Option<PathBuf>;
    /// Returns the app working directory including trailing path separator.
    fn app_working_directory(&self) -> Option<PathBuf>;
    /// Returns the main application URL based on |command_line| and global state.
    fn main_url(&self, command_line: Option<&mut CommandLine>) -> Option<String>;
    /// Returns the background color.
    fn background_color(&self) -> Option<Color>;
    /// Returns true if the Views framework will be used as the global default.
    fn use_views_global(&self) -> bool;
    /// Returns true if Alloy style will be used as the global default. Alloy style
    /// is optional. Windowless rendering requires Alloy style.
    fn use_alloy_style_global(&self) -> bool;
    /// Returns true if touch events are enabled.
    fn touch_events_enabled(&self) -> bool;
    /// Returns true if the default popup implementation should be used.
    fn use_default_popup(&self) -> bool;
    /// Populate |settings| based on command-line arguments.
    fn populate_settings(&self, settings: Settings) -> Settings;
    /// Populate |settings| based on command-line arguments.
    fn populate_browser_settings(&self, settings: BrowserSettings) -> BrowserSettings;
    /// Populate |settings| based on command-line arguments.
    fn populate_osr_settings(&self, settings: OsrRendererSettings) -> OsrRendererSettings;
    /// Returns the object used to create/manage RootWindow instances.
    fn root_window_manager(&self) -> Option<Arc<RootWindowManager>>;
}

static MAIN_CONTEXT: OnceLock<Option<Arc<Mutex<MainContextImpl>>>> = OnceLock::new();

pub fn get_main_context() -> Option<Arc<Mutex<dyn MainContext>>> {
    MAIN_CONTEXT.get()?.as_ref().map(|context| {
        let context: Arc<Mutex<dyn MainContext>> = context.clone();
        context
    })
}

pub fn create_main_context(
    command_line: Option<CommandLine>,
    terminate_when_all_windows_closed: bool,
) -> Arc<dyn MainContext> {
    Arc::new(MainContextImpl {
        command_line,
        terminate_when_all_windows_closed,
        state: Default::default(),
        background_color: None,
        browser_background_color: None,
        use_windowless_rendering: false,
        windowless_frame_rate: None,
        use_views: false,
        use_alloy_style: false,
        root_window_manager: None,
        shared_texture_enabled: false,
        external_begin_frame_enabled: false,
    })
}

// Initialize CEF and associated main context state. This method must be
// called on the same thread that created this object.
pub fn initialize_main_context(
    main_args: &MainArgs,
    settings: &Settings,
    mut application: App,
    windows_sandbox_information: *mut u8,
) -> bool {
    let Some(context) = MAIN_CONTEXT.get().and_then(|context| context.clone()) else {
        return false;
    };
    debug_assert!(matches!(
        context.lock().map(|context| context.state),
        Ok(ContextState::New)
    ));

    if initialize(
        Some(main_args),
        Some(settings),
        Some(&mut application),
        windows_sandbox_information,
    ) == 0
    {
        return false;
    }

    let Ok(mut context) = context.lock() else {
        return false;
    };

    // Need to create the RootWindowManager after calling CefInitialize because
    // TempWindowX11 uses cef_get_xdisplay().
    context.root_window_manager = Some(Arc::new(RootWindowManager));
    context.state = ContextState::Initialized;
    true
}

/// Shut down CEF and associated context state. This method must be called on
/// the same thread that created this object.
pub fn shutdown_main_context() {
    let context = MAIN_CONTEXT.set(None);
    debug_assert!(matches!(context, Err(Some(_))));
    let Err(Some(context)) = context else {
        return;
    };
    let context = context.lock();
    debug_assert!(context.is_ok());
    let Ok(mut context) = context else {
        return;
    };
    debug_assert!(matches!(context.state, ContextState::Initialized));

    context.root_window_manager = None;
    context.state = ContextState::Shutdown;
}

/// The default URL to load in a browser window.
pub const DEFAULT_URL: &str = "https://www.google.com";

const fn color_set_argb(a: u8, r: u8, g: u8, b: u8) -> Color {
    Color::from_be_bytes([a, r, g, b])
}

fn parse_color(color: &str) -> Option<Color> {
    match color.to_ascii_lowercase().as_str() {
        "black" => Some(color_set_argb(255, 0, 0, 0)),
        "blue" => Some(color_set_argb(255, 0, 0, 255)),
        "green" => Some(color_set_argb(255, 0, 255, 0)),
        "red" => Some(color_set_argb(255, 255, 0, 0)),
        "white" => Some(color_set_argb(255, 255, 255, 255)),
        // Use the default color.
        _ => None,
    }
}

#[derive(Clone, Copy, Default, Debug)]
enum ContextState {
    #[default]
    New,
    Initialized,
    Shutdown,
}

struct MainContextImpl {
    command_line: Option<CommandLine>,
    terminate_when_all_windows_closed: bool,
    state: ContextState,
    background_color: Option<Color>,
    browser_background_color: Option<Color>,
    use_windowless_rendering: bool,
    windowless_frame_rate: Option<u32>,
    use_views: bool,
    use_alloy_style: bool,
    root_window_manager: Option<Arc<RootWindowManager>>,
    shared_texture_enabled: bool,
    external_begin_frame_enabled: bool,
}

impl MainContext for MainContextImpl {
    /// Returns the global command-line.
    fn command_line(&self) -> Option<CommandLine> {
        self.command_line.clone()
    }

    /// Returns the full path to the console log file.
    fn console_log_path(&self) -> Option<PathBuf> {
        Some(self.app_working_directory()?.join("console.log"))
    }

    /// Returns the full path to |file_name|.
    #[cfg(target_os = "windows")]
    fn download_path(&self, file_name: &str) -> Option<PathBuf> {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
        use windows_sys::Win32::{Foundation::*, UI::Shell::*};

        let mut buffer = [0u16; MAX_PATH as usize];
        unsafe {
            SHGetFolderPathW(
                ptr::null_mut(),
                (CSIDL_PERSONAL | CSIDL_FLAG_CREATE) as i32,
                ptr::null_mut(),
                0,
                buffer.as_mut_ptr(),
            )
        };
        let end = buffer.iter().position(|&x| x == 0).unwrap_or(buffer.len());
        Some(PathBuf::from(OsString::from_wide(&buffer[..end])).join(file_name))
    }

    /// Returns the full path to |file_name|.
    #[cfg(not(target_os = "windows"))]
    fn download_path(&self, _file_name: &str) -> Option<PathBuf> {
        None
    }

    /// Returns the app working directory including trailing path separator.
    fn app_working_directory(&self) -> Option<PathBuf> {
        env::current_dir().ok()
    }

    /// Returns the main application URL based on |command_line| and global state.
    fn main_url(&self, command_line: Option<&mut CommandLine>) -> Option<String> {
        let command_line = command_line.cloned().or(self.command_line())?;
        let url_switch = CefString::from(client_switches::URL);
        let main_url = if command_line.has_switch(Some(&url_switch)) != 0 {
            CefString::from(&command_line.switch_value(Some(&url_switch))).to_string()
        } else if self.use_views
            && command_line.has_switch(Some(&CefString::from(client_switches::HIDE_FRAME))) != 0
        {
            // Use the draggable regions test as the default URL for frameless windows.
            get_test_url("draggable")
        } else {
            DEFAULT_URL.to_string()
        };
        Some(main_url)
    }

    /// Returns the background color.
    fn background_color(&self) -> Option<Color> {
        self.background_color
    }

    /// Returns true if the Views framework will be used as the global default.
    fn use_views_global(&self) -> bool {
        self.use_views
    }

    /// Returns true if Alloy style will be used as the global default. Alloy style
    /// is optional. Windowless rendering requires Alloy style.
    fn use_alloy_style_global(&self) -> bool {
        self.use_alloy_style
    }

    /// Returns true if touch events are enabled.
    fn touch_events_enabled(&self) -> bool {
        let Some(command_line) = self.command_line.as_ref() else {
            return false;
        };
        let name = CefString::from("touch-events");
        let value = CefString::from(&command_line.switch_value(Some(&name))).to_string();
        value == "enabled"
    }

    /// Returns true if the default popup implementation should be used.
    fn use_default_popup(&self) -> bool {
        !self.use_windowless_rendering
            && self.command_line.as_ref().map_or(0, |command_line| {
                command_line.has_switch(Some(&CefString::from(client_switches::USE_DEFAULT_POPUP)))
            }) != 0
    }

    /// Populate |settings| based on command-line arguments.
    fn populate_settings(&self, settings: Settings) -> Settings {
        let settings =
            ClientAppBrowser::populate_settings(self.command_line.clone(), vec![], settings);

        let lang_switch = CefString::from("lang");

        Settings {
            cache_path: self
                .command_line
                .as_ref()
                .map(|command_line| {
                    CefString::from(&command_line.switch_value(Some(&CefString::from(CACHE_PATH))))
                })
                .unwrap_or_default(),
            windowless_rendering_enabled: if self.use_windowless_rendering {
                1
            } else {
                settings.windowless_rendering_enabled
            },
            background_color: self
                .browser_background_color
                .unwrap_or(settings.background_color),
            accept_language_list: self
                .command_line
                .as_ref()
                .and_then(|command_line| {
                    if command_line.has_switch(Some(&lang_switch)) != 0 {
                        Some(CefString::from(
                            &command_line.switch_value(Some(&lang_switch)),
                        ))
                    } else {
                        None
                    }
                })
                .unwrap_or(settings.accept_language_list),
            chrome_policy_id: if self.command_line.as_ref().map_or(0, |command_line| {
                command_line.has_switch(Some(&CefString::from("enable-chrome-policy")))
            }) != 0
            {
                #[cfg(target_os = "windows")]
                let policy_id = r#"SOFTWARE\Policies\Google\Chrome"#;
                #[cfg(target_os = "macos")]
                let policy_id = r#"com.google.Chrome"#;
                #[cfg(target_os = "linux")]
                let policy_id = r#"/etc/opt/chrome/policies"#;
                #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
                let policy_id = r#"/etc/opt/chrome/policies"#;
                CefString::from(policy_id)
            } else {
                settings.chrome_policy_id
            },
            ..settings
        }
    }

    /// Populate |settings| based on command-line arguments.
    fn populate_browser_settings(&self, settings: BrowserSettings) -> BrowserSettings {
        let hide_chrome_bubbles = self.command_line.as_ref().map_or(0, |command_line| {
            command_line.has_switch(Some(&CefString::from(HIDE_CHROME_BUBBLES)))
        }) != 0;

        BrowserSettings {
            windowless_frame_rate: self
                .windowless_frame_rate
                .and_then(|rate| i32::try_from(rate).ok())
                .unwrap_or(0),
            background_color: self
                .browser_background_color
                .unwrap_or(settings.background_color),
            chrome_status_bubble: if hide_chrome_bubbles {
                State::DISABLED
            } else {
                settings.chrome_status_bubble
            },
            chrome_zoom_bubble: if hide_chrome_bubbles {
                State::DISABLED
            } else {
                settings.chrome_zoom_bubble
            },
            ..settings
        }
    }

    /// Populate |settings| based on command-line arguments.
    fn populate_osr_settings(&self, settings: OsrRendererSettings) -> OsrRendererSettings {
        let (show_update_rect, fake_screen_bounds) =
            self.command_line
                .as_ref()
                .map_or((false, false), |command_line| {
                    (
                        command_line.has_switch(Some(&CefString::from(SHOW_UPDATE_RECT))) != 0,
                        command_line.has_switch(Some(&CefString::from(FAKE_SCREEN_BOUNDS))) != 0,
                    )
                });

        OsrRendererSettings {
            show_update_rect,
            real_screen_bounds: !fake_screen_bounds,
            shared_texture_enabled: self.shared_texture_enabled,
            begin_frame_rate: self.windowless_frame_rate,
            background_color: self
                .browser_background_color
                .unwrap_or(settings.background_color),
        }
    }

    /// Returns the object used to create/manage RootWindow instances.
    fn root_window_manager(&self) -> Option<Arc<RootWindowManager>> {
        debug_assert!(matches!(self.state, ContextState::Initialized));
        self.root_window_manager.clone()
    }
}
