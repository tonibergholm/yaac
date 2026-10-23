//! Checks on the URLs a caller asks the core to load.

use crate::error::bad_request;
use yaac_api::ApiError;

pub(crate) const NEW_TAB_URL: &str = "about:blank";
pub(crate) const SIDEBAR_URL: &str = "app://ui/sidebar.html";
const UI_PREFIX: &str = "app://ui/";

/// Accepts `http`, `https`, `about:blank` and `app://ui/...`. The `app://ui` pages belong to the
/// system, so only a caller with `allow_app` may load them.
pub(crate) fn check(url: &str, allow_app: bool) -> Result<(), ApiError> {
    if url.is_empty() {
        return Err(bad_request("the url is empty"));
    }
    if url.chars().any(char::is_whitespace) {
        return Err(bad_request("the url contains whitespace"));
    }
    if url == NEW_TAB_URL {
        return Ok(());
    }
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        return if rest.is_empty() {
            Err(bad_request("the url has no host"))
        } else {
            Ok(())
        };
    }
    if url.starts_with(UI_PREFIX) {
        return if allow_app {
            Ok(())
        } else {
            Err(bad_request("app://ui pages are reserved for the system"))
        };
    }
    Err(bad_request(
        "only http, https, about:blank and app://ui urls are accepted",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaac_api::ErrorCode;

    #[test]
    fn web_and_blank_urls_are_accepted_for_everyone() {
        for url in [
            "https://example.com/a?b=c",
            "http://localhost:8080",
            NEW_TAB_URL,
        ] {
            assert!(check(url, false).is_ok(), "{url}");
            assert!(check(url, true).is_ok(), "{url}");
        }
    }

    #[test]
    fn malformed_urls_are_bad_requests() {
        for url in [
            "",
            "https://",
            "ftp://x",
            "javascript:alert(1)",
            "https://a b",
            "about:config",
        ] {
            assert_eq!(
                check(url, true).unwrap_err().code,
                ErrorCode::BadRequest,
                "{url}"
            );
        }
    }

    #[test]
    fn app_ui_urls_are_reserved_for_the_system() {
        assert_eq!(
            check(SIDEBAR_URL, false).unwrap_err().code,
            ErrorCode::BadRequest
        );
        assert!(check(SIDEBAR_URL, true).is_ok());
    }
}
