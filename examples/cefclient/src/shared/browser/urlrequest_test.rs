use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use std::{
    mem, slice,
    sync::{Arc, Mutex},
};

const TEST_URL_PATH: &str = "/urlrequest";
const TEST_MESSAGE_NAME: &str = "URLRequestTest:";

type RequestCallback = Option<Box<dyn Send + Sync + FnOnce(Errorcode, Vec<u8>)>>;

struct RequestClientInner {
    callback: RequestCallback,
    download_data: Vec<u8>,
}

impl RequestClientInner {
    fn new(callback: RequestCallback) -> Arc<Mutex<Self>> {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        Arc::new(Mutex::new(Self {
            callback,
            download_data: Default::default(),
        }))
    }

    fn detach(&mut self) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.callback = None;
    }

    fn on_request_complete(&mut self, request: &Urlrequest) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Some(callback) = self.callback.take() {
            callback(request.request_error(), mem::take(&mut self.download_data));
        }
    }

    fn on_download_data(&mut self, data: &[u8]) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.download_data.extend_from_slice(data);
    }
}

wrap_urlrequest_client! {
    struct TestUrlRequestClient {
        inner: Arc<Mutex<RequestClientInner>>,
    }

    impl UrlrequestClient {
        fn on_request_complete(&self, request: Option<&mut Urlrequest>) {
            if let (Ok(mut inner), Some(request)) = (self.inner.lock(), request) {
                inner.on_request_complete(request);
            }
        }

        fn on_download_data(
            &self,
            _request: Option<&mut Urlrequest>,
            data: *const u8,
            data_length: usize,
        ) {
            if !data.is_null() && data_length > 0
                && let Ok(mut inner) = self.inner.lock() {
                    let data = unsafe { slice::from_raw_parts(data, data_length) };
                    inner.on_download_data(data);
                }
        }
    }
}

#[derive(Default)]
struct HandlerInner {
    callback: Option<Arc<Mutex<dyn BrowserSideCallback>>>,
    request_client: Option<Arc<Mutex<RequestClientInner>>>,
    request: Option<Urlrequest>,
}

impl HandlerInner {
    // Cancel the currently pending URL request, if any.
    fn cancel_pending_request(&mut self) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let request_client = self.request_client.take();
        if let Some(mut request_client) = request_client
            .as_ref()
            .and_then(|request_client| request_client.lock().ok())
        {
            // Don't execute the callback when we explicitly cancel the request.
            request_client.detach();
        }

        if let Some(request) = self.request.take() {
            request.cancel();
        }

        let callback = self.callback.take();
        if let Some(callback) = callback.as_ref().and_then(|callback| callback.lock().ok()) {
            // Must always execute |callback_| before deleting it.
            callback.failure(
                Errorcode::ABORTED.get_raw(),
                &get_error_string(Errorcode::ABORTED),
            );
        }
    }

    fn on_request_complete(&mut self, error_code: Errorcode, download_data: Vec<u8>) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let callback = self.callback.take();
        let Some(callback) = callback.as_ref().and_then(|callback| callback.lock().ok()) else {
            return;
        };
        if error_code == Errorcode::NONE {
            match String::from_utf8(download_data) {
                Ok(value) => callback.success_str(&value),
                Err(err) => callback.success_binary(err.as_bytes()),
            }
        } else {
            callback.failure(error_code.get_raw(), &get_error_string(error_code));
        }
    }
}

impl Drop for HandlerInner {
    fn drop(&mut self) {
        self.cancel_pending_request();
    }
}

struct Handler {
    inner: Arc<Mutex<HandlerInner>>,
}

impl Handler {
    fn new() -> Self {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        Handler {
            inner: Default::default(),
        }
    }
}

impl BrowserSideHandler for Handler {
    // Called due to cefQuery execution in urlrequest.html.
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
        if !is_test_url(frame.clone(), TEST_URL_PATH) || !request.starts_with(TEST_MESSAGE_NAME) {
            return false;
        }

        let load_url = request.trim_start_matches(TEST_MESSAGE_NAME);

        if let (Some(frame), Ok(mut inner)) = (frame, self.inner.lock()) {
            inner.cancel_pending_request();
            debug_assert!(inner.callback.is_none());
            debug_assert!(inner.request_client.is_none());
            debug_assert!(inner.request.is_none());

            inner.callback = Some(callback);

            // Create a CefRequest for the specified URL.
            if let Some(mut request) = request_create() {
                request.set_url(Some(&CefString::from(load_url)));
                request.set_method(Some(&CefString::from("GET")));

                // Callback to be executed on request completion.
                // It's safe to use base::Unretained() here because there is only one
                // RequestClient pending at any given time and we explicitly detach the
                // callback in the Handler destructor.
                let request_callback = {
                    let inner = self.inner.clone();
                    Box::new(move |error_code: Errorcode, download_data: Vec<u8>| {
                        if let Ok(mut inner) = inner.lock() {
                            inner.on_request_complete(error_code, download_data);
                        }
                    })
                };

                // Create and start a new CefURLRequest associated with the frame, so
                // that it shares authentication with ClientHandler::GetAuthCredentials.
                let request_client = RequestClientInner::new(Some(request_callback));
                inner.request_client = Some(request_client.clone());
                let mut client = TestUrlRequestClient::new(request_client);
                inner.request = frame.create_urlrequest(Some(&mut request), Some(&mut client));
            }
        }

        true
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler::new())
}
