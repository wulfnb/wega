//! Pure potentially-trustworthy-origin check (secure-context exposure decision).
use url::{Host, Url};

/// `true` when documents loaded from `url` are potentially trustworthy.
pub fn trustworthy(url: &Url) -> bool {
    if matches!(url.scheme(), "https" | "wss" | "file") {
        return true;
    }
    if !matches!(url.scheme(), "http" | "ws") {
        return false;
    }
    match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain(host)) => {
            let host = host.trim_end_matches('.');
            host == "localhost" || host.ends_with(".localhost")
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_and_loopback_determine_potential_trust() {
        for address in [
            "https://example.com/",
            "http://localhost/",
            "http://a.localhost/",
            "http://127.0.0.2/",
            "http://[::1]/",
            "file:///fixture.html",
        ] {
            assert!(trustworthy(&Url::parse(address).unwrap()), "{address}");
        }
        for address in [
            "http://example.com/",
            "http://localhost.example/",
            "http://192.168.1.1/",
            "data:text/html,x",
            "about:blank",
        ] {
            assert!(!trustworthy(&Url::parse(address).unwrap()), "{address}");
        }
    }
}
