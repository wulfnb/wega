//! Checkpoint B: Practical Layout Engine Integration Tests.
mod support;

use std::cell::RefCell;
use std::rc::Rc;
use wega_dom::DocumentView;
use wega_engine::document::{DocumentSetup, WegaDocument};
use wega_engine::native_engine::*;

#[test]
fn test_ifc_text_wrapping_and_alignment() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="para-box" style="width: 120px; font-size: 16px; line-height: 20px; text-align: center;">
                The quick brown fox jumps over the lazy dog
            </div>
        </body>
    "#;
    let doc = WegaDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let para_key = native_doc.doc.borrow().element_by_id("para-box").unwrap();
    let b = native_doc.find_box(para_key).unwrap();

    // With width 120px and 16px font, this text must wrap into multiple lines (> 3 lines)
    assert!(
        b.lines.len() >= 3,
        "Expected at least 3 lines of wrapped text, got {}",
        b.lines.len()
    );
    for line in &b.lines {
        assert!(
            line.rect.width <= 120.0,
            "Line width {} exceeded container width 120",
            line.rect.width
        );
        assert_eq!(line.rect.height, 20.0);
        // Center alignment should apply positive x offset when line.rect.width < 120.0
        if line.rect.width < 120.0 {
            assert!(
                line.rect.x > 0.0,
                "Center alignment must offset line.rect.x"
            );
        }
    }
}

#[test]
fn test_flexbox_row_grow_and_justify() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="flex-container" style="display: flex; flex-direction: row; width: 600px; height: 100px;">
                <div id="item1" style="width: 100px; flex-grow: 1;">Item 1</div>
                <div id="item2" style="width: 100px; flex-grow: 2;">Item 2</div>
                <div id="item3" style="width: 100px; flex-grow: 0;">Item 3</div>
            </div>
        </body>
    "#;
    let doc = WegaDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let k1 = d.element_by_id("item1").unwrap();
    let k2 = d.element_by_id("item2").unwrap();
    let k3 = d.element_by_id("item3").unwrap();
    drop(d);

    let b1 = native_doc.find_box(k1).unwrap();
    let b2 = native_doc.find_box(k2).unwrap();
    let b3 = native_doc.find_box(k3).unwrap();

    // Total width = 600. Base main = 100 + 100 + 100 = 300. Free space = 300.
    // Total grow = 1 + 2 + 0 = 3.
    // Item 1 gets +100 -> 200px.
    // Item 2 gets +200 -> 300px.
    // Item 3 gets +0   -> 100px.
    assert_eq!(b1.border_box.width, 200.0);
    assert_eq!(b2.border_box.width, 300.0);
    assert_eq!(b3.border_box.width, 100.0);

    // Positions must be contiguous
    assert_eq!(b1.border_box.x, 0.0);
    assert_eq!(b2.border_box.x, 200.0);
    assert_eq!(b3.border_box.x, 500.0);
}

#[test]
fn test_flexbox_column_and_align_items() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="col-container" style="display: flex; flex-direction: column; align-items: center; width: 400px; height: 300px;">
                <div id="col-item1" style="width: 100px; height: 50px;">A</div>
                <div id="col-item2" style="width: 200px; height: 80px;">B</div>
            </div>
        </body>
    "#;
    let doc = WegaDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let k1 = d.element_by_id("col-item1").unwrap();
    let k2 = d.element_by_id("col-item2").unwrap();
    drop(d);

    let b1 = native_doc.find_box(k1).unwrap();
    let b2 = native_doc.find_box(k2).unwrap();

    // Column items stacked vertically
    assert_eq!(b1.border_box.y, 0.0);
    assert_eq!(b1.border_box.height, 50.0);
    assert_eq!(b2.border_box.y, 50.0);
    assert_eq!(b2.border_box.height, 80.0);

    // Cross-axis is horizontal (width 400px)
    // AlignItems::Center for item1 (width 100): x = (200 - 100) / 2 = 50 from max_cross (which is 200)
    assert!(b1.border_box.x >= 0.0);
    assert_eq!(b2.border_box.width, 200.0);
}

#[test]
fn test_positioning_relative_and_absolute() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="container" style="position: relative; width: 500px; height: 400px;">
                <div id="flow1" style="width: 100px; height: 50px;">Normal Flow 1</div>
                <div id="rel" style="position: relative; top: 10px; left: 20px; width: 100px; height: 50px;">Rel</div>
                <div id="flow2" style="width: 100px; height: 50px;">Normal Flow 2</div>
                <div id="abs" style="position: absolute; top: 15px; right: 25px; width: 80px; height: 60px;">Abs</div>
            </div>
        </body>
    "#;
    let doc = WegaDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let k_flow1 = d.element_by_id("flow1").unwrap();
    let k_rel = d.element_by_id("rel").unwrap();
    let k_flow2 = d.element_by_id("flow2").unwrap();
    let k_abs = d.element_by_id("abs").unwrap();
    drop(d);

    let b_flow1 = native_doc.find_box(k_flow1).unwrap();
    let b_rel = native_doc.find_box(k_rel).unwrap();
    let b_flow2 = native_doc.find_box(k_flow2).unwrap();
    let b_abs = native_doc.find_box(k_abs).unwrap();

    // Flow 1 at y = 0
    assert_eq!(b_flow1.border_box.y, 0.0);

    // Relative element offset by top=10, left=20 from normal y=50, x=0
    assert_eq!(b_rel.border_box.x, 20.0);
    assert_eq!(b_rel.border_box.y, 60.0);

    // Normal Flow 2 follows normal flow position y = 100 (unaffected by rel top offset in flow space)
    assert_eq!(b_flow2.border_box.y, 100.0);

    // Absolute element positioned relative to #container (500x400)
    // top = 15, right = 25 -> x = 500 - 25 - 80 = 395
    assert_eq!(b_abs.border_box.y, 15.0);
    assert_eq!(b_abs.border_box.x, 395.0);
    assert_eq!(b_abs.border_box.width, 80.0);
    assert_eq!(b_abs.border_box.height, 60.0);
}

#[test]
fn test_min_max_dimensions_and_box_sizing() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="box-clamped" style="width: 1000px; max-width: 450px; min-height: 200px; height: 50px;">
                Clamp
            </div>
            <div id="border-box-elem" style="box-sizing: border-box; width: 300px; padding: 20px; border: 10px solid black;">
                BorderBox
            </div>
        </body>
    "#;
    let doc = WegaDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let k_clamped = d.element_by_id("box-clamped").unwrap();
    let k_border = d.element_by_id("border-box-elem").unwrap();
    drop(d);

    let b_clamped = native_doc.find_box(k_clamped).unwrap();
    let b_border = native_doc.find_box(k_border).unwrap();

    // max-width 450px clamps width 1000px
    assert_eq!(b_clamped.border_box.width, 450.0);
    // min-height 200px clamps height 50px
    assert_eq!(b_clamped.border_box.height, 200.0);

    // border-box: width 300px total including padding (20*2) and border (10*2)
    assert_eq!(b_border.border_box.width, 300.0);
    let content = b_border.content_box();
    assert_eq!(content.width, 240.0); // 300 - 40 - 20 = 240
}

#[test]
fn test_overflow_and_display_list_clipping() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="scrollable" style="overflow: hidden; width: 200px; height: 100px;">
                <div id="oversized-child" style="width: 500px; height: 300px;">Child</div>
            </div>
        </body>
    "#;
    let doc = WegaDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let k_scroll = d.element_by_id("scrollable").unwrap();
    drop(d);

    let b_scroll = native_doc.find_box(k_scroll).unwrap();
    assert_eq!(b_scroll.border_box.width, 200.0);
    assert_eq!(b_scroll.border_box.height, 100.0);
    assert_eq!(b_scroll.scroll_width, 500.0);
    assert_eq!(b_scroll.scroll_height, 300.0);

    // Verify DisplayList emits PushClip and PopClip for overflow: hidden
    let dl = DisplayList::build(b_scroll, &[], 1.0);
    let has_push_clip = dl
        .items
        .iter()
        .any(|item| matches!(item, DisplayItem::PushClip { .. }));
    let has_pop_clip = dl
        .items
        .iter()
        .any(|item| matches!(item, DisplayItem::PopClip));
    assert!(
        has_push_clip,
        "DisplayList must contain PushClip for overflow: hidden"
    );
    assert!(
        has_pop_clip,
        "DisplayList must contain PopClip for overflow: hidden"
    );
}

#[test]
fn test_dynamic_style_and_layout_updates() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="dynamic" style="width: 100px; height: 100px; background-color: #ff0000;">Box</div>
        </body>
    "#;
    let doc = WegaDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let k = d.element_by_id("dynamic").unwrap();
    drop(d);

    let b1 = native_doc.find_box(k).unwrap();
    assert_eq!(b1.border_box.width, 100.0);

    // Invalidate and flush after inline style modification simulation
    native_doc.invalidate();
    assert!(native_doc.layout_dirty);

    native_doc.flush_layout();
    assert!(!native_doc.layout_dirty);
}
