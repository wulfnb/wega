use super::*;
use std::{cell::RefCell, rc::Rc};

type Writes = Rc<RefCell<Vec<(Vec<String>, usize)>>>;
struct Store {
    fail_load: bool,
    fail_save: bool,
    writes: Writes,
}
impl ProfileStore for Store {
    fn load(&mut self) -> Result<Profile, String> {
        if self.fail_load {
            Err("broken profile".into())
        } else {
            Ok(Profile::default())
        }
    }
    fn save(&self, profile: &Profile) -> Result<(), String> {
        if self.fail_save {
            return Err("disk full".into());
        }
        self.writes
            .borrow_mut()
            .push((profile.urls.clone(), profile.active));
        Ok(())
    }
}
fn application(fail_load: bool, fail_save: bool) -> (Application, Writes, Option<String>) {
    let writes = Rc::new(RefCell::new(Vec::new()));
    let (application, warning) = Application::load(Box::new(Store {
        fail_load,
        fail_save,
        writes: writes.clone(),
    }));
    (application, writes, warning)
}
#[test]
fn engine_selection_stays_aligned_through_filtering_and_failed_saves() {
    use crate::engine_choice::EngineKind::{Servo, Wega};
    let (mut app, _, _) = application(false, false);
    app.save_engine_tabs(
        vec![
            ("data:text/html,temp".into(), "Temporary".into(), Servo),
            ("https://example.test/".into(), "Article".into(), Wega),
        ],
        1,
    )
    .unwrap();
    assert_eq!(app.profile().engines, vec![Wega]);
    assert_eq!(app.profile().active, 0);
    assert_eq!(app.profile().titles, vec!["Article"]);
    let (mut app, _, _) = application(false, true);
    assert!(
        app.save_engine_tabs(
            vec![("https://example.test/".into(), "Article".into(), Wega)],
            0
        )
        .is_err()
    );
    assert!(app.profile().engines.is_empty());
    assert_eq!(
        crate::engine_choice::EngineKind::parse("servo").unwrap(),
        Servo
    );
    assert!(crate::engine_choice::EngineKind::parse("unknown").is_err());
}
#[test]
fn storage_errors_never_overwrite_recovery_data() {
    let (mut app, writes, warning) = application(true, false);
    assert!(warning.unwrap().contains("broken profile"));
    app.apply(Command::ClearHistory);
    assert!(
        app.save_session(vec!["https://example.com".into()], 0)
            .is_err()
    );
    assert!(writes.borrow().is_empty());
    let (mut app, _, warning) = application(false, true);
    assert!(warning.is_none());
    let original_urls = app.profile().urls.clone();
    let original_active = app.profile().active;
    assert_eq!(app.save_session(vec![], 0).unwrap_err(), "disk full");
    assert_eq!(app.profile().urls, original_urls);
    assert_eq!(app.profile().active, original_active);
}
#[test]
fn restored_selection_tracks_filtered_tabs() {
    let (mut app, writes, _) = application(false, false);
    app.save_session(
        vec![
            "data:text/plain,secret".into(),
            "https://a.test".into(),
            "https://b.test".into(),
        ],
        2,
    )
    .unwrap();
    assert_eq!(
        *writes.borrow(),
        vec![(vec!["https://a.test".into(), "https://b.test".into()], 1)]
    );
    app.save_session(vec!["data:text/plain,secret".into()], usize::MAX)
        .unwrap();
    assert!(app.profile().urls.is_empty());
    assert_eq!(app.profile().active, 0);
}
#[test]
fn settings_and_collections_use_validated_commands() {
    let (mut app, _, _) = application(false, false);
    assert_eq!(app.apply(Command::Layout(TabLayout::Top)), Event::Changed);
    app.apply(Command::Theme(Theme::Paper));
    app.apply(Command::Dock(Dock::Window));
    assert_eq!(app.profile().layout, TabLayout::Top);
    assert_eq!(app.profile().theme, Theme::Paper);
    assert_eq!(app.profile().devtools_dock, Dock::Window);
    assert_eq!(app.apply(Command::SidebarWidth(f32::NAN)), Event::Unchanged);
    app.apply(Command::SidebarWidth(900.0));
    assert_eq!(app.profile().sidebar_width, 500.0);
    app.apply(Command::SidebarWidth(0.0));
    assert_eq!(app.profile().sidebar_width, 150.0);
    assert_eq!(
        app.apply(Command::Policy(Policy {
            offline: true,
            ..Policy::default()
        })),
        Event::PolicyChanged
    );
    assert!(app.profile().policy.offline);
    let count = app.profile().bookmarks.len();
    app.apply(Command::ToggleBookmark {
        title: "A".into(),
        url: "https://a.test".into(),
    });
    assert_eq!(app.profile().bookmarks.len(), count + 1);
    assert_eq!(
        app.apply(Command::RemoveBookmark(usize::MAX)),
        Event::Unchanged
    );
    app.apply(Command::RemoveBookmark(count));
    assert_eq!(app.profile().bookmarks.len(), count);
    for index in 0..1002 {
        app.apply(Command::Visit(Bookmark {
            title: index.to_string(),
            url: format!("https://{index}.test"),
            visited_at: index,
            visit_count: 0,
        }));
    }
    assert_eq!(app.profile().history.len(), 1000);
    app.apply(Command::Visit(Bookmark {
        title: "Renamed".into(),
        url: "https://1001.test".into(),
        visited_at: 2000,
        visit_count: 0,
    }));
    assert_eq!(app.profile().history.len(), 1000);
    assert_eq!(app.profile().history[0].title, "Renamed");
    assert_eq!(
        app.apply(Command::Visit(Bookmark {
            title: "secret".into(),
            url: "data:text/plain,secret".into(),
            visited_at: 0,
            visit_count: 0,
        })),
        Event::Unchanged
    );
    app.apply(Command::ClearHistory);
    assert!(app.profile().history.is_empty());
}
#[test]
fn favicon_commands_bound_memory_and_reject_bad_pixels() {
    let (mut app, _, _) = application(false, false);
    for (width, height, rgba) in [
        (0, 1, vec![]),
        (1, 0, vec![]),
        (33, 1, vec![]),
        (1, 33, vec![]),
        (1, 1, vec![0]),
    ] {
        assert_eq!(
            app.apply(Command::Favicon {
                url: "bad".into(),
                image: SavedFavicon::Rgba {
                    width,
                    height,
                    rgba
                }
            }),
            Event::Unchanged
        );
    }
    for index in 0..129 {
        app.apply(Command::Favicon {
            url: format!("{index:03}"),
            image: SavedFavicon::Rgba {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
        });
    }
    assert_eq!(app.profile().favicons.len(), 128);
    assert!(!app.profile().favicons.contains_key("000"));
    app.apply(Command::Favicon {
        url: "128".into(),
        image: SavedFavicon::Rgba {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        },
    });
    assert_eq!(app.profile().favicons.len(), 128);
    assert_eq!(
        app.profile().favicons["128"],
        SavedFavicon::Rgba {
            width: 1,
            height: 1,
            rgba: vec![255; 4]
        }
    );
    assert_eq!(
        app.apply(Command::Favicon {
            url: "png".into(),
            image: SavedFavicon::Png(b"not a png".to_vec()),
        }),
        Event::Unchanged
    );
}

#[test]
fn download_policy_precedes_external_effects_and_adapter_errors_survive() {
    #[derive(Default)]
    struct Downloads {
        requests: Vec<(String, String, usize, String)>,
    }
    impl crate::downloads::DownloadPort for Downloads {
        fn start(
            &mut self,
            url: &str,
            filename: &str,
            connections: usize,
            cookies: String,
        ) -> Result<(), String> {
            self.requests
                .push((url.into(), filename.into(), connections, cookies));
            Err("destination unavailable".into())
        }
    }
    let (mut app, _, _) = application(false, false);
    let mut downloads = Downloads::default();
    assert!(
        app.start_download(&mut downloads, "file:///etc/passwd", "", 1, String::new())
            .is_err()
    );
    app.apply(Command::Policy(Policy {
        offline: true,
        ..Policy::default()
    }));
    assert!(
        app.start_download(&mut downloads, "https://a.test/file", "", 1, String::new())
            .unwrap_err()
            .contains("blocked")
    );
    app.apply(Command::Policy(Policy {
        blocked_hosts: "a.test".into(),
        ..Policy::default()
    }));
    assert!(
        app.start_download(&mut downloads, "https://a.test/file", "", 1, String::new())
            .is_err()
    );
    assert!(downloads.requests.is_empty());
    app.apply(Command::Policy(Policy::default()));
    assert_eq!(
        app.start_download(
            &mut downloads,
            "https://a.test/file",
            "file",
            4,
            "private cookie".into()
        )
        .unwrap_err(),
        "destination unavailable"
    );
    assert_eq!(
        downloads.requests,
        vec![(
            "https://a.test/file".into(),
            "file".into(),
            4,
            "private cookie".into()
        )]
    );
}

#[test]
fn visit_frequency_survives_reload_saturates_and_clears_with_history() {
    let (mut app, _, _) = application(false, false);
    let visit = |url: &str| {
        Command::Visit(Bookmark {
            title: url.into(),
            url: url.into(),
            visited_at: 1,
            visit_count: 0,
        })
    };
    app.apply(visit("https://frequent.test/"));
    app.apply(visit("https://frequent.test/"));
    app.apply(visit("https://frequent.test/"));
    app.apply(visit("https://new.test/"));
    let restored: Profile =
        serde_json::from_str(&serde_json::to_string(app.profile()).unwrap()).unwrap();
    assert_eq!(restored.history[1].visit_count, 3);
    assert_eq!(
        restored.new_tab_shortcuts()[2].url,
        "https://frequent.test/"
    );
    assert_eq!(restored.history[0].url, "https://new.test/");
    app.profile.history[1].visit_count = u64::MAX;
    app.apply(visit("https://frequent.test/"));
    assert_eq!(app.profile.history[0].visit_count, u64::MAX);
    // A legacy history entry had an unknown count but was visited at least once.
    app.profile.history[0].visit_count = 0;
    app.apply(visit("https://frequent.test/"));
    assert_eq!(app.profile.history[0].visit_count, 2);
    app.apply(Command::ClearHistory);
    assert!(app.profile.history.is_empty());
    assert_eq!(
        app.profile.new_tab_shortcuts().len(),
        app.profile.bookmarks.len()
    );
}

#[test]
fn performance_counter_migrates_once_and_preserves_later_opt_out() {
    struct Existing(Policy);
    impl ProfileStore for Existing {
        fn load(&mut self) -> Result<Profile, String> {
            Ok(Profile {
                policy: self.0.clone(),
                ..Profile::default()
            })
        }
        fn save(&self, _: &Profile) -> Result<(), String> {
            Ok(())
        }
    }
    let old: Policy = serde_json::from_str(r#"{"show_performance":false}"#).unwrap();
    assert_eq!(old.performance_visibility_version, 0);
    let (app, _) = Application::load(Box::new(Existing(old)));
    assert!(app.profile().policy.show_performance);
    assert_eq!(app.profile().policy.performance_visibility_version, 1);
    let opted_out = Policy {
        show_performance: false,
        ..Policy::default()
    };
    let (app, _) = Application::load(Box::new(Existing(opted_out)));
    assert!(!app.profile().policy.show_performance);
}
