//! The document host's implementation of the script runtime's host callbacks.
use crate::dom::Dom;

impl crate::v8::native_ops::NativeHost for Dom {
    fn fetch(
        &self,
        mut request: wega_network::Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<wega_network::Response, String>>>>
    {
        let blocked = self.connect_check(&request.url);
        if !blocked.is_empty() {
            return Box::pin(std::future::ready(Err(blocked)));
        }
        let document = self.document.borrow().url().clone();
        let document_url = document.clone();
        request.origin = document.origin().ascii_serialization();
        request.initiator = Some(document);
        request.top_level = false;
        request.destination = wega_network::ResourceDestination::Fetch;
        request.redirect = "manual".into();
        request.limit = self.limits.resource_bytes.min(wega_network::MAX_BODY);
        self.network.request(request, Some(document_url))
    }

    fn call(&self, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
        Dom::call(self, args)
    }
    fn cookie_header(&self, address: &str, include: bool) -> String {
        let Ok(url) = url::Url::parse(address) else {
            return String::new();
        };
        if !include {
            return String::new();
        }
        let document = self.document.borrow().url().clone();
        let context = self.network.cookies.context(&url, Some(&document), false);
        self.network.cookies.get(&url, false, context)
    }
    fn cookie_response(&self, address: &str, include: bool, value: &str) {
        let Ok(url) = url::Url::parse(address) else {
            return;
        };
        if !include {
            return;
        }
        let document = self.document.borrow().url().clone();
        let context = self.network.cookies.context(&url, Some(&document), false);
        self.network.cookies.set(&url, value, false, context);
    }
    fn document_origin(&self) -> String {
        self.document.borrow().url().origin().ascii_serialization()
    }
    fn ws_headers(&self, address: &str) -> Vec<(String, String)> {
        let document = self.document.borrow().url().clone();
        let mut headers = vec![("origin".to_owned(), document.origin().ascii_serialization())];
        if let Ok(url) = url::Url::parse(address) {
            let context = self.network.cookies.context(&url, Some(&document), false);
            let cookie = self.network.cookies.get(&url, false, context);
            if !cookie.is_empty() {
                headers.push(("cookie".to_owned(), cookie));
            }
        }
        headers
    }
    fn referrer(&self, address: &str) -> String {
        let Ok(url) = url::Url::parse(address) else {
            return String::new();
        };
        let document = self.document.borrow().url().clone();
        crate::network::referrer(&document, &url, None).unwrap_or_default()
    }
    fn connect_check(&self, address: &str) -> String {
        let Ok(url) = url::Url::parse(address) else {
            return "Invalid URL".into();
        };
        let mut policy_url = url.clone();
        if url.scheme() == "ws" {
            let _ = policy_url.set_scheme("http");
        }
        if url.scheme() == "wss" {
            let _ = policy_url.set_scheme("https");
        }
        if matches!(url.scheme(), "http" | "https" | "ws" | "wss")
            && !self
                .network
                .policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .permits(&policy_url)
        {
            // A controlling service worker may still answer from its caches.
            let document = self.document.borrow().url().clone();
            if !self.hubs.service.controls(&self.network, &document) {
                return "Network request blocked by browsing policy".into();
            }
        }
        let document = self.document.borrow().url().clone();
        if crate::network::mixed_content(Some(&document), &url, false) {
            return format!("Mixed content blocked: {url}");
        }
        if !self
            .csp
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .allows_url("connect-src", &url, &document, None)
        {
            return format!("Refused to connect to {url}: Content Security Policy connect-src");
        }
        String::new()
    }
}
