//! Persisted identity is independent of either rendering backend.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineKind {
    #[default]
    Servo,
    Wega,
}
impl EngineKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Servo => "Servo",
            Self::Wega => "Wega (experimental)",
        }
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "servo" => Ok(Self::Servo),
            "wega" => Ok(Self::Wega),
            _ => Err("Engine must be servo or wega".into()),
        }
    }
}

/// Preferences are scoped to a tuple origin, including non-default ports.
pub fn site_key(url: &url::Url) -> Option<String> {
    matches!(url.scheme(), "http" | "https").then(|| url.origin().ascii_serialization())
}
/// Small static-document rollout. Dynamic apps/media/editor sites remain Servo.
pub fn rollout_site(url: &url::Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("example.com" | "example.org" | "example.net")
        )
        && url.port_or_known_default() == Some(443)
}
pub fn choose(
    policy: &crate::model::Policy,
    url: &url::Url,
    saved: Option<EngineKind>,
    explicit: Option<EngineKind>,
    available: bool,
) -> EngineKind {
    if url.scheme() == "wega" {
        return EngineKind::Servo;
    }
    let choice = explicit
        .or_else(|| site_key(url).and_then(|key| policy.site_engines.get(&key).copied()))
        .or(saved)
        .unwrap_or_else(|| {
            if policy.wega_rollout && rollout_site(url) {
                EngineKind::Wega
            } else {
                EngineKind::Servo
            }
        });
    if choice == EngineKind::Wega && !available {
        EngineKind::Servo
    } else {
        choice
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origin_preferences_do_not_match_credentials_paths_subdomains_or_other_ports() {
        let mut policy = crate::model::Policy::default();
        let url = url::Url::parse("https://EXAMPLE.test:443/a").unwrap();
        let key = site_key(&url).unwrap();
        assert_eq!(key, "https://example.test");
        policy.site_engines.insert(key, EngineKind::Wega);
        for address in ["https://example.test/b", "https://user:pass@example.test/"] {
            assert_eq!(
                choose(
                    &policy,
                    &url::Url::parse(address).unwrap(),
                    None,
                    None,
                    true
                ),
                EngineKind::Wega
            );
        }
        for address in [
            "https://example.test.evil/",
            "https://sub.example.test/",
            "http://example.test/",
            "https://example.test:8443/",
            "https://evil.test/example.test",
        ] {
            assert_eq!(
                choose(
                    &policy,
                    &url::Url::parse(address).unwrap(),
                    None,
                    None,
                    true
                ),
                EngineKind::Servo
            );
        }
        assert_eq!(
            choose(&policy, &url, None, Some(EngineKind::Servo), true),
            EngineKind::Servo
        );
        assert_eq!(choose(&policy, &url, None, None, false), EngineKind::Servo);
    }
    #[test]
    fn rollout_is_narrow_optional_and_subordinate_to_preferences_and_saved_tabs() {
        let mut policy = crate::model::Policy::default();
        let url = url::Url::parse("https://example.com/").unwrap();
        assert_eq!(choose(&policy, &url, None, None, true), EngineKind::Wega);
        assert_eq!(
            choose(&policy, &url, Some(EngineKind::Servo), None, true),
            EngineKind::Servo
        );
        for address in [
            "http://example.com/",
            "https://example.com:8443/",
            "https://example.com.evil/",
            "https://www.youtube.com/",
            "https://docs.google.com/",
        ] {
            assert!(!rollout_site(&url::Url::parse(address).unwrap()));
        }
        policy.wega_rollout = false;
        assert_eq!(choose(&policy, &url, None, None, true), EngineKind::Servo);
        policy
            .site_engines
            .insert(site_key(&url).unwrap(), EngineKind::Wega);
        let restored: crate::model::Policy =
            serde_json::from_str(&serde_json::to_string(&policy).unwrap()).unwrap();
        assert_eq!(choose(&restored, &url, None, None, true), EngineKind::Wega);
        assert_eq!(
            choose(
                &policy,
                &url::Url::parse("wega://settings").unwrap(),
                None,
                Some(EngineKind::Wega),
                true
            ),
            EngineKind::Servo
        );
    }
}
