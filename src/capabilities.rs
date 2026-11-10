//! Native browser policy and bounded external-application prompts.
use std::collections::VecDeque;
pub enum Decision {
    External(url::Url),
}
pub struct Prompt {
    pub origin: String,
    pub title: String,
    pub detail: String,
    pub decision: Decision,
}
#[derive(Default)]
pub struct Dialogs {
    pub prompts: VecDeque<Prompt>,
    pub permissions: wega::permissions::Permissions<crate::permission_adapter::Permission>,
}
impl Dialogs {
    pub fn is_empty(&self) -> bool {
        self.prompts.is_empty() && self.permissions.is_empty()
    }
    pub fn push_prompt(&mut self, prompt: Prompt) -> bool {
        if wega::permissions::admit(
            self.prompts.iter().map(|prompt| prompt.origin.as_str()),
            &prompt.origin,
        ) {
            self.prompts.push_back(prompt);
            true
        } else {
            false
        }
    }
    pub fn clear(&mut self) {
        self.prompts.clear();
        self.permissions.clear();
    }
}
#[derive(Default)]
pub struct State {
    pub settings: bool,
    pub media: std::collections::HashMap<u64, crate::native_media::MediaInfo>,
    pub media_owner: Option<u64>,
    pub site_info: bool,
    pub clear_data: bool,
    /// Second step before clearing session cookies (it signs out of every site).
    /// Cookie values can hold session tokens; they stay hidden unless asked for.
    pub show_cookie_values: bool,
    pub delete_site: Option<String>,
    pub settings_section: usize,
    pub diagnostics: bool,
    pub cookies: Vec<String>,
    pub find_open: bool,
    pub find: String,
    /// "3 of 12" / "No matches", written by the page script's callback.
    pub find_status: std::rc::Rc<std::cell::RefCell<String>>,
    /// The find field takes focus once each time the bar opens.
    pub find_focused: bool,
    pub script_source: String,
}

#[cfg(test)]
mod tests {
    use wega::model::Policy;
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
}
