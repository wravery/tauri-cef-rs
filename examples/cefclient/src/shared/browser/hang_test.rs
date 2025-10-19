use super::{base_client_handler::*, test_runner::*};
use cef::{wrapper::message_router::*, *};
use std::sync::{Arc, Mutex};

const TEST_URL_PATH: &str = "/hang";
const TEST_MESSAGE_NAME: &str = "HangTest:";

struct Handler;

impl BrowserSideHandler for Handler {
    /// Called due to cefQuery execution in hang.html.
    fn on_query_str(
        &self,
        browser: Option<Browser>,
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

        let Some(client_handler) =
            browser.and_then(|browser| BaseClientHandler::find_browser_id(browser.identifier()))
        else {
            return false;
        };
        let (Ok(mut client_handler), Ok(callback)) = (client_handler.lock(), callback.lock())
        else {
            return false;
        };

        match request.trim_start_matches(TEST_MESSAGE_NAME) {
            "getcommand" => {
                let current = match client_handler.hang_action() {
                    HangAction::Default => "default",
                    HangAction::Wait => "wait",
                    HangAction::Terminate => "terminate",
                };
                callback.success_str(current);
            }
            "setdefault" => {
                client_handler.set_hang_action(HangAction::Default);
            }
            "setwait" => {
                client_handler.set_hang_action(HangAction::Wait);
            }
            "setterminate" => {
                client_handler.set_hang_action(HangAction::Terminate);
            }
            message_name => {
                unreachable!("Unrecognized {TEST_MESSAGE_NAME} message: {message_name}")
            }
        }

        true
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler)
}
