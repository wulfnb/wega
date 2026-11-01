//! Secure-context exposure belongs to the document, not the host's permissions.
use url::{Host, Url};
pub(crate) fn trustworthy(url: &Url) -> bool {
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
pub(crate) fn configure(runtime: &mut deno_core::JsRuntime, url: &Url) -> Result<(), String> {
    use deno_core::v8;
    deno_core::scope!(scope, runtime);
    let global = scope.get_current_context().global(scope);
    let secure = trustworthy(url);
    let key = v8::String::new(scope, "isSecureContext").ok_or("Cannot allocate exposure name")?;
    let flag = v8::Boolean::new(scope, secure);
    if global.define_own_property(
        scope,
        key.into(),
        flag.into(),
        v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
    ) != Some(true)
    {
        return Err("Cannot configure document secure context".into());
    }
    if !secure {
        let key = v8::String::new(scope, "crypto").ok_or("Cannot allocate crypto name")?;
        let crypto = global
            .get(scope, key.into())
            .and_then(|value| value.to_object(scope))
            .ok_or("Missing Crypto object")?;
        // Remove the privileged operations from the actual prototype as well:
        // shadowing only the instance leaves its inherited getter callable via
        // Object.getPrototypeOf(crypto) or Crypto.prototype.
        let prototype = crypto
            .get_prototype(scope)
            .and_then(|value| value.to_object(scope));
        for target in std::iter::once(crypto).chain(prototype) {
            for name in ["subtle", "randomUUID"] {
                let key = v8::String::new(scope, name).ok_or("Cannot allocate exposure name")?;
                let absent = v8::undefined(scope);
                if target.define_own_property(
                    scope,
                    key.into(),
                    absent.into(),
                    v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
                ) != Some(true)
                {
                    return Err("Cannot restrict insecure Crypto exposure".into());
                }
            }
        }
        let key = v8::String::new(scope, "CryptoKey").ok_or("Cannot allocate exposure name")?;
        global.delete(scope, key.into());
    }
    Ok(())
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
