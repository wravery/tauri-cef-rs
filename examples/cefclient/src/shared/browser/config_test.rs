use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const TEST_URL_PATH: &str = "/config";

// Application-specific error codes.
const REQUEST_FAILED_ERROR: i32 = 2;

// Common to all messages.
struct Name;

impl Name {
    const KEY: &str = "name";
    const GLOBAL_CONFIG: &str = "global_config";
    const SUBSCRIBE: &str = "subscribe";
}

wrap_preference_observer! {
    struct ConfigPreferenceObserver {
        manager: PreferenceManager,
        global: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    }

    impl PreferenceObserver {
        fn on_preference_changed(&self, name: Option<&CefString>) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let (Some(name), Some(payload)) = (name, dictionary_value_create()) else {
                return;
            };
            payload.set_string(
                Some(&CefString::from("type")),
                Some(&CefString::from("preference")),
            );
            payload.set_bool(
                Some(&CefString::from("global")),
                self.global.into(),
            );
            payload.set_string(Some(&CefString::from("name")), Some(name));
            if let Some(mut value) = self.manager.preference(Some(name)) {
                payload.set_int(
                    Some(&CefString::from("value_type")),
                    value.get_type().get_raw() as i32,
                );
                payload.set_value(Some(&CefString::from("value")), Some(&mut value));
            } else {
                payload.set_int(
                    Some(&CefString::from("value_type")),
                    ValueType::NULL.get_raw() as i32,
                );
                payload.set_null(Some(&CefString::from("value")));
            }

            send_success(self.callback.clone(), payload);
        }
    }
}

wrap_setting_observer! {
    struct ConfigSettingObserver {
        context: RequestContext,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    }

    impl SettingObserver {
        fn on_setting_changed(
            &self,
            requesting_url: Option<&CefString>,
            top_level_url: Option<&CefString>,
            content_type: ContentSettingTypes,
        ) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let Some(payload) = dictionary_value_create() else {
                return;
            };
            payload.set_string(
                Some(&CefString::from("type")),
                Some(&CefString::from("setting")),
            );
            payload.set_string(Some(&CefString::from("requesting_url")), requesting_url);
            payload.set_string(Some(&CefString::from("top_level_url")), top_level_url);
            payload.set_int(
                Some(&CefString::from("content_type")),
                content_type.get_raw() as i32,
            );
            if let Some(mut value) =
                self.context
                    .website_setting(requesting_url, top_level_url, content_type)
            {
                payload.set_int(
                    Some(&CefString::from("value_type")),
                    value.get_type().get_raw() as i32,
                );
                payload.set_value(Some(&CefString::from("value")), Some(&mut value));
            } else {
                payload.set_int(
                    Some(&CefString::from("value_type")),
                    ValueType::NULL.get_raw() as i32,
                );
                payload.set_null(Some(&CefString::from("value")));
            }

            send_success(self.callback.clone(), payload);
        }
    }
}

/// Subscription state associated with a single browser.
#[derive(Default)]
struct SubscriptionState {
    query_id: i64,
    global_pref_observer: Option<PreferenceObserver>,
    global_pref_registration: Option<Registration>,
    context_pref_observer: Option<PreferenceObserver>,
    context_pref_registration: Option<Registration>,
    context_setting_observer: Option<SettingObserver>,
    context_setting_registration: Option<Registration>,
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

    /// Verify that |key| exists in |dictionary| and has type |value_type|. Fails
    /// |callback| and returns false on failure.
    fn verify_key(
        dictionary: &DictionaryValue,
        key: &CefString,
        value_type: ValueType,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        if dictionary.has_key(Some(key)) == 0 || dictionary.get_type(Some(key)) != value_type {
            let error_message = format!("Missing or incorrectly formatted message key: {key}");
            send_failure(callback, MESSAGE_FORMAT_ERROR, &error_message);
            false
        } else {
            true
        }
    }

    fn make_list_value(values: CefStringList) -> Option<ListValue> {
        let result = list_value_create()?;
        let values: Vec<_> = values.into_iter().collect();
        if result.set_size(values.len()) == 0 {
            return None;
        }
        for (index, value) in values.into_iter().enumerate() {
            result.set_string(index, Some(&CefString::from(value.as_str())));
        }
        Some(result)
    }

    fn send_global_config(&self, callback: Arc<Mutex<dyn BrowserSideCallback>>) {
        let mut switches = CefStringList::new();
        preference_manager_get_chrome_variations_as_switches(Some(&mut switches));
        let mut strings = CefStringList::new();
        preference_manager_get_chrome_variations_as_strings(Some(&mut strings));

        let Some(payload) = dictionary_value_create() else {
            return;
        };
        let key = CefString::from("switches");
        if let Some(mut switches) = Self::make_list_value(switches) {
            payload.set_list(Some(&key), Some(&mut switches));
        } else {
            payload.set_null(Some(&key));
        }
        let key = CefString::from("strings");
        if let Some(mut strings) = Self::make_list_value(strings) {
            payload.set_list(Some(&key), Some(&mut strings));
        } else {
            payload.set_null(Some(&key));
        }

        send_success(callback, payload);
    }

    fn create_subscription(
        &self,
        browser: Option<Browser>,
        query_id: i64,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
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

        let (Some(global_pref_manager), Some(request_context)) = (
            preference_manager_get_global(),
            browser.host().and_then(|host| host.request_context()),
        ) else {
            return false;
        };

        let mut global_pref_observer =
            ConfigPreferenceObserver::new(global_pref_manager.clone(), true, callback.clone());
        let global_pref_registration =
            global_pref_manager.add_preference_observer(None, Some(&mut global_pref_observer));
        let mut context_pref_observer =
            ConfigPreferenceObserver::new((&request_context).into(), false, callback.clone());
        let context_pref_registration =
            request_context.add_preference_observer(None, Some(&mut context_pref_observer));
        let mut context_setting_observer =
            ConfigSettingObserver::new(request_context.clone(), callback.clone());
        let context_setting_registration =
            request_context.add_setting_observer(Some(&mut context_setting_observer));
        let state = SubscriptionState {
            query_id,
            global_pref_observer: Some(global_pref_observer),
            global_pref_registration,
            context_pref_observer: Some(context_pref_observer),
            context_pref_registration,
            context_setting_observer: Some(context_setting_observer),
            context_setting_registration,
        };

        subscription_state_map.insert(browser_id, state);
        true
    }

    fn remove_subscription(&self, browser_id: i32) {
        if let Ok(mut subscription_state_map) = self.subscription_state_map.lock() {
            subscription_state_map.remove(&browser_id);
        }
    }
}

impl BrowserSideHandler for Handler {
    /// Called due to cefQuery execution in config.html.
    fn on_query_str(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        query_id: i64,
        request: &str,
        persistent: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
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
        if !Self::verify_key(&request, &key, ValueType::STRING, callback.clone()) {
            return true;
        }
        let message_name = CefString::from(&request.string(Some(&key))).to_string();
        match message_name.as_str() {
            Name::GLOBAL_CONFIG => {
                // JavaScript is requesting a JSON representation of the global config.
                self.send_global_config(callback);
                true
            }
            Name::SUBSCRIBE => {
                // Subscribe to notifications from observers.
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
            _ => false,
        }
    }

    fn on_query_canceled(&self, browser: Option<Browser>, _frame: Option<Frame>, _query_id: i64) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Some(browser) = browser {
            self.remove_subscription(browser.identifier());
        }
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler::new())
}
