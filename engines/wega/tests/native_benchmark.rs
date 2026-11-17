//! Checkpoint E: Native vs Blitz Performance Benchmarks.
mod support;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::layout::{LayoutControl, ViewportConfig};
use wega_dom::render::PaintSource;
use wega_engine::native_engine::*;

#[test]
fn test_native_vs_blitz_benchmarks() {
    let html = r#"
        <!doctype html>
        <html>
        <head>
            <style>
                .grid { display: grid; grid-template-columns: 100px 100px 100px; }
                .card { margin: 10px; padding: 15px; border: 2px solid #333333; border-radius: 8px; }
                .flex-row { display: flex; flex-direction: row; justify-content: space-between; }
                .badge { width: 50px; height: 20px; background-color: #00aa00; }
                p { font-size: 14px; line-height: 18px; color: #222222; }
            </style>
        </head>
        <body style="margin: 0;">
            <div class="grid">
                <div class="card">
                    <div class="flex-row">
                        <div class="badge">A1</div>
                        <div class="badge">A2</div>
                    </div>
                    <p>Sample card paragraph with wrapped text inside native rendering engine benchmark test.</p>
                </div>
                <div class="card">
                    <div class="flex-row">
                        <div class="badge">B1</div>
                        <div class="badge">B2</div>
                    </div>
                    <p>Another card paragraph with text wrapping and computed style evaluation metrics.</p>
                </div>
                <div class="card">
                    <div class="flex-row">
                        <div class="badge">C1</div>
                        <div class="badge">C2</div>
                    </div>
                    <p>Third column testing grid layout performance and skia display list generation throughput.</p>
                </div>
            </div>
        </body>
        </html>
    "#;

    let viewport = ViewportConfig {
        width_px: 1280,
        height_px: 800,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    };

    // 1. Benchmark Native Engine
    let iterations = 100;

    let native_start = Instant::now();
    for _ in 0..iterations {
        let doc = BlitzDocument::from_html(html, DocumentSetup::default());
        let mut native_doc = NativeDocument::new(Rc::new(RefCell::new(doc)));
        native_doc.flush_layout();
        let _scene = native_doc.record_scene(&viewport);
    }
    let native_duration = native_start.elapsed();
    let native_avg_ms = native_duration.as_secs_f64() * 1000.0 / iterations as f64;

    // 2. Benchmark Blitz Fallback
    let blitz_start = Instant::now();
    for _ in 0..iterations {
        let mut doc = BlitzDocument::from_html(html, DocumentSetup::default());
        doc.flush_layout(0.0);
        let _scene = doc.record_scene(&viewport);
    }
    let blitz_duration = blitz_start.elapsed();
    let blitz_avg_ms = blitz_duration.as_secs_f64() * 1000.0 / iterations as f64;

    println!("\n=== WEGA NATIVE ENGINE BENCHMARK RESULTS ===");
    println!(
        "Document + Layout + Scene Recording ({} iterations):",
        iterations
    );
    println!("  Native Wega Engine : {:.3} ms/cycle", native_avg_ms);
    println!("  Blitz Fallback     : {:.3} ms/cycle", blitz_avg_ms);
    println!(
        "  Relative Ratio     : {:.2}x",
        native_avg_ms / blitz_avg_ms.max(0.001)
    );
    println!("============================================\n");

    assert!(
        native_avg_ms <= blitz_avg_ms * 1.5,
        "Native engine cycle ({:.3}ms) must be competitive with Blitz fallback ({:.3}ms)",
        native_avg_ms,
        blitz_avg_ms
    );
}
