use std::{sync::Arc, time::Duration};
use url::Url;
use wega_engine::{Engine, Limits, NetworkPolicy, View};

fn test_view(html: &str) -> View {
    let temp = std::env::temp_dir().join(format!("wega-canvas-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp);
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(5),
            ..Limits::default()
        },
        NetworkPolicy::default(),
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(temp);
    let url = Url::parse("https://example.com/index.html").unwrap();
    let mut view = engine.create_view(url).unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view
}

fn eval_json(view: &mut View, script: &str) -> serde_json::Value {
    let raw = view.evaluate_json(script).unwrap();
    serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null)
}

#[test]
fn canvas_2d_raster_surface_draw_and_pixel_readback() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="100" height="50"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const canvas = document.getElementById("c");
        const ctx = canvas.getContext("2d");
        // Fill left half with red
        ctx.fillStyle = "#ff0000";
        ctx.fillRect(0, 0, 50, 50);
        // Fill right half with green
        ctx.fillStyle = "#00ff00";
        ctx.fillRect(50, 0, 50, 50);

        const imgData = ctx.getImageData(0, 0, 100, 50);
        // Left pixel (0, 0) -> Red
        const r1 = imgData.data[0];
        const g1 = imgData.data[1];
        const b1 = imgData.data[2];
        const a1 = imgData.data[3];
        // Right pixel (50, 0) -> Green
        const offset = (50 * 4);
        const r2 = imgData.data[offset];
        const g2 = imgData.data[offset + 1];
        const b2 = imgData.data[offset + 2];
        const a2 = imgData.data[offset + 3];

        ({ r1, g1, b1, a1, r2, g2, b2, a2 })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("r1").and_then(|v| v.as_u64()), Some(255));
    assert_eq!(res.get("g1").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(res.get("a1").and_then(|v| v.as_u64()), Some(255));

    assert_eq!(res.get("r2").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(res.get("g2").and_then(|v| v.as_u64()), Some(255));
    assert_eq!(res.get("a2").and_then(|v| v.as_u64()), Some(255));
}

#[test]
fn canvas_paths_curves_arcs_and_hit_testing() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="200" height="200"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const canvas = document.getElementById("c");
        const ctx = canvas.getContext("2d");

        ctx.beginPath();
        ctx.rect(10, 10, 80, 80);
        const inRect = ctx.isPointInPath(50, 50);
        const outRect = ctx.isPointInPath(5, 5);

        ctx.beginPath();
        ctx.arc(100, 100, 40, 0, Math.PI * 2);
        const inArc = ctx.isPointInPath(100, 100);
        const outArc = ctx.isPointInPath(160, 160);

        ({ inRect, outRect, inArc, outArc })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("inRect").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("outRect").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(res.get("inArc").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("outArc").and_then(|v| v.as_bool()), Some(false));
}

#[test]
fn canvas_text_rendering_and_font_metrics() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="300" height="150"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const canvas = document.getElementById("c");
        const ctx = canvas.getContext("2d");
        ctx.font = "24px sans-serif";
        const metrics = ctx.measureText("Hello Wega Standalone Engine");
        ctx.fillText("Hello Wega", 10, 50);

        ({
            width: metrics.width,
            ascent: metrics.actualBoundingBoxAscent,
            descent: metrics.actualBoundingBoxDescent
        })
    "####;

    let res = eval_json(&mut view, script);
    let width = res.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!(
        width > 50.0,
        "Measured text width should be > 50, got {}",
        width
    );
}

#[test]
fn canvas_to_data_url_and_to_blob_serialization() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="64" height="64"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const canvas = document.getElementById("c");
        const ctx = canvas.getContext("2d");
        ctx.fillStyle = "#0000ff";
        ctx.fillRect(0, 0, 64, 64);

        const dataUrl = canvas.toDataURL("image/png");
        let blobType = "";
        let blobSize = 0;
        canvas.toBlob((b) => {
            if (b) {
                blobType = b.type;
                blobSize = b.size;
            }
        }, "image/png");

        ({ dataUrl, validPrefix: dataUrl.startsWith("data:image/png;base64,") })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("validPrefix").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn canvas_resizing_resets_bitmap_and_drawing_state() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="50" height="50"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const canvas = document.getElementById("c");
        const ctx = canvas.getContext("2d");
        ctx.fillStyle = "#ff0000";
        ctx.fillRect(0, 0, 50, 50);
        ctx.lineWidth = 10;
        ctx.globalAlpha = 0.5;

        // Change width -> per spec, resizing resets the bitmap and all context drawing state
        canvas.width = 100;

        const imgData = ctx.getImageData(0, 0, 10, 10);
        // After resize, pixel buffer must be cleared to transparent black (all 0s)
        const isTransparent = imgData.data[0] === 0 && imgData.data[3] === 0;

        ({
            width: canvas.width,
            isTransparent,
            defaultFill: ctx.fillStyle,
            defaultLineWidth: ctx.lineWidth,
            defaultAlpha: ctx.globalAlpha
        })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("width").and_then(|v| v.as_u64()), Some(100));
    assert_eq!(
        res.get("isTransparent").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        res.get("defaultFill").and_then(|v| v.as_str()),
        Some("#000000")
    );
    assert_eq!(
        res.get("defaultLineWidth").and_then(|v| v.as_f64()),
        Some(1.0)
    );
    assert_eq!(res.get("defaultAlpha").and_then(|v| v.as_f64()), Some(1.0));
}

#[test]
fn canvas_tainting_blocks_cross_origin_readback_with_security_error() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="50" height="50"></canvas>
        <img id="crossImg" src="https://other.example.com/image.png">
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const canvas = document.getElementById("c");
        const ctx = canvas.getContext("2d");
        const img = document.getElementById("crossImg");

        // Drawing cross-origin untrusted image taints the canvas
        ctx.drawImage(img, 0, 0);

        let getImageDataThrew = false;
        try {
            ctx.getImageData(0, 0, 10, 10);
        } catch (e) {
            getImageDataThrew = (e.name === "SecurityError");
        }

        let toDataURLThrew = false;
        try {
            canvas.toDataURL();
        } catch (e) {
            toDataURLThrew = (e.name === "SecurityError");
        }

        ({ getImageDataThrew, toDataURLThrew })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(
        res.get("getImageDataThrew").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        res.get("toDataURLThrew").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn canvas_memory_accounting_and_quota_enforcement() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c1" width="100" height="100"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const c1 = document.getElementById("c1");
        const ctx1 = c1.getContext("2d");

        // 100 * 100 * 4 = 40,000 bytes
        const mem1 = HTMLCanvasElement.getMemoryUsage();

        // Create large canvas that exceeds quota
        let quotaExceeded = false;
        try {
            // Attempting to create an oversized 50000x50000 canvas exceeds memory limit
            new OffscreenCanvas(50000, 50000);
        } catch (e) {
            quotaExceeded = String(e.message || e).includes("QuotaExceededError");
        }

        // Destroy c1 and verify memory reclaimed
        HTMLCanvasElement.destroy(c1);
        const memAfter = HTMLCanvasElement.getMemoryUsage();

        ({
            allocated: mem1 > 0,
            quotaExceeded,
            reclaimed: memAfter < mem1
        })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("allocated").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        res.get("quotaExceeded").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(res.get("reclaimed").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn offscreencanvas_context_and_worker_transfer() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="80" height="80"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const canvas = document.getElementById("c");
        const offscreen = canvas.transferControlToOffscreen();

        let elemContextThrew = false;
        try {
            canvas.getContext("2d");
        } catch (e) {
            elemContextThrew = (e.name === "InvalidStateError");
        }

        const octx = offscreen.getContext("2d");
        octx.fillStyle = "#ff0000";
        octx.fillRect(0, 0, 40, 40);

        // Offscreen transfer detaches source
        const transferred = offscreen.transfer();
        let detachedThrew = false;
        try {
            offscreen.getContext("2d");
        } catch (e) {
            detachedThrew = (e.name === "InvalidStateError");
        }

        ({
            elemContextThrew,
            hasContext: !!octx,
            detachedThrew,
            transferredWidth: transferred.width
        })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(
        res.get("elemContextThrew").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(res.get("hasContext").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        res.get("detachedThrew").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        res.get("transferredWidth").and_then(|v| v.as_u64()),
        Some(80)
    );
}
