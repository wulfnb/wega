//! Host-enforced request modes and CORS. Renderer adapters may additionally
//! filter response objects, but cannot authorize cookies or response bytes.
use super::*;
fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}
fn credentials(request: &Request, target: &Url) -> bool {
    request.credentials == "include"
        || (request.credentials != "omit"
            && request.origin == target.origin().ascii_serialization())
}
fn check(response: &Response, origin: &str, include: bool) -> Result<(), String> {
    let allowed = header(response, "access-control-allow-origin")
        .ok_or("Cross-origin response lacks Access-Control-Allow-Origin")?;
    if allowed != "*" && allowed != origin {
        return Err("Cross-origin response origin mismatch".into());
    }
    if include
        && (allowed == "*" || header(response, "access-control-allow-credentials") != Some("true"))
    {
        return Err("Cross-origin credentials were not authorized".into());
    }
    Ok(())
}
fn safe_header(name: &str, value: &str) -> bool {
    if value.len() > 128 {
        return false;
    }
    match name.to_ascii_lowercase().as_str() {
        "accept" => !value.bytes().any(|byte| {
            byte < 32 && byte != 9 || b"\"():<>?@[\\]{}".contains(&byte) || byte == 127
        }),
        "accept-language" | "content-language" => value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b" *,-.;=".contains(&byte)),
        "content-type" => [
            "application/x-www-form-urlencoded",
            "multipart/form-data",
            "text/plain",
        ]
        .contains(
            &value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                .as_str(),
        ),
        "range" => value
            .strip_prefix("bytes=")
            .and_then(|value| value.split_once('-'))
            .is_some_and(|(start, end)| {
                !start.is_empty()
                    && start.bytes().all(|value| value.is_ascii_digit())
                    && end.bytes().all(|value| value.is_ascii_digit())
            }),
        _ => false,
    }
}
fn unsafe_headers(request: &Request) -> Vec<String> {
    let mut names: Vec<_> = request
        .headers
        .iter()
        .filter(|(name, value)| {
            ![
                "origin",
                "referer",
                "cookie",
                "user-agent",
                "host",
                "content-length",
                "connection",
                "accept-encoding",
            ]
            .iter()
            .any(|host| name.eq_ignore_ascii_case(host))
                && !safe_header(name, value)
        })
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    names.sort();
    names.dedup();
    names
}
fn fetch_like(request: &Request) -> bool {
    matches!(
        request.destination,
        ResourceDestination::Fetch
            | ResourceDestination::Xhr
            | ResourceDestination::Module
            | ResourceDestination::Font
    )
}
impl Broker {
    pub(crate) fn cors_before(
        &self,
        request: &mut Request,
        session: &Session,
    ) -> Result<(), String> {
        if !session.bound || request.top_level || !fetch_like(request) {
            return Ok(());
        }
        let target = Url::parse(&request.url).map_err(|error| error.to_string())?;
        session
            .policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .permits_request(request, &target)?;
        let cross = target.origin().ascii_serialization() != request.origin;
        if request.mode == "same-origin" && cross {
            return Err("Cross-origin request blocked by same-origin mode".into());
        }
        if request.mode == "no-cors" {
            if !matches!(request.method.as_str(), "GET" | "HEAD" | "POST") {
                return Err("Unsafe method in no-cors request".into());
            }
            request.headers.retain(|(name, value)| {
                ["origin", "referer"]
                    .iter()
                    .any(|host| name.eq_ignore_ascii_case(host))
                    || safe_header(name, value)
            });
            return Ok(());
        }
        if !cross {
            return Ok(());
        }
        let names = unsafe_headers(request);
        if matches!(request.method.as_str(), "GET" | "HEAD" | "POST") && names.is_empty() {
            return Ok(());
        }
        let mut preflight = Request::subresource(&target, ResourceDestination::Fetch, 4096);
        preflight.method = "OPTIONS".into();
        preflight.origin = request.origin.clone();
        preflight.initiator = request.initiator.clone();
        preflight.credentials = "omit".into();
        preflight.redirect = "error".into();
        preflight.mode = "cors".into();
        preflight.headers = vec![
            ("origin".into(), request.origin.clone()),
            (
                "access-control-request-method".into(),
                request.method.clone(),
            ),
        ];
        if !names.is_empty() {
            preflight
                .headers
                .push(("access-control-request-headers".into(), names.join(",")));
        }
        let response = self.client.fetch_with_hsts(
            &preflight,
            &session.policy,
            &session.cookies,
            Some(&self.hsts),
        )?;
        if !(200..300).contains(&response.status) {
            return Err("CORS preflight status rejected".into());
        }
        let include = credentials(request, &target);
        check(&response, &request.origin, include)?;
        let methods: Vec<_> = header(&response, "access-control-allow-methods")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .collect();
        if !matches!(request.method.as_str(), "GET" | "HEAD" | "POST")
            && !methods.contains(&request.method.as_str())
            && !(methods.contains(&"*") && !include)
        {
            return Err("CORS preflight method rejected".into());
        }
        let allowed = header(&response, "access-control-allow-headers")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let allowed: Vec<_> = allowed.split(',').map(str::trim).collect();
        if names.iter().any(|name| {
            !allowed.contains(&name.as_str())
                && !(allowed.contains(&"*") && !include && name != "authorization")
        }) {
            return Err("CORS preflight header rejected".into());
        }
        Ok(())
    }
    pub(crate) fn cors_after(
        &self,
        request: &Request,
        session: &Session,
        response: &mut Response,
    ) -> Result<(), String> {
        if !session.bound {
            return Ok(());
        }
        // Network-only credentials remain in the host cookie jar.
        response.headers.retain(|(name, _)| {
            !name.eq_ignore_ascii_case("set-cookie") && !name.eq_ignore_ascii_case("set-cookie2")
        });
        if request.top_level {
            return Ok(());
        }
        let target = Url::parse(&response.url).map_err(|error| error.to_string())?;
        let cross = target.origin().ascii_serialization() != request.origin;
        if fetch_like(request) && cross {
            if request.mode == "no-cors" {
                response.body.clear();
                response
                    .headers
                    .retain(|(name, _)| name.eq_ignore_ascii_case("location"));
            } else {
                check(response, &request.origin, credentials(request, &target))?;
            }
        } else if cross {
            // A destination label cannot turn credentialed HTML/XML/JSON into an
            // image/font/style channel readable by a compromised content process.
            let mime = header(response, "content-type")
                .unwrap_or_default()
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            let sensitive = mime == "text/html"
                || mime == "application/json"
                || mime.ends_with("+json")
                || mime == "text/xml"
                || mime == "application/xml"
                || mime.ends_with("+xml") && mime != "image/svg+xml";
            if sensitive {
                return Err("Cross-origin document data cannot be read as a subresource".into());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_credentials_and_unsafe_no_cors_headers_are_rejected() {
        let response = Response {
            url: "https://other.example/".into(),
            status: 200,
            headers: vec![("access-control-allow-origin".into(), "*".into())],
            redirected: false,
            body: vec![],
        };
        assert!(check(&response, "https://site.example", true).is_err());
        assert!(check(&response, "https://site.example", false).is_ok());
        assert!(!safe_header("authorization", "secret"));
        assert!(!safe_header("content-type", "application/json"));
        assert!(safe_header("content-type", "text/plain"));
    }
}
