//! A static file server for the WPT checkout, standing in for `wptserve`.
//!
//! Tests reference shared files by absolute path (`/resources/testharness.js`,
//! `/fonts/Ahem.ttf`, `/css/support/...`), so they must be served from the WPT
//! root. `.sub.` substitution, `.headers` files and Python handlers are not
//! supported; discovery skips tests that need them.
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// Replaces upstream `testharnessreport.js`: collect results into a global the
/// runner polls, and keep the harness from writing its output into the page.
pub const TESTHARNESS_REPORT: &str = r#"
setup({output: false, explicit_timeout: false});
globalThis.__wptResults = null;
add_completion_callback(function (tests, status) {
  globalThis.__wptResults = {
    status: status.status,
    message: status.message || null,
    tests: tests.map(function (t) {
      return {name: t.name, status: t.status, message: t.message || null};
    })
  };
});
"#;

/// Serve `root` on an ephemeral loopback port in background threads.
pub fn start(root: PathBuf) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let root = Arc::new(root);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                let _ = handle(stream, &root);
            });
        }
    });
    Ok(port)
}

fn handle(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    // Drain headers; bodies are never needed for GET.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    let target = request.split_whitespace().nth(1).unwrap_or("/");
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let (status, mime, body) = match respond(root, path) {
        Some((mime, body)) => ("200 OK", mime, body),
        None => ("404 Not Found", "text/plain", b"not found".to_vec()),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}

fn respond(root: &Path, path: &str) -> Option<(&'static str, Vec<u8>)> {
    if path == "/resources/testharnessreport.js" {
        return Some(("text/javascript", TESTHARNESS_REPORT.as_bytes().to_vec()));
    }
    let relative = percent_decode(path.trim_start_matches('/'));
    let relative = Path::new(&relative);
    // Refuse anything that could escape the WPT root.
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let file = root.join(relative);
    let body = std::fs::read(&file).ok()?;
    Some((mime(&file), body))
}

pub fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "xht" | "xhtml" => "application/xhtml+xml",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "css" => "text/css",
        "js" => "text/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
