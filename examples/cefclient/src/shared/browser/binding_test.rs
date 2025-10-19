use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use std::sync::{Arc, Mutex};

const TEST_URL_PATH: &str = "/binding";
const TEST_MESSAGE_NAME: &str = "BindingTest:";

struct Handler;

impl BrowserSideHandler for Handler {
    /// Called due to cefQuery execution in binding.html.
    fn on_query_str(
        &self,
        _browser: Option<Browser>,
        frame: Option<Frame>,
        _query_id: i64,
        request: &str,
        _persistent: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        // Only handle messages from the test URL.
        if !is_test_url(frame, TEST_URL_PATH) || !request.starts_with(TEST_MESSAGE_NAME) {
            return false;
        }

        if let Ok(callback) = callback.lock() {
            let message_name: String = request
                .trim_start_matches(TEST_MESSAGE_NAME)
                .chars()
                .rev()
                .collect();
            callback.success_str(&message_name);
        }
        true
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler)
}
