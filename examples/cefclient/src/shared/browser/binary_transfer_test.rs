use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use std::sync::{Arc, Mutex};

const TEST_URL_PATH: &str = "/binary_transfer";

struct Handler;

impl BrowserSideHandler for Handler {
    /// Called due to cefQuery execution in binary_transfer.html.
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
        if !is_test_url(frame, TEST_URL_PATH) {
            return false;
        }

        if let Ok(callback) = callback.lock() {
            callback.success_str(request);
        }
        true
    }

    /// Called due to cefQuery execution in binary_transfer.html.
    fn on_query_binary(
        &self,
        _browser: Option<Browser>,
        frame: Option<Frame>,
        _query_id: i64,
        request: &dyn BinaryBuffer,
        _persistent: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        // Only handle messages from the test URL.
        if !is_test_url(frame, TEST_URL_PATH) {
            return false;
        }

        if let Ok(callback) = callback.lock() {
            callback.success_binary(request.data());
        }
        true
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler)
}
