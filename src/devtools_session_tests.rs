use super::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use wega::engine_protocol::{
    DocumentId, DocumentRevision,
    identity::{FrameId, TabId},
    inspector::{INSPECTOR_VERSION, InspectorErrorCode},
};

#[derive(Default)]
struct Inner {
    next: u64,
    live: Option<InspectorSessionId>,
    opened: Vec<InspectorSessionId>,
    closed: Vec<InspectorSessionId>,
    requests: Vec<(InspectorSessionId, InspectorCommand)>,
    pending: Vec<InspectorCommand>,
    results: Vec<Completed>,
    events: Vec<InspectorEvent>,
    document: u64,
    closed_process: bool,
    refuse_open: Option<InspectorErrorCode>,
    respond: bool,
    attach_failures: u8,
    snapshot_target: Option<TargetId>,
    snapshot_document: Option<DocumentContext>,
    calls: usize,
}

/// A scripted page: it enforces session liveness exactly like the real registry does.
#[derive(Clone)]
struct Fake {
    target: Rc<RefCell<TargetId>>,
    inner: Rc<RefCell<Inner>>,
    base: u64,
}

fn target(tab: u64, process: u8) -> TargetId {
    TargetId {
        tab: TabId(tab),
        frame: FrameId(1),
        process: ProcessSessionId([process; 16]),
    }
}

impl Fake {
    fn new(tab: u64, process: u8, base: u64) -> Self {
        let fake = Self {
            target: Rc::new(RefCell::new(target(tab, process))),
            inner: Default::default(),
            base,
        };
        fake.inner.borrow_mut().respond = true;
        fake
    }
    fn document(&self) -> DocumentContext {
        DocumentContext {
            document: DocumentId(self.inner.borrow().document),
            revision: DocumentRevision(self.inner.borrow().document),
        }
    }
    fn navigate(&self) {
        self.inner.borrow_mut().document += 1;
    }
    fn restart(&self, process: u8) {
        let tab = self.target.borrow().tab.0;
        *self.target.borrow_mut() = target(tab, process);
        let mut inner = self.inner.borrow_mut();
        inner.live = None;
        inner.closed_process = false;
        inner.pending.clear();
        inner.results.clear();
    }
    fn crash(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.closed_process = true;
        inner.live = None;
    }
    fn live(&self) -> Option<InspectorSessionId> {
        self.inner.borrow().live
    }
    fn opened(&self) -> Vec<InspectorSessionId> {
        self.inner.borrow().opened.clone()
    }
    fn closed(&self) -> Vec<InspectorSessionId> {
        self.inner.borrow().closed.clone()
    }
    fn calls(&self) -> usize {
        self.inner.borrow().calls
    }
    fn count(&self, command: InspectorCommand) -> usize {
        self.inner
            .borrow()
            .requests
            .iter()
            .filter(|(_, c)| *c == command)
            .count()
    }
    fn snapshot(&self) -> StatusSnapshot {
        let inner = self.inner.borrow();
        StatusSnapshot {
            target: inner.snapshot_target.unwrap_or(*self.target.borrow()),
            document: inner.snapshot_document.unwrap_or(DocumentContext {
                document: DocumentId(inner.document),
                revision: DocumentRevision(inner.document),
            }),
            url: format!("https://example.test/{}", self.target.borrow().tab.0),
            title: "t".into(),
            loading: false,
            viewport: None,
            nodes: 3,
            js_heap_bytes: 1,
            can_go_back: false,
            can_go_forward: false,
            protocol_version: 22,
            inspector_version: INSPECTOR_VERSION,
            engine: "fake".into(),
        }
    }
}

impl InspectorPort for Fake {
    fn target(&self) -> Option<TargetId> {
        Some(*self.target.borrow())
    }
    fn open(&self) -> Result<InspectorSessionId, InspectorError> {
        let mut inner = self.inner.borrow_mut();
        inner.calls += 1;
        if let Some(code) = inner.refuse_open {
            return Err(InspectorError::new(code, "refused"));
        }
        if inner.closed_process {
            return Err(InspectorError::new(InspectorErrorCode::Unavailable, "gone"));
        }
        inner.next += 1;
        let id = InspectorSessionId(self.base + inner.next);
        if let Some(old) = inner.live.replace(id) {
            inner.closed.push(old);
        }
        inner.opened.push(id);
        inner.pending.clear();
        inner.pending.push(InspectorCommand::Attach);
        inner.requests.push((id, InspectorCommand::Attach));
        Ok(id)
    }
    fn request(
        &self,
        session: InspectorSessionId,
        command: InspectorCommand,
    ) -> Result<(), InspectorError> {
        let mut inner = self.inner.borrow_mut();
        inner.calls += 1;
        if inner.live != Some(session) {
            return Err(InspectorError::new(
                InspectorErrorCode::StaleSession,
                "not live",
            ));
        }
        inner.requests.push((session, command));
        inner.pending.push(command);
        Ok(())
    }
    fn close(&self, session: InspectorSessionId) {
        let mut inner = self.inner.borrow_mut();
        inner.calls += 1;
        if inner.live == Some(session) {
            inner.live = None;
            inner.pending.clear();
            inner.results.clear();
        }
        inner.closed.push(session);
    }
    fn poll(&self, session: InspectorSessionId) -> InspectorPoll {
        let snapshot = self.snapshot();
        let document = self.document();
        let mut inner = self.inner.borrow_mut();
        inner.calls += 1;
        let live = inner.live == Some(session);
        if live && inner.respond {
            for command in std::mem::take(&mut inner.pending) {
                let result = match command {
                    InspectorCommand::Attach if inner.attach_failures > 0 => {
                        inner.attach_failures -= 1;
                        Err(InspectorError::new(InspectorErrorCode::Timeout, "slow"))
                    }
                    InspectorCommand::Attach => Ok(InspectorResponse::Attached),
                    InspectorCommand::SubscribeStatus => Ok(InspectorResponse::Subscribed),
                    InspectorCommand::GetStatusSnapshot => {
                        Ok(InspectorResponse::Status(Box::new(snapshot.clone())))
                    }
                    _ => continue,
                };
                inner.results.push(Completed {
                    session,
                    command,
                    result,
                });
            }
        }
        InspectorPoll {
            document,
            closed: inner.closed_process,
            results: if live {
                std::mem::take(&mut inner.results)
            } else {
                Vec::new()
            },
            events: if live {
                std::mem::take(&mut inner.events)
            } else {
                Vec::new()
            },
        }
    }
}

struct Tabs {
    pages: BTreeMap<u64, Fake>,
    unloaded: Vec<u64>,
}
impl Tabs {
    fn new(keys: &[u64]) -> Self {
        Self {
            pages: keys
                .iter()
                .map(|key| (*key, Fake::new(*key, *key as u8, key * 1_000)))
                .collect(),
            unloaded: Vec::new(),
        }
    }
    fn resolve(&self, key: u64) -> Resolved<'_> {
        if self.unloaded.contains(&key) {
            return Resolved::Unloaded;
        }
        self.pages
            .get(&key)
            .map_or(Resolved::Closed, |page| Resolved::Page(page))
    }
}

struct Clock(Instant);
impl Clock {
    fn at(&self, ms: u64) -> Instant {
        self.0 + Duration::from_millis(ms)
    }
}

fn run(view: &mut DevtoolsView, tabs: &Tabs, clock: &Clock, ms: u64, selected: u64, pinned: bool) {
    view.tick(clock.at(ms), selected, pinned, &|key| tabs.resolve(key));
}

#[test]
fn opening_attaches_to_the_selected_tab_and_shows_its_own_snapshot() {
    let tabs = Tabs::new(&[1, 2]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..4 {
        run(&mut view, &tabs, &clock, step * 10, 1, false);
    }
    assert_eq!(view.state, ViewState::Attached);
    let snapshot = view.snapshot().expect("snapshot");
    assert_eq!(snapshot.target.tab, TabId(1));
    assert_eq!(snapshot.url, "https://example.test/1");
    assert_eq!(tabs.pages[&1].opened().len(), 1);
    assert_eq!(tabs.pages[&2].calls(), 0, "the other tab was never touched");
    assert_eq!(tabs.pages[&1].count(InspectorCommand::SubscribeStatus), 1);
}

#[test]
fn a_closed_panel_never_touches_any_page() {
    let tabs = Tabs::new(&[1, 2]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    for step in 0..100 {
        run(&mut view, &tabs, &clock, step * 100, 1, false);
    }
    assert!(!view.is_open() && view.snapshot().is_none());
    assert_eq!(tabs.pages.values().map(Fake::calls).sum::<usize>(), 0);
    assert!(tabs.pages.values().all(|page| page.live().is_none()));
}

#[test]
fn closing_detaches_and_reopening_uses_a_new_session() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    run(&mut view, &tabs, &clock, 0, 1, false);
    run(&mut view, &tabs, &clock, 10, 1, false);
    let first = view.session().unwrap();
    view.close(&|key| tabs.resolve(key));
    assert_eq!(view.state, ViewState::Idle);
    assert!(view.snapshot().is_none() && view.session().is_none());
    assert!(tabs.pages[&1].live().is_none());
    assert!(tabs.pages[&1].closed().contains(&first));
    view.open(1);
    run(&mut view, &tabs, &clock, 20, 1, false);
    assert_ne!(view.session().unwrap(), first);
}

#[test]
fn switching_tabs_detaches_the_old_target_and_never_shows_its_data_under_the_new_one() {
    let tabs = Tabs::new(&[1, 2]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    let a = view.session().unwrap();
    assert_eq!(view.snapshot().unwrap().target.tab, TabId(1));
    // Select tab 2: the very same tick must clear A's data and move to B.
    tabs.pages[&2].inner.borrow_mut().respond = false;
    run(&mut view, &tabs, &clock, 10, 2, false);
    assert!(
        view.snapshot().is_none(),
        "A's data must not outlive the switch"
    );
    assert_eq!(view.state, ViewState::Connecting);
    assert!(tabs.pages[&1].closed().contains(&a));
    assert!(tabs.pages[&1].live().is_none());
    assert_eq!(view.target_tab(), Some(2));
    assert_eq!(tabs.pages[&2].opened().len(), 1);
    // A late snapshot naming tab 1 can never be accepted from tab 2's page.
    tabs.pages[&2].inner.borrow_mut().snapshot_target = Some(target(1, 1));
    tabs.pages[&2].inner.borrow_mut().respond = true;
    for step in 0..4 {
        run(&mut view, &tabs, &clock, 20 + step, 2, false);
        assert!(view.snapshot().is_none(), "foreign snapshot displayed");
    }
    tabs.pages[&2].inner.borrow_mut().snapshot_target = None;
    for step in 0..4 {
        run(&mut view, &tabs, &clock, 40 + step, 2, false);
    }
    assert_eq!(view.snapshot().unwrap().target.tab, TabId(2));
}

#[test]
fn a_detached_window_stays_on_its_target_when_the_selection_moves() {
    let tabs = Tabs::new(&[1, 2]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, true);
    }
    let session = view.session().unwrap();
    let before = tabs.pages[&2].calls();
    for step in 0..20 {
        run(&mut view, &tabs, &clock, 10 + step * 100, 2, true);
    }
    assert_eq!(view.target_tab(), Some(1));
    assert_eq!(
        view.session(),
        Some(session),
        "the session is not re-targeted"
    );
    assert_eq!(view.snapshot().unwrap().target.tab, TabId(1));
    assert_eq!(tabs.pages[&2].calls(), before);
    assert!(tabs.pages[&2].live().is_none());
}

#[test]
fn unpinning_a_window_follows_the_selection_deliberately() {
    let tabs = Tabs::new(&[1, 2]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, true);
    }
    run(&mut view, &tabs, &clock, 10, 2, false);
    assert_eq!(view.target_tab(), Some(2));
    assert!(tabs.pages[&1].live().is_none());
}

#[test]
fn closing_the_target_tab_ends_the_session_and_never_redirects_a_pinned_window() {
    let mut tabs = Tabs::new(&[1, 2]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, true);
    }
    tabs.pages.remove(&1);
    view.on_tab_closed(1);
    assert_eq!(view.state, ViewState::TargetClosed);
    assert!(view.snapshot().is_none() && view.session().is_none());
    for step in 0..10 {
        run(&mut view, &tabs, &clock, 10 + step, 2, true);
    }
    assert_eq!(view.state, ViewState::TargetClosed);
    assert_eq!(tabs.pages[&2].calls(), 0, "tab 2 was never attached");
}

#[test]
fn an_unloaded_target_shows_no_page_and_reattaches_when_a_new_process_appears() {
    let mut tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, true);
    }
    let first = view.session().unwrap();
    tabs.unloaded.push(1);
    run(&mut view, &tabs, &clock, 10, 1, true);
    assert_eq!(view.state, ViewState::NoPage);
    assert!(view.session().is_none() && view.snapshot().is_none());
    // The tab loads again in a brand-new process.
    tabs.unloaded.clear();
    tabs.pages[&1].restart(77);
    for step in 0..4 {
        run(&mut view, &tabs, &clock, 20 + step, 1, true);
    }
    assert_eq!(view.state, ViewState::Attached);
    assert_ne!(view.session().unwrap(), first);
    assert_eq!(
        view.snapshot().unwrap().target.process,
        ProcessSessionId([77; 16])
    );
}

#[test]
fn a_process_restart_invalidates_the_old_session_even_without_an_unloaded_tick() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    let old = view.session().unwrap();
    tabs.pages[&1].restart(55);
    for step in 0..4 {
        run(&mut view, &tabs, &clock, 10 + step, 1, false);
    }
    assert_ne!(view.session().unwrap(), old);
    assert_eq!(
        view.snapshot().unwrap().target.process,
        ProcessSessionId([55; 16])
    );
}

#[test]
fn a_crash_shows_unavailable_then_recovers_only_with_a_new_process() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    tabs.pages[&1].crash();
    run(&mut view, &tabs, &clock, 10, 1, false);
    assert!(matches!(view.state, ViewState::Unavailable(_)));
    assert!(view.snapshot().is_none() && view.session().is_none());
    // Still dead: retries are rate-limited and never succeed.
    let calls = tabs.pages[&1].calls();
    for step in 0..50 {
        run(&mut view, &tabs, &clock, 20 + step * 10, 1, false);
    }
    assert!(matches!(view.state, ViewState::Unavailable(_)));
    assert!(
        tabs.pages[&1].calls() - calls < 10,
        "retry storm against a dead process"
    );
    tabs.pages[&1].restart(99);
    for step in 0..4 {
        run(&mut view, &tabs, &clock, 2_000 + step, 1, false);
    }
    assert_eq!(view.state, ViewState::Attached);
}

#[test]
fn navigation_clears_the_snapshot_and_a_stale_document_snapshot_is_never_shown() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    let old_document = view.snapshot().unwrap().document;
    // The page navigates; the engine's snapshot for the old stamp is still in flight.
    {
        let page = &tabs.pages[&1];
        page.inner.borrow_mut().snapshot_document = Some(old_document);
        page.navigate();
        let document = page.document();
        page.inner
            .borrow_mut()
            .events
            .push(InspectorEvent::DocumentChanged { document });
    }
    for step in 0..3 {
        run(&mut view, &tabs, &clock, 10 + step, 1, false);
        assert!(
            view.snapshot().is_none(),
            "stale data shown after navigation"
        );
    }
    assert_ne!(view.state, ViewState::Attached);
    tabs.pages[&1].inner.borrow_mut().snapshot_document = None;
    for step in 0..4 {
        run(&mut view, &tabs, &clock, 100 + step, 1, false);
    }
    assert_eq!(view.snapshot().unwrap().document, tabs.pages[&1].document());
}

#[test]
fn a_stalled_page_is_reported_without_blocking_or_flooding_requests() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    tabs.pages[&1].inner.borrow_mut().respond = false;
    let started = Instant::now();
    for step in 1..=400 {
        run(&mut view, &tabs, &clock, 10 + step * 10, 1, false);
    }
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "tick must never wait"
    );
    assert_eq!(view.state, ViewState::NotResponding);
    assert!(
        view.snapshot().is_none(),
        "no data is shown for a hung page"
    );
    let outstanding = tabs.pages[&1].count(InspectorCommand::GetStatusSnapshot);
    // One request per refresh slot while stalled, never one per frame.
    assert!(
        outstanding <= 3,
        "{outstanding} snapshot requests against a stalled page"
    );
    // It recovers when the page answers again.
    tabs.pages[&1].inner.borrow_mut().respond = true;
    for step in 0..6 {
        run(&mut view, &tabs, &clock, 10_000 + step * 10, 1, false);
    }
    assert_eq!(view.state, ViewState::Attached);
}

#[test]
fn refresh_is_once_per_interval_not_per_frame() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    let before = tabs.pages[&1].count(InspectorCommand::GetStatusSnapshot);
    // 10 seconds of 60 fps frames.
    for frame in 0..600 {
        run(&mut view, &tabs, &clock, 100 + frame * 16, 1, false);
    }
    let requests = tabs.pages[&1].count(InspectorCommand::GetStatusSnapshot) - before;
    assert!(
        (8..=11).contains(&requests),
        "{requests} snapshot requests in 10 s"
    );
}

#[test]
fn a_transient_attach_failure_is_retried_but_bounded() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    tabs.pages[&1].inner.borrow_mut().attach_failures = 2;
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..10 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    assert_eq!(view.state, ViewState::Attached);
    assert_eq!(tabs.pages[&1].count(InspectorCommand::Attach), 3);
    let tabs = Tabs::new(&[1]);
    tabs.pages[&1].inner.borrow_mut().attach_failures = 99;
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..40 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    assert!(matches!(view.state, ViewState::Unavailable(_)));
    assert!(tabs.pages[&1].count(InspectorCommand::Attach) <= 1 + MAX_ATTACH_RETRIES as usize + 1);
}

#[test]
fn a_refused_open_reports_unavailable_and_retries_slowly() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    tabs.pages[&1].inner.borrow_mut().refuse_open = Some(InspectorErrorCode::Unavailable);
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..100 {
        run(&mut view, &tabs, &clock, step * 10, 1, false);
    }
    assert!(matches!(view.state, ViewState::Unavailable(_)));
    assert!(
        tabs.pages[&1].calls() <= 3,
        "retries are rate limited to one per second"
    );
}

#[test]
fn rapid_open_and_close_never_leaves_two_live_sessions_or_reuses_an_id() {
    let tabs = Tabs::new(&[1, 2]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    let mut seen = Vec::new();
    for round in 0..200_u64 {
        view.open(1 + round % 2);
        run(&mut view, &tabs, &clock, round, 1 + round % 2, false);
        if let Some(session) = view.session() {
            seen.push(session);
        }
        if round % 3 != 0 {
            view.close(&|key| tabs.resolve(key));
        }
        let live = tabs
            .pages
            .values()
            .filter(|page| page.live().is_some())
            .count();
        assert!(live <= 1, "round {round}: {live} live sessions");
    }
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), seen.len(), "a session id was reused");
}

#[test]
fn dropped_events_are_surfaced_and_trigger_a_refresh() {
    let tabs = Tabs::new(&[1]);
    let clock = Clock(Instant::now());
    let mut view = DevtoolsView::default();
    view.open(1);
    for step in 0..3 {
        run(&mut view, &tabs, &clock, step, 1, false);
    }
    let before = tabs.pages[&1].count(InspectorCommand::GetStatusSnapshot);
    tabs.pages[&1]
        .inner
        .borrow_mut()
        .events
        .push(InspectorEvent::EventsDropped { count: 41 });
    run(&mut view, &tabs, &clock, 10, 1, false);
    run(&mut view, &tabs, &clock, 11, 1, false);
    assert_eq!(view.dropped_events(), 41);
    assert!(tabs.pages[&1].count(InspectorCommand::GetStatusSnapshot) > before);
}

#[test]
fn a_tab_with_an_open_inspector_is_not_discardable_by_the_view_contract() {
    // The browser's `tab_busy` consults `is_open` and `target_tab`; pin their meaning.
    let mut view = DevtoolsView::default();
    assert!(!view.is_open() && view.target_tab().is_none());
    view.open(9);
    assert!(view.is_open() && view.target_tab() == Some(9));
}
