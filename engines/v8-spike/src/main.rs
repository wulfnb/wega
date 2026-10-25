//! Spike: does V8 (via deno_core) link and run against Blitz's DOM in one binary?
//! Usage: `v8-spike v8` or `v8-spike quickjs` — run each in its own process so RSS is comparable.
use anyrender::ImageRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, NodeId};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use deno_core::{JsRuntime, OpState, RuntimeOptions, op2};
use std::{cell::RefCell, rc::Rc, time::Instant};

type Doc = Rc<RefCell<BaseDocument>>;

const PAGE: &str = r#"<!doctype html><title>Spike</title><style>body{margin:0}button{border:0;background:#00ff00;width:180px;height:80px}</style><button id="b">0</button>"#;

// The same DOM wrapper for both VMs: only `__host(op, a, b)` differs.
const SCRIPT: &str = r#"
const document = { querySelector: s => { const id = __host('query', s, ''); return id === '' ? null : {
  get textContent() { return __host('text', id, ''); },
  set textContent(v) { __host('setText', id, String(v)); } }; } };
const button = document.querySelector('#b');
button.textContent = 'V8 works';
const fib = n => n < 2 ? n : fib(n - 1) + fib(n - 2);
let t = Date.now(); const f = fib(30); const fibMs = Date.now() - t;
t = Date.now(); for (let i = 0; i < 20000; i++) button.textContent = String(i); const domMs = Date.now() - t;
button.textContent = 'V8 works';
__host('report', JSON.stringify({ fib: f, fibMs, domMs }), '');
"#;

fn host(doc: &Doc, op: &str, a: &str, b: &str) -> String {
    let mut doc = doc.borrow_mut();
    let id = || a.parse::<u64>().ok().map(NodeId::from_u64);
    match op {
        "query" => doc
            .query_selector(a)
            .ok()
            .flatten()
            .map(|id| id.as_u64().to_string())
            .unwrap_or_default(),
        "text" => id()
            .and_then(|id| doc.get_node(id))
            .map(|n| n.text_content())
            .unwrap_or_default(),
        "setText" => {
            if let Some(id) = id() {
                let mut mutation = doc.mutate();
                let child = mutation.create_text_node(b);
                mutation.replace_children(id, &[child]);
            }
            String::new()
        }
        "report" => {
            REPORT.with(|r| *r.borrow_mut() = a.to_string());
            String::new()
        }
        _ => String::new(),
    }
}
thread_local!(static REPORT: RefCell<String> = const { RefCell::new(String::new()) });

#[op2]
#[string]
fn op_host(
    state: &mut OpState,
    #[string] op: String,
    #[string] a: String,
    #[string] b: String,
) -> String {
    host(state.borrow::<Doc>(), &op, &a, &b)
}
deno_core::extension!(spike, ops = [op_host]);

fn run_v8(doc: Doc) -> String {
    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = tokio.enter();
    // V8_FLAGS="--jitless" or "--lite-mode" to measure V8's low-memory modes.
    if let Ok(flags) = std::env::var("V8_FLAGS") {
        let mut args = vec![String::new()];
        args.extend(flags.split_whitespace().map(str::to_owned));
        deno_core::v8_set_flags(args);
    }
    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![spike::init()],
        ..Default::default()
    });
    runtime.op_state().borrow_mut().put(doc);
    let source = format!(
        "globalThis.__host = (o, a, b) => Deno.core.ops.op_host(o, String(a), String(b));\n{SCRIPT}"
    );
    runtime
        .execute_script("spike.js", source)
        .expect("V8 script failed");
    REPORT.with(|r| r.borrow().clone())
}

fn run_quickjs(doc: Doc) -> String {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    context.with(|ctx| {
        let f = rquickjs::Function::new(ctx.clone(), move |o: String, a: String, b: String| {
            host(&doc, &o, &a, &b)
        })
        .unwrap();
        ctx.globals().set("__host", f).unwrap();
        ctx.eval::<(), _>(SCRIPT.replace("'V8 works'", "'QuickJS works'"))
            .expect("QuickJS script failed")
    });
    REPORT.with(|r| r.borrow().clone())
}

#[cfg(not(unix))]
fn peak_rss_mb() -> f64 {
    f64::NAN // Not measured on Windows; the spike only checks linking and behaviour there.
}

#[cfg(unix)]
fn peak_rss_mb() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    // macOS reports bytes, Linux kilobytes.
    let bytes = if cfg!(target_os = "macos") {
        usage.ru_maxrss as f64
    } else {
        usage.ru_maxrss as f64 * 1024.0
    };
    bytes / 1024.0 / 1024.0
}

fn main() {
    let engine = std::env::args().nth(1).unwrap_or_else(|| "v8".into());
    let start = Instant::now();
    let doc: Doc = Rc::new(RefCell::new(
        HtmlDocument::from_html(PAGE, DocumentConfig::default()).into_inner(),
    ));
    println!(
        "[{engine}] Blitz document parsed, peak RSS {:.1} MB",
        peak_rss_mb()
    );

    let t = Instant::now();
    let report = match engine.as_str() {
        "v8" => run_v8(doc.clone()),
        "quickjs" => run_quickjs(doc.clone()),
        other => panic!("unknown engine {other}"),
    };
    println!(
        "[{engine}] VM start + script: {:?}, result {report}",
        t.elapsed()
    );
    println!("[{engine}] after JS, peak RSS {:.1} MB", peak_rss_mb());

    let text = {
        let d = doc.borrow();
        let id = d.query_selector("#b").unwrap().unwrap();
        d.get_node(id).unwrap().text_content()
    };
    println!("[{engine}] Blitz DOM button text = {text:?}");
    assert!(text.ends_with("works"), "JS did not change Blitz's DOM");

    let (w, h) = (200, 120);
    let mut d = doc.borrow_mut();
    d.set_viewport(Viewport {
        window_size: (w, h),
        hidpi_scale: 1.0,
        zoom: 1.0,
        color_scheme: ColorScheme::Light,
    });
    d.resolve(0.0);
    let mut renderer = VelloCpuImageRenderer::new(w, h);
    let mut rgba = vec![];
    renderer.render_to_vec(
        |scene| blitz_paint::paint_scene(scene, &mut d, 1.0, w, h, 0, 0),
        &mut rgba,
    );
    let px = &rgba[(20 * w as usize + 20) * 4..][..4];
    println!("[{engine}] painted {w}x{h}, pixel at (20,20) = {px:?}");
    assert!(px[1] > 240 && px[0] < 10, "button not painted green");
    println!(
        "[{engine}] OK total {:?}, final peak RSS {:.1} MB",
        start.elapsed(),
        peak_rss_mb()
    );
}
