//! Checks on the URLs the core loads into content views.

use crate::error::bad_request;
use yaac_api::ApiError;
use yaac_model::Node;

pub(crate) const NEW_TAB_URL: &str = "about:blank";
pub(crate) const SIDEBAR_URL: &str = "app://ui/sidebar.html";

/// Accepts `http`, `https` and `about:blank`. Every other scheme is refused, `app://` included,
/// for every principal: the sidebar is created only by the core's own startup.
pub(crate) fn check(url: &str) -> Result<(), ApiError> {
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
    Err(bad_request(
        "only http, https and about:blank urls are accepted",
    ))
}

/// The URL a content view starts on. A stored URL that fails `check` (for example one the engine
/// reported as `javascript:` or `file:`) is replaced by `about:blank`; the stored value stays as is.
pub(crate) fn load_url(node: &Node) -> String {
    match &node.url {
        Some(url) if check(url).is_ok() => url.clone(),
        _ => NEW_TAB_URL.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaac_api::ErrorCode;

    #[test]
    fn web_and_blank_urls_are_accepted() {
        for url in [
            "https://example.com/a?b=c",
            "http://localhost:8080",
            NEW_TAB_URL,
        ] {
            assert!(check(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn malformed_urls_are_bad_requests() {
        for url in [
            "",
            "https://",
            "ftp://x",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "https://a b",
            "about:config",
        ] {
            assert_eq!(check(url).unwrap_err().code, ErrorCode::BadRequest, "{url}");
        }
    }

    #[test]
    fn app_urls_are_refused_for_everyone() {
        assert_eq!(check(SIDEBAR_URL).unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(
            check("app://ui/api").unwrap_err().code,
            ErrorCode::BadRequest
        );
    }
}
