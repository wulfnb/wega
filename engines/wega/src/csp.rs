//! Content Security Policy (CSP Level 3, enforced subset): fetch directives for
//! scripts, styles, images, fonts and connections, inline-script nonces and
//! hashes, and eval. Reporting and frame directives are not implemented.
use sha2::Digest;
use url::Url;

#[derive(Clone, Debug, Default)]
struct Policy {
    directives: Vec<(String, Vec<String>)>,
}
impl Policy {
    fn parse(text: &str) -> Self {
        let mut directives: Vec<(String, Vec<String>)> = Vec::new();
        for part in text.split(';') {
            let mut tokens = part.split_ascii_whitespace();
            let Some(name) = tokens.next() else { continue };
            let name = name.to_ascii_lowercase();
            // The first occurrence of a directive wins.
            if directives.iter().any(|(existing, _)| *existing == name) {
                continue;
            }
            directives.push((name, tokens.map(str::to_owned).collect()));
        }
        Self { directives }
    }
    fn get(&self, name: &str) -> Option<&[String]> {
        self.directives
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
    }
    /// The effective source list for a directive, following CSP's fallback lists.
    fn effective(&self, directive: &str) -> Option<&[String]> {
        let chain: &[&str] = match directive {
            "script-src-elem" => &["script-src-elem", "script-src", "default-src"],
            "script-src-attr" => &["script-src-attr", "script-src", "default-src"],
            "style-src-elem" => &["style-src-elem", "style-src", "default-src"],
            "img-src" => &["img-src", "default-src"],
            "font-src" => &["font-src", "default-src"],
            "connect-src" => &["connect-src", "default-src"],
            "script-src" => &["script-src", "default-src"],
            other => return self.get(other),
        };
        chain.iter().find_map(|name| self.get(name))
    }
}

/// One document's policies; every policy must allow a request.
#[derive(Clone, Debug, Default)]
pub(crate) struct Csp {
    policies: Vec<Policy>,
}

fn matches_source(source: &str, url: &Url, document: &Url) -> bool {
    let lower = source.to_ascii_lowercase();
    match lower.as_str() {
        "*" => {
            // `*` does not match data:, blob: or filesystem: (CSP3 §6.7.2.6).
            !matches!(url.scheme(), "data" | "blob" | "filesystem")
        }
        "'self'" => {
            url.origin() == document.origin()
                || (document.scheme() == "http"
                    && url.scheme() == "https"
                    && url.host() == document.host())
        }
        "'none'" => false,
        _ if lower.starts_with('\'') => false,
        _ if lower.ends_with(':') && !lower.contains('/') => {
            let scheme = &lower[..lower.len() - 1];
            url.scheme() == scheme
                || (scheme == "http" && url.scheme() == "https")
                || (scheme == "ws" && url.scheme() == "wss")
        }
        _ => matches_host_source(&lower, url, document),
    }
}
fn matches_host_source(source: &str, url: &Url, document: &Url) -> bool {
    let (scheme, rest) = match source.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, source),
    };
    let scheme_ok = match scheme {
        Some(scheme) => {
            url.scheme() == scheme
                || (scheme == "http" && url.scheme() == "https")
                || (scheme == "ws" && url.scheme() == "wss")
        }
        None => match document.scheme() {
            "http" => matches!(url.scheme(), "http" | "https"),
            other => url.scheme() == other || (other == "https" && url.scheme() == "https"),
        },
    };
    if !scheme_ok {
        return false;
    }
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], Some(&rest[at..])),
        None => (rest, None),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(']') || authority.starts_with('[') => {
            (host, Some(port))
        }
        _ => (authority, None),
    };
    let Some(url_host) = url.host_str() else {
        return false;
    };
    let url_host = url_host.to_ascii_lowercase();
    let host_ok = if let Some(suffix) = host.strip_prefix("*.") {
        url_host.ends_with(&format!(".{suffix}"))
    } else {
        host == "*" || url_host == host
    };
    if !host_ok {
        return false;
    }
    let port_ok = match port {
        Some("*") => true,
        Some(port) => {
            url.port_or_known_default()
                .map(|p| p.to_string())
                .as_deref()
                == Some(port)
        }
        None => {
            let default = match scheme.unwrap_or(url.scheme()) {
                "http" | "ws" => 80,
                "https" | "wss" => 443,
                _ => 0,
            };
            url.port_or_known_default() == Some(default)
                // An http: source also matches https on the default port upgrade.
                || (url.scheme() == "https" && url.port_or_known_default() == Some(443))
        }
    };
    if !port_ok {
        return false;
    }
    match path {
        None | Some("/") => true,
        Some(path) if path.ends_with('/') => url.path().starts_with(path),
        Some(path) => url.path() == path,
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Csp {
    /// Add one Content-Security-Policy header or meta value (comma-separated
    /// values are separate policies).
    pub fn add(&mut self, value: &str) {
        for policy in value.split(',') {
            let policy = Policy::parse(policy);
            if !policy.directives.is_empty() && self.policies.len() < 16 {
                self.policies.push(policy);
            }
        }
    }
    /// Whether a URL may be fetched for a directive, optionally by an element nonce.
    pub fn allows_url(
        &self,
        directive: &str,
        url: &Url,
        document: &Url,
        nonce: Option<&str>,
    ) -> bool {
        self.policies.iter().all(|policy| {
            let Some(sources) = policy.effective(directive) else {
                return true;
            };
            if directive.starts_with("script")
                && let Some(nonce) = nonce.filter(|n| !n.is_empty())
                && sources.iter().any(|s| s == &format!("'nonce-{nonce}'"))
            {
                return true;
            }
            // 'strict-dynamic' ignores host lists; only nonced/hashed or trusted
            // (dynamically inserted) scripts run.
            if directive.starts_with("script")
                && sources
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case("'strict-dynamic'"))
            {
                return false;
            }
            sources
                .iter()
                .any(|source| matches_source(source, url, document))
        })
    }
    /// Scripts inserted by already-trusted script under 'strict-dynamic'.
    pub fn allows_dynamic_script(&self, url: &Url, document: &Url) -> bool {
        self.policies.iter().all(|policy| {
            let Some(sources) = policy.effective("script-src-elem") else {
                return true;
            };
            sources
                .iter()
                .any(|s| s.eq_ignore_ascii_case("'strict-dynamic'"))
                || sources
                    .iter()
                    .any(|source| matches_source(source, url, document))
        })
    }
    /// Inline `<script>` text, by nonce, hash or 'unsafe-inline'.
    pub fn allows_inline_script(&self, nonce: Option<&str>, source: &str) -> bool {
        self.allows_inline("script-src-elem", nonce, source)
    }
    pub fn allows_inline(&self, directive: &str, nonce: Option<&str>, source: &str) -> bool {
        self.policies.iter().all(|policy| {
            let Some(sources) = policy.effective(directive) else {
                return true;
            };
            if let Some(nonce) = nonce.filter(|n| !n.is_empty())
                && sources.iter().any(|s| s == &format!("'nonce-{nonce}'"))
            {
                return true;
            }
            let hashes: [(&str, String); 3] = [
                ("sha256", base64(&sha2::Sha256::digest(source.as_bytes()))),
                ("sha384", base64(&sha2::Sha384::digest(source.as_bytes()))),
                ("sha512", base64(&sha2::Sha512::digest(source.as_bytes()))),
            ];
            if sources.iter().any(|s| {
                hashes
                    .iter()
                    .any(|(algorithm, digest)| *s == format!("'{algorithm}-{digest}'"))
            }) {
                return true;
            }
            // 'unsafe-inline' is ignored when a nonce, hash or 'strict-dynamic' is present.
            let strict = sources.iter().any(|s| {
                let s = s.to_ascii_lowercase();
                s.starts_with("'nonce-")
                    || s.starts_with("'sha256-")
                    || s.starts_with("'sha384-")
                    || s.starts_with("'sha512-")
                    || s == "'strict-dynamic'"
            });
            !strict
                && sources
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case("'unsafe-inline'"))
        })
    }
    pub fn allows_eval(&self) -> bool {
        self.policies.iter().all(|policy| {
            policy.effective("script-src").is_none_or(|sources| {
                sources.iter().any(|s| {
                    s.eq_ignore_ascii_case("'unsafe-eval'")
                        || s.eq_ignore_ascii_case("'wasm-unsafe-eval'")
                })
            })
        })
    }
    /// Blitz subresources carry no destination, so classify by extension; an
    /// unknown extension is allowed if any of the image/style/font lists allow it.
    pub fn allows_subresource(&self, url: &Url, document: &Url) -> bool {
        if self.policies.is_empty() {
            return true;
        }
        let path = url.path().to_ascii_lowercase();
        let extension = path
            .rsplit_once('.')
            .map(|(_, e)| e)
            .unwrap_or_default();
        let directives: &[&str] = match extension {
            "css" => &["style-src-elem"],
            "woff" | "woff2" | "ttf" | "otf" | "eot" => &["font-src"],
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "ico" | "bmp" => {
                &["img-src"]
            }
            _ if path.contains("/css/") || path.ends_with("/css") || path.contains("stylesheet") => {
                &["style-src-elem"]
            }
            _ => &["img-src", "style-src-elem", "font-src"],
        };
        directives
            .iter()
            .any(|directive| self.allows_url(directive, url, document, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }
    #[test]
    fn source_lists_self_hosts_schemes_and_fallbacks() {
        let doc = url("https://app.example/page");
        let mut csp = Csp::default();
        csp.add("default-src 'self'; script-src 'self' https://cdn.example/js/ *.static.example; img-src data: https:");
        assert!(csp.allows_url(
            "script-src-elem",
            &url("https://app.example/a.js"),
            &doc,
            None
        ));
        assert!(csp.allows_url(
            "script-src-elem",
            &url("https://cdn.example/js/a.js"),
            &doc,
            None
        ));
        assert!(!csp.allows_url(
            "script-src-elem",
            &url("https://cdn.example/other/a.js"),
            &doc,
            None
        ));
        assert!(csp.allows_url(
            "script-src-elem",
            &url("https://x.static.example/a.js"),
            &doc,
            None
        ));
        assert!(!csp.allows_url(
            "script-src-elem",
            &url("https://evil.example/a.js"),
            &doc,
            None
        ));
        assert!(csp.allows_url("img-src", &url("data:image/png,x"), &doc, None));
        assert!(csp.allows_url("img-src", &url("https://any.example/x.png"), &doc, None));
        assert!(!csp.allows_url("connect-src", &url("https://api.other/"), &doc, None));
        assert!(csp.allows_url("connect-src", &url("https://app.example/api"), &doc, None));
        assert!(!csp.allows_eval());
    }
    #[test]
    fn inline_scripts_need_nonce_hash_or_unsafe_inline() {
        let source = "alert(1)";
        let digest = base64(&sha2::Sha256::digest(source.as_bytes()));
        let mut csp = Csp::default();
        csp.add(&format!(
            "script-src 'nonce-abc' 'sha256-{digest}' 'unsafe-inline'"
        ));
        assert!(csp.allows_inline_script(Some("abc"), "anything"));
        assert!(csp.allows_inline_script(None, source));
        assert!(
            !csp.allows_inline_script(None, "other()"),
            "unsafe-inline is ignored beside a nonce"
        );
        let mut open = Csp::default();
        open.add("script-src 'unsafe-inline' 'unsafe-eval'");
        assert!(open.allows_inline_script(None, "other()"));
        assert!(open.allows_eval());
        assert!(Csp::default().allows_inline_script(None, "x"));
    }
    #[test]
    fn every_policy_must_allow_and_strict_dynamic_ignores_hosts() {
        let doc = url("https://a.example/");
        let mut csp = Csp::default();
        csp.add("script-src https:, script-src 'self'");
        assert!(csp.allows_url(
            "script-src-elem",
            &url("https://a.example/x.js"),
            &doc,
            None
        ));
        assert!(!csp.allows_url(
            "script-src-elem",
            &url("https://b.example/x.js"),
            &doc,
            None
        ));
        let mut strict = Csp::default();
        strict.add("script-src 'nonce-n' 'strict-dynamic' https:");
        assert!(!strict.allows_url(
            "script-src-elem",
            &url("https://b.example/x.js"),
            &doc,
            None
        ));
        assert!(strict.allows_url(
            "script-src-elem",
            &url("https://b.example/x.js"),
            &doc,
            Some("n")
        ));
        assert!(strict.allows_dynamic_script(&url("https://b.example/x.js"), &doc));
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(base64(b"ab"), "YWI=");
    }
    #[test]
    fn subresource_css_paths_match_style_src() {
        let doc = url("https://trak.in/stories/");
        let mut csp = Csp::default();
        csp.add("default-src 'none'; style-src-elem https://trak.in; img-src 'none'");
        assert!(csp.allows_subresource(
            &url("https://trak.in/stories/site-skin/css/awesome-css"),
            &doc
        ));
        assert!(!csp.allows_subresource(
            &url("https://trak.in/images/photo.png"),
            &doc
        ));
    }
}
