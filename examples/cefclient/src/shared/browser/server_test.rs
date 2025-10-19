use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use std::sync::{Arc, Mutex, Weak};
use tests_shared::browser::resource_util::*;

// Application-specific error codes.
const ACTION_STATE_ERROR: i32 = 1;

struct Name;

impl Name {
    // JSON dictionary keys.
    const ACTION_KEY: &str = "action";
    const RESULT_KEY: &str = "result";
    const PORT_KEY: &str = "port";
    const STATUS_KEY: &str = "status";
    const MESSAGE_KEY: &str = "message";
}

// Server default values.
const SERVER_ADDRESS: &str = "127.0.0.1";
const SERVER_PORT_DEFAULT: u16 = 8099;
const SERVER_BACKLOG: i32 = 10;
const DEFAULT_PATH: &str = "websocket.html";

type CompleteCallback = Option<Box<dyn Send + Sync + Fn(bool)>>;

struct TestServerHandlerInner {
    weak_self: Weak<Mutex<Self>>,
    server: Option<Server>,
    // The below members are only accessed on the UI thread.
    port: u16,
    complete_callback: CompleteCallback,
}

impl TestServerHandlerInner {
    fn new() -> Arc<Mutex<Self>> {
        Arc::new_cyclic(|weak_self| {
            Mutex::new(Self {
                weak_self: weak_self.clone(),
                server: None,
                port: 0,
                complete_callback: None,
            })
        })
    }

    // |complete_callback| will be executed on the UI thread after completion.
    fn start_server(&mut self, port: u16, complete_callback: CompleteCallback) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        debug_assert!(self.server.is_none());
        debug_assert!((1025..=65535).contains(&port));
        if let Some(this) = self.weak_self.upgrade() {
            let mut handler = TestServerHandler::new(this);
            self.port = port;
            self.complete_callback = complete_callback;
            server_create(
                Some(&CefString::from(SERVER_ADDRESS)),
                port,
                SERVER_BACKLOG,
                Some(&mut handler),
            );
        }
    }

    // |complete_callback| will be executed on the UI thread after completion.
    fn stop_server(&mut self, complete_callback: CompleteCallback) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        debug_assert!(self.server.is_some());
        if let Some(server) = self.server.clone() {
            self.complete_callback = complete_callback;
            server.shutdown();
        }
    }

    fn on_server_created(&mut self, server: &Server) {
        debug_assert!(self.server.is_none());
        self.server = Some(server.clone());
        self.run_complete_callback(server.is_running() != 0);
    }

    fn on_server_destroyed(&mut self, _server: &Server) {
        debug_assert!(self.server.is_some());
        self.server = None;
        self.run_complete_callback(true);
    }

    fn on_http_request(server: &Server, connection_id: i32, request: &Request) {
        // Parse the request URL and retrieve the path without leading slash.
        let url = CefString::from(&request.url());
        let mut parts = Default::default();
        if parse_url(Some(&url), Some(&mut parts)) == 0 {
            return;
        }
        let path = parts.path.to_string();

        let path = path.trim_start_matches('/');
        let path = if path.is_empty() { DEFAULT_PATH } else { path };

        let (path, mime_type) = path
            .rfind('.')
            .map_or((format!("{path}.html"), None), |index| {
                let extension = CefString::from(path[index..].trim_start_matches('.'));
                let mime_type = CefString::from(&get_mime_type(Some(&extension)));
                (path.to_string(), Some(mime_type))
            });
        let mime_type = mime_type.unwrap_or_else(|| CefString::from("text/html"));

        let mut extra_headers = Default::default();
        let stream = if path == "request.html" {
            // Return the request contents.
            get_dump_response(request, &mut extra_headers)
        } else {
            // Load any resource supported by cefclient.
            get_binary_resource_reader(&path)
        };

        if let Some(stream) = stream {
            Self::send_http_response_stream(
                server,
                connection_id,
                mime_type,
                stream,
                extra_headers,
            );
        } else {
            server.send_http404_response(connection_id);
        }
    }

    fn on_web_socket_request(callback: &Callback) {
        // Always accept WebSocket connections.
        callback.cont();
    }

    fn on_web_socket_message(server: &Server, connection_id: i32, data: &[u8]) {
        // Echo the reverse of the message.
        let mut data = data.to_vec();
        data.reverse();
        server.send_web_socket_message(connection_id, Some(&data));
    }

    fn run_complete_callback(&mut self, success: bool) {
        if currently_on(ThreadId::UI) == 0 {
            if let Some(this) = self.weak_self.upgrade() {
                let mut task = RunCompleteCallbackTask::new(this, success);
                post_task(ThreadId::UI, Some(&mut task));
            }
            return;
        }

        if let Some(callback) = self.complete_callback.take() {
            callback(success);
        }
    }

    fn send_http_response_stream(
        server: &Server,
        connection_id: i32,
        mime_type: CefString,
        stream: StreamReader,
        mut extra_headers: CefStringMultimap,
    ) {
        // Determine the stream size.
        stream.seek(0, SEEK_END);
        let content_length = stream.tell();
        stream.seek(0, SEEK_SET);

        // Send response headers.
        server.send_http_response(
            connection_id,
            200,
            Some(&mime_type),
            content_length,
            Some(&mut extra_headers),
        );

        // Send stream contents.
        let mut buffer = [0; 8192];
        loop {
            let size = stream.read(buffer.as_mut_ptr(), 1, buffer.len());
            if size > 0 {
                server.send_raw_data(connection_id, Some(&buffer[..size]));
            }
            if stream.eof() != 0 || size == 0 {
                break;
            }
        }

        // Close the connection.
        server.close_connection(connection_id);
    }
}

wrap_task! {
    struct RunCompleteCallbackTask {
        inner: Arc<Mutex<TestServerHandlerInner>>,
        success: bool,
    }

    impl Task {
        fn execute(&self) {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.run_complete_callback(self.success);
        }
    }
}

// Handles the HTTP/WebSocket server.
wrap_server_handler! {
    struct TestServerHandler {
        inner: Arc<Mutex<TestServerHandlerInner>>,
    }

    impl ServerHandler {
        fn on_server_created(&self, server: Option<&mut Server>) {
            debug_assert_eq!(currently_on(ThreadId::UI), 0);
            if let (Some(server), Ok(mut inner)) = (server, self.inner.lock()) {
                inner.on_server_created(server);
            }
        }

        fn on_server_destroyed(&self, server: Option<&mut Server>) {
            debug_assert_eq!(currently_on(ThreadId::UI), 0);
            if let (Some(server), Ok(mut inner)) = (server, self.inner.lock()) {
                inner.on_server_destroyed(server);
            }
        }

        fn on_http_request(
            &self,
            server: Option<&mut Server>,
            connection_id: i32,
            _client_address: Option<&CefString>,
            request: Option<&mut Request>,
        ) {
            if let (Some(server), Some(request)) = (server, request) {
                TestServerHandlerInner::on_http_request(server, connection_id, request);
            }
        }

        fn on_web_socket_request(
            &self,
            _server: Option<&mut Server>,
            _connection_id: i32,
            _client_address: Option<&CefString>,
            _request: Option<&mut Request>,
            callback: Option<&mut Callback>,
        ) {
            if let Some(callback) = callback {
                TestServerHandlerInner::on_web_socket_request(callback);
            }
        }

        fn on_web_socket_message(
            &self,
            server: Option<&mut Server>,
            connection_id: i32,
            data: Option<&[u8]>,
        ) {
            if let (Some(server), Some(data)) = (server, data) {
                TestServerHandlerInner::on_web_socket_message(server, connection_id, data)
            }
        }
    }
}

struct Handler {
    weak_self: Weak<Self>,
    handler: Mutex<Option<Arc<Mutex<TestServerHandlerInner>>>>,
}

impl Handler {
    fn new() -> Arc<Self> {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        Arc::new_cyclic(|weak_self| Self {
            weak_self: weak_self.clone(),
            handler: Mutex::new(None),
        })
    }

    fn is_test_url(frame: Option<Frame>) -> bool {
        frame
            .map(|frame| CefString::from(&frame.url()).to_string())
            .is_some_and(|url| url.starts_with(get_test_url("server").as_str()))
    }

    fn handle_query_action(&self, callback: Arc<Mutex<dyn BrowserSideCallback>>) {
        let (Ok(handler), Some(result)) = (self.handler.lock(), dictionary_value_create()) else {
            return;
        };

        if let Some(handler) = handler.as_ref().and_then(|handler| handler.lock().ok()) {
            result.set_int(Some(&CefString::from(Name::PORT_KEY)), handler.port.into());
            result.set_string(
                Some(&CefString::from(Name::STATUS_KEY)),
                Some(&CefString::from("running")),
            );
        } else {
            result.set_int(
                Some(&CefString::from(Name::PORT_KEY)),
                SERVER_PORT_DEFAULT.into(),
            );
            result.set_string(
                Some(&CefString::from(Name::STATUS_KEY)),
                Some(&CefString::from("stopped")),
            );
        }

        Self::send_response(callback, true, result);
    }

    fn handle_start_action(
        &self,
        request: DictionaryValue,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) {
        let Ok(mut handler) = self.handler.lock() else {
            return;
        };
        if handler.is_some() {
            if let Ok(callback) = callback.lock() {
                callback.failure(ACTION_STATE_ERROR, "Server is currently running");
            }
            return;
        }

        let port_key = CefString::from(Name::PORT_KEY);
        if !verify_key(&request, &port_key, ValueType::INT, callback.clone()) {
            return;
        }
        let port = request.int(Some(&port_key));
        if !(8000..=65535).contains(&port) {
            if let Ok(callback) = callback.lock() {
                callback.failure(MESSAGE_FORMAT_ERROR, "Invalid port number specified");
            }
            return;
        }

        let server_handler = TestServerHandlerInner::new();
        *handler = Some(server_handler.clone());

        // Start the server. OnComplete will be executed upon completion.
        let Ok(mut handler) = server_handler.lock() else {
            return;
        };

        let weak_self = self.weak_self.clone();
        handler.start_server(
            port as u16,
            Some(Box::new(move |success| {
                if let Some(this) = weak_self.upgrade() {
                    this.on_start_complete(callback.clone(), success);
                }
            })),
        );
    }

    fn handle_stop_action(&self, callback: Arc<Mutex<dyn BrowserSideCallback>>) {
        let Ok(mut handler) = self.handler.lock() else {
            return;
        };

        if let Some(mut server_handler) = handler.as_ref().and_then(|handler| handler.lock().ok()) {
            // Stop the server. OnComplete will be executed upon completion.
            let weak_self = self.weak_self.clone();
            server_handler.stop_server(Some(Box::new(move |success| {
                if let Some(this) = weak_self.upgrade() {
                    this.on_stop_complete(callback.clone(), success);
                }
            })));
        } else {
            if let Ok(callback) = callback.lock() {
                callback.failure(ACTION_STATE_ERROR, "Server is not currently running");
            }
            return;
        };

        *handler = None;
    }

    // Server start completed.
    fn on_start_complete(&self, callback: Arc<Mutex<dyn BrowserSideCallback>>, success: bool) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if !success && let Ok(mut handler) = self.handler.lock() {
            *handler = None;
        }
        if let Some(result) = dictionary_value_create() {
            if !success {
                result.set_string(
                    Some(&CefString::from(Name::MESSAGE_KEY)),
                    Some(&CefString::from("Server failed to start.")),
                );
            }
            Self::send_response(callback, success, result);
        }
    }

    // Server stop completed.
    fn on_stop_complete(&self, callback: Arc<Mutex<dyn BrowserSideCallback>>, success: bool) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Some(result) = dictionary_value_create() {
            if !success {
                result.set_string(
                    Some(&CefString::from(Name::MESSAGE_KEY)),
                    Some(&CefString::from("Server failed to stop.")),
                );
            }
            Self::send_response(callback, success, result);
        }
    }

    // Send a response in the format expected by server.html.
    fn send_response(
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
        success: bool,
        result: DictionaryValue,
    ) {
        result.set_string(
            Some(&CefString::from(Name::RESULT_KEY)),
            Some(&CefString::from(if success {
                "success"
            } else {
                "failure"
            })),
        );

        if let (Some(message), Ok(callback)) = (dictionary_to_json(result), callback.lock()) {
            callback.success_str(&message);
        }
    }
}

impl BrowserSideHandler for Handler {
    // Called due to cefQuery execution in server.html.
    fn on_query_str(
        &self,
        _browser: Option<Browser>,
        frame: Option<Frame>,
        _query_id: i64,
        request: &str,
        _persistent: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Only handle messages from the test URL.
        if !Self::is_test_url(frame) {
            return false;
        }

        // Parse |request| as a JSON dictionary.
        let Some(request) = parse_json_dictionary(request) else {
            send_failure(callback, MESSAGE_FORMAT_ERROR, "Incorrect message format");
            return true;
        };

        let key = CefString::from(Name::ACTION_KEY);
        if verify_key(&request, &key, ValueType::STRING, callback.clone()) {
            match CefString::from(&request.string(Some(&key)))
                .to_string()
                .as_str()
            {
                "query" => self.handle_query_action(callback),
                "start" => self.handle_start_action(request, callback),
                "stop" => self.handle_stop_action(callback),
                action => {
                    send_failure(
                        callback,
                        MESSAGE_FORMAT_ERROR,
                        format!("Unrecognized action: {action}").as_str(),
                    );
                }
            }
        }

        true
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Handler::new()
}
