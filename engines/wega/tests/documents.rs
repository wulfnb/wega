//! Navigation responses: character encodings, MIME types, downloads, refresh.
mod support;
use support::{Reply, Server, engine, open, settle};
use wega_engine::PageEvent;

fn documents() -> Server {
    Server::new(|seen| match seen.path.as_str() {
        "/sjis" => {
            let (bytes, _, _) =
                encoding_rs::SHIFT_JIS.encode("<title>日本語</title><p id=t>こんにちは</p>");
            Reply::ok("text/html; charset=Shift_JIS", bytes.into_owned())
        }
        "/latin" => Reply::ok(
            "text/html",
            b"<meta charset=windows-1252><title>caf\xe9</title>".to_vec(),
        ),
        "/bom" => Reply::ok(
            "text/html; charset=iso-8859-1",
            "\u{feff}<title>naïve</title>".as_bytes().to_vec(),
        ),
        "/notes.txt" => Reply::ok("text/plain", "<b>not markup</b>"),
        "/pixel.png" => Reply::ok("image/png", include_bytes!("fixtures/pixel.png").to_vec()),
        "/report.pdf" => Reply::ok("application/pdf", b"%PDF-1.7".to_vec()),
        "/attachment" => Reply::ok("text/html", "<title>x</title>")
            .header("Content-Disposition", "attachment; filename=x.html"),
        "/refresh" => Reply::html("<title>old</title>").header("Refresh", "0; url=/target"),
        "/missing" => Reply::html("<title>Not here</title>").status("404 Not Found"),
        _ => Reply::html("<title>start</title>"),
    })
}

#[test]
fn document_revisions_change_on_replacement_but_not_mutation_or_fragments() {
    let server = documents();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    let first = view.document_revision();
    assert!(first > 0);
    view.evaluate_json("document.body.append(document.createElement('p'));true")
        .unwrap();
    assert_eq!(view.document_revision(), first);
    view.load(
        format!("{}#part", server.url("127.0.0.1", "/"))
            .parse()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(view.document_revision(), first);
    view.set_html("<title>Replacement</title>").unwrap();
    assert!(view.document_revision() > first);
    let second = view.document_revision();
    view.load(server.url("127.0.0.1", "/notes.txt").parse().unwrap())
        .unwrap();
    settle(&mut view);
    assert!(view.document_revision() > second);
}

#[test]
fn transport_meta_and_bom_charsets_decode_documents() {
    let server = documents();
    let engine = engine();
    let view = open(&engine, &server.url("127.0.0.1", "/sjis"));
    assert_eq!(view.title(), "日本語");
    assert_eq!(view.text("#t").as_deref(), Some("こんにちは"));
    assert_eq!(
        open(&engine, &server.url("127.0.0.1", "/latin")).title(),
        "café"
    );
    assert_eq!(
        open(&engine, &server.url("127.0.0.1", "/bom")).title(),
        "naïve"
    );
}

#[test]
fn text_images_and_error_statuses_are_shown_and_files_are_downloads() {
    let server = documents();
    let engine = engine();
    let text = open(&engine, &server.url("127.0.0.1", "/notes.txt"));
    assert_eq!(text.text("pre").as_deref(), Some("<b>not markup</b>"));
    assert_eq!(text.title(), "notes.txt");
    let image = open(&engine, &server.url("127.0.0.1", "/pixel.png"));
    assert!(image.node_id("img").is_some());
    assert_eq!(
        open(&engine, &server.url("127.0.0.1", "/missing")).title(),
        "Not here"
    );
    let mut view = open(&engine, &server.url("127.0.0.1", "/"));
    for path in ["/report.pdf", "/attachment"] {
        view.load(server.url("127.0.0.1", path).parse().unwrap())
            .unwrap();
        settle(&mut view);
        assert_eq!(view.title(), "start", "a download keeps the current page");
        assert_eq!(
            view.events.pop_front(),
            Some(PageEvent::Download(
                server.url("127.0.0.1", path).parse().unwrap()
            ))
        );
        assert_eq!(view.url.path(), "/");
    }
}

#[test]
fn refresh_headers_navigate_after_load() {
    let server = documents();
    let engine = engine();
    let mut view = open(&engine, &server.url("127.0.0.1", "/refresh"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let next = loop {
        view.poll();
        if let Some(url) = view.next_navigation() {
            break url;
        }
        assert!(std::time::Instant::now() < deadline, "no refresh");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(next.path(), "/target");
}
