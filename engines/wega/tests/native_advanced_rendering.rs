//! Checkpoint C: Advanced Rendering Integration Tests.
mod support;

use std::cell::RefCell;
use std::rc::Rc;
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::DocumentView;
use wega_engine::native_engine::*;

#[test]
fn test_css_grid_layout() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="grid-container" style="display: grid; grid-template-columns: 150px 250px; width: 400px; height: 100px;">
                <div id="cell1">Cell 1</div>
                <div id="cell2">Cell 2</div>
                <div id="cell3">Cell 3</div>
                <div id="cell4">Cell 4</div>
            </div>
        </body>
    "#;
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let k1 = d.element_by_id("cell1").unwrap();
    let k2 = d.element_by_id("cell2").unwrap();
    let k3 = d.element_by_id("cell3").unwrap();
    let k4 = d.element_by_id("cell4").unwrap();
    drop(d);

    let b1 = native_doc.find_box(k1).unwrap();
    let b2 = native_doc.find_box(k2).unwrap();
    let b3 = native_doc.find_box(k3).unwrap();
    let b4 = native_doc.find_box(k4).unwrap();

    // Row 0: cell 1 at (0, 0), cell 2 at (150, 0)
    assert_eq!(b1.border_box.x, 0.0);
    assert_eq!(b1.border_box.y, 0.0);
    assert_eq!(b1.border_box.width, 150.0);

    assert_eq!(b2.border_box.x, 150.0);
    assert_eq!(b2.border_box.y, 0.0);
    assert_eq!(b2.border_box.width, 250.0);

    // Row 1: cell 3 at (0, 40), cell 4 at (150, 40)
    assert_eq!(b3.border_box.x, 0.0);
    assert_eq!(b3.border_box.y, 40.0);
    assert_eq!(b3.border_box.width, 150.0);

    assert_eq!(b4.border_box.x, 150.0);
    assert_eq!(b4.border_box.y, 40.0);
    assert_eq!(b4.border_box.width, 250.0);
}

#[test]
fn test_table_layout() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <table id="tbl" style="display: table; width: 300px;">
                <tr style="display: table-row;">
                    <td id="td1" style="display: table-cell;">A1</td>
                    <td id="td2" style="display: table-cell;">B1</td>
                </tr>
                <tr style="display: table-row;">
                    <td id="td3" style="display: table-cell;">A2</td>
                    <td id="td4" style="display: table-cell;">B2</td>
                </tr>
            </table>
        </body>
    "#;
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let k1 = d.element_by_id("td1").unwrap();
    let k2 = d.element_by_id("td2").unwrap();
    let k3 = d.element_by_id("td3").unwrap();
    let k4 = d.element_by_id("td4").unwrap();
    drop(d);

    let b1 = native_doc.find_box(k1).unwrap();
    let b2 = native_doc.find_box(k2).unwrap();
    let b3 = native_doc.find_box(k3).unwrap();
    let b4 = native_doc.find_box(k4).unwrap();

    // 2 columns across 300px = 150px each
    assert_eq!(b1.border_box.width, 150.0);
    assert_eq!(b2.border_box.width, 150.0);
    assert_eq!(b1.border_box.x, 0.0);
    assert_eq!(b2.border_box.x, 150.0);

    // Row 2 starts at row 1 height
    assert_eq!(b3.border_box.x, 0.0);
    assert!(b3.border_box.y >= 30.0);
    assert_eq!(b4.border_box.x, 150.0);
    assert!(b4.border_box.y >= 30.0);
}

#[test]
fn test_stacking_contexts_and_z_index_paint_order() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="stack-parent" style="position: relative; width: 400px; height: 300px; background-color: #ffffff;">
                <div id="pos-child" style="position: relative; z-index: 10; width: 50px; height: 50px; background-color: #ff0000;">Pos</div>
                <div id="normal-child" style="width: 50px; height: 50px; background-color: #00ff00;">Normal</div>
                <div id="neg-child" style="position: relative; z-index: -5; width: 50px; height: 50px; background-color: #0000ff;">Neg</div>
            </div>
        </body>
    "#;
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let parent_key = d.element_by_id("stack-parent").unwrap();
    drop(d);

    let parent_box = native_doc.find_box(parent_key).unwrap();
    let dl = DisplayList::build(parent_box, &[], 1.0);

    // Verify ordering of FillRect items
    // Negative child background (blue: #0000ff) must be emitted BEFORE normal flow child (green: #00ff00),
    // and normal flow child must be emitted BEFORE positive child (red: #ff0000).
    let fills: Vec<Color> = dl
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::FillRect { color, .. } => Some(*color),
            _ => None,
        })
        .collect();

    let blue = Color {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    };
    let green = Color {
        r: 0,
        g: 255,
        b: 0,
        a: 255,
    };
    let red = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };

    let idx_blue = fills
        .iter()
        .position(|c| *c == blue)
        .expect("Blue fill should be present");
    let idx_green = fills
        .iter()
        .position(|c| *c == green)
        .expect("Green fill should be present");
    let idx_red = fills
        .iter()
        .position(|c| *c == red)
        .expect("Red fill should be present");

    assert!(
        idx_blue < idx_green,
        "Negative z-index child must be painted before normal child: blue {} < green {}",
        idx_blue,
        idx_green
    );
    assert!(
        idx_green < idx_red,
        "Normal child must be painted before positive z-index child: green {} < red {}",
        idx_green,
        idx_red
    );
}

#[test]
fn test_transforms_and_box_shadow() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="transformed" style="transform: translate(25px, 50px); box-shadow: 4px 6px 8px #112233; border: 2px solid #334455; border-radius: 10px; width: 100px; height: 100px;">
                Transformed
            </div>
        </body>
    "#;
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let key = d.element_by_id("transformed").unwrap();
    drop(d);

    let b = native_doc.find_box(key).unwrap();
    let dl = DisplayList::build(b, &[], 1.0);

    let has_push_transform = dl
        .items
        .iter()
        .any(|item| matches!(item, DisplayItem::PushTransform { .. }));
    let has_pop_transform = dl
        .items
        .iter()
        .any(|item| matches!(item, DisplayItem::PopTransform));
    assert!(has_push_transform, "DisplayList must include PushTransform");
    assert!(has_pop_transform, "DisplayList must include PopTransform");

    let has_box_shadow = dl.items.iter().any(|item| {
        if let DisplayItem::BoxShadow {
            offset_x,
            offset_y,
            border_radius,
            ..
        } = item
        {
            *offset_x == 4.0 && *offset_y == 6.0 && *border_radius == 10.0
        } else {
            false
        }
    });
    assert!(
        has_box_shadow,
        "DisplayList must include BoxShadow with offset (4, 6) and border_radius 10"
    );

    let has_rounded_stroke = dl.items.iter().any(|item| {
        if let DisplayItem::StrokeRect { border_radius, .. } = item {
            *border_radius == 10.0
        } else {
            false
        }
    });
    assert!(
        has_rounded_stroke,
        "DisplayItem::StrokeRect must have border_radius 10.0"
    );
}

#[test]
fn test_iframe_viewport_composition_and_paint_ordering() {
    let html = r#"
        <!doctype html>
        <body style="margin: 0;">
            <div id="wrapper" style="position: relative; width: 600px; height: 400px;">
                <iframe id="test-frame" style="width: 300px; height: 200px; border: 1px solid black;"></iframe>
                <div id="overlay" style="position: relative; z-index: 20; width: 100px; height: 100px; background-color: #ff00ff;">Overlay</div>
            </div>
        </body>
    "#;
    let doc = BlitzDocument::from_html(html, DocumentSetup::default());
    let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
    native_doc.flush_layout();

    let d = native_doc.doc.borrow();
    let iframe_key = d.element_by_id("test-frame").unwrap();
    let wrapper_key = d.element_by_id("wrapper").unwrap();
    drop(d);

    // Create a mock child iframe scene
    let iframe_scene = anyrender::Scene::new();
    // Add child scene to embedded list
    let embedded = vec![(iframe_key, iframe_scene)];

    let wrapper_box = native_doc.find_box(wrapper_key).unwrap();
    let dl = DisplayList::build(wrapper_box, &embedded, 1.0);

    // Verify embedded scene is in display list
    let embedded_pos = dl
        .items
        .iter()
        .position(|item| matches!(item, DisplayItem::EmbeddedScene { .. }))
        .expect("EmbeddedScene should be present in display list");

    // Verify overlay (z-index: 20, magenta fill) is painted AFTER the embedded scene
    let magenta = Color {
        r: 255,
        g: 0,
        b: 255,
        a: 255,
    };
    let overlay_pos = dl
        .items
        .iter()
        .position(|item| {
            if let DisplayItem::FillRect { color, .. } = item {
                *color == magenta
            } else {
                false
            }
        })
        .expect("Overlay FillRect should be present");

    assert!(
        embedded_pos < overlay_pos,
        "Embedded iframe scene ({}) must be painted before z-index: 20 overlay ({})",
        embedded_pos,
        overlay_pos
    );
}
