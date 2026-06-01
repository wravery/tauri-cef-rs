// Copyright (c) 2024 The Chromium Embedded Framework Authors. All rights
// reserved. Use of this source code is governed by a BSD-style license that
// can be found in the LICENSE file.

//! Rust port of the [`client_handler`](https://github.com/chromiumembedded/cef/blob/master/tests/cefclient/browser/client_handler.h)
//! implementation for cefclient. This file implements all CEF handler delegates
//! needed for full browser functionality including downloads, context menus,
//! drag-and-drop, keyboard input, permissions, dialogs, and more.
//!
//! This corresponds to Phase 1.3 of the porting plan - the core ClientHandler
//! that provides all delegate callbacks. Platform-specific UI (GTK, Win32, Cocoa)
//! and OSR rendering are handled in subsequent phases.

use crate::shared::browser::root_window::RootWindowConfig;

use super::{base_client_handler::*, main_context::*, test_runner};
use cef::*;
use std::{
    fs::*,
    io::{self, Write},
    mem,
    path::PathBuf,
    ptr, slice,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tests_shared::{
    browser::main_message_loop::*,
    common::{binary_value_utils::*, client_switches},
};

// ─── ClientHandlerDelegate trait ─────────────────────────────────────────────

/// Delegate interface for window-level events from ClientHandler.
/// Methods are called on the CEF UI thread.
pub trait ClientHandlerDelegate: Send + Sync {
    fn use_views(&self) -> bool;
    fn use_alloy_style(&self) -> bool;
    fn on_browser_created(&self, browser: &Browser);
    fn on_browser_closing(&self, browser: &Browser);
    fn on_browser_closed(&self, browser: &Browser);
    fn on_set_address(&self, url: &str);
    fn on_set_title(&self, title: &str);
    fn on_set_favicon(&self, image: &Image);
    fn on_set_fullscreen(&self, fullscreen: bool);
    fn on_auto_resize(&self, new_size: Size);
    fn on_contents_bounds(&self, new_bounds: Rect);
    fn on_set_loading_state(&self, is_loading: bool, can_go_back: bool, can_go_forward: bool);
    fn on_set_draggable_regions(&self, regions: &[DraggableRegion]);
    fn on_set_focus(&self, source: FocusSource) -> bool;
    fn on_take_focus(&self, next: bool);
    fn on_before_context_menu(&self, model: &MenuModel);
    fn root_window_screen_rect(&self) -> Option<Rect>;
}

struct TestMenuState {
    check_item: bool,
    radio_item: i32,
    chrome_theme_mode_item: i32,
    chrome_theme_color_item: i32,
}

impl Default for TestMenuState {
    fn default() -> Self {
        Self {
            check_item: true,
            radio_item: 0,
            chrome_theme_mode_item: -1,
            chrome_theme_color_item: -1,
        }
    }
}

#[repr(i32)]
#[derive(Copy, Clone, PartialEq, Eq)]
enum ClientMenuId {
    ShowDevTools = sys::cef_menu_id_t::MENU_ID_USER_FIRST as i32,
    CloseDevTools,
    InspectElement,
    ShowSslInfo,
    CursorChangeDisabled,
    MediaHandlingDisabled,
    Offline,
    TestMenuSubMenu,
    TestMenuCheckItem,
    TestMenuRadioItem1,
    TestMenuRadioItem2,
    TestMenuRadioItem3,

    // Chrome theme selection.
    TestMenuTheme,
    TestMenuThemeModeSystem,
    TestMenuThemeModeLight,
    TestMenuThemeModeDark,
    TestMenuThemeColorDefault,
    TestMenuThemeColorRed,
    TestMenuThemeColorGreen,
    TestMenuThemeColorBlue,
    TestMenuThemeCustom,
}

const TEST_MENU_THEME_MODE_FIRST: ClientMenuId = ClientMenuId::TestMenuThemeModeSystem;
const TEST_MENU_THEME_MODE_LAST: ClientMenuId = ClientMenuId::TestMenuThemeModeDark;
const TEST_MENU_THEME_COLOR_FIRST: ClientMenuId = ClientMenuId::TestMenuThemeColorDefault;
const TEST_MENU_THEME_COLOR_LAST: ClientMenuId = ClientMenuId::TestMenuThemeColorBlue;

impl From<ClientMenuId> for i32 {
    fn from(value: ClientMenuId) -> Self {
        value as i32
    }
}

impl TryFrom<i32> for ClientMenuId {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        const SHOW_DEVTOOLS: i32 = ClientMenuId::ShowDevTools as i32;
        const CLOSE_DEVTOOLS: i32 = ClientMenuId::CloseDevTools as i32;
        const INSPECT_ELEMENT: i32 = ClientMenuId::InspectElement as i32;
        const SHOW_SSL_INFO: i32 = ClientMenuId::ShowSslInfo as i32;
        const CURSOR_CHANGE_DISABLED: i32 = ClientMenuId::CursorChangeDisabled as i32;
        const MEDIA_HANDLING_DISABLED: i32 = ClientMenuId::MediaHandlingDisabled as i32;
        const OFFLINE: i32 = ClientMenuId::Offline as i32;
        const SUBMENU: i32 = ClientMenuId::TestMenuSubMenu as i32;
        const CHECK_ITEM: i32 = ClientMenuId::TestMenuCheckItem as i32;
        const RADIO_ITEM_1: i32 = ClientMenuId::TestMenuRadioItem1 as i32;
        const RADIO_ITEM_2: i32 = ClientMenuId::TestMenuRadioItem2 as i32;
        const RADIO_ITEM_3: i32 = ClientMenuId::TestMenuRadioItem3 as i32;
        const THEME: i32 = ClientMenuId::TestMenuTheme as i32;
        const THEME_MODE_SYSTEM: i32 = ClientMenuId::TestMenuThemeModeSystem as i32;
        const THEME_MODE_LIGHT: i32 = ClientMenuId::TestMenuThemeModeLight as i32;
        const THEME_MODE_DARK: i32 = ClientMenuId::TestMenuThemeModeDark as i32;
        const THEME_COLOR_DEFAULT: i32 = ClientMenuId::TestMenuThemeColorDefault as i32;
        const THEME_COLOR_RED: i32 = ClientMenuId::TestMenuThemeColorRed as i32;
        const THEME_COLOR_GREEN: i32 = ClientMenuId::TestMenuThemeColorGreen as i32;
        const THEME_COLOR_BLUE: i32 = ClientMenuId::TestMenuThemeColorBlue as i32;
        const THEME_CUSTOM: i32 = ClientMenuId::TestMenuThemeCustom as i32;

        Ok(match value {
            SHOW_DEVTOOLS => ClientMenuId::ShowDevTools,
            CLOSE_DEVTOOLS => ClientMenuId::CloseDevTools,
            INSPECT_ELEMENT => ClientMenuId::InspectElement,
            SHOW_SSL_INFO => ClientMenuId::ShowSslInfo,
            CURSOR_CHANGE_DISABLED => ClientMenuId::CursorChangeDisabled,
            MEDIA_HANDLING_DISABLED => ClientMenuId::MediaHandlingDisabled,
            OFFLINE => ClientMenuId::Offline,
            SUBMENU => ClientMenuId::TestMenuSubMenu,
            CHECK_ITEM => ClientMenuId::TestMenuCheckItem,
            RADIO_ITEM_1 => ClientMenuId::TestMenuRadioItem1,
            RADIO_ITEM_2 => ClientMenuId::TestMenuRadioItem2,
            RADIO_ITEM_3 => ClientMenuId::TestMenuRadioItem3,
            THEME => ClientMenuId::TestMenuTheme,
            THEME_MODE_SYSTEM => ClientMenuId::TestMenuThemeModeSystem,
            THEME_MODE_LIGHT => ClientMenuId::TestMenuThemeModeLight,
            THEME_MODE_DARK => ClientMenuId::TestMenuThemeModeDark,
            THEME_COLOR_DEFAULT => ClientMenuId::TestMenuThemeColorDefault,
            THEME_COLOR_RED => ClientMenuId::TestMenuThemeColorRed,
            THEME_COLOR_GREEN => ClientMenuId::TestMenuThemeColorGreen,
            THEME_COLOR_BLUE => ClientMenuId::TestMenuThemeColorBlue,
            THEME_CUSTOM => ClientMenuId::TestMenuThemeCustom,
            _ => return Err(()),
        })
    }
}

const COLOR_TRANSPARENT: Color = 0x00000000;
const COLOR_RED: Color = 0xFFFF0000;
const COLOR_GREEN: Color = 0xFF00FF00;
const COLOR_BLUE: Color = 0xFF0000FF;

pub const FOCUS_NODE_CHANGED_MESSAGE: &str = "ClientRenderer.FocusedNodeChanged";

fn get_time_string(value: &Time) -> String {
    let mut time_t = 0;
    if 1 != time_to_timet(Some(value), Some(&mut time_t)) || time_t == 0 {
        return "Unspecified".into();
    }

    let month = match value.month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Invalid".into(),
    };

    let Time {
        day_of_month,
        year,
        hour,
        minute,
        second,
        ..
    } = value;

    format!("{month} {day_of_month}, {year} {hour:02}:{minute:02}:{second:02}")
}

fn get_base_time_string(value: Basetime) -> String {
    let mut time = Default::default();
    if 1 != time_from_basetime(value.into(), Some(&mut time)) {
        return "Invalid".into();
    }

    get_time_string(&time)
}

fn get_binary_string(value: Option<&BinaryValue>) -> String {
    let Some(value) = value else {
        return "&npsb;".into();
    };

    let size = value.size();
    let mut buffer = vec![0u8; size];
    let size = value.data(Some(&mut buffer), 0);
    buffer.truncate(size);
    CefString::from(&base64_encode(Some(&buffer))).to_string()
}

macro_rules! add_flags {
    ($value:ident, $name:ident, $prefix:literal, [ $($flag:ident),+ ]) => {
        {
            let mut result = Vec::new();
            let raw_value = $value.get_raw();

            $(
                if raw_value & $name::$flag.get_raw() != 0 {
                    result.push(concat!($prefix, stringify!($flag)));
                }
            )+

            if result.is_empty() {
                return "nbsp;".to_string();
            }

            result.push("");
            result.join("<br/>")
        }
    };
}

macro_rules! match_values {
    ($value:ident, $name:ident, $prefix:literal, [ $($option:ident),+ ]) => {
        match $value {
            $(
                $name::$option => return concat!($prefix, stringify!($option)).to_string(),
            )+
            _ => {},
        }
    };
}

fn get_cert_status_string(status: CertStatus) -> String {
    add_flags!(
        status,
        CertStatus,
        "CERT_STATUS_",
        [
            COMMON_NAME_INVALID,
            DATE_INVALID,
            AUTHORITY_INVALID,
            NO_REVOCATION_MECHANISM,
            UNABLE_TO_CHECK_REVOCATION,
            REVOKED,
            INVALID,
            WEAK_SIGNATURE_ALGORITHM,
            NON_UNIQUE_NAME,
            WEAK_KEY,
            PINNED_KEY_MISSING,
            NAME_CONSTRAINT_VIOLATION,
            VALIDITY_TOO_LONG,
            IS_EV,
            REV_CHECKING_ENABLED,
            SHA1_SIGNATURE_PRESENT,
            CT_COMPLIANCE_FAILED
        ]
    )
}

fn get_ssl_version_string(version: SslVersion) -> String {
    match_values!(
        version,
        SslVersion,
        "SSL_CONNECTION_VERSION_",
        [UNKNOWN, SSL2, SSL3, TLS1, TLS1_1, TLS1_2, TLS1_3, QUIC]
    );

    Default::default()
}

fn get_content_status_string(status: SslContentStatus) -> String {
    match_values!(status, SslContentStatus, "SSL_CONTENT_", [NORMAL_CONTENT]);

    add_flags!(
        status,
        SslContentStatus,
        "SSL_CONTENT_",
        [DISPLAYED_INSECURE_CONTENT, RAN_INSECURE_CONTENT]
    )
}

/// Return HTML string with information about a certificate.
fn get_certificate_information(cert: X509Certificate, cert_status: CertStatus) -> String {
    // Build a table showing certificate information. Various types of invalid
    // certificates can be tested using https://badssl.com/.
    let mut result = vec![
        r#"<h3>X.509 Certificate Information:</h3>"#,
        r#"<table border=1><tr><th>Field</th><th>Value</th></tr>"#,
    ];

    let cert_status = if cert_status != CertStatus::NONE {
        Some(get_cert_status_string(cert_status))
    } else {
        None
    };
    if let Some(cert_status) = cert_status.as_deref() {
        result.push(r#"<tr><td>Status</td><td>"#);
        result.push(cert_status);
        result.push(r#"</td></tr>"#);
    }

    let subject = cert.subject();
    let subject = subject.map(|subject| CefString::from(&subject.display_name()).to_string());
    let subject = subject.as_deref().unwrap_or("&nbsp;");
    result.push(r#"<tr><td>Subject</td><td>"#);
    result.push(subject);
    result.push(r#"</td></tr>"#);

    let issuer = cert.issuer();
    let issuer = issuer.map(|issuer| CefString::from(&issuer.display_name()).to_string());
    let issuer = issuer.as_deref().unwrap_or("&nbsp;");
    result.push(r#"<tr><td>Issuer</td><td>"#);
    result.push(issuer);
    result.push(r#"</td></tr>"#);

    let serial_number = get_binary_string(cert.serial_number().as_ref());
    result.push(r#"<tr><td>Serial #*</td><td>"#);
    result.push(&serial_number);
    result.push(r#"</td></tr>"#);

    let valid_start = get_base_time_string(cert.valid_start());
    result.push(r#"<tr><td>Valid Start</td><td>"#);
    result.push(&valid_start);
    result.push(r#"</td></tr>"#);

    let valid_expiry = get_base_time_string(cert.valid_expiry());
    result.push(r#"<tr><td>Valid Expiry</td><td>"#);
    result.push(&valid_expiry);
    result.push(r#"</td></tr>"#);

    let mut der_chain_list = Vec::new();
    cert.derencoded_issuer_chain(Some(&mut der_chain_list));
    let mut pem_chain_list = Vec::new();
    cert.pemencoded_issuer_chain(Some(&mut pem_chain_list));
    assert_eq!(der_chain_list.len(), pem_chain_list.len());

    let der_encoded = cert.derencoded();
    let pem_encoded = cert.pemencoded();

    let der_chain: Vec<_> = std::iter::once(der_encoded)
        .chain(der_chain_list.into_iter())
        .map(|der| get_binary_string(der.as_ref()))
        .collect();
    let pem_chain: Vec<_> = std::iter::once(pem_encoded)
        .chain(pem_chain_list.into_iter())
        .map(|pem| get_binary_string(pem.as_ref()))
        .collect();

    for (der, pem) in der_chain.iter().zip(pem_chain.iter()) {
        result.push(r#"<tr><td>DER Encoded*</td>"#);
        result.push(r#"<td style="max-width:800px;overflow:scroll;">"#);
        result.push(der.as_str());
        result.push(r#"</td></tr>"#);
        result.push(r#"<tr><td>PEM Encoded*</td>"#);
        result.push(r#"<td style="max-width:800px;overflow:scroll;">"#);
        result.push(pem.as_str());
        result.push(r#"</td></tr>"#);
    }

    result.push(r#"</table> * Displayed value is base64 encoded."#);

    result.join("")
}

fn on_test_process_message_received(
    frame: &Frame,
    process_message: &ProcessMessage,
    finish_time: ElapsedMicros,
) {
    assert_ne!(process_message.is_valid(), 0);
    let Some(input_args) = process_message.argument_list() else {
        return;
    };
    assert_eq!(input_args.size(), 1);
    let Some(input_arg) = input_args.binary(0) else {
        return;
    };
    let renderer_message = RendererMessage::from(&input_arg);
    let name = str::from_utf8(TEST_SEND_PROCESS_MESSAGE)
        .ok()
        .map(CefString::from);
    let Some(mut response) = process_message_create(name.as_ref()) else {
        return;
    };
    let Some(args) = response.argument_list() else {
        return;
    };
    let data = BrowserMessage {
        test_id: renderer_message.test_id,
        duration: finish_time.elapsed(&renderer_message.start_time),
        start_time: ElapsedMicros::now(),
    };
    let Some(mut data) = Option::<BinaryValue>::from(&data) else {
        return;
    };
    args.set_binary(0, Some(&mut data));
    frame.send_process_message(ProcessId::RENDERER, Some(&mut response));
}

fn on_test_smr_process_message_received(
    frame: &Frame,
    process_message: &ProcessMessage,
    finish_time: ElapsedMicros,
) {
    assert_ne!(process_message.is_valid(), 0);
    let Some(region) = process_message.shared_memory_region() else {
        return;
    };
    debug_assert!(region.size() >= mem::size_of::<RendererMessage>());
    let data = unsafe { slice::from_raw_parts(region.memory() as *const u8, region.size()) };
    let Some(data) = binary_value_create(Some(data)) else {
        return;
    };
    let renderer_message = RendererMessage::from(&data);
    let name = str::from_utf8(TEST_SEND_SMR_PROCESS_MESSAGE)
        .ok()
        .map(CefString::from);
    let data = BrowserMessage {
        test_id: renderer_message.test_id,
        duration: finish_time.elapsed(&renderer_message.start_time),
        start_time: ElapsedMicros::now(),
    };
    let Some(data) = Option::<BinaryValue>::from(&data) else {
        return;
    };
    let Some(builder) = shared_process_message_builder_create(name.as_ref(), data.size()) else {
        return;
    };
    unsafe {
        ptr::copy_nonoverlapping(
            data.raw_data() as *const u8,
            builder.memory() as *mut u8,
            data.size(),
        );
    }
    let Some(mut response) = builder.build() else {
        return;
    };
    frame.send_process_message(ProcessId::RENDERER, Some(&mut response));
}

/// Only the specified icons will be allowed.
fn is_allowed_page_action_icon(icon_type: ChromePageActionIconType) -> bool {
    matches!(
        icon_type,
        ChromePageActionIconType::FIND | ChromePageActionIconType::ZOOM
    )
}

/// All configurable buttons will be disabled.
fn is_allowed_toolbar_button(_button_type: ChromeToolbarButtonType) -> bool {
    false
}

/// Version-safe static declarations of IDC variables using names from
/// cef_command_ids.h.
macro_rules! declare_command_id {
    ($name:ident) => {{
        static IDC: OnceLock<i32> = OnceLock::new();
        *IDC.get_or_init(|| unsafe {
            sys::cef_id_for_command_id_name(cef::resources::$name.as_ptr())
        })
    }};
}

fn is_allowed_app_menu_command_id(command_id: i32) -> bool {
    static ALLOWED_COMMAND_IDS: OnceLock<Vec<i32>> = OnceLock::new();
    let allowed_command_ids = ALLOWED_COMMAND_IDS
        .get_or_init(|| {
            // Only the commands in this array will be allowed.
            vec![
                declare_command_id!(IDC_NEW_WINDOW),
                declare_command_id!(IDC_NEW_INCOGNITO_WINDOW),
                // Zoom buttons.
                declare_command_id!(IDC_ZOOM_MENU),
                declare_command_id!(IDC_ZOOM_PLUS),
                declare_command_id!(IDC_ZOOM_NORMAL),
                declare_command_id!(IDC_ZOOM_MINUS),
                declare_command_id!(IDC_FULLSCREEN),
                declare_command_id!(IDC_PRINT),
                declare_command_id!(IDC_FIND),
                declare_command_id!(IDC_FIND_NEXT),
                declare_command_id!(IDC_FIND_PREVIOUS),
                // "More tools" sub-menu and contents.
                declare_command_id!(IDC_MORE_TOOLS_MENU),
                declare_command_id!(IDC_CLEAR_BROWSING_DATA),
                declare_command_id!(IDC_MANAGE_EXTENSIONS),
                declare_command_id!(IDC_PERFORMANCE),
                declare_command_id!(IDC_TASK_MANAGER),
                declare_command_id!(IDC_DEV_TOOLS),
                // Edit buttons.
                declare_command_id!(IDC_EDIT_MENU),
                declare_command_id!(IDC_CUT),
                declare_command_id!(IDC_COPY),
                declare_command_id!(IDC_PASTE),
                declare_command_id!(IDC_OPTIONS),
                declare_command_id!(IDC_EXIT),
            ]
        })
        .as_slice();

    allowed_command_ids.contains(&command_id)
}

fn is_allowed_context_menu_command_id(command_id: i32) -> bool {
    // Allow commands added by web content.
    if command_id >= declare_command_id!(IDC_CONTENT_CONTEXT_CUSTOM_FIRST)
        && command_id <= declare_command_id!(IDC_CONTENT_CONTEXT_CUSTOM_LAST)
    {
        return true;
    }

    // Allow commands added by extensions.
    if command_id >= declare_command_id!(IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST)
        && command_id <= declare_command_id!(IDC_EXTENSIONS_CONTEXT_CUSTOM_LAST)
    {
        return true;
    }

    static ALLOWED_COMMAND_IDS: OnceLock<Vec<i32>> = OnceLock::new();
    let allowed_command_ids = ALLOWED_COMMAND_IDS
        .get_or_init(|| {
            // Only the commands in this array will be allowed.
            vec![
                // Page navigation.
                declare_command_id!(IDC_BACK),
                declare_command_id!(IDC_FORWARD),
                declare_command_id!(IDC_RELOAD),
                declare_command_id!(IDC_RELOAD_BYPASSING_CACHE),
                declare_command_id!(IDC_RELOAD_CLEARING_CACHE),
                declare_command_id!(IDC_STOP),
                // Printing.
                declare_command_id!(IDC_PRINT),
                // Edit controls.
                declare_command_id!(IDC_CONTENT_CONTEXT_CUT),
                declare_command_id!(IDC_CONTENT_CONTEXT_COPY),
                declare_command_id!(IDC_CONTENT_CONTEXT_PASTE),
                declare_command_id!(IDC_CONTENT_CONTEXT_PASTE_AND_MATCH_STYLE),
                declare_command_id!(IDC_CONTENT_CONTEXT_DELETE),
                declare_command_id!(IDC_CONTENT_CONTEXT_SELECTALL),
                declare_command_id!(IDC_CONTENT_CONTEXT_UNDO),
                declare_command_id!(IDC_CONTENT_CONTEXT_REDO),
            ]
        })
        .as_slice();

    allowed_command_ids.contains(&command_id)
}

fn filter_context_menu_model(model: &MenuModel) {
    // Evaluate from the bottom to the top because we'll be removing menu items.
    for i in (0..model.count()).rev() {
        match model.type_at(i) {
            MenuItemType::SUBMENU => {
                // Filter sub-menu and remove if empty.
                let Some(sub_menu) = model.sub_menu_at(i) else {
                    continue;
                };
                filter_context_menu_model(&sub_menu);
                if sub_menu.count() == 0 {
                    model.remove_at(i);
                }
            }
            MenuItemType::SEPARATOR => {
                // A separator shouldn't be the first or last element in the menu, and
                // there shouldn't be multiple in a row.
                if i == 0
                    || i == model.count() - 1
                    || model.type_at(i + 1) == MenuItemType::SEPARATOR
                {
                    model.remove_at(i);
                }
            }
            _ => {
                if !is_allowed_context_menu_command_id(model.command_id_at(i)) {
                    model.remove_at(i);
                }
            }
        }
    }
}

wrap_download_image_callback! {
    struct ClientDownloadImageCallback {
        inner: Arc<Mutex<ClientHandler>>
    }

    impl DownloadImageCallback {
        fn on_download_image_finished(
            &self,
            image_url: Option<&CefString>,
            http_status_code: i32,
            image: Option<&mut Image>,
        ) {
            let Some(image) = image.cloned() else {
                return;
            };
            let Ok(handler) = self.inner.lock() else {
                return;
            };
            handler.notify_favicon(image);
        }
    }
}

// Client handler abstract base class. Provides common functionality shared by
// all concrete client handler implementations.
pub struct ClientHandler {
    weak_self: Weak<Mutex<Self>>,
    base: Arc<Mutex<BaseClientHandler>>,
    delegate: Option<Box<dyn ClientHandlerDelegate>>,
    use_views: bool,
    use_alloy_style: bool,
    is_osr: bool,
    with_controls: bool,
    startup_url: String,
    mouse_cursor_change_disabled: bool,
    media_handling_disabled: bool,
    offline: bool,
    filter_chrome_commands: bool,
    download_favicon_images: bool,
    #[cfg(target_os = "linux")]
    file_dialog_handler: Option<DialogHandler>,
    #[cfg(target_os = "linux")]
    js_dialog_handler: Option<JsdialogHandler>,
    #[cfg(target_os = "linux")]
    print_handler: Option<PrintHandler>,
    test_menu_state: TestMenuState,
    console_log_file: Option<PathBuf>,
    focus_on_editable_field: bool,
}

impl ClientHandler {
    /// Creates a new windowed ClientHandler.
    pub fn new(
        delegate: Option<Box<dyn ClientHandlerDelegate>>,
        is_osr: bool,
        with_controls: bool,
        startup_url: &str,
    ) -> Arc<Mutex<Self>> {
        let (use_views_global, use_alloy_style_global, console_log_file) = get_main_context()
            .and_then(|main_context| {
                let main_context = main_context.lock().ok()?;
                Some((
                    main_context.use_views_global(),
                    main_context.use_alloy_style_global(),
                    main_context.console_log_path(),
                ))
            })
            .unwrap_or((false, false, None));

        let (use_views, use_alloy_style) = delegate
            .as_ref()
            .map_or((use_views_global, use_alloy_style_global), |d| {
                (d.use_views(), d.use_alloy_style())
            });

        struct BaseDelegate;

        impl BaseClientHandlerDelegate for BaseDelegate {
            fn track_as_other_browser(&self) -> bool {
                false
            }
        }

        let base = BaseClientHandler::new(Box::new(BaseDelegate));

        debug_assert!(console_log_file.is_some());

        // Read command line settings.
        let command_line = command_line_get_global();
        let (mouse_cursor_change_disabled, offline, filter_chrome_commands) =
            command_line.map_or((false, false, false), |command_line| {
                (
                    command_line.has_switch(Some(&CefString::from(
                        client_switches::MOUSE_CURSOR_CHANGE_DISABLED,
                    ))) != 0,
                    command_line.has_switch(Some(&CefString::from(client_switches::OFFLINE))) != 0,
                    command_line.has_switch(Some(&CefString::from(
                        client_switches::FILTER_CHROME_COMMANDS,
                    ))) != 0,
                )
            });

        Arc::new_cyclic(|weak_self| {
            Mutex::new(Self {
                weak_self: weak_self.clone(),
                base,
                delegate,
                use_views,
                use_alloy_style,
                is_osr,
                with_controls,
                startup_url: startup_url.into(),
                mouse_cursor_change_disabled,
                media_handling_disabled: true,
                offline,
                filter_chrome_commands,
                download_favicon_images: false,
                #[cfg(target_os = "linux")]
                file_dialog_handler: None,
                #[cfg(target_os = "linux")]
                js_dialog_handler: None,
                #[cfg(target_os = "linux")]
                print_handler: None,
                test_menu_state: Default::default(),
                console_log_file,
                focus_on_editable_field: false,
            })
        })
    }

    pub fn detach_delegate(&mut self) {
        assert!(currently_on_main_thread());
        debug_assert!(self.delegate.is_some());
        self.delegate = None;
    }

    fn on_process_message_received(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        source_process: ProcessId,
        message: Option<ProcessMessage>,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let finish_time = ElapsedMicros::now();
        if let Ok(base) = self.base.lock() {
            if base.on_process_message_received(
                browser.clone(),
                frame.clone(),
                source_process,
                message.clone(),
            ) {
                return true;
            }
        }

        let (Some(frame), Some(message)) = (frame, message) else {
            return false;
        };
        let message_name = CefString::from(&message.name()).to_string();
        match message_name.as_bytes() {
            TEST_SEND_PROCESS_MESSAGE => {
                on_test_process_message_received(&frame, &message, finish_time);
                true
            }
            TEST_SEND_SMR_PROCESS_MESSAGE => {
                on_test_process_message_received(&frame, &message, finish_time);
                true
            }
            _ => false,
        }
    }

    pub fn on_chrome_command(&self, command_id: i32, disposition: WindowOpenDisposition) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        debug_assert!(!self.use_alloy_style);

        let allowed = is_allowed_app_menu_command_id(command_id)
            || is_allowed_context_menu_command_id(command_id);

        let block = if self.filter_chrome_commands {
            // Block all commands that aren't specifically allowed.
            !allowed
        } else if !self.with_controls {
            // If controls are hidden, block all commands that don't target the current
            // tab or aren't specifically allowed.
            disposition != WindowOpenDisposition::CURRENT_TAB || !allowed
        } else {
            // Default handling.
            false
        };

        if block {
            eprintln!("Blocking command {command_id} with disposition {disposition:?}");
        }

        block
    }

    pub fn is_chrome_app_menu_item_visible(&self, command_id: i32) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        debug_assert!(!self.use_alloy_style);
        !self.filter_chrome_commands || is_allowed_app_menu_command_id(command_id)
    }

    pub fn is_chrome_page_action_icon_visible(&self, icon_type: ChromePageActionIconType) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        debug_assert!(!self.use_alloy_style);
        !self.filter_chrome_commands || is_allowed_page_action_icon(icon_type)
    }

    pub fn is_chrome_toolbar_button_visible(&self, button_type: ChromeToolbarButtonType) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        debug_assert!(!self.use_alloy_style);
        !self.filter_chrome_commands || is_allowed_toolbar_button(button_type)
    }

    pub fn on_before_context_menu(
        &mut self,
        browser: &Browser,
        params: &ContextMenuParams,
        model: &MenuModel,
    ) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);

        if !self.use_alloy_style && (!self.with_controls || self.filter_chrome_commands) {
            // Remove all disallowed menu items.
            filter_context_menu_model(model);
        }

        const TYPEFLAG_MASK: u32 = sys::cef_context_menu_type_flags_t::CM_TYPEFLAG_PAGE.0
            | sys::cef_context_menu_type_flags_t::CM_TYPEFLAG_FRAME.0;
        let type_flags: sys::cef_context_menu_type_flags_t = params.type_flags().into();
        if (type_flags.0 & TYPEFLAG_MASK) != 0 {
            // Add a separator if the menu already has items.
            if model.count() > 0 {
                model.add_separator();
            }

            // Add DevTools items to all context menus.
            model.add_item(
                ClientMenuId::ShowDevTools.into(),
                Some(&CefString::from("&Show DevTools")),
            );
            model.add_item(
                ClientMenuId::CloseDevTools.into(),
                Some(&CefString::from("Close DevTools")),
            );

            if self.use_alloy_style {
                // Chrome style already gives us an "Inspect" menu item.
                model.add_separator();
                model.add_item(
                    ClientMenuId::InspectElement.into(),
                    Some(&CefString::from("Inspect Element")),
                );
            }

            if self.has_ssl_information(browser) {
                model.add_separator();
                model.add_item(
                    ClientMenuId::ShowSslInfo.into(),
                    Some(&CefString::from("Show SSL Info")),
                );
            }

            if self.use_alloy_style {
                // TODO(chrome-runtime): Add support for this.
                model.add_separator();
                model.add_item(
                    ClientMenuId::CursorChangeDisabled.into(),
                    Some(&CefString::from("Cursor change disabled")),
                );
                if self.mouse_cursor_change_disabled {
                    model.set_checked(ClientMenuId::CursorChangeDisabled.into(), 1);
                }

                model.add_separator();
                model.add_item(
                    ClientMenuId::MediaHandlingDisabled.into(),
                    Some(&CefString::from("Media handling disabled")),
                );
                if self.media_handling_disabled {
                    model.set_checked(ClientMenuId::MediaHandlingDisabled.into(), 1);
                }
            }

            model.add_separator();
            model.add_item(
                ClientMenuId::Offline.into(),
                Some(&CefString::from("Offline mode")),
            );
            if self.offline {
                model.set_checked(ClientMenuId::Offline.into(), 1);
            }

            // Test context menu features.
            self.build_test_menu(browser, model);
        }
    }

    pub fn on_context_menu_command(
        &mut self,
        browser: &Browser,
        command_id: i32,
        params: &ContextMenuParams,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);

        let Ok(command_id) = ClientMenuId::try_from(command_id) else {
            return false;
        };
        match command_id {
            ClientMenuId::ShowDevTools => {
                self.show_dev_tools(browser, Default::default());
                true
            }
            ClientMenuId::CloseDevTools => {
                self.close_dev_tools(browser);
                true
            }
            ClientMenuId::InspectElement => {
                self.show_dev_tools(
                    browser,
                    Point {
                        x: params.xcoord(),
                        y: params.ycoord(),
                    },
                );
                true
            }
            ClientMenuId::ShowSslInfo => {
                self.show_ssl_information(browser);
                true
            }
            ClientMenuId::CursorChangeDisabled => {
                self.mouse_cursor_change_disabled = !self.mouse_cursor_change_disabled;
                true
            }
            ClientMenuId::MediaHandlingDisabled => {
                self.media_handling_disabled = !self.media_handling_disabled;
                true
            }
            ClientMenuId::Offline => {
                self.offline = !self.offline;
                self.set_offline_state(browser, self.offline);
                true
            }
            _ => self.execute_test_menu(browser, command_id.into()),
        }
    }

    pub fn on_address_change(&self, frame: &Frame, url: String) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);

        // Only update the address for the main (top-level) frame.
        if frame.is_main() != 0 {
            self.notify_address(url);
        }
    }

    fn notify_address(&self, url: String) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_address(url);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_set_address(&url);
        }
    }

    pub fn on_title_change(&self, title: String) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.notify_title(title);
    }

    fn notify_title(&self, title: String) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_title(title);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_set_title(&title);
        }
    }

    pub fn on_favicon_url_change(&self, browser: &Browser, icon_url: &str) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if !self.download_favicon_images {
            return;
        }
        let (Some(inner), Some(host)) = (self.weak_self.upgrade(), browser.host()) else {
            return;
        };
        let mut handler = ClientDownloadImageCallback::new(inner);
        host.download_image(
            Some(&CefString::from(icon_url)),
            1,
            16,
            0,
            Some(&mut handler),
        );
    }

    fn notify_favicon(&self, image: Image) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_favicon(image);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_set_favicon(&image);
        }
    }

    pub fn on_fullscreen_mode_change(&self, fullscreen: bool) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.notify_fullscreen(fullscreen);
    }

    fn notify_fullscreen(&self, fullscreen: bool) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_fullscreen(fullscreen);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_set_fullscreen(fullscreen);
        }
    }

    pub fn on_console_message(&self, level: LogSeverity, message: &str, source: &str, line: i32) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let Some(console_log_file) = self.console_log_file.as_deref() else {
            return;
        };

        let level = match level {
            LogSeverity::VERBOSE => Some("Debug"),
            LogSeverity::INFO => Some("Info"),
            LogSeverity::WARNING => Some("Warn"),
            LogSeverity::ERROR => Some("Error"),
            _ => None,
        };
        let _ = (move || -> io::Result<()> {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(console_log_file)?;
            if let Some(level) = level {
                writeln!(file, "Level: {level}")?;
            }

            writeln!(file, "Message: {message}")?;
            writeln!(file, "Source: {source}")?;
            writeln!(file, "Line: {line}")?;
            writeln!(file, "-----------------------")?;
            file.flush()
        })();
    }

    pub fn on_auto_resize(&self, size: Size) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.notify_auto_resize(size);
    }

    fn notify_auto_resize(&self, size: Size) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_auto_resize(size);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_auto_resize(size);
        }
    }

    pub fn on_cursor_change(&self) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Return true to disable default handling of cursor changes.
        self.mouse_cursor_change_disabled
    }

    pub fn on_contents_bounds_change(&self, bounds: Rect) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.notify_contents_bounds(bounds);
    }

    fn notify_contents_bounds(&self, bounds: Rect) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_contents_bounds(bounds);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_contents_bounds(bounds);
        }
    }

    pub fn root_window_screen_rect(&self) -> Option<Rect> {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.delegate
            .as_ref()
            .and_then(|delegate| delegate.root_window_screen_rect())
    }

    pub fn can_download(&self) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Allow the download.
        true
    }

    pub fn on_before_download(
        &self,
        suggested_name: &str,
        callback: BeforeDownloadCallback,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let download_path = get_main_context()
            .and_then(|c| {
                let c = c.lock().ok()?;
                let path = c.download_path(suggested_name)?.display().to_string();
                Some(CefString::from(path.as_str()))
            })
            .unwrap_or_else(|| CefString::from(suggested_name));

        // Continue the download and show the "Save As" dialog.
        callback.cont(Some(&download_path), 1);
        true
    }

    pub fn on_download_updated(&self, browser: &Browser, download: &DownloadItem) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if download.is_complete() != 0 {
            let file_name = CefString::from(&download.full_path());
            let message = format!("File {file_name} downloaded successfully.");
            test_runner::alert(browser, &message);
        }
    }

    pub fn on_drag_enter(
        &self,
        browser: &Browser,
        drag_data: &DragData,
        mask: DragOperationsMask,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Forbid dragging of URLs and files.
        let mask = sys::cef_drag_operations_mask_t::from(mask);
        let block = (mask.0 & sys::cef_drag_operations_mask_t::DRAG_OPERATION_LINK.0) != 0
            && drag_data.is_fragment() == 0;
        if block {
            test_runner::alert(browser, "cefclient blocks dragging of URLs and files");
        }
        block
    }

    pub fn on_draggable_regions_changed(&self, regions: &[DraggableRegion]) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.notify_draggable_regions(regions);
    }

    fn notify_draggable_regions(&self, regions: &[DraggableRegion]) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            let regions = regions.to_vec();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_draggable_regions(&regions);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_set_draggable_regions(regions);
        }
    }

    pub fn on_take_focus(&self, next: bool) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.notify_take_focus(next)
    }

    fn notify_take_focus(&self, next: bool) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_take_focus(next);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_take_focus(next);
        }
    }

    pub fn on_set_focus(&self, source: FocusSource) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);

        if let Ok(base) = self.base.lock()
            && base.on_set_focus()
        {
            true
        } else if let Some(delegate) = &self.delegate
            && delegate.on_set_focus(source)
        {
            true
        } else {
            false
        }
    }

    pub fn on_pre_key_event(&self, _browser: &Browser, _event: &KeyEvent) -> bool {
        // if _event.focus_on_editable_field == 0 && _event.windows_key_code == 0x20 {
        //     // Special handling for the space character when an input element does not
        //     // have focus. Handling the event in OnPreKeyEvent() keeps the event from
        //     // being processed in the renderer. If we instead handled the event in the
        //     // OnKeyEvent() method the space key would cause the window to scroll in
        //     // addition to showing the alert box.
        //     if _event.type_ == KeyEventType::RAWKEYDOWN {
        //         test_runner::alert(_browser, "You pressed the space bar!");
        //     }
        //     return true;
        // }

        false
    }

    pub fn on_before_popup(
        &self,
        browser: &Browser,
        popup_id: i32,
        target_disposition: WindowOpenDisposition,
        popup_features: &PopupFeatures,
        window_info: &mut WindowInfo,
        client: &mut Option<Client>,
        settings: &mut BrowserSettings,
    ) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);

        if target_disposition == WindowOpenDisposition::NEW_PICTURE_IN_PICTURE {
            // Use default handling for document picture-in-picture popups.
            *client = None;
            return;
        }

        // Potentially create a new RootWindow for the popup browser that will be
        // created asynchronously.
        self.create_popup_window(
            browser,
            popup_id,
            false,
            popup_features,
            window_info,
            client,
            settings,
        );
    }

    pub fn on_before_popup_aborted(&self, browser: &Browser, popup_id: i32) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Some(manager) = get_main_context().and_then(|context| {
            let context = context.lock().ok()?;
            context.root_window_manager()
        }) {
            manager.abort_or_close_popup(browser.identifier(), popup_id);
        }
    }

    pub fn on_before_dev_tools_popup(
        &self,
        browser: &Browser,
        window_info: &mut WindowInfo,
        client: &mut Option<Client>,
        settings: &mut BrowserSettings,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Potentially create a new RootWindow for the DevTools popup browser that
        // will be created immediately after this method returns.
        !self.create_popup_window(
            browser,
            -1,
            true,
            &Default::default(),
            window_info,
            client,
            settings,
        )
    }

    pub fn on_after_created(&self, browser: &Browser) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);

        // Sanity-check the configured runtime style.
        assert_eq!(
            browser.host().map(|host| host.runtime_style()),
            Some(if self.use_alloy_style {
                RuntimeStyle::ALLOY
            } else {
                RuntimeStyle::CHROME
            })
        );

        if let Ok(mut base) = self.base.lock() {
            base.on_after_created(browser);
        }

        if self.offline {
            self.set_offline_state(browser, true);
        }

        self.notify_browser_created(browser);
    }

    fn notify_browser_created(&self, browser: &Browser) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            let browser = browser.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_browser_created(&browser);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_browser_created(browser);
        }
    }

    pub fn do_close(&self, browser: &Browser) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.notify_browser_closing(browser);

        // Allow the close. For windowed browsers this will result in the OS close
        // event being sent.
        false
    }

    fn notify_browser_closing(&self, browser: &Browser) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            let browser = browser.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_browser_closing(&browser);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_browser_closing(browser);
        }
    }

    pub fn on_before_close(&self, browser: &Browser) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);

        // Close all popups that have this browser as the opener.
        self.on_before_popup_aborted(browser, -1);

        if let Ok(mut base) = self.base.lock() {
            base.on_before_close(browser);
        }

        self.notify_browser_closed(browser);
    }

    fn notify_browser_closed(&self, browser: &Browser) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            let browser = browser.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_browser_closed(&browser);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_browser_closed(browser);
        }
    }

    pub fn on_loading_state_change(
        &self,
        is_loading: bool,
        can_go_back: bool,
        can_go_forward: bool,
    ) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Ok(mut base) = self.base.lock() {
            base.on_loading_state_change(is_loading);
        }
        self.notify_loading_state(is_loading, can_go_back, can_go_forward);
    }

    fn notify_loading_state(&self, is_loading: bool, can_go_back: bool, can_go_forward: bool) {
        if currently_on(ThreadId::UI) == 0 {
            // Execute this method on the main thread.
            let weak_self = self.weak_self.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.notify_loading_state(is_loading, can_go_back, can_go_forward);
                }),
            );
            return;
        }

        if let Some(delegate) = &self.delegate {
            delegate.on_set_loading_state(is_loading, can_go_back, can_go_forward);
        }
    }

    pub fn on_request_media_access_permission(
        &self,
        requested_permissions: u32,
        callback: MediaAccessCallback,
    ) {
        callback.cont(if self.media_handling_disabled {
            MediaAccessPermissionTypes::NONE.get_raw()
        } else {
            requested_permissions
        })
    }

    pub fn on_open_url_from_tab(
        &self,
        target_url: &str,
        target_disposition: WindowOpenDisposition,
    ) -> bool {
        let handled = matches!(
            target_disposition,
            WindowOpenDisposition::NEW_BACKGROUND_TAB | WindowOpenDisposition::NEW_FOREGROUND_TAB
        );
        if handled
            && let Some(context) = get_main_context()
            && let Ok(context) = context.lock()
            && let Some(manager) = context.root_window_manager()
        {
            // Handle middle-click and ctrl + left-click by opening the URL in a new
            // browser window.
            let _ = manager.create_root_window(RootWindowConfig {
                with_controls: self.with_controls,
                with_osr: self.is_osr,
                url: target_url.to_string(),
                ..Default::default()
            });
        }

        // Open the URL in the current browser window.
        handled.into()
    }

    pub fn resource_request_handler(&self) -> Option<ResourceRequestHandler> {
        debug_assert_ne!(currently_on(ThreadId::IO), 0);
        self.weak_self
            .upgrade()
            .map(ClientHandlerResourceRequestHandler::new)
    }

    pub fn auth_credentials(&self, is_proxy: bool, host: &str, callback: AuthCallback) -> bool {
        debug_assert_ne!(currently_on(ThreadId::IO), 0);
        // Used for testing authentication with a proxy server.
        // For example, CCProxy on Windows.
        if is_proxy {
            callback.cont(
                Some(&CefString::from("guest")),
                Some(&CefString::from("guest")),
            );
            return true;
        }

        // Used for testing authentication with https://jigsaw.w3.org/HTTP/.
        if host == "jigsaw.w3.org" {
            callback.cont(
                Some(&CefString::from("guest")),
                Some(&CefString::from("guest")),
            );
            return true;
        }

        false
    }

    pub fn on_certificate_error(
        &self,
        cert_error: Errorcode,
        request_url: &str,
        callback: Callback,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if cert_error == Errorcode::CERT_COMMON_NAME_INVALID
            && request_url.starts_with("https://www.magpcss.com/")
        {
            // Allow magpcss.com to load despite having a certificate common name of
            // magpcss.org.
            callback.cont();
            return true;
        }
        // Cancel the request.
        false
    }

    pub fn on_selected_client_certificate(
        &self,
        certificates: &mut [X509Certificate],
        callback: SelectClientCertificateCallback,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let cert_name = command_line_get_global().and_then(|c| {
            let name = Some(CefString::from(client_switches::SSL_CLIENT_CERTIFICATE));
            if c.has_switch(name.as_ref()) != 0 {
                Some(CefString::from(&c.switch_value(name.as_ref())).to_string())
            } else {
                None
            }
        });
        let Some(cert_name) = cert_name else {
            return false;
        };

        if cert_name.is_empty() {
            callback.select(None);
        } else if let Some(cert) = certificates.iter_mut().find(|cert| {
            let subject = cert
                .subject()
                .map(|s| CefString::from(&s.display_name()).to_string())
                .unwrap_or_default();
            subject == cert_name
        }) {
            callback.select(Some(cert));
        }

        true
    }

    pub fn on_render_process_terminated(
        &self,
        browser: &Browser,
        status: TerminationStatus,
        error_string: &str,
    ) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Ok(base) = self.base.lock() {
            base.on_render_process_terminated(browser);
        }

        let status = test_runner::get_termination_status_string(status);
        eprintln!("Render process terminated with status {status} ({error_string})");

        let Some(frame) = browser.main_frame() else {
            return;
        };
        let url = CefString::from(&frame.url()).to_string();

        // Don't reload if the termination occurred before any URL had successfully
        // loaded.
        if url.is_empty() {
            return;
        }

        // Convert URLs to lowercase for easier comparison.
        let url = url.to_ascii_lowercase();
        let startup_url = self.startup_url.to_ascii_lowercase();

        // Don't reload the URL that just resulted in termination.
        if url.starts_with(&startup_url) {
            return;
        }

        frame.load_url(Some(&CefString::from(self.startup_url.as_str())));
    }

    pub fn on_document_available_in_main_frame(&self, browser: &Browser) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Restore offline mode after main frame navigation. Otherwise, offline state
        // (e.g. `navigator.onLine`) might be wrong in the renderer process.
        if self.offline {
            self.set_offline_state(browser, true);
        }
    }

    pub fn on_protocol_execution(&self, request: &Request) -> bool {
        debug_assert_ne!(currently_on(ThreadId::IO), 0);
        let url = CefString::from(&request.url()).to_string();
        // Allow OS execution of Spotify URIs.
        url.starts_with("spotify:")
    }

    pub fn show_dev_tools(&self, browser: &Browser, inspect_element_at: Point) {
        if currently_on(ThreadId::UI) == 0 {
            let weak_self = self.weak_self.clone();
            let browser = browser.clone();
            OnceClosure::post_once(
                ThreadId::UI,
                Box::new(move || {
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    let Ok(this) = this.lock() else {
                        return;
                    };
                    this.show_dev_tools(&browser, inspect_element_at);
                }),
            );
            return;
        }

        if let Some(host) = browser.host() {
            host.show_dev_tools(
                Some(&Default::default()),
                None,
                Some(&Default::default()),
                Some(&inspect_element_at),
            );
        }
    }

    pub fn close_dev_tools(&self, browser: &Browser) {
        if let Some(host) = browser.host() {
            host.close_dev_tools();
        }
    }

    pub fn has_ssl_information(&self, browser: &Browser) -> bool {
        browser
            .host()
            .and_then(|h| h.visible_navigation_entry())
            .and_then(|nav| nav.sslstatus())
            .map_or(0, |status| status.is_secure_connection())
            != 0
    }

    pub fn show_ssl_information(&self, browser: &Browser) {
        let Some(nav) = browser.host().and_then(|h| h.visible_navigation_entry()) else {
            return;
        };
        let Some(ssl) = nav.sslstatus() else {
            return;
        };

        let Some(manager) = get_main_context().and_then(|context| {
            let context = context.lock().ok()?;
            context.root_window_manager()
        }) else {
            return;
        };

        let mut content = vec![
            r#"<html><head><title>SSL Information</title></head>"#,
            r#"<body bgcolor="white">"#,
            r#"<h3>SSL Connection</h3>"#,
            r#"<table border=1><tr><th>Field</th><th>Value</th></tr>"#,
        ];

        let mut parts = Default::default();
        let server = if parse_url(Some(&CefString::from(&nav.url())), Some(&mut parts)) != 0 {
            let (host, port) = (parts.host.to_string(), parts.port.to_string());
            Some(if port.is_empty() {
                host
            } else {
                format!("{}:{}", host, port)
            })
        } else {
            None
        };

        if let Some(server) = server.as_deref() {
            content.push(r#"<tr><td>Server</td><td>"#);
            content.push(server);
            content.push(r#"</td></tr>"#);
        }

        let ssl_version = get_ssl_version_string(ssl.sslversion());
        content.push(r#"<tr><td>SSL Version</td><td>"#);
        content.push(&ssl_version);
        content.push(r#"</td></tr>"#);
        let content_status = get_content_status_string(ssl.content_status());
        content.push(r#"<tr><td>Content Status</td><td>"#);
        content.push(&content_status);
        content.push(r#"</td></tr>"#);

        content.push(r#"</table>"#);

        let cert = ssl
            .x509_certificate()
            .map(|cert| get_certificate_information(cert.clone(), ssl.cert_status()));
        if let Some(cert) = cert.as_deref() {
            content.push(cert);
        }

        content.push(r#"</body></html>"#);

        let url = test_runner::get_data_uri(content.join("").as_bytes(), "text/html");
        let _ = manager.create_root_window(RootWindowConfig {
            with_controls: false,
            with_osr: self.is_osr,
            url,
            ..Default::default()
        });
    }

    pub fn create_popup_window(
        &self,
        browser: &Browser,
        popup_id: i32,
        is_devtools: bool,
        popup_features: &PopupFeatures,
        window_info: &mut WindowInfo,
        client: &mut Option<Client>,
        settings: &mut BrowserSettings,
    ) -> bool {
        let Some(manager) = get_main_context().and_then(|context| {
            let context = context.lock().ok()?;
            context.root_window_manager()
        }) else {
            return false;
        };

        manager
            .create_root_window_as_popup(
                self.use_views,
                self.use_alloy_style,
                self.with_controls && !is_devtools,
                self.is_osr,
                browser.identifier(),
                popup_id,
                is_devtools,
                popup_features,
                window_info,
                client,
                settings,
            )
            .is_some()
    }

    pub fn build_test_menu(&mut self, browser: &Browser, model: &MenuModel) {
        if model.count() > 0 {
            model.add_separator();
        }

        // Build the sub menu.
        let Some(submenu) = model.add_sub_menu(
            ClientMenuId::TestMenuSubMenu.into(),
            Some(&CefString::from("Context Menu Test")),
        ) else {
            return;
        };
        submenu.add_check_item(
            ClientMenuId::TestMenuCheckItem.into(),
            Some(&CefString::from("Check Item")),
        );
        submenu.add_radio_item(
            ClientMenuId::TestMenuRadioItem1.into(),
            Some(&CefString::from("Radio Item 1")),
            0,
        );
        submenu.add_radio_item(
            ClientMenuId::TestMenuRadioItem2.into(),
            Some(&CefString::from("Radio Item 2")),
            0,
        );
        submenu.add_radio_item(
            ClientMenuId::TestMenuRadioItem3.into(),
            Some(&CefString::from("Radio Item 3")),
            0,
        );

        {
            // Check the check item.
            if self.test_menu_state.check_item {
                submenu.set_checked(ClientMenuId::TestMenuCheckItem.into(), 1);
            }

            // Check the selected radio item.
            submenu.set_checked(
                i32::from(ClientMenuId::TestMenuRadioItem1) + self.test_menu_state.radio_item,
                1,
            );
        }
        // Build the theme sub menu.
        let Some(theme_menu) = model.add_sub_menu(
            ClientMenuId::TestMenuTheme.into(),
            Some(&CefString::from("Theme")),
        ) else {
            return;
        };
        theme_menu.add_radio_item(
            ClientMenuId::TestMenuThemeModeSystem.into(),
            Some(&CefString::from("System")),
            1,
        );
        theme_menu.add_radio_item(
            ClientMenuId::TestMenuThemeModeLight.into(),
            Some(&CefString::from("Light")),
            1,
        );
        theme_menu.add_radio_item(
            ClientMenuId::TestMenuThemeModeDark.into(),
            Some(&CefString::from("Dark")),
            1,
        );
        theme_menu.add_separator();
        theme_menu.add_radio_item(
            ClientMenuId::TestMenuThemeColorDefault.into(),
            Some(&CefString::from("Default")),
            2,
        );
        theme_menu.add_radio_item(
            ClientMenuId::TestMenuThemeColorRed.into(),
            Some(&CefString::from("Red")),
            2,
        );
        theme_menu.add_radio_item(
            ClientMenuId::TestMenuThemeColorGreen.into(),
            Some(&CefString::from("Green")),
            2,
        );
        theme_menu.add_radio_item(
            ClientMenuId::TestMenuThemeColorBlue.into(),
            Some(&CefString::from("Blue")),
            2,
        );

        if !self.use_alloy_style {
            theme_menu.add_separator();
            theme_menu.add_item(
                ClientMenuId::TestMenuThemeCustom.into(),
                Some(&CefString::from("Custom...")),
            );
        }

        let Some(request_context) = browser.host().and_then(|h| h.request_context()) else {
            return;
        };
        let checked_mode_item = match request_context.chrome_color_scheme_mode() {
            ColorVariant::SYSTEM => Some(ClientMenuId::TestMenuThemeModeSystem),
            ColorVariant::LIGHT => Some(ClientMenuId::TestMenuThemeModeLight),
            ColorVariant::DARK => Some(ClientMenuId::TestMenuThemeModeDark),
            _ => None,
        };
        let checked_color_item = match request_context.chrome_color_scheme_color() {
            COLOR_TRANSPARENT => Some(ClientMenuId::TestMenuThemeColorDefault),
            COLOR_RED => Some(ClientMenuId::TestMenuThemeColorRed),
            COLOR_GREEN => Some(ClientMenuId::TestMenuThemeColorGreen),
            COLOR_BLUE => Some(ClientMenuId::TestMenuThemeColorBlue),
            _ => None,
        };

        // Check the selected radio item, if any.
        if let Some(checked_mode_item) = checked_mode_item {
            theme_menu.set_checked(checked_mode_item.into(), 1);

            // Update the selected item.
            self.test_menu_state.chrome_theme_mode_item =
                i32::from(checked_mode_item) - i32::from(TEST_MENU_THEME_MODE_FIRST);
        }
        if let Some(checked_color_item) = checked_color_item {
            theme_menu.set_checked(checked_color_item.into(), 1);

            // Update the selected item.
            self.test_menu_state.chrome_theme_color_item =
                i32::from(checked_color_item) - i32::from(TEST_MENU_THEME_COLOR_FIRST);
        }
    }

    /// ExecuteTestMenu - handle test context menu commands.
    pub fn execute_test_menu(&mut self, browser: &Browser, command_id: i32) -> bool {
        const CHECK_ITEM: i32 = ClientMenuId::TestMenuCheckItem as i32;
        const RADIO_ITEM_FIRST: i32 = ClientMenuId::TestMenuRadioItem1 as i32;
        const RADIO_ITEM_LAST: i32 = ClientMenuId::TestMenuRadioItem3 as i32;
        const THEME_MODE_FIRST: i32 = TEST_MENU_THEME_MODE_FIRST as i32;
        const THEME_MODE_LAST: i32 = TEST_MENU_THEME_MODE_LAST as i32;
        const THEME_COLOR_FIRST: i32 = TEST_MENU_THEME_COLOR_FIRST as i32;
        const THEME_COLOR_LAST: i32 = TEST_MENU_THEME_COLOR_LAST as i32;
        const THEME_CUSTOM: i32 = ClientMenuId::TestMenuThemeCustom as i32;

        match command_id {
            CHECK_ITEM => {
                // Toggle the check item.
                self.test_menu_state.check_item = !self.test_menu_state.check_item;
                true
            }
            RADIO_ITEM_FIRST..=RADIO_ITEM_LAST => {
                // Store the selected radio item.
                self.test_menu_state.radio_item = command_id - RADIO_ITEM_FIRST;
                true
            }
            THEME_MODE_FIRST..=THEME_COLOR_LAST => {
                // Update the selected item.
                let (variant, color) = match command_id {
                    THEME_MODE_FIRST..=THEME_MODE_LAST => {
                        let selected_mode_item = command_id - THEME_MODE_FIRST;
                        self.test_menu_state.chrome_theme_mode_item = selected_mode_item;

                        const THEME_MODE_SYSTEM: i32 = ClientMenuId::TestMenuThemeModeSystem as i32;
                        const THEME_MODE_LIGHT: i32 = ClientMenuId::TestMenuThemeModeLight as i32;
                        const THEME_MODE_DARK: i32 = ClientMenuId::TestMenuThemeModeDark as i32;
                        let variant = match command_id {
                            THEME_MODE_SYSTEM => ColorVariant::SYSTEM,
                            THEME_MODE_LIGHT => ColorVariant::LIGHT,
                            THEME_MODE_DARK => ColorVariant::DARK,
                            // Don't change the color mode unless a selection has been made.
                            _ => ColorVariant::TONAL_SPOT,
                        };

                        (variant, COLOR_TRANSPARENT)
                    }
                    THEME_COLOR_FIRST..=THEME_COLOR_LAST => {
                        let selected_color_item = command_id - THEME_COLOR_FIRST;
                        self.test_menu_state.chrome_theme_color_item = selected_color_item;

                        const THEME_COLOR_RED: i32 = ClientMenuId::TestMenuThemeColorRed as i32;
                        const THEME_COLOR_GREEN: i32 = ClientMenuId::TestMenuThemeColorGreen as i32;
                        const THEME_COLOR_BLUE: i32 = ClientMenuId::TestMenuThemeColorBlue as i32;
                        let color = match command_id {
                            THEME_COLOR_RED => COLOR_RED,
                            THEME_COLOR_GREEN => COLOR_GREEN,
                            THEME_COLOR_BLUE => COLOR_BLUE,
                            // Don't change the user color unless a selection has been made.
                            _ => COLOR_TRANSPARENT,
                        };

                        (ColorVariant::TONAL_SPOT, color)
                    }
                    _ => unreachable!(),
                };

                if let Some(request_context) = browser.host().and_then(|h| h.request_context()) {
                    request_context.set_chrome_color_scheme(variant, color);
                }

                true
            }
            THEME_CUSTOM => {
                if let Some(main_frame) = browser.main_frame() {
                    main_frame.load_url(Some(&CefString::from("chrome://settings/manageProfile")));
                }

                true
            }
            _ => {
                // Allow default handling to proceed.
                false
            }
        }
    }

    pub fn set_offline_state(&self, browser: &Browser, offline: bool) {
        let (Some(host), Some(mut params)) = (browser.host(), dictionary_value_create()) else {
            return;
        };
        // See DevTools protocol docs for message format specification.
        params.set_bool(Some(&CefString::from("offline")), offline.into());
        params.set_double(Some(&CefString::from("latency")), 0.0);
        params.set_double(Some(&CefString::from("downloadThroughput")), 0.0);
        params.set_double(Some(&CefString::from("uploadThroughput")), 0.0);
        host.execute_dev_tools_method(
            0,
            Some(&CefString::from("Network.emulateNetworkConditions")),
            Some(&mut params),
        );
    }
}

wrap_client! {
    struct ClientHandlerClient {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl Client {
        fn command_handler(&self) -> Option<CommandHandler> {
            Some(ClientHandlerCommandHandler::new(self.inner.clone()))
        }

        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(ClientHandlerContextMenuHandler::new(self.inner.clone()))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(ClientHandlerDisplayHandler::new(self.inner.clone()))
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(ClientHandlerDownloadHandler::new(self.inner.clone()))
        }

        fn drag_handler(&self) -> Option<DragHandler> {
            Some(ClientHandlerDragHandler::new(self.inner.clone()))
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(ClientHandlerKeyboardHandler::new(self.inner.clone()))
        }

        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(ClientHandlerPermissionHandler::new(self.inner.clone()))
        }

        fn on_process_message_received(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> i32 {
            let Ok(inner) = self.inner.lock() else {
                return 0;
            };
            if inner.on_process_message_received(browser.cloned(), frame.cloned(), source_process, message.cloned()) {
                1
            } else {
                0
            }
        }

        #[cfg(target_os = "linux")]
        fn dialog_handler(&self) -> Option<DialogHandler> {
            let inner = self.inner.lock().ok()?;
            inner.file_dialog_handler.clone()
        }

        #[cfg(target_os = "linux")]
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            let inner = self.inner.lock().ok()?;
            inner.js_dialog_handler.clone()
        }

        #[cfg(target_os = "linux")]
        fn print_handler(&self) -> Option<PrintHandler> {
            let inner = self.inner.lock().ok()?;
            inner.print_handler.clone()
        }
    }
}

wrap_command_handler! {
    struct ClientHandlerCommandHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl CommandHandler {
        fn on_chrome_command(
            &self,
            _browser: Option<&mut Browser>,
            command_id: i32,
            disposition: WindowOpenDisposition,
        ) -> i32 {
            self.inner.lock().map_or(0, |handler| {
                handler.on_chrome_command(command_id, disposition).into()
            })
        }

        fn is_chrome_app_menu_item_visible(
            &self,
            _browser: Option<&mut Browser>,
            command_id: i32,
        ) -> i32 {
            self.inner.lock().map_or(0, |handler| {
                handler.is_chrome_app_menu_item_visible(command_id).into()
            })
        }

        fn is_chrome_page_action_icon_visible(&self, icon_type: ChromePageActionIconType) -> i32 {
            self.inner.lock().map_or(0, |handler| {
                handler.is_chrome_page_action_icon_visible(icon_type).into()
            })
        }

        fn is_chrome_toolbar_button_visible(&self, button_type: ChromeToolbarButtonType) -> i32 {
            self.inner.lock().map_or(0, |handler| {
                handler.is_chrome_toolbar_button_visible(button_type).into()
            })
        }
    }
}

wrap_context_menu_handler! {
    struct ClientHandlerContextMenuHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            if let (Some(browser), Some(params), Some(model), Ok(mut handler)) =
                (browser, params, model, self.inner.lock())
            {
                handler.on_before_context_menu(browser, params, model);
            }
        }

        fn on_context_menu_command(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            command_id: i32,
            _event_flags: EventFlags,
        ) -> i32 {
            if let (Some(browser), Some(params), Ok(mut handler)) =
                (browser, params, self.inner.lock())
            {
                handler
                    .on_context_menu_command(browser, command_id, params)
                    .into()
            } else {
                0
            }
        }
    }
}

wrap_display_handler! {
    struct ClientHandlerDisplayHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl DisplayHandler {
        fn on_address_change(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            if let (Some(frame), Some(url), Ok(handler)) = (frame, url, self.inner.lock()) {
                handler.on_address_change(frame, url.to_string());
            }
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let (Some(title), Ok(handler)) = (title, self.inner.lock()) {
                handler.on_title_change(title.to_string());
            }
        }

        fn on_favicon_urlchange(
            &self,
            browser: Option<&mut Browser>,
            icon_urls: Option<&mut CefStringList>,
        ) {
            if let (Some(browser), Some(url), Ok(handler)) = (
                browser.cloned(),
                icon_urls.and_then(|list| mem::take(list).into_iter().next()),
                self.inner.lock(),
            ) {
                handler.on_favicon_url_change(&browser, &url);
            }
        }

        fn on_fullscreen_mode_change(&self, browser: Option<&mut Browser>, fullscreen: i32) {
            if let Ok(handler) = self.inner.lock() {
                handler.on_fullscreen_mode_change(fullscreen != 0);
            }
        }

        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            level: LogSeverity,
            message: Option<&CefString>,
            source: Option<&CefString>,
            line: i32,
        ) -> i32 {
            if let (Some(message), Some(source), Ok(handler)) = (
                message.map(CefString::to_string),
                source.map(CefString::to_string),
                self.inner.lock(),
            ) {
                handler.on_console_message(level, &message, &source, line);
            }
            0
        }

        fn on_auto_resize(&self, browser: Option<&mut Browser>, new_size: Option<&Size>) -> i32 {
            if let (Some(new_size), Ok(handler)) = (new_size, self.inner.lock()) {
                handler.on_auto_resize(new_size.clone());
            }
            1
        }

        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: CursorHandle,
            _type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> i32 {
            self.inner
                .lock()
                .map_or(0, |handler| handler.on_cursor_change().into())
        }

        fn on_contents_bounds_change(
            &self,
            _browser: Option<&mut Browser>,
            new_bounds: Option<&Rect>,
        ) -> i32 {
            if let (Some(new_bounds), Ok(handler)) = (new_bounds, self.inner.lock()) {
                handler.on_contents_bounds_change(new_bounds.clone());
            }
            1
        }

        fn root_window_screen_rect(
            &self,
            _browser: Option<&mut Browser>,
            rect: Option<&mut Rect>,
        ) -> i32 {
            if let (Some(rect), Ok(handler)) = (rect, self.inner.lock())
                && let Some(screen_rect) = handler.root_window_screen_rect()
            {
                *rect = screen_rect;
                1
            } else {
                0
            }
        }
    }
}

wrap_download_handler! {
    struct ClientHandlerDownloadHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl DownloadHandler {
        fn can_download(
            &self,
            browser: Option<&mut Browser>,
            url: Option<&CefString>,
            request_method: Option<&CefString>,
        ) -> i32 {
            self.inner
                .lock()
                .map_or(0, |handler| handler.can_download().into())
        }

        fn on_before_download(
            &self,
            browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> i32 {
            if let (Some(suggested_name), Some(callback), Ok(handler)) =
                (suggested_name, callback, self.inner.lock())
            {
                handler
                    .on_before_download(&suggested_name.to_string(), callback.clone())
                    .into()
            } else {
                0
            }
        }

        fn on_download_updated(
            &self,
            browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            _callback: Option<&mut DownloadItemCallback>,
        ) {
            if let (Some(browser), Some(download_item), Ok(handler)) =
                (browser, download_item, self.inner.lock())
            {
                handler.on_download_updated(browser, download_item);
            }
        }
    }
}

wrap_drag_handler! {
    struct ClientHandlerDragHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl DragHandler {
        fn on_drag_enter(
            &self,
            browser: Option<&mut Browser>,
            drag_data: Option<&mut DragData>,
            mask: DragOperationsMask,
        ) -> i32 {
            if let (Some(browser), Some(drag_data), Ok(handler)) =
                (browser, drag_data, self.inner.lock())
            {
                handler.on_drag_enter(browser, drag_data, mask).into()
            } else {
                0
            }
        }

        fn on_draggable_regions_changed(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            regions: Option<&[DraggableRegion]>,
        ) {
            if let (Some(regions), Ok(handler)) = (regions, self.inner.lock()) {
                handler.on_draggable_regions_changed(regions)
            }
        }
    }
}

wrap_focus_handler! {
    struct ClientHandlerFocusHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl FocusHandler {
        fn on_take_focus(&self, _browser: Option<&mut Browser>, next: i32) {
            if let Ok(handler) = self.inner.lock() {
                handler.on_take_focus(next != 0)
            }
        }

        fn on_set_focus(&self, _browser: Option<&mut Browser>, source: FocusSource) -> i32 {
            self.inner
                .lock()
                .map_or(0, |handler| handler.on_set_focus(source).into())
        }
    }
}

#[cfg(target_os = "macos")]
type OsEvent = *mut u8;

#[cfg(target_os = "windows")]
type OsEvent<'a> = Option<&'a mut sys::MSG>;

#[cfg(target_os = "linux")]
type OsEvent<'a> = Option<&'a mut sys::XEvent>;

wrap_keyboard_handler! {
    struct ClientHandlerKeyboardHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: OsEvent,
            _is_keyboard_shortcut: Option<&mut i32>,
        ) -> i32 {
            if let (Some(browser), Some(event), Ok(handler)) = (browser, event, self.inner.lock()) {
                handler.on_pre_key_event(browser, event).into()
            } else {
                0
            }
        }
    }
}

wrap_life_span_handler! {
    struct ClientHandlerLifeSpanHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            popup_id: i32,
            _target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
            popup_features: Option<&PopupFeatures>,
            window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>,
            settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut i32>,
        ) -> i32 {
            if let (
                Some(browser),
                Some(popup_features),
                Some(window_info),
                Some(client),
                Some(settings),
                Ok(handler),
            ) = (
                browser,
                popup_features,
                window_info,
                client,
                settings,
                self.inner.lock(),
            ) {
                handler.on_before_popup(
                    browser,
                    popup_id,
                    target_disposition,
                    popup_features,
                    window_info,
                    client,
                    settings,
                );
            }
            0
        }

        fn on_before_popup_aborted(&self, browser: Option<&mut Browser>, popup_id: i32) {
            if let (Some(browser), Ok(handler)) = (browser, self.inner.lock()) {
                handler.on_before_popup_aborted(browser, popup_id);
            }
        }

        fn on_before_dev_tools_popup(
            &self,
            browser: Option<&mut Browser>,
            window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>,
            settings: Option<&mut BrowserSettings>,
            extra_info: Option<&mut Option<DictionaryValue>>,
            use_default_window: Option<&mut i32>,
        ) {
            if let (Some(browser), Some(window_info), Some(client), Some(settings), Some(use_default_window), Ok(handler)) = (browser, window_info, client, settings, use_default_window, self.inner.lock()) {
                *use_default_window = handler.on_before_dev_tools_popup(browser, window_info, client, settings).into();
            }
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let (Some(browser), Ok(handler)) = (browser, self.inner.lock()) {
                handler.on_after_created(browser);
            }
        }

        fn do_close(&self, browser: Option<&mut Browser>) -> i32 {
            if let (Some(browser), Ok(handler)) = (browser, self.inner.lock()) {
                handler.do_close(browser).into()
            } else {
                0
            }
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let (Some(browser), Ok(handler)) = (browser, self.inner.lock()) {
                handler.on_before_close(browser);
            }
        }
    }
}

wrap_load_handler! {
    struct ClientHandlerLoadHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: i32,
            can_go_back: i32,
            can_go_forward: i32,
        ) {
            if let Ok(handler) = self.inner.lock() {
                handler.on_loading_state_change(is_loading != 0, can_go_back != 0, can_go_forward != 0);
            }
        }
    }
}

wrap_permission_handler! {
    struct ClientHandlerPermissionHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl PermissionHandler {
        fn on_request_media_access_permission(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> i32 {
            if let (Some(callback), Ok(handler)) = (callback.cloned(), self.inner.lock()) {
                handler.on_request_media_access_permission(requested_permissions, callback);
                1
            } else {
                0
            }
        }
    }
}

wrap_request_handler! {
    struct ClientHandlerRequestHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl RequestHandler {
        fn on_open_urlfrom_tab(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            target_url: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
        ) -> i32 {
            if let (Some(target_url), Ok(handler)) = (target_url, self.inner.lock()) {
                handler
                    .on_open_url_from_tab(&target_url.to_string(), target_disposition)
                    .into()
            } else {
                0
            }
        }

        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: i32,
            _is_download: i32,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut i32>,
        ) -> Option<ResourceRequestHandler> {
            self.inner
                .lock()
                .ok()
                .and_then(|handler| handler.resource_request_handler())
        }

        fn auth_credentials(
            &self,
            _browser: Option<&mut Browser>,
            _origin_url: Option<&CefString>,
            is_proxy: i32,
            host: Option<&CefString>,
            _port: i32,
            _realm: Option<&CefString>,
            _scheme: Option<&CefString>,
            callback: Option<&mut AuthCallback>,
        ) -> i32 {
            if let (Some(host), Some(callback), Ok(handler)) =
                (host, callback.cloned(), self.inner.lock())
            {
                handler
                    .auth_credentials(is_proxy != 0, &host.to_string(), callback)
                    .into()
            } else {
                0
            }
        }

        fn on_certificate_error(
            &self,
            _browser: Option<&mut Browser>,
            cert_error: Errorcode,
            request_url: Option<&CefString>,
            _ssl_info: Option<&mut Sslinfo>,
            callback: Option<&mut Callback>,
        ) -> i32 {
            if let (Some(request_url), Some(callback), Ok(handler)) =
                (request_url, callback.cloned(), self.inner.lock())
            {
                handler
                    .on_certificate_error(cert_error, &request_url.to_string(), callback)
                    .into()
            } else {
                0
            }
        }

        fn on_select_client_certificate(
            &self,
            _browser: Option<&mut Browser>,
            _is_proxy: i32,
            _host: Option<&CefString>,
            _port: i32,
            certificates: Option<&[Option<X509Certificate>]>,
            callback: Option<&mut SelectClientCertificateCallback>,
        ) -> i32 {
            if let (Some(certificates), Some(callback), Ok(handler)) =
                (certificates, callback.cloned(), self.inner.lock())
            {
                let mut certificates: Vec<_> = certificates.iter().flatten().cloned().collect();
                handler
                    .on_selected_client_certificate(&mut certificates, callback)
                    .into()
            } else {
                0
            }
        }

        fn on_render_process_terminated(
            &self,
            browser: Option<&mut Browser>,
            status: TerminationStatus,
            _error_code: i32,
            error_string: Option<&CefString>,
        ) {
            if let (Some(browser), Some(error_string), Ok(handler)) =
                (browser, error_string, self.inner.lock())
            {
                handler.on_render_process_terminated(browser, status, &error_string.to_string())
            }
        }

        fn on_document_available_in_main_frame(&self, browser: Option<&mut Browser>) {
            if let (Some(browser), Ok(handler)) = (browser, self.inner.lock()) {
                handler.on_document_available_in_main_frame(browser)
            }
        }
    }
}

wrap_resource_request_handler! {
    struct ClientHandlerResourceRequestHandler {
        inner: Arc<Mutex<ClientHandler>>,
    }

    impl ResourceRequestHandler {
        fn on_protocol_execution(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            allow_os_execution: Option<&mut i32>,
        ) {
            if let (Some(request), Some(allow_os_execution), Ok(handler)) =
                (request, allow_os_execution, self.inner.lock())
            {
                *allow_os_execution = handler.on_protocol_execution(request).into()
            }
        }
    }
}
