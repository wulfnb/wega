use super::*;

#[test]
fn normalization_keeps_engine_and_title_with_each_valid_address() {
    use crate::engine_choice::EngineKind::Wega;
    let mut profile = Profile {
        urls: vec!["https://".into(), "https://article.test/".into()],
        titles: vec!["Invalid".into(), "Article".into()],
        engines: vec![Wega, Wega],
        ..Default::default()
    };
    profile.normalize();
    assert_eq!(profile.urls, vec!["https://article.test/"]);
    assert_eq!(profile.titles, vec!["Article"]);
    assert_eq!(profile.engines, vec![Wega]);
    let mut legacy: Profile = serde_json::from_str(r#"{"urls":["https://legacy.test/"]}"#).unwrap();
    legacy.normalize();
    assert_eq!(legacy.engines, vec![Wega]);
}

#[test]
fn internal_links_use_only_the_wega_scheme() {
    assert_eq!(
        resolve_address("wega://bookmarks").unwrap().as_str(),
        "wega://bookmarks"
    );
    for retired in ["servo://history", "lotse://settings"] {
        assert!(
            resolve_address(retired).is_err(),
            "{retired} must not resolve"
        );
    }
    let directory =
        std::env::temp_dir().join(format!("wega-internal-profile-{}", std::process::id()));
    let original = Profile {
        urls: vec!["https://example.com/".into(), "wega://newtab".into()],
        active: 1,
        ..Profile::default()
    };
    original.save(&directory).unwrap();
    let restored = Profile::load(&directory).unwrap();
    assert_eq!(restored.urls, vec!["https://example.com/", "wega://newtab"]);
    assert_eq!(restored.active, original.active);
    assert_eq!(restored.bookmarks.len(), original.bookmarks.len());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn addresses_and_encoded_searches() {
    assert_eq!(
        resolve_address("example.com/path").unwrap().as_str(),
        "https://example.com/path"
    );
    assert_eq!(
        resolve_address("localhost:8080/a").unwrap().as_str(),
        "http://localhost:8080/a"
    );
    let url = resolve_address("Rust & Servo + 日本語").unwrap();
    assert_eq!(url.query_pairs().next().unwrap().1, "Rust & Servo + 日本語");
    assert!(resolve_address("javascript://alert(1)").is_err());
}

#[test]
fn old_profiles_keep_undated_history_and_new_visits_round_trip() {
    let mut profile: Profile =
        serde_json::from_str(r#"{"history":[{"title":"Old page","url":"https://example.com/"}]}"#)
            .unwrap();
    assert_eq!(profile.history[0].visited_at, 0);
    assert_eq!(history_date(0, chrono::Local::now()).0, "Earlier visits");
    profile.history[0].visited_at = visit_time();
    let restored: Profile =
        serde_json::from_str(&serde_json::to_string(&profile).unwrap()).unwrap();
    assert_eq!(
        restored.history[0].visited_at,
        profile.history[0].visited_at
    );
    assert_eq!(
        history_date(restored.history[0].visited_at, chrono::Local::now()).0,
        "Today"
    );
}

#[test]
fn reordering_preserves_active_identity() {
    for active in 0..4 {
        for from in 0..4 {
            for to in 0..4 {
                let mut tabs = vec![0, 1, 2, 3];
                let mut selected = active;
                move_tab(&mut tabs, &mut selected, from, to);
                assert_eq!(tabs[selected], active);
            }
        }
    }
}

#[test]
fn profile_round_trip_and_corrupt_file_detection() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("wega-test-{}-{unique}", std::process::id()));
    let profile = Profile {
        layout: TabLayout::Top,
        devtools_dock: crate::model::Dock::Right,
        urls: vec!["https://example.com/".into(), "about:blank".into()],
        active: 1,
        ..Profile::default()
    };
    profile.save(&dir).unwrap();
    let restored = Profile::load(&dir).unwrap();
    assert_eq!(restored.layout, TabLayout::Top);
    assert_eq!(restored.devtools_dock, crate::model::Dock::Right);
    assert_eq!(restored.urls, profile.urls);
    assert_eq!(restored.active, 1);
    std::fs::write(dir.join("browser.json"), b"invalid json").unwrap();
    assert!(Profile::load(&dir).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn address_schemes_local_files_and_invalid_hosts() {
    assert_eq!(resolve_address("  ").unwrap().as_str(), HOME);
    #[cfg(unix)]
    assert_eq!(
        resolve_address("/tmp/a b.html").unwrap().as_str(),
        "file:///tmp/a%20b.html"
    );
    #[cfg(windows)]
    assert_eq!(
        resolve_address("/tmp/a b.html").unwrap(),
        crate::model::Policy::default().search_url("/tmp/a b.html")
    );
    assert!(resolve_address("about:blank").is_ok());
    assert!(resolve_address("about:config").is_err());
    assert!(resolve_address("https://[").is_err());
    assert!(resolve_address("example..test:invalid-port").is_err());
    assert_eq!(
        resolve_address("[::1]:8080").unwrap().as_str(),
        "http://[::1]:8080/"
    );
    let mut tabs = vec![1, 2];
    let mut active = 1;
    move_tab(&mut tabs, &mut active, 10, 0);
    move_tab(&mut tabs, &mut active, 0, 10);
    assert_eq!(tabs, [1, 2]);
    assert_eq!(active, 1);
    let directory = Profile::default_dir();
    assert!(directory.is_absolute() || directory == Path::new(".wega"));
}

#[test]
fn history_grouping_handles_yesterday_older_and_invalid_timestamps() {
    use chrono::{Local, TimeZone};
    let now = Local
        .with_ymd_and_hms(2026, 10, 21, 12, 0, 0)
        .single()
        .unwrap();
    let yesterday = Local
        .with_ymd_and_hms(2026, 10, 20, 12, 30, 0)
        .single()
        .unwrap();
    let older = Local
        .with_ymd_and_hms(2026, 10, 18, 8, 5, 0)
        .single()
        .unwrap();
    assert_eq!(
        history_date(yesterday.timestamp() as u64, now),
        ("Yesterday".into(), "12:30".into())
    );
    assert_eq!(
        history_date(older.timestamp() as u64, now),
        ("18 Oct 2026".into(), "08:05".into())
    );
    assert_eq!(
        history_date(u64::MAX, now),
        ("Earlier visits".into(), String::new())
    );
    assert_eq!(
        history_date(i64::MAX as u64, now),
        ("Earlier visits".into(), String::new())
    );
}

#[test]
fn malformed_host_ports_are_rejected_before_navigation() {
    assert!(resolve_address("localhost:bad").is_err());
    assert!(resolve_address("example.com:bad").is_err());
}

// 1×1 PNG produced by the desktop encoder.
const PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0,
    0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89,
];

#[test]
fn legacy_rgba_favicons_still_load_and_png_favicons_are_stored_as_base64() {
    let legacy: Profile = serde_json::from_str(
        r#"{"favicons":{"https://a.test/":{"width":1,"height":1,"rgba":[1,2,3,255]}}}"#,
    )
    .unwrap();
    assert_eq!(
        legacy.favicons["https://a.test/"],
        SavedFavicon::Rgba {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 255]
        }
    );
    let mut profile = Profile::default();
    profile
        .favicons
        .insert("https://b.test/".into(), SavedFavicon::Png(PNG.to_vec()));
    let json = serde_json::to_string(&profile).unwrap();
    assert!(
        json.contains(&format!(
            r#""https://b.test/":{{"png":"{}"}}"#,
            crate::base64_text::encode(PNG)
        )),
        "{json}"
    );
    let restored: Profile = serde_json::from_str(&json).unwrap();
    assert_eq!(
        restored.favicons["https://b.test/"],
        SavedFavicon::Png(PNG.to_vec())
    );
    assert_eq!(
        restored.favicons["https://b.test/"].dimensions(),
        Some((1, 1))
    );
    assert!(restored.favicons["https://b.test/"].valid(32));
}

#[test]
fn corrupt_or_oversized_png_favicons_are_dropped_without_losing_the_profile() {
    let mut profile: Profile = serde_json::from_str(
        r#"{"urls":["https://keep.test/"],"favicons":{"bad":{"png":"!!!"},"text":{"png":"aGVsbG8="}}}"#,
    )
    .unwrap();
    let mut huge = PNG.to_vec();
    huge[16..20].copy_from_slice(&1000u32.to_be_bytes());
    profile
        .favicons
        .insert("huge".into(), SavedFavicon::Png(huge));
    profile.normalize();
    assert!(profile.favicons.is_empty());
    assert_eq!(profile.urls, vec!["https://keep.test/"]);
}

#[test]
fn saved_titles_stay_aligned_with_urls_after_normalization() {
    let mut profile = Profile {
        urls: vec![
            "javascript://bad".into(),
            "https://a.test/".into(),
            "https://b.test/".into(),
        ],
        titles: vec!["Bad".into(), "A".into()],
        ..Profile::default()
    };
    profile.normalize();
    assert_eq!(profile.urls, vec!["https://a.test/", "https://b.test/"]);
    assert_eq!(profile.titles, vec!["A", ""]);
}

#[test]
fn searches_use_the_configured_engine_and_invalid_custom_templates_fall_back() {
    use crate::model::{Policy, SearchEngine};
    let policy = |engine, template: &str| Policy {
        search_engine: engine,
        search_template: template.into(),
        ..Policy::default()
    };
    assert_eq!(Policy::default().search_engine, SearchEngine::Google);
    assert_eq!(
        resolve_address("rust lang").unwrap().as_str(),
        "https://www.google.com/search?q=rust%20lang"
    );
    assert_eq!(
        resolve_address_with("a&b", &policy(SearchEngine::DuckDuckGo, ""))
            .unwrap()
            .as_str(),
        "https://duckduckgo.com/?q=a%26b"
    );
    assert_eq!(
        resolve_address_with("x y", &policy(SearchEngine::Startpage, ""))
            .unwrap()
            .as_str(),
        "https://www.startpage.com/do/search?query=x%20y"
    );
    let custom = policy(
        SearchEngine::Custom,
        "https://search.test/find?lang=en&term=%s",
    );
    assert_eq!(
        resolve_address_with("日本", &custom).unwrap().as_str(),
        "https://search.test/find?lang=en&term=%E6%97%A5%E6%9C%AC"
    );
    assert_eq!(
        custom.search_form(),
        (
            "https://search.test/find".to_owned(),
            "term".to_owned(),
            vec![("lang".to_owned(), "en".to_owned())]
        )
    );
    // Addresses are never sent to the search engine.
    assert_eq!(
        resolve_address_with("example.com", &custom)
            .unwrap()
            .as_str(),
        "https://example.com/"
    );
    for invalid in [
        "",
        "https://search.test/",
        "javascript:%s",
        "https://a/%s/%s",
    ] {
        assert!(!crate::model::valid_search_template(invalid), "{invalid}");
        assert_eq!(
            resolve_address_with("q", &policy(SearchEngine::Custom, invalid))
                .unwrap()
                .host_str(),
            Some("www.google.com")
        );
    }
    let path = policy(SearchEngine::Custom, "https://search.test/q/%s");
    assert_eq!(
        resolve_address_with("a b", &path).unwrap().as_str(),
        "https://search.test/q/a%20b"
    );
    assert_eq!(
        path.search_form().1,
        "q",
        "Path templates fall back to a form engine"
    );
    for engine in SearchEngine::ALL {
        let (action, field, _) = policy(engine, "").search_form();
        assert!(
            action.starts_with("https://") && !field.is_empty(),
            "{engine:?}"
        );
    }
}

#[test]
fn discard_timeout_defaults_to_thirty_minutes_for_old_profiles() {
    let profile: Profile = serde_json::from_str(r#"{"policy":{"offline":true}}"#).unwrap();
    assert!(profile.policy.offline);
    assert_eq!(profile.policy.discard_after_minutes, 30);
    assert_eq!(
        profile.policy.search_engine,
        crate::model::SearchEngine::Google
    );
}

#[cfg(windows)]
#[test]
fn windows_file_associations_open_absolute_paths_instead_of_searching() {
    assert_eq!(
        resolve_address(r"C:\Users\Alice\some page.html")
            .unwrap()
            .as_str(),
        "file:///C:/Users/Alice/some%20page.html"
    );
    assert_eq!(
        resolve_address(r"\\fileserver\share\some page.html")
            .unwrap()
            .as_str(),
        "file://fileserver/share/some%20page.html"
    );
}

#[test]
fn shortcuts_keep_bookmarks_then_rank_frequent_pages_without_duplicates() {
    let page = |url: &str, visit_count| Bookmark {
        title: url.into(),
        url: url.into(),
        visited_at: 0,
        visit_count,
    };
    let mut profile = Profile {
        bookmarks: vec![page("https://saved.test/", 0)],
        history: vec![
            page("https://recent.test/", 1),
            page("https://saved.test/", 4),
            page("https://frequent.test/", 8),
        ],
        ..Profile::default()
    };
    for number in 0..10 {
        profile
            .history
            .push(page(&format!("https://{number}.test/"), 1));
    }
    let shortcuts = profile.new_tab_shortcuts();
    assert_eq!(shortcuts.len(), 8);
    assert_eq!(shortcuts[0].url, "https://saved.test/");
    assert_eq!(shortcuts[1].url, "https://frequent.test/");
    assert_eq!(shortcuts[2].url, "https://recent.test/");
    assert_eq!(
        shortcuts
            .iter()
            .filter(|page| page.url == "https://saved.test/")
            .count(),
        1
    );
    let old: Profile =
        serde_json::from_str(r#"{"history":[{"title":"Old","url":"https://old.test/"}]}"#).unwrap();
    assert_eq!(old.history[0].visit_count, 0);
    assert_eq!(
        old.new_tab_shortcuts().last().unwrap().url,
        "https://old.test/"
    );
}

#[test]
fn legacy_servo_sessions_migrate_without_losing_tabs_or_preferences() {
    let mut profile: Profile = serde_json::from_str(r#"{
        "urls": ["https://example.test/"],
        "titles": ["Saved work"],
        "engines": ["Servo"],
        "policy": {"wega_only": false, "wega_fallback": true, "site_engines": {"https://example.test": "Servo"}}
    }"#).unwrap();
    profile.normalize();
    assert_eq!(profile.urls, vec!["https://example.test/"]);
    assert_eq!(profile.titles, vec!["Saved work"]);
    assert_eq!(
        profile.engines,
        vec![crate::engine_choice::EngineKind::Wega]
    );
    let saved = serde_json::to_string(&profile).unwrap();
    assert!(!saved.contains("Servo"));
    assert!(!saved.contains("wega_fallback"));
}
