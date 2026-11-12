use super::*;
#[test]
fn host_policy_rejects_lookalikes_and_block_overrides_allow() {
    let policy = Policy {
        blocked_hosts: "ads.example.com".into(),
        allowed_hosts: "*.Example.COM.".into(),
        ..Policy::default()
    };
    for url in ["https://example.com/", "https://sub.example.com/"] {
        assert!(policy.permits(&url::Url::parse(url).unwrap()));
    }
    for url in [
        "https://ads.example.com/",
        "https://child.ads.example.com/",
        "https://example.com.evil/",
        "https://evil-example.com/",
    ] {
        assert!(!policy.permits(&url::Url::parse(url).unwrap()));
    }
    let degenerate = Policy {
        allowed_hosts: "*. .".into(),
        ..Policy::default()
    };
    assert!(!degenerate.permits(&url::Url::parse("data:text/plain,test").unwrap()));
    assert!(!degenerate.permits(&url::Url::parse("https://example.com/").unwrap()));
}

#[test]
fn persisted_theme_names_and_modes_are_compatible() {
    assert_eq!(
        serde_json::from_str::<Theme>("\"Purple\"").unwrap(),
        Theme::Emerald
    );
    for theme in Theme::ALL {
        assert!(!theme.name().is_empty());
        assert_eq!(
            serde_json::from_str::<Theme>(&serde_json::to_string(&theme).unwrap()).unwrap(),
            theme
        );
        assert_eq!(
            theme.glass(),
            matches!(theme, Theme::Glass | Theme::GlassLight | Theme::GlassDark)
        );
    }
    assert!(!Theme::Glass.dark(false));
    assert!(Theme::Glass.dark(true));
    assert!(!Theme::GlassLight.dark(true));
    assert!(!Theme::Paper.dark(true));
    assert!(Theme::Midnight.dark(false));
    assert_eq!(Dock::ALL.len(), 4);
}

#[test]
fn remembered_permission_blocks_are_exact_origin_and_persist_without_grants() {
    let mut policy = Policy::default();
    policy.set_permission_blocked("https://a.test", "Camera", true);
    assert!(policy.permission_blocked("https://a.test", "Camera"));
    for origin in [
        "http://a.test",
        "https://sub.a.test",
        "https://a.test:444",
        "https://a.test.evil",
        "null",
        "https://a.test/path",
    ] {
        assert!(!policy.permission_blocked(origin, "Camera"));
    }
    assert!(!policy.permission_blocked("https://a.test", "Microphone"));
    for origin in [
        "file:///tmp/test",
        "data:text/plain,x",
        "null",
        "https://a.test/path",
    ] {
        policy.set_permission_blocked(origin, "Camera", true);
    }
    assert_eq!(policy.blocked_site_permissions.len(), 1);
    let mut restored: Policy =
        serde_json::from_str(&serde_json::to_string(&policy).unwrap()).unwrap();
    assert!(restored.permission_blocked("https://a.test", "Camera"));
    restored.set_permission_blocked("https://a.test", "Camera", false);
    assert!(restored.blocked_site_permissions.is_empty());
    let legacy: Policy = serde_json::from_str("{}").unwrap();
    assert!(legacy.blocked_site_permissions.is_empty());
}
#[test]
fn address_emphasis_never_highlights_userinfo_or_path_as_host() {
    for (address, host) in [
        (
            "https://evil.test@trusted.test/path/evil.test",
            "trusted.test",
        ),
        ("https://example.test:8443/", "example.test"),
        ("http://[::1]:8080/", "[::1]"),
    ] {
        assert_eq!(&address[address_host_range(address).unwrap()], host);
    }
    for address in [
        "https://EXAMPLE.test",
        "search terms",
        "wega://newtab",
        "https://",
        "data:text/plain,example.test",
    ] {
        assert!(address_host_range(address).is_none());
    }
}
