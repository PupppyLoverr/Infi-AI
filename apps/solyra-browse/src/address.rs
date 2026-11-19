//! What the address bar does with what you type.
//!
//! A URL with a web scheme is used as is. Something that looks like a host
//! (`example.com`, `localhost:3000`, `192.168.1.1`) gets `https://`. Anything
//! else is a search. Schemes that can run code or read local files
//! (`javascript:`, `data:`, `file:`) are refused rather than guessed at.

const SEARCH: &str = "https://duckduckgo.com/?q=";

#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Url(String),
    Refused(String),
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn looks_like_host(s: &str) -> bool {
    if s.contains(char::is_whitespace) || s.is_empty() {
        return false;
    }
    let host = s.split(['/', '?', '#']).next().unwrap_or("");
    let host_only = host
        .rsplit_once(':')
        .filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty())
        .map_or(host, |(h, _)| h);
    if host_only.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host_only.contains('.')
        && !host_only.starts_with('.')
        && !host_only.ends_with('.')
        && host_only
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && host_only
            .rsplit('.')
            .next()
            .is_some_and(|tld| !tld.is_empty())
}

pub fn resolve(input: &str) -> Target {
    let t = input.trim();
    if t.is_empty() {
        return Target::Url("about:blank".into());
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Target::Url(t.to_string());
    }
    if lower == "about:blank" {
        return Target::Url(lower);
    }
    for bad in ["javascript:", "data:", "file:", "vbscript:", "blob:"] {
        if lower.starts_with(bad) {
            return Target::Refused(format!(
                "{} addresses are not opened from the address bar",
                bad.trim_end_matches(':')
            ));
        }
    }
    if looks_like_host(t) {
        let scheme = if lower.starts_with("localhost") || t.starts_with("127.") {
            "http"
        } else {
            "https"
        };
        return Target::Url(format!("{scheme}://{t}"));
    }
    Target::Url(format!("{SEARCH}{}", percent_encode(t)))
}

/// Escape text for HTML (the home page lists titles and URLs from the web).
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> String {
        match resolve(s) {
            Target::Url(u) => u,
            Target::Refused(r) => panic!("refused: {r}"),
        }
    }

    #[test]
    fn urls_hosts_and_searches() {
        assert_eq!(
            url("https://example.com/a?b=1"),
            "https://example.com/a?b=1"
        );
        assert_eq!(url("example.com"), "https://example.com");
        assert_eq!(url("  docs.rs/zgx  "), "https://docs.rs/zgx");
        assert_eq!(url("localhost:3000/x"), "http://localhost:3000/x");
        assert_eq!(url("127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(url("192.168.1.1"), "https://192.168.1.1");
        assert_eq!(
            url("rust borrow checker"),
            "https://duckduckgo.com/?q=rust+borrow+checker"
        );
        assert_eq!(
            url("what is 1.5 + 2?"),
            "https://duckduckgo.com/?q=what+is+1.5+%2B+2%3F"
        );
        assert_eq!(url(""), "about:blank");
    }

    #[test]
    fn dangerous_schemes_are_refused() {
        for bad in [
            "javascript:alert(1)",
            "JAVASCRIPT:x",
            "data:text/html,<b>",
            "file:///etc/passwd",
            "blob:abc",
        ] {
            assert!(matches!(resolve(bad), Target::Refused(_)), "{bad}");
        }
    }

    #[test]
    fn not_a_host() {
        assert!(matches!(resolve("hello"), Target::Url(u) if u.starts_with(SEARCH)));
        assert!(
            matches!(resolve("a.b c"), Target::Url(u) if u.starts_with(SEARCH)),
            "spaces mean search"
        );
        assert!(matches!(resolve("end."), Target::Url(u) if u.starts_with(SEARCH)));
    }

    #[test]
    fn escaping() {
        assert_eq!(
            html_escape("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
        assert_eq!(percent_encode("é"), "%C3%A9");
    }
}
