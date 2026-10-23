//! Limits and redaction for values that come from pages. Titles and URLs are page-controlled, so
//! they are capped before they leave the engine, and logs see only the scheme and host of a URL.

/// Longest title the engine forwards, in characters.
pub const TITLE_MAX_CHARS: usize = 512;
/// Longest URL the engine forwards, in bytes.
pub const URL_MAX_BYTES: usize = 8 * 1024;

/// The longest prefix of `s` with at most `max` characters.
pub fn cap_title(s: &str) -> String {
    s.chars().take(TITLE_MAX_CHARS).collect()
}

/// The longest prefix of `s` with at most `URL_MAX_BYTES` bytes, cut on a character boundary.
pub fn cap_url(s: &str) -> String {
    if s.len() <= URL_MAX_BYTES {
        return s.to_owned();
    }
    let mut end = URL_MAX_BYTES;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// Scheme and host (with port) only. Never the path, query, fragment, credentials or a data
/// payload. Used for every URL in a log line.
pub fn redact_url(url: &str) -> String {
    const HOST_MAX: usize = 128;
    let Some(colon) = url.find(':') else {
        return "<no-scheme>".to_owned();
    };
    let scheme = &url[..colon];
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return "<invalid-scheme>".to_owned();
    }
    let scheme = scheme.to_ascii_lowercase();
    let networked = matches!(
        scheme.as_str(),
        "http" | "https" | "app" | "ws" | "wss" | "ftp"
    );
    if !networked {
        return format!("{scheme}:<redacted>");
    }
    let Some(rest) = url[colon..].strip_prefix("://") else {
        return format!("{scheme}://<no-host>");
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    // Drop userinfo: everything up to the last '@'.
    let host = authority.rsplit('@').next().unwrap_or("");
    let host: String = host.chars().take(HOST_MAX).collect();
    format!("{scheme}://{host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_cap_on_characters() {
        let long: String = "é".repeat(TITLE_MAX_CHARS + 10);
        let capped = cap_title(&long);
        assert_eq!(capped.chars().count(), TITLE_MAX_CHARS);
        assert_eq!(cap_title("short"), "short");
    }

    #[test]
    fn urls_cap_on_a_character_boundary() {
        // Two-byte characters: the cut must not split one.
        let long = "é".repeat(URL_MAX_BYTES);
        let capped = cap_url(&long);
        assert!(capped.len() <= URL_MAX_BYTES);
        assert!(capped.len() >= URL_MAX_BYTES - 1);
        assert_eq!(cap_url("https://a.test/"), "https://a.test/");
    }

    #[test]
    fn logs_keep_scheme_and_host_only() {
        assert_eq!(
            redact_url("https://user:pw@example.com:8443/a/b?token=1#frag"),
            "https://example.com:8443"
        );
        assert_eq!(redact_url("app://ui/api/v1/rpc?x=1"), "app://ui");
        assert_eq!(
            redact_url("http://127.0.0.1:9000/report?name=x"),
            "http://127.0.0.1:9000"
        );
        assert_eq!(
            redact_url("data:text/html;base64,PGgxPnNlY3JldDwvaDE+"),
            "data:<redacted>"
        );
        assert_eq!(
            redact_url("javascript:alert(document.cookie)"),
            "javascript:<redacted>"
        );
        assert_eq!(redact_url("https:///nohost"), "https://");
        assert_eq!(redact_url("no scheme here"), "<no-scheme>");
        assert_eq!(redact_url("bad scheme:x"), "<invalid-scheme>");
        assert_eq!(redact_url("plain"), "<no-scheme>");
    }

    #[test]
    fn logged_hosts_are_capped() {
        let host = "a".repeat(500);
        let redacted = redact_url(&format!("https://{host}/x"));
        assert_eq!(redacted.len(), "https://".len() + 128);
    }
}
