use cef::*;

pub fn resource_response_filter(
    _browser: Option<&mut Browser>,
    _frame: Option<&mut Frame>,
    _request: Option<&mut Request>,
    _response: Option<&mut Response>,
) -> Option<ResponseFilter> {
    todo!("Implement resource_response_filter")
}
