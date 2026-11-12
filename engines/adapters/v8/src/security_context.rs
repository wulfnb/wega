//! Secure-context exposure belongs to the document, not the host's permissions.
use url::Url;
pub use wega_security::secure_context::trustworthy;
pub fn configure(runtime: &mut deno_core::JsRuntime, url: &Url) -> Result<(), String> {
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
