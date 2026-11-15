//! Can an inspector read geometry without ever risking the nested-block layout hang?
//!
//! The hang (roadmap B1) is in `flush_layout`: 122 or more nested blocks never finish laying
//! out. The geometry readers of `wega_dom::layout` take `&self` and cannot flush. These tests
//! establish, on the real adapter, that (1) readers on a never-laid-out pathological document
//! return immediately with defined "no boxes" values, (2) readers after a mutation report the
//! previous layout without recomputing, (3) `layout_pending` tells a caller which of those it is
//! looking at, and (4) the flush is where the hang is (so an inspector must never request one).
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use wega_adapter_blitz::BlitzDocument;
use wega_dom::{
    DocumentView, Mutator, NodeKey,
    layout::{GeometryAccess, HitTest, LayoutControl, Point, Rect, ViewportConfig},
};

/// Exactly the fixture of the engine's `known_gap_nesting_of_122_blocks...` test: auto-height
/// blocks, no stylesheet.
fn nested(depth: usize) -> String {
    format!(
        "<!doctype html><body>{}x{}",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    )
}
/// The same nesting with a fixed height on every block (what an author might write).
fn nested_fixed(depth: usize) -> String {
    format!(
        "<!doctype html><style>div{{height:5px}}</style><body>{}x{}",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    )
}
fn viewport() -> ViewportConfig {
    ViewportConfig {
        width_px: 400,
        height_px: 300,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    }
}
fn document(html: &str) -> BlitzDocument {
    let mut doc =
        BlitzDocument::new(HtmlDocument::from_html(html, DocumentConfig::default()).into_inner());
    doc.set_viewport(viewport());
    doc
}
/// The innermost `<div>` of `nested(depth)`.
fn deepest(doc: &BlitzDocument) -> NodeKey {
    let mut key = doc.root();
    loop {
        let node = doc.node(key).unwrap();
        let next = wega_dom::NodeView::children(node).find(|child| {
            doc.node(*child).is_some_and(|n| {
                wega_dom::NodeView::element_name(n)
                    .is_some_and(|e| e.local != "style" && e.local != "head")
            })
        });
        match next {
            Some(child) => key = child,
            None => return key,
        }
    }
}
/// Run `work` on a thread; fail if it does not finish within `limit`.
fn within<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(limit).ok()
}
/// Every geometry reader of the contract, on `key`.
fn read_everything(doc: &BlitzDocument, key: NodeKey) -> (bool, Rect, usize, f64, f64, bool) {
    let mut rects = Vec::new();
    doc.client_rects(key, &mut rects).unwrap();
    let client = doc.client_box(key).unwrap();
    let _ = (
        doc.offset_box(key).unwrap(),
        doc.scroll_size(key).unwrap(),
        doc.scroll_offset(key).unwrap(),
        doc.content_box(key).unwrap(),
    );
    let _ = doc.viewport();
    let _ = doc.element_at(Point { x: 10.0, y: 10.0 });
    (
        doc.has_boxes(key).unwrap(),
        doc.bounding_rect(key).unwrap(),
        rects.len(),
        client.width,
        client.height,
        doc.layout_pending(),
    )
}

#[test]
fn readers_on_a_pathological_document_that_was_never_laid_out_return_immediately() {
    for depth in [121, 122, 300, 2000] {
        let started = Instant::now();
        let result = within(Duration::from_secs(10), move || {
            let doc = document(&nested(depth));
            let leaf = deepest(&doc);
            read_everything(&doc, leaf)
        })
        .unwrap_or_else(|| panic!("depth {depth}: readers hung"));
        assert!(started.elapsed() < Duration::from_secs(5), "depth {depth}");
        let (has_boxes, rect, rects, width, height, pending) = result;
        assert!(!has_boxes, "depth {depth}: nothing has been laid out");
        assert_eq!(
            (rect, rects, width, height),
            (Rect::ZERO, 0, 0.0, 0.0),
            "depth {depth}"
        );
        assert!(
            pending,
            "depth {depth}: a document that was never resolved reports layout pending"
        );
    }
}

#[test]
fn readers_after_a_mutation_report_the_previous_layout_and_say_it_is_stale() {
    let mut doc = document(&nested_fixed(20));
    doc.flush_layout(0.0);
    let leaf = deepest(&doc);
    assert!(
        !doc.layout_pending(),
        "a resolved document has nothing pending"
    );
    let (_, clean_rect, ..) = read_everything(&doc, leaf);
    assert!(
        clean_rect.height > 0.0,
        "the leaf has a box: {clean_rect:?}"
    );
    // Change something that would move the box. Nothing flushes: the readers still say what the
    // last layout said, and `layout_pending` flags that this is not current.
    wega_dom::mutate(&mut doc)
        .set_attribute(
            leaf,
            html5ever::QualName::new(None, html5ever::ns!(), html5ever::LocalName::from("style")),
            "height:200px;margin:30px",
        )
        .unwrap();
    let (_, stale_rect, _, _, _, pending) = read_everything(&doc, leaf);
    assert_eq!(stale_rect, clean_rect, "readers must not recompute");
    assert!(pending, "a mutation leaves layout pending");
    // Only an explicit flush brings it current.
    doc.flush_layout(0.0);
    assert!(!doc.layout_pending());
    let (_, fresh_rect, ..) = read_everything(&doc, leaf);
    assert!(
        fresh_rect.height > clean_rect.height,
        "{fresh_rect:?} vs {clean_rect:?}"
    );
}

#[test]
fn an_empty_document_and_a_document_waiting_for_resources_are_pending_not_a_panic() {
    let empty = BlitzDocument::empty(Default::default());
    assert!(empty.layout_pending(), "no elements: there is no layout");
    let doc = BlitzDocument::empty(Default::default());
    let _ = doc.viewport();
    assert!(doc.layout_pending());
}

/// A document whose viewport was never configured (the state of a view before its first resize).
fn unconfigured(html: &str) -> BlitzDocument {
    BlitzDocument::new(HtmlDocument::from_html(html, DocumentConfig::default()).into_inner())
}

#[test]
fn the_adapter_alone_does_not_reproduce_the_nested_block_hang() {
    // Observation, pinned: at the adapter (Blitz's own parser, default config, no broker fonts)
    // 122 nested auto-height blocks with text lay out quickly, with and without a configured
    // viewport. The hang of roadmap B1 therefore needs something in the engine's pipeline; the
    // engine-level tests (`reliability.rs`) pin where: a *script-initiated first flush before any
    // paint* hangs, while the same page lays out in ~0.3 s once a paint has configured the view.
    // Either way the readers never flush, and `layout_pending` is the inspector's only question.
    for configured in [true, false] {
        let result = within(Duration::from_secs(10), move || {
            let mut doc = if configured {
                document(&nested(122))
            } else {
                unconfigured(&nested(122))
            };
            doc.flush_layout(0.0);
            !doc.layout_pending()
        });
        assert_eq!(result, Some(true), "configured viewport: {configured}");
    }
}
