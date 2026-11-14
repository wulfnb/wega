//! Stage timings of the paint path (current code only): recording the scene (layout flush,
//! paint preparation, the painter) versus rasterising it. Run:
//! `cargo test --release -p wega-engine --test render_bench -- --ignored --nocapture`
use std::sync::Arc;
use std::time::Instant;
use wega_engine::{Engine, FrameTarget, Limits, NetworkPolicy, Url, ViewportConfig};

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}
#[test]
#[ignore]
fn render_stages() {
    let engine = Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let mut view = engine
        .create_view(Url::parse("https://bench.test/").unwrap())
        .unwrap();
    view.set_html("<!doctype html><style>body{margin:0;font:13px sans-serif}.row{height:18px;padding:0 6px;border-bottom:1px solid #ddd}.row:nth-child(odd){background:#f6f6f6}#box{position:absolute;left:300px;top:20px;width:120px;height:80px;background:#e33;opacity:.8;overflow:hidden}</style><div id=list></div><div id=box></div><script>const l=document.getElementById('list');for(let i=0;i<40;i++){const d=document.createElement('div');d.className='row';d.textContent='Row number '+i+' with some body text';l.appendChild(d)}</script>").unwrap();
    view.poll();
    let rss = wega_platform::resident_bytes();
    for (name, w, h, scale) in [
        ("800x600 @1x", 800, 600, 1.0),
        ("1600x1200 @2x", 1600, 1200, 2.0),
    ] {
        let viewport = ViewportConfig {
            width_px: w,
            height_px: h,
            scale,
            zoom: 1.0,
            dark: false,
        };
        let (mut record, mut raster) = (vec![], vec![]);
        for i in 0..40 {
            view.request_redraw();
            let start = Instant::now();
            view.begin_paint(viewport, i, FrameTarget::Owned).unwrap();
            let recorded = start.elapsed();
            view.wait_painted().unwrap();
            let total = start.elapsed();
            if i >= 5 {
                record.push(recorded.as_secs_f64() * 1e3);
                raster.push((total - recorded).as_secs_f64() * 1e3);
            }
        }
        println!(
            "BENCH {name} record scene (flush+paint prep): {:.2} ms",
            median(record)
        );
        println!(
            "BENCH {name} rasterise and return pixels: {:.2} ms",
            median(raster)
        );
    }
    let before = wega_platform::resident_bytes();
    for i in 0..300 {
        view.request_redraw();
        view.begin_paint(
            ViewportConfig {
                width_px: 800,
                height_px: 600,
                scale: 1.0,
                zoom: 1.0,
                dark: false,
            },
            i,
            FrameTarget::Owned,
        )
        .unwrap();
        view.wait_painted().unwrap();
    }
    let after = wega_platform::resident_bytes();
    println!(
        "BENCH resident memory at start {:?} MiB, before loop {:?}, after 300 frames {:?}",
        rss.map(|b| b >> 20),
        before.map(|b| b >> 20),
        after.map(|b| b >> 20)
    );
}
