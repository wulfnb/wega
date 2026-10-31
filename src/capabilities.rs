//! Browser policies and unprivileged internal documents. No page receives a native IPC bridge.
use servo::protocol_handler::*;
use std::{
    collections::VecDeque,
    sync::{Arc, RwLock},
};

pub use wega::model::Policy;
pub fn intercept(policy: &Policy, load: servo::WebResourceLoad) {
    let url = &load.request().url;
    if policy.offline || !policy.permits(url) {
        let mut response = servo::WebResourceResponse::new(url.clone());
        response.status_code = http::StatusCode::FORBIDDEN;
        response.headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("text/html; charset=utf-8"),
        );
        let body = if policy.offline {
            "Offline mode is enabled. Turn it off in Browser settings."
        } else {
            "This request was blocked by your browser host rules."
        };
        let mut stream = load.intercept(response);
        stream.send_body_data(
            format!(
                "<!doctype html><title>Request blocked</title><h1>Request blocked</h1><p>{body}</p>"
            )
            .into_bytes(),
        );
        stream.finish();
    }
}

pub enum Decision {
    Servo(servo::AllowOrDenyRequest),
    External(url::Url),
    Protocol(
        servo::AllowOrDenyRequest,
        servo::protocol_handler::ProtocolHandlerRegistration,
    ),
}
pub struct Prompt {
    /// Requesting origin; bounds how many prompts one site can queue.
    pub origin: String,
    pub title: String,
    pub detail: String,
    pub decision: Decision,
}
impl Prompt {
    fn deny(self) {
        match self.decision {
            Decision::Servo(request) | Decision::Protocol(request, _) => request.deny(),
            Decision::External(_) => {}
        }
    }
}

/// Site-initiated dialogs waiting for the user. Every queue is bounded in total and
/// per origin (or per page for embedder controls); requests that do not fit are
/// denied at once, so a page cannot grow the UI's memory or bury other sites' dialogs.
#[derive(Default)]
pub struct Dialogs {
    pub prompts: VecDeque<Prompt>,
    pub permissions: wega::permissions::Permissions<crate::permission_adapter::Permission>,
    pub authentications: VecDeque<servo::AuthenticationRequest>,
    pub controls: VecDeque<(servo::WebViewId, servo::EmbedderControl)>,
}
impl Dialogs {
    pub fn is_empty(&self) -> bool {
        self.prompts.is_empty()
            && self.permissions.is_empty()
            && self.authentications.is_empty()
            && self.controls.is_empty()
    }
    pub fn push_prompt(&mut self, prompt: Prompt) -> bool {
        if wega::permissions::admit(
            self.prompts.iter().map(|p| p.origin.as_str()),
            &prompt.origin,
        ) {
            self.prompts.push_back(prompt);
            true
        } else {
            prompt.deny();
            false
        }
    }
    /// Dropping an unanswered authentication request cancels it in Servo.
    pub fn push_authentication(&mut self, request: servo::AuthenticationRequest) -> bool {
        let origins: Vec<String> = self
            .authentications
            .iter()
            .map(|r| r.url().origin().ascii_serialization())
            .collect();
        let admitted = wega::permissions::admit(
            origins.iter().map(String::as_str),
            &request.url().origin().ascii_serialization(),
        );
        if admitted {
            self.authentications.push_back(request);
        }
        admitted
    }
    /// Dropping an embedder control answers it with its default (cancel) response.
    pub fn push_control(
        &mut self,
        owner: servo::WebViewId,
        control: servo::EmbedderControl,
    ) -> bool {
        let owners: Vec<String> = self
            .controls
            .iter()
            .map(|(id, _)| format!("{id:?}"))
            .collect();
        let admitted =
            wega::permissions::admit(owners.iter().map(String::as_str), &format!("{owner:?}"));
        if admitted {
            self.controls.push_back((owner, control));
        }
        admitted
    }
    pub fn has_controls_for(&self, owner: servo::WebViewId) -> bool {
        self.controls.iter().any(|(id, _)| *id == owner)
    }
    /// Cancel controls of a page that is being closed or unloaded.
    pub fn forget(&mut self, owner: servo::WebViewId) {
        self.controls.retain(|(id, _)| *id != owner);
    }
    pub fn clear(&mut self) {
        while let Some(prompt) = self.prompts.pop_front() {
            prompt.deny();
        }
        self.permissions.clear();
        self.authentications.clear();
        self.controls.clear();
    }
}
#[derive(Default)]
pub struct State {
    pub settings: bool,
    pub bluetooth: VecDeque<servo::BluetoothDeviceSelectionRequest>,
    pub traversals: std::collections::HashMap<servo::WebViewId, servo::TraversalId>,
    pub media: std::collections::HashMap<servo::WebViewId, crate::native_media::MediaInfo>,
    pub media_owner: Option<servo::WebViewId>,
    pub site_info: bool,
    pub clear_data: bool,
    /// Second step before clearing session cookies (it signs out of every site).
    pub confirm_session_clear: bool,
    /// Cookie values can hold session tokens; they stay hidden unless asked for.
    pub show_cookie_values: bool,
    pub delete_site: Option<String>,
    pub settings_section: usize,
    pub diagnostics: bool,
    pub sites: Vec<(String, String)>,
    pub cookies: Vec<String>,
    pub script: Option<std::rc::Rc<servo::UserScript>>,
    pub find_open: bool,
    pub find: String,
    /// "3 of 12" / "No matches", written by the page script's callback.
    pub find_status: std::rc::Rc<std::cell::RefCell<String>>,
    /// The find field takes focus once each time the bar opens.
    pub find_focused: bool,
    pub script_source: String,
}

pub type Documents = Arc<RwLock<std::collections::HashMap<String, String>>>;
pub struct InternalPages(pub Documents);
impl ProtocolHandler for InternalPages {
    fn load<'a>(
        &'a self,
        request: &'a mut Request,
        _: &mut DoneChannel,
        _: &FetchContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send + 'a>> {
        let url = request.current_url();
        let page = url.host_str().unwrap_or("newtab");
        let body = self
            .0
            .read()
            .unwrap()
            .get(page)
            .cloned()
            .unwrap_or_else(|| "<h1>Page not found</h1>".into());
        let mut response = Response::new(url, ResourceFetchTiming::new(request.timing_type()));
        response.headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("text/html; charset=utf-8"),
        );
        response.headers.insert(http::header::CONTENT_SECURITY_POLICY, http::HeaderValue::from_static("default-src 'none'; img-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action https:; frame-ancestors 'none'"));
        *response.body.lock() = ResponseBody::Done(body.into_bytes());
        Box::pin(std::future::ready(response))
    }
}
pub fn html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn theme_stylesheet(palette: crate::theme::Palette) -> String {
    let hex = |c: egui::Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
    format!(
        ":root{{color-scheme:{};--wega-text:{};--wega-background:{};--wega-accent:{};--wega-surface:{};--wega-border:{};--wega-selected:{};--wega-muted:{};--wega-danger:{};--wega-on-accent:{}}}",
        if palette.dark { "dark" } else { "light" },
        hex(palette.text),
        hex(palette.inset),
        hex(palette.accent),
        hex(palette.surface),
        hex(palette.border),
        hex(palette.selected),
        hex(palette.muted),
        hex(palette.danger),
        hex(palette.on_accent)
    )
}
pub fn document(title: &str, body: &str, palette: crate::theme::Palette) -> String {
    let variables = theme_stylesheet(palette);
    format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>{}</title><style id='wega-theme'>{variables}</style><style>html{{font:16px system-ui;color:var(--wega-text);background:var(--wega-background)}}body{{max-width:760px;margin:40px auto;padding:24px}}h1{{font-size:1.9rem;margin:.5rem 0 1.25rem}}h2{{font-size:1rem;color:var(--wega-muted);font-weight:600;margin:2rem 0 .75rem}}a{{color:var(--wega-accent)}}input,button{{font:inherit;padding:12px;border-radius:8px;max-width:100%;box-sizing:border-box;background:var(--wega-surface);color:var(--wega-text);border:1px solid var(--wega-border)}}button{{background:var(--wega-accent);color:var(--wega-on-accent);border-color:var(--wega-accent);font-weight:600;cursor:pointer}}.search-controls{{display:flex;align-items:center;gap:8px}}.search-controls input{{width:calc(100% - 120px);min-width:0}}.search-controls button{{width:112px;flex:none}}.search-controls input,.search-controls button{{height:48px}}.hint,.empty,.address{{color:var(--wega-muted)}}.hint{{font-size:.85rem;margin:.5rem 0 0}}.tiles{{display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:12px}}.tile{{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:8px;min-height:92px;padding:14px;text-align:center;color:var(--wega-text);background:var(--wega-surface);border:1px solid var(--wega-border);border-radius:12px;text-decoration:none}}.tile:hover,.tile:focus-visible{{background:var(--wega-selected)}}.tile span{{overflow:hidden;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow-wrap:anywhere;font-size:.9rem}}.recent,.entries{{list-style:none;padding:0}}.recent li,.entries li{{display:flex;gap:12px;align-items:center;margin:0;padding:8px 10px;border-radius:8px}}.recent li:hover,.entries li:hover{{background:var(--wega-surface)}}.entries a{{display:block;text-decoration:none;color:var(--wega-text)}}.address{{display:block;font-size:.8rem;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}.page-icon{{font-size:24px}}nav{{display:flex;flex-wrap:wrap;gap:6px}}nav a{{padding:6px 12px;border-radius:999px;text-decoration:none;background:var(--wega-surface)}}:focus-visible{{outline:3px solid var(--wega-accent);outline-offset:4px}}p{{line-height:1.6}}</style></head><body><nav aria-label='Browser pages'><a href='wega://newtab'>New tab</a><a href='wega://history'>History</a><a href='wega://bookmarks'>Bookmarks</a><a href='wega://settings'>Settings</a></nav><main><h1>{}</h1>{body}</main></body></html>",
        html(title),
        html(title)
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domain_rules_respect_boundaries_and_case() {
        let p = Policy {
            blocked_hosts: "tracker.example".into(),
            allowed_hosts: "example.com".into(),
            ..Default::default()
        };
        assert!(p.permits(&url::Url::parse("https://www.example.com/").unwrap()));
        assert!(!p.permits(&url::Url::parse("https://evil-example.com/").unwrap()));
        assert!(!p.permits(&url::Url::parse("https://tracker.example/").unwrap()));
        assert!(!p.permits(&url::Url::parse("https://example.com.evil.test/").unwrap()));
    }
    #[test]
    fn html_is_inert() {
        assert_eq!(html("<script>\"&'"), "&lt;script&gt;&quot;&amp;&#39;");
    }
}
