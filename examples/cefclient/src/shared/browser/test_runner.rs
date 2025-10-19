use super::*;
use cef::wrapper::message_router::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub use super::response_filter_test::resource_response_filter;

pub type MessageHandlerSet = BTreeMap<HandlerId, Arc<dyn BrowserSideHandler>>;

pub fn create_message_handlers() -> Vec<Arc<dyn BrowserSideHandler>> {
    let message_handler_set = vec![
        binary_transfer_test::create_message_handler(),
        binding_test::create_message_handler(),
        config_test::create_message_handler(),
        dialog_test::create_message_handler(),
        hang_test::create_message_handler(),
        media_router_test::create_message_handler(),
        preferences_test::create_message_handler(),
        server_test::create_message_handler(),
        task_manager_test::create_message_handler(),
        urlrequest_test::create_message_handler(),
        window_test::create_message_handler(),
    ];

    message_handler_set
}

pub type StringResourceMap = BTreeMap<String, String>;

const TEST_HOST: &str = "tests";
const LOCAL_HOST: &str = "localhost";
const TEST_ORIGIN: &str = "http://tests/";

/// Returns "https://tests/<path>".
pub fn get_test_url(name: &str) -> String {
    format!("{TEST_ORIGIN}{name}")
}

/// Application-specific error codes.
pub const MESSAGE_FORMAT_ERROR: i32 = 1;

/// Returns true if |url| is a test URL with the specified |path|. This matches
/// both "https://tests/<path>" and "http://localhost:xxxx/<path>".
pub fn is_test_url(frame: Option<Frame>, path: &str) -> bool {
    let parts = frame.and_then(|frame| {
        let mut parts = Default::default();
        if parse_url(Some(&CefString::from(&frame.url())), Some(&mut parts)) == 0 {
            None
        } else {
            Some(parts)
        }
    });
    parts.is_some_and(|parts| match parts.host.to_string().as_str() {
        TEST_HOST | LOCAL_HOST => parts.path.to_string().starts_with(path),
        _ => false,
    })
}

pub fn parse_json_dictionary(request: &str) -> Option<DictionaryValue> {
    let value = cef::parse_json(Some(&CefString::from(request)), JsonParserOptions::RFC)?;
    if value.get_type() == ValueType::DICTIONARY {
        value.dictionary()
    } else {
        None
    }
}

/// Verify that |key| exists in |dictionary| and has type |value_type|. Fails
/// |callback| and returns false on failure.
pub fn verify_key(
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

/// Convert a dictionary value to a JSON string.
pub fn dictionary_to_json(mut dictionary: DictionaryValue) -> Option<String> {
    let mut value = value_create()?;
    if value.set_dictionary(Some(&mut dictionary)) == 0 {
        return None;
    }
    let json = write_json(Some(&mut value), JsonWriterOptions::DEFAULT);
    Some(CefString::from(&json).to_string())
}

pub fn send_success(callback: Arc<Mutex<dyn BrowserSideCallback>>, result: DictionaryValue) {
    if let Ok(callback) = callback.lock()
        && let Some(result) = dictionary_to_json(result)
    {
        callback.success_str(&result);
    }
}

pub fn send_failure(
    callback: Arc<Mutex<dyn BrowserSideCallback>>,
    error_code: i32,
    error_message: &str,
) {
    if let Ok(callback) = callback.lock() {
        callback.failure(error_code, error_message);
    }
}

pub fn get_dump_response(
    request: &Request,
    response_headers: &mut CefStringMultimap,
) -> Option<StreamReader> {
    let size = string_multimap_size(Some(response_headers));
    let origin = (0..size).find_map(|index| {
        let mut key = Default::default();
        let mut value = Default::default();
        if string_multimap_key(Some(response_headers), index, Some(&mut key)) != 0
            && key.to_string().eq_ignore_ascii_case("origin")
            && string_multimap_value(Some(response_headers), index, Some(&mut value)) != 0
        {
            Some(value.to_string())
        } else {
            None
        }
    });

    if let Some(origin) = origin
        && (origin.starts_with(&format!("https://{TEST_HOST}"))
            || origin.starts_with(&format!("http://{LOCAL_HOST}")))
    {
        // Allow cross-origin XMLHttpRequests from test origins.
        response_headers.append("Access-Control-Allow-Origin", origin.as_str());
        // Allow the custom header from the xmlhttprequest.html example.
        response_headers.append("Access-Control-Allow-Headers", "My-Custom-Header");
    }

    let dump = dump_request_context(request);
    let content = format!(r#"<html><body bgcolor=\"white\"><pre>{dump}</pre></body></html>"#);
    let content = content.as_bytes();
    stream_reader_create_for_data(content.as_ptr() as *mut u8, content.len())
}

fn dump_request_context(request: &Request) -> String {
    let mut lines = vec![];

    let url = CefString::from(&request.url());
    lines.push(format!("URL: {url}"));
    let method = CefString::from(&request.method());
    lines.push(format!("Method: {method}"));

    let mut header_map = Default::default();
    request.header_map(Some(&mut header_map));
    let headers: Vec<_> = header_map
        .into_iter()
        .flat_map(|(key, values)| {
            values
                .into_iter()
                .map(move |value| format!("\t{key}: {value}"))
        })
        .collect();
    if !headers.is_empty() {
        lines.push("Headers:".to_string());
        lines.extend(headers);
    }

    if let Some(post_data) = request.post_data() {
        let mut elements = Default::default();
        post_data.elements(Some(&mut elements));
        let elements: Vec<_> = elements
            .into_iter()
            .filter_map(|entry| {
                entry.and_then(|entry| match entry.get_type() {
                    PostdataelementType::BYTES => {
                        // the element is composed of bytes
                        let size = entry.bytes_count();
                        let data = if size > 0 {
                            let mut data = vec![0; size];
                            let size = entry.bytes(size, data.as_mut_ptr());
                            data.truncate(size);
                            String::from_utf8(data).ok()
                        } else {
                            None
                        }
                        .unwrap_or_else(|| "(empty)".to_string());
                        Some(format!("\tBytes: {data}"))
                    }
                    PostdataelementType::FILE => {
                        let file = CefString::from(&entry.file());
                        Some(format!("\tFile: {file}"))
                    }
                    _ => None,
                })
            })
            .collect();
        if !elements.is_empty() {
            lines.push("Post Data:".to_string());
            lines.extend(elements);
        }
    }

    lines.join("\n")
}

pub fn get_error_string(error_code: Errorcode) -> String {
    match error_code {
        Errorcode::NONE => "ERR_NONE",
        Errorcode::FAILED => "ERR_FAILED",
        Errorcode::ABORTED => "ERR_ABORTED",
        Errorcode::INVALID_ARGUMENT => "ERR_INVALID_ARGUMENT",
        Errorcode::INVALID_HANDLE => "ERR_INVALID_HANDLE",
        Errorcode::FILE_NOT_FOUND => "ERR_FILE_NOT_FOUND",
        Errorcode::TIMED_OUT => "ERR_TIMED_OUT",
        Errorcode::FILE_TOO_BIG => "ERR_FILE_TOO_BIG",
        Errorcode::UNEXPECTED => "ERR_UNEXPECTED",
        Errorcode::ACCESS_DENIED => "ERR_ACCESS_DENIED",
        Errorcode::NOT_IMPLEMENTED => "ERR_NOT_IMPLEMENTED",
        Errorcode::CONNECTION_CLOSED => "ERR_CONNECTION_CLOSED",
        Errorcode::CONNECTION_RESET => "ERR_CONNECTION_RESET",
        Errorcode::CONNECTION_REFUSED => "ERR_CONNECTION_REFUSED",
        Errorcode::CONNECTION_ABORTED => "ERR_CONNECTION_ABORTED",
        Errorcode::CONNECTION_FAILED => "ERR_CONNECTION_FAILED",
        Errorcode::NAME_NOT_RESOLVED => "ERR_NAME_NOT_RESOLVED",
        Errorcode::INTERNET_DISCONNECTED => "ERR_INTERNET_DISCONNECTED",
        Errorcode::SSL_PROTOCOL_ERROR => "ERR_SSL_PROTOCOL_ERROR",
        Errorcode::ADDRESS_INVALID => "ERR_ADDRESS_INVALID",
        Errorcode::ADDRESS_UNREACHABLE => "ERR_ADDRESS_UNREACHABLE",
        Errorcode::SSL_CLIENT_AUTH_CERT_NEEDED => "ERR_SSL_CLIENT_AUTH_CERT_NEEDED",
        Errorcode::TUNNEL_CONNECTION_FAILED => "ERR_TUNNEL_CONNECTION_FAILED",
        Errorcode::NO_SSL_VERSIONS_ENABLED => "ERR_NO_SSL_VERSIONS_ENABLED",
        Errorcode::SSL_VERSION_OR_CIPHER_MISMATCH => "ERR_SSL_VERSION_OR_CIPHER_MISMATCH",
        Errorcode::SSL_RENEGOTIATION_REQUESTED => "ERR_SSL_RENEGOTIATION_REQUESTED",
        Errorcode::CERT_COMMON_NAME_INVALID => "ERR_CERT_COMMON_NAME_INVALID",
        Errorcode::CERT_DATE_INVALID => "ERR_CERT_DATE_INVALID",
        Errorcode::CERT_AUTHORITY_INVALID => "ERR_CERT_AUTHORITY_INVALID",
        Errorcode::CERT_CONTAINS_ERRORS => "ERR_CERT_CONTAINS_ERRORS",
        Errorcode::CERT_NO_REVOCATION_MECHANISM => "ERR_CERT_NO_REVOCATION_MECHANISM",
        Errorcode::CERT_UNABLE_TO_CHECK_REVOCATION => "ERR_CERT_UNABLE_TO_CHECK_REVOCATION",
        Errorcode::CERT_REVOKED => "ERR_CERT_REVOKED",
        Errorcode::CERT_INVALID => "ERR_CERT_INVALID",
        Errorcode::CERT_END => "ERR_CERT_END",
        Errorcode::INVALID_URL => "ERR_INVALID_URL",
        Errorcode::DISALLOWED_URL_SCHEME => "ERR_DISALLOWED_URL_SCHEME",
        Errorcode::UNKNOWN_URL_SCHEME => "ERR_UNKNOWN_URL_SCHEME",
        Errorcode::TOO_MANY_REDIRECTS => "ERR_TOO_MANY_REDIRECTS",
        Errorcode::UNSAFE_REDIRECT => "ERR_UNSAFE_REDIRECT",
        Errorcode::UNSAFE_PORT => "ERR_UNSAFE_PORT",
        Errorcode::INVALID_RESPONSE => "ERR_INVALID_RESPONSE",
        Errorcode::INVALID_CHUNKED_ENCODING => "ERR_INVALID_CHUNKED_ENCODING",
        Errorcode::METHOD_NOT_SUPPORTED => "ERR_METHOD_NOT_SUPPORTED",
        Errorcode::UNEXPECTED_PROXY_AUTH => "ERR_UNEXPECTED_PROXY_AUTH",
        Errorcode::EMPTY_RESPONSE => "ERR_EMPTY_RESPONSE",
        Errorcode::RESPONSE_HEADERS_TOO_BIG => "ERR_RESPONSE_HEADERS_TOO_BIG",
        Errorcode::CACHE_MISS => "ERR_CACHE_MISS",
        Errorcode::INSECURE_RESPONSE => "ERR_INSECURE_RESPONSE",
        _ => {
            let error_code = error_code.get_raw();
            return error_code.to_string();
        }
    }
    .to_string()
}
