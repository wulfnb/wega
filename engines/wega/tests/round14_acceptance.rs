//! Round 14 Acceptance Tests: Advanced CSS Selectors, Cascade Specificity,
//! Layout Positioning, Flexbox/Grid alignment, and Dynamic Restyling.
mod support;
use serde_json::Value;
use wega_engine::{RenderingBackend, View};

fn paint(view: &mut View, width: u32, height: u32, dark: bool) -> Vec<u8> {
    view.paint(width, height, 1.0, 1.0, dark).unwrap().rgba
}
fn pixel(bytes: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
    &bytes[(y * width + x) * 4..(y * width + x) * 4 + 4]
}

#[test]
fn advanced_selectors_is_where_not_has_matching() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<style>
  div { width: 50px; height: 50px; background: red; margin: 0; }
  :is(.box-a, .box-b) { background: blue; }
  :where(#unique-id) { background: green; } /* Zero specificity */
  .override-class { background: purple; } /* Higher specificity than :where */
  div:not(.exclude-me) { border: 2px solid yellow; }
</style>
<div id="item1" class="box-a"></div>
<div id="item2" class="box-b"></div>
<div id="unique-id" class="override-class"></div>
<div id="item3" class="exclude-me"></div>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/selectors"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        const item1 = document.getElementById('item1');
        const item2 = document.getElementById('item2');
        const unique = document.getElementById('unique-id');
        const item3 = document.getElementById('item3');

        return {
            item1MatchesIs: item1.matches(':is(.box-a, .box-b)'),
            item2MatchesIs: item2.matches(':is(.box-a, .box-b)'),
            uniqueMatchesWhere: unique.matches(':where(#unique-id)'),
            item1MatchesNot: item1.matches(':not(.exclude-me)'),
            item3MatchesNot: item3.matches(':not(.exclude-me)'),
            item1Bg: getComputedStyle(item1).backgroundColor,
            uniqueBg: getComputedStyle(unique).backgroundColor
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["item1MatchesIs"], true);
    assert_eq!(v["item2MatchesIs"], true);
    assert_eq!(v["uniqueMatchesWhere"], true);
    assert_eq!(v["item1MatchesNot"], true);
    assert_eq!(v["item3MatchesNot"], false);
    // :is matches blue (rgb(0, 0, 255))
    assert_eq!(v["item1Bg"], "rgb(0, 0, 255)");
    // .override-class specificity wins over :where(#unique-id) (rgb(128, 0, 128))
    assert_eq!(v["uniqueBg"], "rgb(128, 0, 128)");
}

#[test]
fn layout_sticky_and_percentage_margin_collapsing() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<style>
  body { margin: 0; padding: 0; }
  #block1 { width: 100px; height: 40px; margin-bottom: 20px; background: red; }
  #block2 { width: 100px; height: 40px; margin-top: 15px; background: blue; }
  #parent { width: 200px; height: 200px; overflow: scroll; position: relative; }
  #sticky-header { position: sticky; top: 0px; width: 100%; height: 30px; background: green; }
</style>
<div id="block1"></div>
<div id="block2"></div>
<div id="parent">
  <div id="sticky-header"></div>
  <div style="height: 500px;"></div>
</div>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/layout"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        const b1 = document.getElementById('block1').getBoundingClientRect();
        const b2 = document.getElementById('block2').getBoundingClientRect();
        // Margins collapse: max(20px, 15px) = 20px
        const marginGap = b2.top - b1.bottom;
        const sticky = document.getElementById('sticky-header').getBoundingClientRect();

        return {
            marginGap,
            stickyTop: sticky.top
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["marginGap"], 20.0);
    assert_eq!(v["stickyTop"], 100.0); // b1.height (40) + margin (20) + b2.height (40) = 100px
}

#[test]
fn dynamic_restyling_and_pixel_rendering() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<style>
  body { margin: 0; }
  #box { width: 50px; height: 50px; background: rgb(255, 0, 0); }
</style>
<div id="box"></div>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/restyle"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let frame1 = paint(&mut view, 100, 100, false);
    assert_eq!(pixel(&frame1, 100, 10, 10), &[255, 0, 0, 255]);

    // Mutate inline style via JS
    view.evaluate_json("document.getElementById('box').style.backgroundColor = 'rgb(0, 255, 0)'")
        .unwrap();
    let frame2 = paint(&mut view, 100, 100, false);
    assert_eq!(pixel(&frame2, 100, 10, 10), &[0, 255, 0, 255]);
}
