use cef::SchemeOptions;
use tests_shared::common::client_app::ClientAppCustomScheme;

pub fn get_custom_schemes() -> Vec<ClientAppCustomScheme> {
    vec![ClientAppCustomScheme::new(
        "client",
        &[SchemeOptions::STANDARD, SchemeOptions::CORS_ENABLED],
    )]
}
