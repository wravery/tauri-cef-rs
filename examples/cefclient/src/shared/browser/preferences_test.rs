use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use std::sync::{Arc, Mutex};

const TEST_URL_PATH: &str = "/preferences";

// Application-specific error codes.
const PREFERENCE_APPLICATION_ERROR: i32 = 2;

struct Name;

impl Name {
    // Common to all messages.
    const KEY: &str = "name";
    const VALUE_GET: &str = "preferences_get";
    const VALUE_SET: &str = "preferences_set";
    const VALUE_STATE: &str = "preferences_state";

    // Used with "preferences_get" messages.
    const GLOBAL_PREFS_KEY: &str = "global_prefs";
    const INCLUDE_DEFAULTS_KEY: &str = "include_defaults";

    // Used with "preferences_set" messages.
    const PREFERENCES_KEY: &str = "preferences";
}

struct Handler;

impl Handler {
    fn new() -> Self {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        Self
    }

    fn get_preference_manager(browser: &Browser, global_prefs: bool) -> Option<PreferenceManager> {
        if global_prefs {
            preference_manager_get_global()
        } else {
            browser
                .host()
                .and_then(|host| host.request_context())
                .as_ref()
                .map(PreferenceManager::from)
        }
    }

    // Execute |callback| with the preferences dictionary as a JSON string.
    fn on_preferences_get(
        browser: &Browser,
        global_prefs: bool,
        include_defaults: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) {
        let Some(prefs) = Self::get_preference_manager(browser, global_prefs)
            .and_then(|pref_manager| pref_manager.all_preferences(include_defaults.into()))
        else {
            return;
        };
        send_success(callback, prefs);
    }

    fn build_message(changed_names: Vec<String>, error_message: Option<String>) -> String {
        let mut lines = vec![];
        if !changed_names.is_empty() {
            lines.push(format!("Successfully changed {}", changed_names.join(", ")));
        }
        if let Some(error_message) = error_message {
            lines.push(error_message);
        }
        if changed_names.is_empty() {
            lines.push("No preferences changed.".to_string());
        }
        lines.join("\n")
    }

    // Set preferences based on the contents of |preferences|. Execute |callback|
    // with a descriptive result message.
    fn on_preferences_set(
        browser: &Browser,
        global_prefs: bool,
        mut preferences: DictionaryValue,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) {
        let Ok(callback) = callback.lock() else {
            return;
        };
        let (Some(value), Some(pref_manager)) = (
            value_create(),
            Self::get_preference_manager(browser, global_prefs),
        ) else {
            return;
        };

        value.set_dictionary(Some(&mut preferences));
        match Self::apply_preferences(&pref_manager, None, value) {
            Ok(changed_names) => {
                let message = Self::build_message(changed_names, None);
                callback.success_str(&message);
            }
            Err((changed_names, error_message)) => {
                let error_message = Self::build_message(changed_names, Some(error_message));
                callback.failure(PREFERENCE_APPLICATION_ERROR, &error_message)
            }
        }
    }

    // Execute |callback| with the global state dictionary as a JSON string.
    fn on_preferences_state(_browser: &Browser, callback: Arc<Mutex<dyn BrowserSideCallback>>) {
        let (Some(command_line), Some(value)) =
            (command_line_get_global(), dictionary_value_create())
        else {
            return;
        };

        // If spell checking is disabled via the command-line then it cannot be
        // enabled via preferences.
        let spellcheck_disabled =
            command_line.has_switch(Some(&CefString::from("disable-spell-checking"))) != 0;
        value.set_bool(
            Some(&CefString::from("spellcheck_disabled")),
            spellcheck_disabled.into(),
        );

        // If proxy settings are configured via the command-line then they cannot
        // be modified via preferences.
        let proxy_configured = command_line.has_switch(Some(&CefString::from("no-proxy-server")))
            != 0
            || command_line.has_switch(Some(&CefString::from("proxy-auto-detect"))) != 0
            || command_line.has_switch(Some(&CefString::from("proxy-pac-url"))) != 0
            || command_line.has_switch(Some(&CefString::from("proxy-server"))) != 0;
        value.set_bool(
            Some(&CefString::from("proxy_configured")),
            proxy_configured.into(),
        );

        // If allow running insecure content is enabled via the command-line then it
        // cannot be enabled via preferences.
        let allow_running_insecure_content =
            command_line.has_switch(Some(&CefString::from("allow-running-insecure-content"))) != 0;
        value.set_bool(
            Some(&CefString::from("allow_running_insecure_content")),
            allow_running_insecure_content.into(),
        );

        send_success(callback, value);
    }

    // Apply preferences. Returns true on success. Returns false and sets |error|
    // to a descriptive error string on failure. |changed_names| is the list of
    // preferences that were successfully changed.
    fn apply_preferences(
        pref_manager: &PreferenceManager,
        name: Option<&str>,
        value: Value,
    ) -> Result<Vec<String>, (Vec<String>, String)> {
        let name = name.unwrap_or("");
        if !name.is_empty() {
            let name = CefString::from(name);
            if pref_manager.has_preference(Some(&name)) != 0 {
                // The preference exists. Set the value.
                return Self::set_preference(pref_manager, &name, value)
                    .map_err(|error| (vec![], error))
                    .map(|name| name.into_iter().collect());
            }
        }

        if value.get_type() == ValueType::DICTIONARY {
            // A dictionary type value that is not an existing preference. Try to set
            // each of the elements individually.
            let Some(value) = value.dictionary() else {
                return Ok(vec![]);
            };

            let mut keys = Default::default();
            if value.keys(Some(&mut keys)) == 0 {
                return Ok(vec![]);
            }

            let mut changed_names = vec![];
            for key in keys {
                let Some(value) = value.value(Some(&CefString::from(key.as_str()))) else {
                    continue;
                };
                let current_name = if name.is_empty() {
                    key
                } else {
                    format!("{}.{}", name, key)
                };
                match Self::apply_preferences(pref_manager, Some(current_name.as_str()), value) {
                    Ok(names) => changed_names.extend(names),
                    Err((names, error)) => {
                        changed_names.extend(names);
                        return Err((changed_names, error));
                    }
                }
            }

            return Ok(changed_names);
        }

        Err((
            vec![],
            format!("Trying to create an unregistered preference: {name}"),
        ))
    }

    // Set a specific preference value. Returns true if the value is set
    // successfully or has not changed. If the value has changed then |name| will
    // be added to |changed_names|. Returns false and sets |error| to a
    // descriptive error string on failure.
    fn set_preference(
        pref_manager: &PreferenceManager,
        name: &CefString,
        mut value: Value,
    ) -> Result<Option<String>, String> {
        let existing_value = pref_manager.preference(Some(name));
        let Some(existing_value) = existing_value else {
            return Err(format!("Preference not found: {name}"));
        };

        match (value.get_type(), existing_value.get_type()) {
            (ValueType::STRING, ValueType::BOOL) => {
                match CefString::from(&value.string()).to_string().as_str() {
                    "1" | "true" => {
                        value.set_bool(1);
                    }
                    "0" | "false" => {
                        value.set_bool(0);
                    }
                    _ => {}
                }
            }
            (ValueType::STRING, ValueType::INT) => {
                if let Ok(num) = CefString::from(&value.string()).to_string().parse() {
                    value.set_int(num);
                }
            }
            (ValueType::STRING, ValueType::DOUBLE) => {
                if let Ok(num) = CefString::from(&value.string()).to_string().parse::<f64>() {
                    value.set_int(num as i32);
                }
            }
            _ => {}
        }

        // Nothing to do if the value hasn't changed.
        if existing_value.is_equal(Some(&mut value)) != 0 {
            return Ok(None);
        }

        // Attempt to set the preference.
        let mut error_message = Default::default();
        if pref_manager.set_preference(Some(name), Some(&mut value), Some(&mut error_message)) == 0
        {
            return Err(format!("{error_message}: {name}"));
        }

        // The preference was set successfully.
        Ok(Some(name.to_string()))
    }
}

impl BrowserSideHandler for Handler {
    // Called due to cefQuery execution in preferences.html.
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

        let Some(request) = parse_json_dictionary(request) else {
            send_failure(callback, MESSAGE_FORMAT_ERROR, "Incorrect message format");
            return true;
        };

        let Some(browser) = browser else {
            return true;
        };

        // Verify the "name" key.
        let key = CefString::from(Name::KEY);
        if !verify_key(&request, &key, ValueType::STRING, callback.clone()) {
            return true;
        }
        let message_name = CefString::from(&request.string(Some(&key))).to_string();
        match message_name.as_str() {
            Name::VALUE_GET => {
                // JavaScript is requesting a JSON representation of the preferences tree.
                let global_prefs_key = CefString::from(Name::GLOBAL_PREFS_KEY);
                let include_defaults_key = CefString::from(Name::INCLUDE_DEFAULTS_KEY);
                if verify_key(
                    &request,
                    &global_prefs_key,
                    ValueType::BOOL,
                    callback.clone(),
                ) && verify_key(
                    &request,
                    &include_defaults_key,
                    ValueType::BOOL,
                    callback.clone(),
                ) {
                    let global_prefs = request.bool(Some(&global_prefs_key)) != 0;
                    let include_defaults = request.bool(Some(&include_defaults_key)) != 0;
                    Self::on_preferences_get(&browser, global_prefs, include_defaults, callback);
                }
                true
            }
            Name::VALUE_SET => {
                // JavaScript is requesting that preferences be updated to match the
                // specified JSON representation.
                let global_prefs_key = CefString::from(Name::GLOBAL_PREFS_KEY);
                let preferences_key = CefString::from(Name::PREFERENCES_KEY);
                if verify_key(
                    &request,
                    &global_prefs_key,
                    ValueType::BOOL,
                    callback.clone(),
                ) && verify_key(
                    &request,
                    &preferences_key,
                    ValueType::DICTIONARY,
                    callback.clone(),
                ) {
                    let global_prefs = request.bool(Some(&global_prefs_key)) != 0;
                    if let Some(preferences) = request.dictionary(Some(&preferences_key)) {
                        Self::on_preferences_set(&browser, global_prefs, preferences, callback);
                    }
                }
                true
            }
            Name::VALUE_STATE => {
                // JavaScript is requesting global state information.
                Self::on_preferences_state(&browser, callback);
                true
            }
            _ => false,
        }
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler::new())
}
