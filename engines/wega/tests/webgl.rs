use std::{sync::Arc, time::Duration};
use url::Url;
use wega_engine::{Engine, Limits, NetworkPolicy, View};

fn test_view(html: &str) -> View {
    let temp = std::env::temp_dir().join(format!("wega-webgl-test-{}", std::process::id()));
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
fn webgl_context_creation_and_attributes() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c1" width="100" height="50"></canvas>
        <canvas id="c2" width="80" height="60"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const c1 = document.getElementById("c1");
        const gl = c1.getContext("webgl");
        const hasGl = !!gl;
        const width = gl.drawingBufferWidth;
        const height = gl.drawingBufferHeight;
        const hasConstants = (gl.COLOR_BUFFER_BIT === 0x4000) && (gl.TRIANGLES === 4);

        // Cannot mix 2d and webgl on same canvas
        const secondCtx = c1.getContext("2d");
        const mixedBlocked = (secondCtx === null);

        // WebGL2 support
        const c2 = document.getElementById("c2");
        const gl2 = c2.getContext("webgl2");
        const hasGl2 = !!gl2;

        ({ hasGl, width, height, hasConstants, mixedBlocked, hasGl2 })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("hasGl").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("width").and_then(|v| v.as_u64()), Some(100));
    assert_eq!(res.get("height").and_then(|v| v.as_u64()), Some(50));
    assert_eq!(
        res.get("hasConstants").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        res.get("mixedBlocked").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(res.get("hasGl2").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn webgl_clear_color_and_pixel_readback() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="50" height="50"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const c = document.getElementById("c");
        const gl = c.getContext("webgl");

        // Clear with pure red: RGBA(1.0, 0.0, 0.0, 1.0)
        gl.clearColor(1.0, 0.0, 0.0, 1.0);
        gl.clear(gl.COLOR_BUFFER_BIT);

        const pixels = new Uint8Array(4);
        gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);

        const r = pixels[0];
        const g = pixels[1];
        const b = pixels[2];
        const a = pixels[3];

        ({ r, g, b, a })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("r").and_then(|v| v.as_u64()), Some(255));
    assert_eq!(res.get("g").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(res.get("b").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(res.get("a").and_then(|v| v.as_u64()), Some(255));
}

#[test]
fn webgl_shaders_program_linking_and_introspection() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="100" height="100"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const c = document.getElementById("c");
        const gl = c.getContext("webgl");

        const vsSource = `
            attribute vec4 aPosition;
            uniform vec4 uColor;
            void main() {
                gl_Position = aPosition;
            }
        `;
        const fsSource = `
            precision mediump float;
            uniform vec4 uColor;
            void main() {
                gl_FragColor = uColor;
            }
        `;

        const vs = gl.createShader(gl.VERTEX_SHADER);
        gl.shaderSource(vs, vsSource);
        gl.compileShader(vs);
        const vsCompiled = gl.getShaderParameter(vs, gl.COMPILE_STATUS);

        const fs = gl.createShader(gl.FRAGMENT_SHADER);
        gl.shaderSource(fs, fsSource);
        gl.compileShader(fs);
        const fsCompiled = gl.getShaderParameter(fs, gl.COMPILE_STATUS);

        const prog = gl.createProgram();
        gl.attachShader(prog, vs);
        gl.attachShader(prog, fs);
        gl.linkProgram(prog);
        const linked = gl.getProgramParameter(prog, gl.LINK_STATUS);

        const posLoc = gl.getAttribLocation(prog, "aPosition");
        const colorLoc = gl.getUniformLocation(prog, "uColor");

        ({
            vsCompiled,
            fsCompiled,
            linked,
            hasPosLoc: posLoc >= 0,
            hasColorLoc: colorLoc !== null
        })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("vsCompiled").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("fsCompiled").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("linked").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("hasPosLoc").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("hasColorLoc").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn webgl_buffer_upload_and_draw_arrays() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="50" height="50"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const c = document.getElementById("c");
        const gl = c.getContext("webgl");

        const vs = gl.createShader(gl.VERTEX_SHADER);
        gl.shaderSource(vs, "attribute vec2 aPos; void main() { gl_Position = vec4(aPos, 0.0, 1.0); }");
        gl.compileShader(vs);

        const fs = gl.createShader(gl.FRAGMENT_SHADER);
        gl.shaderSource(fs, "precision mediump float; uniform vec4 uColor; void main() { gl_FragColor = uColor; }");
        gl.compileShader(fs);

        const prog = gl.createProgram();
        gl.attachShader(prog, vs);
        gl.attachShader(prog, fs);
        gl.linkProgram(prog);
        gl.useProgram(prog);

        // Upload vertex buffer
        const buf = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, buf);
        const vertices = new Float32Array([-1, -1, 1, -1, -1, 1]);
        gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);

        const posLoc = gl.getAttribLocation(prog, "aPos");
        gl.enableVertexAttribArray(posLoc);
        gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

        // Set uniform color to green: (0.0, 1.0, 0.0, 1.0)
        const colLoc = gl.getUniformLocation(prog, "uColor");
        gl.uniform4f(colLoc, 0.0, 1.0, 0.0, 1.0);

        gl.viewport(0, 0, 50, 50);
        gl.drawArrays(gl.TRIANGLES, 0, 3);

        const pixels = new Uint8Array(4);
        gl.readPixels(25, 25, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);

        ({
            r: pixels[0],
            g: pixels[1],
            b: pixels[2],
            a: pixels[3]
        })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("r").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(res.get("g").and_then(|v| v.as_u64()), Some(255));
    assert_eq!(res.get("b").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(res.get("a").and_then(|v| v.as_u64()), Some(255));
}

#[test]
fn webgl_cross_origin_texture_taints_and_blocks_readback() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="40" height="40"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const c = document.getElementById("c");
        const gl = c.getContext("webgl");

        const tex = gl.createTexture();
        gl.bindTexture(gl.TEXTURE_2D, tex);

        // Simulate tainted cross-origin image
        const img = {
            src: "http://untrusted-cross-origin.com/image.png",
            width: 10,
            height: 10
        };

        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);

        let readPixelsThrew = false;
        try {
            const pixels = new Uint8Array(4);
            gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        } catch (e) {
            readPixelsThrew = (e.name === "SecurityError") || String(e.message || e).includes("insecure");
        }

        ({ readPixelsThrew })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(
        res.get("readPixelsThrew").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn webgl_context_loss_and_restoration_extension() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c" width="50" height="50"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const c = document.getElementById("c");
        const gl = c.getContext("webgl");

        let lostFired = false;
        let restoredFired = false;

        c.addEventListener("webglcontextlost", (e) => {
            lostFired = true;
            e.preventDefault();
        });
        c.addEventListener("webglcontextrestored", () => {
            restoredFired = true;
        });

        const ext = gl.getExtension("WEBGL_lose_context");
        const hasExt = !!ext;

        ext.loseContext();
        const isLost = gl.isContextLost();

        ext.restoreContext();
        const isRestored = !gl.isContextLost();

        ({
            hasExt,
            lostFired,
            isLost,
            restoredFired,
            isRestored
        })
    "####;

    let res = eval_json(&mut view, script);
    assert_eq!(res.get("hasExt").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("lostFired").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(res.get("isLost").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        res.get("restoredFired").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(res.get("isRestored").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn webgl_memory_accounting_and_quota_enforcement() {
    let html = r####"<!doctype html>
    <html><body>
        <canvas id="c1" width="100" height="100"></canvas>
    </body></html>"####;
    let mut view = test_view(html);

    let script = r####"
        const c1 = document.getElementById("c1");
        const gl1 = c1.getContext("webgl");

        const mem1 = WebGLRenderingContext.getMemoryUsage();

        let quotaExceeded = false;
        try {
            // Attempting to create an oversized 40000x40000 canvas exceeds memory limit
            const big = new OffscreenCanvas(40000, 40000);
            big.getContext("webgl");
        } catch (e) {
            quotaExceeded = String(e.message || e).includes("QuotaExceededError");
        }

        HTMLCanvasElement.destroy(c1);
        const memAfter = WebGLRenderingContext.getMemoryUsage();

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
fn view_resilient_screenshot_composition_and_zoom() {
    let html = r####"<!doctype html>
    <html>
    <head><style>body { margin: 0; background: #0000ff; }</style></head>
    <body><div style="width: 100px; height: 100px; background: #ff0000;"></div></body>
    </html>"####;
    let mut view = test_view(html);

    // Test full viewport screenshot
    let full = view.screenshot(200, 200, 1.0, 1.0, None).unwrap();
    assert_eq!(full.len(), 200 * 200 * 4);

    // Top-left 100x100 should be red
    assert_eq!(full[0], 255); // R
    assert_eq!(full[1], 0); // G
    assert_eq!(full[2], 0); // B
    assert_eq!(full[3], 255); // A

    // Test cropped rect screenshot (10x10 subregion)
    let sub = view
        .screenshot(200, 200, 1.0, 1.0, Some((0, 0, 10, 10)))
        .unwrap();
    assert_eq!(sub.len(), 10 * 10 * 4);
    assert_eq!(sub[0], 255);
    assert_eq!(sub[1], 0);
    assert_eq!(sub[2], 0);
    assert_eq!(sub[3], 255);

    // Test zoomed screenshot
    let zoomed = view.screenshot(200, 200, 1.0, 1.5, None).unwrap();
    assert_eq!(zoomed.len(), 200 * 200 * 4);
}
