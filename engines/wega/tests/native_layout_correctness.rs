use std::{cell::RefCell, rc::Rc};
use wega_dom::DocumentView;
use wega_engine::{
    document::{DocumentSetup, WegaDocument},
    native_engine::*,
};
fn document(html: &str) -> NativeDocument<WegaDocument> {
    let mut d = NativeDocument::new(Rc::new(RefCell::new(WegaDocument::from_html(
        html,
        DocumentSetup::default(),
    ))));
    d.flush_layout();
    d
}
fn rect(d: &NativeDocument<WegaDocument>, id: &str) -> wega_dom::layout::Rect {
    let key = d.doc.borrow().element_by_id(id).unwrap();
    d.bounding_rect(key).unwrap()
}
#[test]
fn blocks_margins_containing_blocks_and_descendant_translation() {
    let d = document(
        "<body style='margin:0'><div style='width:400px;height:200px;position:relative'><div id=center style='width:100px;height:10px;margin:auto'></div><div id=percent style='height:50%;width:50%'></div><div id=bottom style='position:absolute;bottom:10px;right:20px;width:30px;height:40px'><b id=inner>x</b></div></div><div id=one style='height:20px;margin-bottom:30px'></div><div id=two style='height:20px;margin-top:10px'></div><div id=relative style='position:relative;left:50px;top:20px'><p id=relchild>hello</p></div></body>",
    );
    assert_eq!(rect(&d, "center").x, 150.0);
    assert_eq!(rect(&d, "percent").width, 200.0);
    assert_eq!(rect(&d, "percent").height, 100.0);
    assert_eq!(rect(&d, "bottom").x, 350.0);
    assert_eq!(rect(&d, "bottom").y, 150.0);
    assert!(rect(&d, "inner").x >= 350.0);
    assert_eq!(rect(&d, "two").y - rect(&d, "one").bottom(), 30.0);
    assert_eq!(rect(&d, "relchild").x, 50.0);
}
#[test]
fn flex_weighted_shrink_wrap_column_reverse_and_nested_content() {
    let d = document(
        "<body style='margin:0'><div style='display:flex;width:300px'><div id=a style='width:200px;min-width:0'>a</div><div id=b style='width:400px;min-width:0'><p id=nested>b</p></div></div><div style='display:flex;width:210px;flex-wrap:wrap;gap:10px'><div id=w1 style='width:100px;height:20px'></div><div id=w2 style='width:100px;height:30px'></div><div id=w3 style='width:100px;height:20px'></div></div><div id=col style='display:flex;flex-direction:column-reverse;width:100px;height:300px'><div id=c1 style='height:100px;flex-grow:1'></div><div id=c2 style='height:100px;flex-grow:1'></div></div></body>",
    );
    assert!((rect(&d, "a").width - 100.0).abs() < 0.01);
    assert!((rect(&d, "b").width - 200.0).abs() < 0.01);
    assert_eq!(rect(&d, "nested").x, 100.0);
    assert_eq!(rect(&d, "w2").x, 110.0);
    assert_eq!(rect(&d, "w3").y - rect(&d, "w1").y, 40.0);
    assert_eq!(rect(&d, "c1").height, 150.0);
    assert_eq!(rect(&d, "c1").y - rect(&d, "col").y, 150.0);
    assert_eq!(rect(&d, "c2").y, rect(&d, "col").y);
}
#[test]
fn grid_tracks_spans_and_content_rows() {
    let d = document(
        "<body style='margin:0'><div id=g style='display:grid;width:400px;grid-template-columns:minmax(0px,1fr) 2fr;gap:10px'><div id=a style='height:25px'></div><div id=b style='height:40px'></div><div id=c style='grid-column:1 / span 2;height:30px'><p id=nested style='margin:0'>nested</p></div></div></body>",
    );
    assert_eq!(rect(&d, "a").width, 130.0);
    assert_eq!(rect(&d, "b").width, 260.0);
    assert_eq!(rect(&d, "b").x, 140.0);
    assert_eq!(rect(&d, "c").width, 400.0);
    assert_eq!(rect(&d, "c").y, 50.0);
    assert_eq!(rect(&d, "g").height, 80.0);
    assert_eq!(rect(&d, "nested").y, 50.0);
}
#[test]
fn table_intrinsic_columns_spanning_and_real_row_height() {
    let d = document(
        "<body style='margin:0'><table id=t style='width:300px;border-spacing:0px'><tr><td id=a rowspan=2>A</td><td id=b>Substantially wider column</td></tr><tr><td id=c>C</td></tr><tr><td id=d colspan=2>D</td></tr></table></body>",
    );
    assert!(rect(&d, "b").width > rect(&d, "a").width);
    assert_eq!(rect(&d, "c").x, rect(&d, "b").x);
    assert_eq!(
        rect(&d, "a").height,
        rect(&d, "b").height + rect(&d, "c").height
    );
    assert_eq!(rect(&d, "d").width, 300.0);
    // One 20px line plus the UA's 1px cell padding above and below.
    assert_eq!(rect(&d, "d").height, 22.0);
}

#[test]
fn constrained_fraction_tracks_and_border_box_min_max() {
    let d = document(
        "<body style='margin:0'><div style='display:grid;width:200px;grid-template-columns:minmax(150px,1fr) 1fr'><div id=a></div><div id=b></div></div><div id=limited style='box-sizing:border-box;width:300px;height:300px;max-width:200px;max-height:100px;padding:20px;border:5px solid black'></div></body>",
    );
    assert_eq!(rect(&d, "a").width, 150.0);
    assert_eq!(rect(&d, "b").width, 50.0);
    assert_eq!(rect(&d, "limited").width, 200.0);
    assert_eq!(rect(&d, "limited").height, 100.0);
}
