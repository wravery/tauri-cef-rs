use cef::*;

pub mod base_client_handler;
pub mod binary_transfer_test;
pub mod binding_test;
pub mod config_test;
pub mod dialog_test;
pub mod hang_test;
pub mod image_cache;
pub mod main_context;
pub mod media_router_test;
pub mod osr_renderer;
pub mod preferences_test;
pub mod response_filter_test;
pub mod root_window;
pub mod server_test;
pub mod task_manager_test;
pub mod test_runner;
pub mod urlrequest_test;
pub mod window_test;
pub mod window_test_runner;

#[cfg(target_os = "linux")]
/// The Linux client uses GTK instead of the underlying platform type (X11).
pub type ClientWindowHandle = sys::cef_window_handle_t;
#[cfg(not(target_os = "linux"))]
pub type ClientWindowHandle = sys::cef_window_handle_t;
