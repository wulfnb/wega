use super::*;
#[derive(Default)]
struct Engine {
    calls: Vec<String>,
    zoom: f32,
    history: bool,
}
impl NavigationPort for Engine {
    fn load(&mut self, url: url::Url) {
        self.calls.push(url.to_string());
    }
    fn reload(&mut self) {
        self.calls.push("reload".into());
    }
    fn traverse(&mut self, forward: bool) -> bool {
        self.calls.push(format!("forward={forward}"));
        self.history
    }
    fn zoom(&self) -> f32 {
        self.zoom
    }
    fn set_zoom(&mut self, value: f32) {
        self.zoom = value;
    }
}
#[test]
fn navigation_validates_intent_before_touching_the_engine() {
    let mut engine = Engine {
        zoom: 1.0,
        ..Engine::default()
    };
    assert!(dispatch(Command::Load("javascript://bad".into()), &mut engine).is_err());
    assert!(engine.calls.is_empty());
    assert!(dispatch(Command::Load("example.com".into()), &mut engine).unwrap());
    dispatch(Command::Reload, &mut engine).unwrap();
    assert!(!dispatch(Command::Traverse { forward: true }, &mut engine).unwrap());
    engine.history = true;
    assert!(dispatch(Command::Traverse { forward: false }, &mut engine).unwrap());
    assert_eq!(
        engine.calls,
        [
            "https://example.com/",
            "reload",
            "forward=true",
            "forward=false"
        ]
    );
    for factor in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(dispatch(Command::Zoom { factor }, &mut engine).is_err());
        assert_eq!(engine.zoom, 1.0);
    }
    dispatch(Command::Zoom { factor: 100.0 }, &mut engine).unwrap();
    assert_eq!(engine.zoom, 5.0);
    dispatch(Command::Zoom { factor: 0.001 }, &mut engine).unwrap();
    assert_eq!(engine.zoom, 0.3);
    dispatch(Command::ResetZoom, &mut engine).unwrap();
    assert_eq!(engine.zoom, 1.0);
}

#[test]
fn completion_ignores_old_pages_and_waits_for_matching_engine_progress() {
    let mut load = Completion::new(Some("https://new.test/".into()), "https://old.test/".into());
    assert!(!load.ready("https://old.test/", true));
    assert!(!load.ready("https://new.test/", true));
    load.started();
    assert!(!load.ready("https://old.test/", true));
    assert!(!load.ready("https://new.test/", false));
    assert!(load.ready("https://new.test/", true));
    assert!(load.ready("https://redirect.test/", true));
    let mut traversal = Completion::new(None, "https://old.test/".into());
    assert!(!traversal.ready("https://old.test/", true));
    traversal.traversed();
    assert!(!traversal.ready("https://old.test/", false));
    assert!(traversal.ready("https://new.test/", true));
    let mut refresh = Completion::new(None, "https://same.test/".into());
    refresh.started();
    assert!(refresh.ready("https://same.test/", true));
}

#[test]
fn history_targets_preserve_duplicate_urls_and_validate_boundaries() {
    let mut history = History::default();
    assert_eq!(history.target(false), None);
    assert_eq!(history.target(true), None);
    history.update(
        vec![
            "https://same.test/".into(),
            "https://same.test/".into(),
            "https://last.test/".into(),
        ],
        1,
    );
    assert_eq!(history.target(false), Some("https://same.test/"));
    assert_eq!(history.target(true), Some("https://last.test/"));
    history.update(vec!["https://only.test/".into()], usize::MAX);
    assert_eq!(history.target(false), None);
    assert_eq!(history.target(true), None);
    history.update(vec![], 0);
    assert_eq!(history.target(true), None);
}

#[test]
fn history_push_traverse_and_clear_semantics() {
    let mut history = History::new();
    assert!(!history.can_back());
    assert!(!history.can_forward());

    history.push("wega://newtab".into());
    assert!(!history.can_back());
    assert!(!history.can_forward());

    history.push("https://example.com/".into());
    assert!(history.can_back());
    assert!(!history.can_forward());
    assert_eq!(history.target(false), Some("wega://newtab"));

    assert_eq!(history.traverse(false), Some("wega://newtab"));
    assert!(!history.can_back());
    assert!(history.can_forward());
    assert_eq!(history.target(true), Some("https://example.com/"));

    // Navigating from back state truncates forward history
    history.push("wega://history".into());
    assert!(history.can_back());
    assert!(!history.can_forward());
    assert_eq!(history.target(false), Some("wega://newtab"));

    history.clear();
    assert!(!history.can_back());
    assert!(!history.can_forward());
}

#[test]
fn web_content_cannot_open_local_files_or_top_level_data_documents() {
    let url = |s: &str| url::Url::parse(s).unwrap();
    let web = url("https://example.com/");
    let file = url("file:///tmp/a.html");
    let data = url("data:text/html,<h1>x</h1>");
    let internal = url("wega://history");
    assert!(!content_navigation_allowed(&file, Some(&web)));
    assert!(!content_navigation_allowed(&file, None), "Script popups");
    assert!(!content_navigation_allowed(&data, Some(&web)));
    assert!(!content_navigation_allowed(&data, Some(&data)));
    assert!(!content_navigation_allowed(&file, Some(&data)));
    assert!(
        content_navigation_allowed(&file, Some(&file)),
        "Local fixtures link to each other"
    );
    assert!(
        content_navigation_allowed(&file, Some(&internal)),
        "History lists local files"
    );
    assert!(content_navigation_allowed(&data, Some(&internal)));
    assert!(content_navigation_allowed(&web, Some(&file)));
    assert!(content_navigation_allowed(
        &url("blob:https://example.com/x"),
        Some(&web)
    ));

    // Internal privileges protection
    assert!(
        !content_navigation_allowed(&internal, Some(&web)),
        "Untrusted web content cannot navigate to internal pages"
    );
    assert!(
        !content_navigation_allowed(&internal, None),
        "Script popups cannot navigate to internal pages"
    );
    assert!(
        content_navigation_allowed(&internal, Some(&url("wega://newtab"))),
        "Internal pages can link to each other"
    );
}

#[test]
fn navigation_port_defaults_and_completion_update() {
    let mut engine = Engine::default();
    engine.reload_bypass_cache();
    assert_eq!(engine.calls, ["reload"]);
    engine.clear_history();

    let mut comp = Completion::new(
        Some("https://example.com/".into()),
        "https://old.com/".into(),
    );
    assert!(!comp.update(true, "https://example.com/"));
    assert!(comp.update(false, "https://example.com/"));

    let mut empty_hist = History::new();
    assert_eq!(empty_hist.traverse(false), None);
    assert_eq!(empty_hist.traverse(true), None);
    empty_hist.clear();
    assert_eq!(empty_hist.traverse(false), None);

    let mut hist = History::new();
    hist.push("https://a.test/".into());
    hist.push("https://b.test/".into());
    assert_eq!(hist.traverse(false), Some("https://a.test/"));
    assert_eq!(hist.traverse(true), Some("https://b.test/"));
    assert_eq!(hist.traverse(true), None);
}
