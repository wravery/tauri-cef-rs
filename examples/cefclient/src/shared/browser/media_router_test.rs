use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

const TEST_URL_PATH: &str = "/media_router";

// Application-specific error codes.
const REQUEST_FAILED_ERROR: i32 = 2;

// Common to all messages.
struct Name;

impl Name {
    const KEY: &str = "name";
    const VALUE_SUBSCRIBE: &str = "subscribe";
    const VALUE_CREATE_ROUTE: &str = "createRoute";
    const VALUE_TERMINATE_ROUTE: &str = "terminateRoute";
    const VALUE_SEND_MESSAGE: &str = "sendMessage";
    const SOURCE_KEY: &str = "source_urn";
    const SINK_KEY: &str = "sink_id";
    const ROUTE_KEY: &str = "route_id";
    const MESSAGE_KEY: &str = "message";
    const SUCCESS_KEY: &str = "success";
    const PAYLOAD_KEY: &str = "payload";
}

type RouterCallback = Arc<Mutex<dyn BrowserSideCallback>>;

wrap_media_route_create_callback! {
    struct TestMediaRouteCreateCallback {
        create_callback: Arc<Mutex<Option<RouterCallback>>>,
    }

    impl MediaRouteCreateCallback {
        fn on_media_route_create_finished(
            &self,
            result: MediaRouteCreateResult,
            error: Option<&CefString>,
            route: Option<&mut MediaRoute>,
        ) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let (Some(route), Some(create_callback)) = (
                route,
                self.create_callback
                    .lock()
                    .ok()
                    .and_then(|mut callback| callback.take()),
            ) else {
                return;
            };
            match (result, dictionary_value_create()) {
                (MediaRouteCreateResult::OK, Some(value)) => {
                    value.set_string(
                        Some(&CefString::from(Name::ROUTE_KEY)),
                        Some(&CefString::from(&route.id())),
                    );
                    send_success(create_callback, value);
                }
                _ => {
                    send_failure(
                        create_callback,
                        REQUEST_FAILED_ERROR + result.get_raw() as i32,
                        error.map(CefString::to_string).unwrap_or_default().as_str(),
                    );
                }
            }
        }
    }
}

type DeviceInfoCallbackType = Arc<Box<dyn Send + Sync + Fn(&str, &MediaSinkDeviceInfo)>>;

wrap_media_sink_device_info_callback! {
    struct TestMediaSinkDeviceInfoCallback {
        sink_id: String,
        callback: Arc<Mutex<Option<DeviceInfoCallbackType>>>,
    }

    impl MediaSinkDeviceInfoCallback {
        fn on_media_sink_device_info(&self, device_info: Option<&MediaSinkDeviceInfo>) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            if let (Some(device_info), Some(callback)) = (
                device_info,
                self.callback
                    .lock()
                    .ok()
                    .and_then(|mut callback| callback.take()),
            ) {
                callback(&self.sink_id, device_info);
            }
        }
    }
}

#[derive(Clone)]
struct DeviceInfo {
    size: usize,
    ip_address: String,
    port: i32,
    model_name: String,
}

impl From<&MediaSinkDeviceInfo> for DeviceInfo {
    fn from(device_info: &MediaSinkDeviceInfo) -> Self {
        DeviceInfo {
            size: device_info.size,
            ip_address: device_info.ip_address.to_string(),
            port: device_info.port,
            model_name: device_info.model_name.to_string(),
        }
    }
}

#[derive(Clone)]
struct SinkInfo {
    sink: MediaSink,
    device_info: Option<DeviceInfo>,
}

type SinkInfoMap = BTreeMap<String, SinkInfo>;

type RouteMap = BTreeMap<String, MediaRoute>;

/// Observes MediaRouter events. Only accessed on the UI thread.
struct MediaRouterObserver {
    weak_self: Weak<Mutex<Self>>,
    media_router: MediaRouter,
    subscription_callback: RouterCallback,
    /// Used to uniquely identify a call to OnSinks(), for the purpose of
    /// associating OnMediaSinkDeviceInfo() callbacks.
    next_sink_query_id: i32,
    /// State from the most recent call to OnSinks().
    sink_info_map: SinkInfoMap,
    pending_sink_query_id: i32,
    pending_sink_callbacks: usize,
    /// State from the most recent call to OnRoutes().
    route_map: RouteMap,
}

impl MediaRouterObserver {
    fn new(media_router: MediaRouter, subscription_callback: RouterCallback) -> Arc<Mutex<Self>> {
        Arc::new_cyclic(|weak_self| {
            Mutex::new(Self {
                weak_self: weak_self.clone(),
                media_router,
                subscription_callback,
                next_sink_query_id: 0,
                sink_info_map: Default::default(),
                pending_sink_query_id: -1,
                pending_sink_callbacks: 0,
                route_map: Default::default(),
            })
        })
    }

    fn create_route(
        &self,
        source_urn: &str,
        sink_id: &str,
        callback: RouterCallback,
    ) -> Result<(), String> {
        let mut source = self
            .source(source_urn)
            .ok_or_else(|| format!("Invalid source: {source_urn}"))?;
        let mut sink = self
            .sink(sink_id)
            .ok_or_else(|| format!("Invalid sink: {sink_id}"))?;

        let mut callback = TestMediaRouteCreateCallback::new(Arc::new(Mutex::new(Some(callback))));
        self.media_router
            .create_route(Some(&mut source), Some(&mut sink), Some(&mut callback));
        Ok(())
    }

    fn terminate_route(&self, route_id: &str) -> Result<(), String> {
        let route = self
            .route(route_id)
            .ok_or_else(|| format!("Invalid route: {route_id}"))?;
        route.terminate();
        Ok(())
    }

    fn send_route_message(&self, route_id: &str, message: &[u8]) -> Result<(), String> {
        let route = self
            .route(route_id)
            .ok_or_else(|| format!("Invalid route: {route_id}"))?;
        route.send_route_message(Some(message));
        Ok(())
    }

    fn on_sinks(&mut self, sinks: &[Option<MediaSink>]) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let sinks: Vec<_> = sinks.iter().filter_map(|sink| sink.clone()).collect();
        self.sink_info_map.clear();

        // Reset pending sink state.
        self.pending_sink_callbacks = sinks.len();
        self.next_sink_query_id += 1;
        self.pending_sink_query_id = self.next_sink_query_id;

        if sinks.is_empty() {
            // No sinks, send the response immediately.
            self.send_sinks_response();
            return;
        }

        for sink in sinks {
            let sink_id = CefString::from(&sink.id()).to_string();
            let sink_info = SinkInfo {
                sink: sink.clone(),
                device_info: None,
            };
            self.sink_info_map.insert(sink_id.clone(), sink_info);

            // Request the device info asynchronously. Send the response once all
            // callbacks have executed.
            let mut callback = {
                let weak_self = self.weak_self.clone();
                let pending_sink_query_id = self.pending_sink_query_id;
                TestMediaSinkDeviceInfoCallback::new(
                    sink_id,
                    Arc::new(Mutex::new(Some(Arc::new(Box::new(
                        move |sink_id, device_info| {
                            let Some(this) = weak_self.upgrade() else {
                                return;
                            };
                            let Ok(mut this) = this.lock() else {
                                return;
                            };
                            this.on_sink_device_info(pending_sink_query_id, sink_id, device_info);
                        },
                    ))))),
                )
            };
            sink.device_info(Some(&mut callback));
        }
    }

    fn on_routes(&mut self, routes: &[Option<MediaRoute>]) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let routes: Vec<_> = routes.iter().filter_map(|route| route.clone()).collect();
        self.route_map.clear();

        let (Some(payload), Some(mut route_list)) =
            (dictionary_value_create(), list_value_create())
        else {
            return;
        };
        route_list.set_size(routes.len());

        for (index, route) in routes.into_iter().enumerate() {
            let route_id = CefString::from(&route.id()).to_string();
            self.route_map.insert(route_id, route.clone());

            let mut route_value = dictionary_value_create();
            if let Some(route_value) = route_value.as_ref() {
                route_value.set_string(
                    Some(&CefString::from("id")),
                    Some(&CefString::from(&route.id())),
                );
                route_value.set_string(
                    Some(&CefString::from(Name::SOURCE_KEY)),
                    route
                        .source()
                        .map(|source| CefString::from(&source.id()))
                        .as_ref(),
                );
                route_value.set_string(
                    Some(&CefString::from(Name::SINK_KEY)),
                    route
                        .sink()
                        .map(|sink| CefString::from(&sink.id()))
                        .as_ref(),
                );
            }

            route_list.set_dictionary(index, route_value.as_mut());
        }

        payload.set_list(Some(&CefString::from("routes_list")), Some(&mut route_list));
        self.send_response("onRoutes", payload);
    }

    fn on_route_state_changed(&self, route: &MediaRoute, state: MediaRouteConnectionState) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let Some(payload) = dictionary_value_create() else {
            return;
        };
        payload.set_string(
            Some(&CefString::from(Name::ROUTE_KEY)),
            Some(&CefString::from(&route.id())),
        );
        payload.set_int(Some(&CefString::from("connection_state")), state.get_raw());
        self.send_response("onRouteStateChanged", payload);
    }

    fn on_route_message_received(&self, route: &MediaRoute, message: &[u8]) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let (Ok(message), Some(payload)) = (
            String::from_utf8(message.to_vec()),
            dictionary_value_create(),
        ) else {
            return;
        };
        payload.set_string(
            Some(&CefString::from(Name::ROUTE_KEY)),
            Some(&CefString::from(&route.id())),
        );
        payload.set_string(
            Some(&CefString::from(Name::MESSAGE_KEY)),
            Some(&CefString::from(message.as_str())),
        );
        self.send_response("onRouteMessageReceived", payload);
    }

    fn source(&self, source_urn: &str) -> Option<MediaSource> {
        self.media_router.source(Some(&CefString::from(source_urn)))
    }

    fn sink(&self, sink_id: &str) -> Option<MediaSink> {
        self.sink_info_map
            .get(sink_id)
            .map(|sink_info| sink_info.sink.clone())
    }

    fn on_sink_device_info(
        &mut self,
        sink_query_id: i32,
        sink_id: &str,
        sink_info: &MediaSinkDeviceInfo,
    ) {
        // Discard callbacks that arrive after a new call to OnSinks().
        if sink_query_id != self.pending_sink_query_id {
            return;
        }

        if let Some(entry) = self.sink_info_map.get_mut(sink_id) {
            entry.device_info = Some(sink_info.into());
        }

        // Send the response once we've received all expected callbacks.
        debug_assert!(self.pending_sink_callbacks > 0);
        self.pending_sink_callbacks -= 1;
        if self.pending_sink_callbacks == 0 {
            self.send_sinks_response();
        }
    }

    fn route(&self, route_id: &str) -> Option<MediaRoute> {
        self.route_map.get(route_id).cloned()
    }

    fn send_response(&self, name: &str, mut payload: DictionaryValue) {
        let Some(result) = dictionary_value_create() else {
            return;
        };
        result.set_string(
            Some(&CefString::from(Name::KEY)),
            Some(&CefString::from(name)),
        );
        result.set_dictionary(
            Some(&CefString::from(Name::PAYLOAD_KEY)),
            Some(&mut payload),
        );
        send_success(self.subscription_callback.clone(), result);
    }

    fn send_sinks_response(&self) {
        let (Some(payload), Some(mut sinks_list)) =
            (dictionary_value_create(), list_value_create())
        else {
            return;
        };
        let sinks: Vec<_> = self
            .sink_info_map
            .iter()
            .filter_map(|(sink_id, sink_info)| {
                let sink_value = dictionary_value_create()?;
                let device_info = sink_info.device_info.as_ref()?;
                sink_value.set_string(
                    Some(&CefString::from("id")),
                    Some(&CefString::from(sink_id.as_str())),
                );
                sink_value.set_string(
                    Some(&CefString::from("name")),
                    Some(&CefString::from(&sink_info.sink.name())),
                );
                sink_value.set_int(
                    Some(&CefString::from("icon")),
                    sink_info.sink.icon_type().get_raw() as i32,
                );
                sink_value.set_string(
                    Some(&CefString::from("ip_address")),
                    Some(&CefString::from(device_info.ip_address.as_str())),
                );
                sink_value.set_int(Some(&CefString::from("port")), device_info.port);
                sink_value.set_string(
                    Some(&CefString::from("model_name")),
                    Some(&CefString::from(device_info.model_name.as_str())),
                );
                let sink_type = if sink_info.sink.is_cast_sink() != 0 {
                    "cast"
                } else if sink_info.sink.is_dial_sink() != 0 {
                    "dial"
                } else {
                    "unknown"
                };
                sink_value.set_string(
                    Some(&CefString::from("type")),
                    Some(&CefString::from(sink_type)),
                );
                Some(sink_value)
            })
            .collect();
        sinks_list.set_size(sinks.len());
        for (index, mut sink) in sinks.into_iter().enumerate() {
            sinks_list.set_dictionary(index, Some(&mut sink));
        }

        payload.set_list(Some(&CefString::from("sinks_list")), Some(&mut sinks_list));
        self.send_response("onSinks", payload);
    }
}

wrap_media_observer! {
    struct TestMediaObserver {
        inner: Arc<Mutex<MediaRouterObserver>>,
    }

    impl MediaObserver {
        fn on_sinks(&self, sinks: Option<&[Option<MediaSink>]>) {
            let (Some(sinks), Ok(mut inner)) = (sinks, self.inner.lock()) else {
                return;
            };
            inner.on_sinks(sinks);
        }

        fn on_routes(&self, routes: Option<&[Option<MediaRoute>]>) {
            let (Some(routes), Ok(mut inner)) = (routes, self.inner.lock()) else {
                return;
            };
            inner.on_routes(routes);
        }

        fn on_route_state_changed(
            &self,
            route: Option<&mut MediaRoute>,
            state: MediaRouteConnectionState,
        ) {
            let (Some(route), Ok(inner)) = (route, self.inner.lock()) else {
                return;
            };
            inner.on_route_state_changed(route, state);
        }

        fn on_route_message_received(
            &self,
            route: Option<&mut MediaRoute>,
            message: Option<&[u8]>,
        ) {
            let (Some(route), Some(message), Ok(inner)) = (route, message, self.inner.lock())
            else {
                return;
            };
            inner.on_route_message_received(route, message);
        }
    }
}

/// Subscription state associated with a single browser.
#[derive(Clone, Default)]
struct SubscriptionState {
    query_id: i64,
    observer: Option<Arc<Mutex<MediaRouterObserver>>>,
    registration: Option<Registration>,
}

type SubscriptionStateMap = BTreeMap<i32, SubscriptionState>;

struct Handler {
    subscription_state_map: Mutex<SubscriptionStateMap>,
}

impl Handler {
    fn new() -> Self {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        Self {
            subscription_state_map: Default::default(),
        }
    }

    fn send_success_ack(callback: RouterCallback) {
        let Some(result) = dictionary_value_create() else {
            return;
        };
        result.set_bool(Some(&CefString::from(Name::SUCCESS_KEY)), 1);
        send_success(callback, result);
    }

    fn create_subscription(
        &self,
        browser: Option<Browser>,
        query_id: i64,
        callback: RouterCallback,
    ) -> bool {
        let (Some(browser), Ok(mut subscription_state_map)) =
            (browser, self.subscription_state_map.lock())
        else {
            return false;
        };
        let browser_id = browser.identifier();
        if subscription_state_map.contains_key(&browser_id) {
            // An subscription already exists for this browser.
            return false;
        }

        let media_router = browser
            .host()
            .and_then(|host| host.request_context())
            .and_then(|request_context| request_context.media_router(None))
            .and_then(|media_router| {
                let observer = MediaRouterObserver::new(media_router.clone(), callback.clone());
                let registration = media_router
                    .add_observer(Some(&mut TestMediaObserver::new(observer.clone())))?;
                subscription_state_map.insert(
                    browser_id,
                    SubscriptionState {
                        query_id,
                        registration: Some(registration),
                        observer: Some(observer),
                    },
                );
                Some(media_router)
            });

        if let Some(media_router) = media_router {
            // Trigger sink and route callbacks.
            media_router.notify_current_sinks();
            media_router.notify_current_routes();
        }

        true
    }

    fn remove_subscription(&self, browser_id: i32, query_id: i64) {
        if let Ok(mut subscription_state_map) = self.subscription_state_map.lock()
            && let Some(state) = subscription_state_map.get(&browser_id)
            && state.query_id == query_id
        {
            subscription_state_map.remove(&browser_id);
        }
    }

    fn media_observer(&self, browser_id: i32) -> Option<Arc<Mutex<MediaRouterObserver>>> {
        let subscription_state_map = self.subscription_state_map.lock().ok()?;
        let state = subscription_state_map.get(&browser_id)?;
        state.observer.clone()
    }
}

impl BrowserSideHandler for Handler {
    /// Called due to cefQuery execution in media_router.html.
    fn on_query_str(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        query_id: i64,
        request: &str,
        persistent: bool,
        callback: RouterCallback,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        // Only handle messages from the test URL.
        if !is_test_url(frame, TEST_URL_PATH) {
            return false;
        }

        let Some(request) = parse_json_dictionary(request) else {
            send_failure(callback, MESSAGE_FORMAT_ERROR, "Incorrect message format");
            return true;
        };

        // Verify the "name" key.
        let key = CefString::from(Name::KEY);
        if !verify_key(&request, &key, ValueType::STRING, callback.clone()) {
            return true;
        }
        let message_name = CefString::from(&request.string(Some(&key))).to_string();
        match message_name.as_str() {
            Name::VALUE_SUBSCRIBE => {
                // Subscribe to notifications from the media router.
                if !persistent {
                    send_failure(
                        callback,
                        MESSAGE_FORMAT_ERROR,
                        "Subscriptions must be persistent",
                    );
                } else if !self.create_subscription(browser, query_id, callback.clone()) {
                    send_failure(
                        callback,
                        REQUEST_FAILED_ERROR,
                        "Browser is already subscribed",
                    );
                }
                true
            }
            message_name => {
                // All other messages require a current subscription.
                let media_observer = browser
                    .map(|browser| browser.identifier())
                    .and_then(|browser_id| self.media_observer(browser_id));
                let Some(media_observer) = media_observer
                    .as_ref()
                    .and_then(|media_observer| media_observer.lock().ok())
                else {
                    send_failure(
                        callback,
                        REQUEST_FAILED_ERROR,
                        "Browser is not currently subscribed",
                    );
                    return false;
                };

                match message_name {
                    Name::VALUE_CREATE_ROUTE => {
                        // Create a new route.
                        let source_urn_key = CefString::from(Name::SOURCE_KEY);
                        let sink_id_key = CefString::from(Name::SINK_KEY);
                        if verify_key(
                            &request,
                            &source_urn_key,
                            ValueType::STRING,
                            callback.clone(),
                        ) && verify_key(
                            &request,
                            &sink_id_key,
                            ValueType::STRING,
                            callback.clone(),
                        ) {
                            let source_urn =
                                CefString::from(&request.string(Some(&source_urn_key))).to_string();
                            let sink_id =
                                CefString::from(&request.string(Some(&sink_id_key))).to_string();

                            // |callback| will be executed once the route is created.
                            if let Err(error_message) =
                                media_observer.create_route(&source_urn, &sink_id, callback.clone())
                            {
                                send_failure(callback, REQUEST_FAILED_ERROR, &error_message);
                            }
                        }

                        true
                    }
                    Name::VALUE_TERMINATE_ROUTE => {
                        // Terminate an existing route.
                        let route_key = CefString::from(Name::ROUTE_KEY);
                        if verify_key(&request, &route_key, ValueType::STRING, callback.clone()) {
                            let route_id =
                                CefString::from(&request.string(Some(&route_key))).to_string();

                            if let Err(error_message) = media_observer.terminate_route(&route_id) {
                                send_failure(callback, REQUEST_FAILED_ERROR, &error_message);
                            } else {
                                Self::send_success_ack(callback);
                            }
                        }

                        true
                    }
                    Name::VALUE_SEND_MESSAGE => {
                        // Send a route message.
                        let route_key = CefString::from(Name::ROUTE_KEY);
                        let message_key = CefString::from(Name::MESSAGE_KEY);
                        if verify_key(&request, &route_key, ValueType::STRING, callback.clone())
                            && verify_key(
                                &request,
                                &message_key,
                                ValueType::STRING,
                                callback.clone(),
                            )
                        {
                            let route_id =
                                CefString::from(&request.string(Some(&route_key))).to_string();
                            let message =
                                CefString::from(&request.string(Some(&message_key))).to_string();

                            if let Err(error_message) =
                                media_observer.send_route_message(&route_id, message.as_bytes())
                            {
                                send_failure(callback, REQUEST_FAILED_ERROR, &error_message);
                            } else {
                                Self::send_success_ack(callback);
                            }
                        }

                        true
                    }
                    _ => false,
                }
            }
        }
    }

    fn on_query_canceled(&self, browser: Option<Browser>, _frame: Option<Frame>, query_id: i64) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Some(browser) = browser {
            self.remove_subscription(browser.identifier(), query_id);
        }
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler::new())
}
