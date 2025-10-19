use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use std::{
    path,
    sync::{Arc, Mutex},
};

const TEST_URL_PATH: &str = "/dialogs";

struct MessageName;

impl MessageName {
    const FILE_OPEN_PNG: &str = "DialogTest.FileOpenPng";
    const FILE_OPEN_IMAGE: &str = "DialogTest.FileOpenImage";
    const FILE_OPEN_MULTIPLE: &str = "DialogTest.FileOpenMultiple";
    const FILE_OPEN_FOLDER: &str = "DialogTest.FileOpenFolder";
    const FILE_SAVE: &str = "DialogTest.FileSave";
}

#[derive(Default)]
struct DialogState {
    mode: FileDialogMode,
    last_file: Option<String>,
    pending: bool,
}

type RouterCallback = Arc<Mutex<dyn BrowserSideCallback>>;

wrap_run_file_dialog_callback! {
    struct DialogCallback {
        router_callback: Arc<Mutex<Option<RouterCallback>>>,
        dialog_state: Arc<Mutex<Option<Arc<Mutex<DialogState>>>>>,
    }

    impl RunFileDialogCallback {
        fn on_file_dialog_dismissed(&self, file_paths: Option<&mut CefStringList>) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let (Some(router_callback), Some(dialog_state)) = (
                self.router_callback
                    .lock()
                    .ok()
                    .and_then(|mut callback| callback.take()),
                self.dialog_state
                    .lock()
                    .ok()
                    .and_then(|mut state| state.take()),
            ) else {
                return;
            };
            let (Ok(router_callback), Ok(mut dialog_state)) =
                (router_callback.lock(), dialog_state.lock())
            else {
                return;
            };
            debug_assert!(dialog_state.pending);

            let file_paths: Vec<_> = file_paths
                .cloned()
                .map(|paths| paths.into_iter().collect())
                .unwrap_or_default();
            if !file_paths.is_empty() {
                dialog_state.last_file = file_paths.first().cloned().map(|mut path| {
                    if dialog_state.mode == FileDialogMode::OPEN_FOLDER
                        && !path.ends_with(path::MAIN_SEPARATOR)
                    {
                        // Add a trailing slash so we know it's a directory. Otherwise, file
                        // dialogs will think the last path component is a file name.
                        path.push(path::MAIN_SEPARATOR);
                    }
                    path
                });
            }

            // Send a message back to the render process with the list of file paths.
            let response = file_paths.join("|");
            router_callback.success_str(&response);

            dialog_state.pending = false;
        }
    }
}

#[derive(Default)]
struct Handler {
    dialog_state: Arc<Mutex<DialogState>>,
}

impl BrowserSideHandler for Handler {
    /// Called due to cefQuery execution in binding.html.
    fn on_query_str(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        _query_id: i64,
        request: &str,
        _persistent: bool,
        callback: RouterCallback,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Only handle messages from the test URL.
        if !is_test_url(frame, TEST_URL_PATH) {
            return false;
        }

        let (mode, title, last_file, mut accept_filters) = {
            let Ok(mut dialog_state) = self.dialog_state.lock() else {
                return false;
            };
            debug_assert!(!dialog_state.pending);

            let mut accept_filters = CefStringList::new();
            let title = match request {
                MessageName::FILE_OPEN_PNG => {
                    dialog_state.mode = FileDialogMode::OPEN;
                    accept_filters.append(".png");
                    "My Open PNG Dialog"
                }
                MessageName::FILE_OPEN_IMAGE => {
                    dialog_state.mode = FileDialogMode::OPEN;
                    accept_filters.append("image/*");
                    "My Open Image Dialog"
                }
                MessageName::FILE_OPEN_MULTIPLE => {
                    dialog_state.mode = FileDialogMode::OPEN_MULTIPLE;
                    "My Open MultiType Dialog"
                }
                MessageName::FILE_OPEN_FOLDER => {
                    dialog_state.mode = FileDialogMode::OPEN_FOLDER;
                    "My Open Folder Dialog"
                }
                MessageName::FILE_SAVE => {
                    dialog_state.mode = FileDialogMode::SAVE;
                    "My Save Dialog"
                }
                message_name => unreachable!("Unrecognized message: {message_name}"),
            };

            if matches!(
                dialog_state.mode,
                FileDialogMode::OPEN_MULTIPLE | FileDialogMode::SAVE
            ) {
                // Build filters based on mime time.
                accept_filters.append("image/*");

                // Build filters based on file extension.
                accept_filters.append(".log");
                accept_filters.append(".patch");
            }

            dialog_state.pending = true;

            (
                dialog_state.mode,
                title,
                dialog_state.last_file.as_deref().map(CefString::from),
                accept_filters,
            )
        };

        let Some(host) = browser.and_then(|browser| browser.host()) else {
            return false;
        };
        let mut callback = DialogCallback::new(
            Arc::new(Mutex::new(Some(callback))),
            Arc::new(Mutex::new(Some(self.dialog_state.clone()))),
        );
        host.run_file_dialog(
            mode,
            Some(&CefString::from(title)),
            last_file.as_ref(),
            Some(&mut accept_filters),
            Some(&mut callback),
        );

        true
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler::default())
}
