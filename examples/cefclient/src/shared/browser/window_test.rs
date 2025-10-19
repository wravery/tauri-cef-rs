use super::{test_runner::*, window_test_runner::*};
use cef::{wrapper::message_router::*, *};
use std::sync::{Arc, Mutex};
use tests_shared::browser::main_message_loop;

const TEST_URL_PATH: &str = "/window";

struct MessageName;

impl MessageName {
    const POSITION: &str = "WindowTest.Position:";
    const MINIMIZE: &str = "WindowTest.Minimize";
    const MAXIMIZE: &str = "WindowTest.Maximize";
    const RESTORE: &str = "WindowTest.Restore";
    const FULLSCREEN: &str = "WindowTest.Fullscreen";
    const TITLEBAR_HEIGHT: &str = "WindowTest.TitlebarHeight:";
}

fn try_parse_position(message: &str) -> Option<Rect> {
    if !message.starts_with(MessageName::POSITION) {
        return None;
    }
    let message = message.trim_start_matches(MessageName::POSITION);
    let parts = message
        .split(',')
        .map(|part| part.parse())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    match parts.as_slice() {
        [x, y, width, height] => Some(Rect {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
        }),
        _ => None,
    }
}

fn try_parse_height(message: &str) -> Option<f32> {
    if !message.starts_with(MessageName::TITLEBAR_HEIGHT) {
        return None;
    }
    let message = message.trim_start_matches(MessageName::TITLEBAR_HEIGHT);
    message.parse().ok()
}

struct Handler;

impl Handler {
    fn run_on_main_thread(
        browser: &Browser,
        request: &str,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) {
        {
            let main_message_loop = main_message_loop::get_main_message_loop();
            let Ok(mut main_message_loop) = main_message_loop.lock() else {
                return;
            };
            let Some(main_message_loop) = main_message_loop.as_mut() else {
                return;
            };

            if !main_message_loop.run_tasks_on_current_thread() {
                let browser = browser.clone();
                let request = request.to_string();
                main_message_loop.post_once(Box::new(move || {
                    Self::run_on_main_thread(&browser, &request, callback);
                }));
                return;
            }
        }

        let mut runner = create_window_test_runner(browser);
        match request {
            MessageName::MINIMIZE => runner.minimize(),
            MessageName::MAXIMIZE => runner.maximize(),
            MessageName::RESTORE => runner.restore(),
            MessageName::FULLSCREEN => runner.fullscreen(),
            message => {
                if let Some(position) = try_parse_position(message) {
                    runner.set_pos(position.x, position.y, position.width, position.height);
                } else if let Some(height) = try_parse_height(message) {
                    runner.set_titlebar_height(height);
                } else {
                    if let Ok(callback) = callback.lock() {
                        let error_message = format!("Invalid request: {message}");
                        callback.failure(MESSAGE_FORMAT_ERROR, &error_message);
                    }
                    return;
                }
            }
        }

        if let Ok(callback) = callback.lock() {
            callback.success_str("");
        }
    }
}

impl BrowserSideHandler for Handler {
    /// Called due to cefQuery execution in window.html.
    fn on_query_str(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        _query_id: i64,
        request: &str,
        _persistent: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Only handle messages from the test URL.
        if !is_test_url(frame, TEST_URL_PATH) {
            return false;
        }

        if let Some(browser) = browser {
            Self::run_on_main_thread(&browser, request, callback);
        }

        true
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler)
}
