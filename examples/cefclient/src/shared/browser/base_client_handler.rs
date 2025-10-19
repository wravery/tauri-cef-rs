use super::test_runner::*;
use crate::shared::browser::main_context::*;
use cef::{
    wrapper::{message_router::*, resource_manager::*},
    *,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tests_shared::common::client_switches::*;

pub trait BaseClientHandlerDelegate: Send + Sync {
    /// True if this handler should call
    /// RootWindowManager::OtherBrowser[Created|Closed].
    fn track_as_other_browser(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub enum HangAction {
    #[default]
    Default,
    Wait,
    Terminate,
}

fn browser_handler_map() -> &'static Mutex<BTreeMap<i32, Weak<Mutex<BaseClientHandler>>>> {
    static BROWSER_HANDLER_MAP: OnceLock<Mutex<BTreeMap<i32, Weak<Mutex<BaseClientHandler>>>>> =
        OnceLock::new();
    BROWSER_HANDLER_MAP.get_or_init(Default::default)
}

pub struct BaseClientHandler {
    weak_self: Weak<Mutex<Self>>,
    delegate: Box<dyn BaseClientHandlerDelegate>,
    // The current number of browsers using this handler.
    browser_count: u32,
    // Handles the browser side of query routing. The renderer side is handled
    // in client_renderer.cc.
    message_router: OnceLock<Arc<BrowserSideRouter>>,
    // Set of Handlers registered with the message router.
    message_handler_set: MessageHandlerSet,
    // Manages the registration and delivery of resources.
    resource_manager: Arc<Mutex<ResourceManager>>,
    // Used to manage string resources in combination with StringResourceProvider.
    // Only accessed on the IO thread.
    string_resource_map: StringResourceMap,
    hang_action: HangAction,
    // True for the initial navigation after browser creation.
    initial_navigation: bool,
}

impl BaseClientHandler {
    pub fn new(delegate: Box<dyn BaseClientHandlerDelegate>) -> Arc<Mutex<Self>> {
        Arc::new_cyclic(|weak_self| {
            Mutex::new(Self {
                weak_self: weak_self.clone(),
                delegate,
                browser_count: 0,
                message_router: Default::default(),
                message_handler_set: Default::default(),
                resource_manager: ResourceManager::new(),
                string_resource_map: Default::default(),
                hang_action: Default::default(),
                initial_navigation: true,
            })
        })
    }

    pub fn find_browser_id(browser_id: i32) -> Option<Arc<Mutex<Self>>> {
        let map = browser_handler_map().lock().ok()?;
        let weak_self = map.get(&browser_id)?;
        weak_self.upgrade()
    }

    pub fn add_browser_id(&self, browser_id: i32) -> bool {
        let Ok(mut map) = browser_handler_map().lock() else {
            return false;
        };
        map.insert(browser_id, self.weak_self.clone());
        true
    }

    pub fn as_client(&self) -> Option<Client> {
        Some(BaseClientHandlerClient::new(self.weak_self.upgrade()?))
    }

    /// Returns the number of browsers currently using this handler. Can only be
    /// called on the CEF UI thread.
    pub fn browser_count(&self) -> u32 {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.browser_count
    }

    /// Set a string resource for loading via StringResourceProvider.
    pub fn set_string_resource(&mut self, page: &str, data: &str) {
        if currently_on(ThreadId::IO) == 0 {
            let Some(inner) = self.weak_self.upgrade() else {
                return;
            };
            let mut task =
                BaseClientHandlerSetStringResource::new(inner, page.to_string(), data.to_string());
            post_task(ThreadId::IO, Some(&mut task));
            return;
        }

        self.string_resource_map
            .insert(page.to_string(), data.to_string());
    }

    pub fn set_hang_action(&mut self, action: HangAction) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.hang_action = action;
    }

    pub fn hang_action(&self) -> HangAction {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.hang_action
    }

    pub fn should_request_focus(&self) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        !self.initial_navigation
            || command_line_get_global().map_or(0, |command_line| {
                // Don't give focus to the browser on creation.
                command_line.has_switch(Some(&CefString::from(NO_ACTIVATE)))
            }) == 0
    }

    fn on_after_created(&mut self, browser: Option<Browser>) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.browser_count += 1;

        self.message_router.get_or_init(|| {
            // Create the browser-side router for query handling.
            let message_router = BrowserSideRouter::new(Default::default());

            // Register handlers with the router.
            for handler in create_message_handlers() {
                if let Some(handler_id) = message_router.add_handler(handler.clone(), false) {
                    self.message_handler_set.insert(handler_id, handler);
                }
            }

            message_router
        });

        if self.delegate.track_as_other_browser()
            && let (Some(root_window_manager), Some(browser), Some(host)) = (
                get_main_context().and_then(|context| {
                    context
                        .lock()
                        .ok()
                        .and_then(|context| context.root_window_manager())
                }),
                browser.as_ref(),
                browser.as_ref().and_then(|browser| browser.host()),
            )
        {
            let opener_id = host.opener_identifier();
            root_window_manager.other_browser_created(
                browser.identifier(),
                if opener_id == 0 {
                    None
                } else {
                    Some(opener_id)
                },
            );
        }
    }

    fn on_before_close(&mut self, browser: Option<Browser>) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.browser_count -= 1;
        if self.browser_count == 0 {
            // Remove and delete message router handlers.
            if let Some(message_router) = self.message_router.take() {
                for &handler_id in self.message_handler_set.keys() {
                    message_router.remove_handler(handler_id);
                }
            }
        }

        self.message_router.get_or_init(|| {
            // Create the browser-side router for query handling.
            let message_router = BrowserSideRouter::new(Default::default());

            // Register handlers with the router.
            for handler in create_message_handlers() {
                if let Some(handler_id) = message_router.add_handler(handler.clone(), false) {
                    self.message_handler_set.insert(handler_id, handler);
                }
            }

            message_router
        });

        if self.delegate.track_as_other_browser()
            && let (Some(root_window_manager), Some(browser), Some(host)) = (
                get_main_context().and_then(|context| {
                    context
                        .lock()
                        .ok()
                        .and_then(|context| context.root_window_manager())
                }),
                browser.as_ref(),
                browser.as_ref().and_then(|browser| browser.host()),
            )
        {
            let opener_id = host.opener_identifier();
            root_window_manager.other_browser_closed(
                browser.identifier(),
                if opener_id == 0 {
                    None
                } else {
                    Some(opener_id)
                },
            );
        }
    }

    fn on_loading_state_change(&mut self, is_loading: bool) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if !is_loading && self.initial_navigation {
            self.initial_navigation = false;
        }
    }
}

wrap_client! {
    struct BaseClientHandlerClient {
        inner: Arc<Mutex<BaseClientHandler>>,
    }

    impl Client {
        fn focus_handler(&self) -> Option<FocusHandler> {
            Some(BaseClientHandlerFocusHandler::new(self.inner.clone()))
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(BaseClientHandlerLifeSpanHandler::new(self.inner.clone()))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(BaseClientHandlerLoadHandler::new(self.inner.clone()))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(BaseClientHandlerRequestHandler::new(self.inner.clone()))
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
            let Some(message_router) = inner.message_router.get() else {
                return 0;
            };
            if message_router.on_process_message_received(
                browser.cloned(),
                frame.cloned(),
                source_process,
                message.cloned(),
            ) {
                1
            } else {
                0
            }
        }
    }
}

wrap_focus_handler! {
    struct BaseClientHandlerFocusHandler {
        inner: Arc<Mutex<BaseClientHandler>>,
    }

    impl FocusHandler {
        fn on_set_focus(
            &self,
            _browser: Option<&mut Browser>,
            _source: FocusSource,
        ) -> i32 {
            let Ok(inner) = self.inner.lock() else {
                return 0;
            };
            if inner.should_request_focus() {
                0
            } else {
                1
            }
        }
    }
}

wrap_life_span_handler! {
    struct BaseClientHandlerLifeSpanHandler {
        inner: Arc<Mutex<BaseClientHandler>>,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.on_after_created(browser.cloned());
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.on_before_close(browser.cloned());
        }
    }
}

wrap_load_handler! {
    struct BaseClientHandlerLoadHandler {
        inner: Arc<Mutex<BaseClientHandler>>,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: i32,
            _can_go_back: i32,
            _can_go_forward: i32,
        ) {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.on_loading_state_change(is_loading != 0);
        }
    }
}

wrap_request_handler! {
    struct BaseClientHandlerRequestHandler {
        inner: Arc<Mutex<BaseClientHandler>>,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            if let Some(message_router) = self
                .inner
                .lock()
                .ok()
                .and_then(|inner| inner.message_router.get().cloned())
            {
                message_router.on_before_browse(browser.cloned(), frame.cloned());
            }
            0
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
            Some(BaseClientHandlerResourceRequestHandler::new(self.inner.clone()))
        }

        fn on_render_process_unresponsive(
            &self,
            _browser: Option<&mut Browser>,
            callback: Option<&mut UnresponsiveProcessCallback>,
        ) -> i32 {
            if let Ok(inner) = self.inner.lock() {
                match (&inner.hang_action, callback) {
                    (HangAction::Default, _) => return 0,
                    (HangAction::Wait, Some(callback)) => {
                        callback.wait();
                    }
                    (HangAction::Terminate, Some(callback)) => {
                        callback.terminate();
                    }
                    _ => {}
                }
            }
            1
        }

        fn on_render_process_terminated(
            &self,
            browser: Option<&mut Browser>,
            _status: TerminationStatus,
            _error_code: ::std::os::raw::c_int,
            _error_string: Option<&CefString>,
        ) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            if let Some(message_router) = self
                .inner
                .lock()
                .ok()
                .and_then(|inner| inner.message_router.get().cloned())
            {
                message_router.on_render_process_terminated(browser.cloned());
            }
        }
    }
}

wrap_resource_request_handler! {
    struct BaseClientHandlerResourceRequestHandler {
        inner: Arc<Mutex<BaseClientHandler>>,
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            callback: Option<&mut Callback>,
        ) -> ReturnValue {
            debug_assert_ne!(currently_on(ThreadId::IO), 0);
            let (Some(browser), Some(frame), Some(request), Some(callback)) = (
                browser.cloned(),
                frame.cloned(),
                request.cloned(),
                callback.cloned(),
            ) else {
                return ReturnValue::CONTINUE;
            };
            let Ok(inner) = self.inner.lock() else {
                return ReturnValue::CONTINUE;
            };
            let Ok(mut resource_manager) = inner.resource_manager.lock() else {
                return ReturnValue::CONTINUE;
            };
            resource_manager.on_before_resource_load(browser, frame, request, callback)
        }

        fn resource_handler(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            debug_assert_ne!(currently_on(ThreadId::IO), 0);
            let browser = browser.cloned()?;
            let frame = frame.cloned()?;
            let request = request.cloned()?;
            let inner = self.inner.lock().ok()?;
            let mut resource_manager = inner.resource_manager.lock().ok()?;
            resource_manager.resource_handler(browser, frame, request)
        }

        fn resource_response_filter(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            response: Option<&mut Response>,
        ) -> Option<ResponseFilter> {
            debug_assert_ne!(currently_on(ThreadId::IO), 0);
            resource_response_filter(browser, frame, request, response)
        }
    }
}

wrap_task! {
    struct BaseClientHandlerSetStringResource {
        inner: Arc<Mutex<BaseClientHandler>>,
        page: String,
        data: String,
    }

    impl Task {
        fn execute(&self) {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.set_string_resource(&self.page, &self.data);
        }
    }
}
