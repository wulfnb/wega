use super::*;
pub(super) fn start(id: &str, owner: &str) -> Update {
    Update {
        id: id.into(),
        owner: owner.into(),
        at: Instant::now(),
        payload: Payload::Request {
            initial: true,
            url: "https://example.com/data?q=a%26b".into(),
            method: "GET".into(),
            headers: vec![],
            body: Body::default(),
            kind: "Fetch".into(),
            started_ms: 0,
            connect_ms: 0.0,
            send_ms: 0.0,
        },
    }
}
#[test]
fn clear_does_not_resurrect_responses_and_tabs_are_isolated() {
    let mut r = Recorder::default();
    r.apply(start("a", "one"));
    r.apply(start("b", "two"));
    r.clear("one");
    r.apply(Update {
        id: "a".into(),
        owner: "one".into(),
        at: Instant::now(),
        payload: Payload::Response {
            status: 200,
            reason: "OK".into(),
            headers: vec![],
            body: Body::default(),
            cached: false,
        },
    });
    assert!(r.visible("one").is_empty());
    assert_eq!(r.visible("two").len(), 1);
}
#[test]
fn bodies_are_bounded_and_har_is_structured() {
    let body = Body::from_bytes(Some(&vec![b'x'; BODY_LIMIT + 1]));
    assert!(body.truncated());
    assert_eq!(body.bytes.len(), BODY_LIMIT);
    let mut r = Recorder::default();
    r.apply(start("a", "one"));
    let har = r.har("one");
    assert_eq!(
        har["log"]["entries"][0]["request"]["queryString"][0]["value"],
        "a&b"
    );
    assert_eq!(iso_date(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso_date(1_704_067_200_123), "2024-01-01T00:00:00.123Z");
}
#[test]
fn filters_sorting_and_request_replay_validation() {
    let mut r = Recorder::default();
    r.apply(start("a", "tab"));
    r.apply(start("b", "tab"));
    r.rows[0].status = Some(404);
    r.rows[0].response_body.size = 100;
    r.rows[1].status = Some(200);
    r.rows[1].response_body.size = 9;
    r.rows[1].kind = "CSS".into();
    r.query = "domain:EXAMPLE.COM method:get -status:404".into();
    assert_eq!(r.visible("tab")[0].id, "b");
    r.types.insert("Fetch".into());
    assert!(r.visible("tab").is_empty());
    r.types.insert("CSS".into());
    assert_eq!(r.visible("tab").len(), 1);
    r.query.clear();
    r.sort = Sort::Size;
    assert_eq!(r.visible("tab")[0].id, "b");
    r.descending = true;
    assert_eq!(r.visible("tab")[0].id, "a");
    let row = &mut r.rows[0];
    row.method = "POST".into();
    row.request_body = Body::from_bytes(Some(b"it's $(literal)"));
    assert!(row.curl().unwrap().contains("'it'\"'\"'s $(literal)'"));
    let mut composer = Composer::from_request(row);
    assert!(composer.script().unwrap().contains("fetch("));
    composer.method = "GET".into();
    assert!(composer.script().is_err());
    composer.body.clear();
    composer.method = "trace".into();
    assert!(composer.script().is_err());
    composer.method = "GET".into();
    composer.url = "file:///tmp/test".into();
    assert!(composer.script().is_err());
    row.request_body.size += 1;
    assert!(row.curl().is_err());
}
#[test]
fn redirect_updates_create_hops_but_late_updates_after_clear_do_not() {
    let mut recorder = Recorder::default();
    recorder.apply(start("redirect", "tab"));
    let mut hop = start("redirect", "tab");
    if let Payload::Request { initial, url, .. } = &mut hop.payload {
        *initial = false;
        *url = "https://example.com/final".into();
    }
    recorder.apply(hop);
    assert_eq!(recorder.visible("tab").len(), 2);
    recorder.query = "final".into();
    assert_eq!(recorder.visible("tab").len(), 1);
    recorder.clear("tab");
    let mut late = start("redirect", "tab");
    if let Payload::Request { initial, .. } = &mut late.payload {
        *initial = false;
    }
    recorder.apply(late);
    assert!(recorder.rows.is_empty());
}

#[test]
fn network_rows_handle_security_body_updates_filters_and_all_sorts() {
    let mut r = Recorder::default();
    r.apply(start("a", "tab"));
    r.apply(start("b", "tab"));
    r.apply(Update {
        id: "a".into(),
        owner: "tab".into(),
        at: Instant::now(),
        payload: Payload::Security("TLS 1.3".into()),
    });
    assert_eq!(r.rows[0].security, "TLS 1.3");
    r.rows[0].response_body = Body::from_bytes(Some(b"retained"));
    r.apply(Update {
        id: "a".into(),
        owner: "tab".into(),
        at: Instant::now(),
        payload: Payload::Response {
            status: 200,
            reason: "OK".into(),
            headers: vec![("Content-Length".into(), "8".into())],
            body: Body::default(),
            cached: false,
        },
    });
    assert_eq!(r.rows[0].response_body.text(), Some("retained"));
    assert_eq!(r.rows[0].transfer(), Some(8));
    r.rows[1].url = "https://z.example.com/z".into();
    r.rows[1].status = Some(0);
    r.rows[1].method = "POST".into();
    r.rows[1].kind = "WS".into();
    r.rows[1].cached = true;
    r.rows[1].imported = true;
    r.rows[1].transferred = Some(10);
    r.rows[1].recorded_duration = Some(100.0);
    r.rows[1].initiator = "script".into();
    for sort in [
        Sort::Started,
        Sort::Status,
        Sort::Method,
        Sort::Domain,
        Sort::File,
        Sort::Type,
        Sort::Size,
        Sort::Transferred,
        Sort::Duration,
        Sort::Initiator,
    ] {
        r.sort = sort;
        r.descending = false;
        let ascending: Vec<_> = r.visible("tab").iter().map(|row| row.uid).collect();
        r.descending = true;
        let descending: Vec<_> = r.visible("tab").iter().map(|row| row.uid).collect();
        // Stable equal-key sorting need not reverse equal elements.
        if sort != Sort::Size {
            assert_eq!(ascending, descending.into_iter().rev().collect::<Vec<_>>());
        }
    }
    for query in [
        "is:cached",
        "is:imported",
        "type:WS",
        "status:Error",
        "POST -example.com/data",
    ] {
        assert!(matches_query(&r.rows[1], query), "{query}");
        assert!(!matches_query(&r.rows[0], query), "{query}");
    }
    assert!(matches_query(&r.rows[0], "https://example.com"));
    assert_eq!(size_label(10), "10 B");
    assert_eq!(duration_label(12.4), "12 ms");
    assert_eq!(duration_label(12_000.0), "12.00 s");
    assert_eq!(size_label(1024), "1.00 kB");
    assert_eq!(size_label(1048576), "1.00 MB");
    r.rows[1].transferred = None;
    assert_eq!(r.rows[1].transfer(), Some(0));
    r.rows[1].url = "invalid".into();
    assert_eq!(r.rows[1].domain(), "");
    assert_eq!(r.rows[1].filename(), "invalid");
    r.preserve = true;
    r.navigation("tab");
    assert_eq!(r.rows.len(), 2);
    r.preserve = false;
    r.navigation("tab");
    assert!(r.rows.is_empty());
    for i in 0..ROW_LIMIT + 1 {
        r.apply(start(&i.to_string(), "tab"));
    }
    assert_eq!(r.rows.len(), ROW_LIMIT);
    assert_eq!(r.rows.front().unwrap().id, "1");
}

#[test]
fn request_replay_rejects_binary_nuls_and_malformed_headers() {
    let mut r = Recorder::default();
    r.apply(start("a", "tab"));
    let row = &mut r.rows[0];
    row.request_headers = vec![
        ("content-length".into(), "2".into()),
        ("connection".into(), "close".into()),
        ("cookie".into(), "secret".into()),
        ("X-Test".into(), "quote'".into()),
    ];
    let c = Composer::from_request(row);
    assert_eq!(c.headers, "X-Test: quote'");
    assert!(!row.curl().unwrap().contains("content-length"));
    row.request_body = Body::from_bytes(Some(&[255]));
    assert!(row.curl().unwrap_err().contains("Binary"));
    row.request_body = Body::from_bytes(Some(b"a\0b"));
    assert!(row.curl().unwrap_err().contains("NUL"));
    let mut c = Composer {
        url: "https://example.com".into(),
        method: "POST".into(),
        headers: "X-Test: value\n\n".into(),
        body: "payload".into(),
    };
    let script = c.script().unwrap();
    assert!(script.contains("payload"));
    assert!(script.contains("x-test"));
    for h in [
        "missing separator",
        "invalid name: value",
        "valid: bad\0value",
    ] {
        c.headers = h.into();
        assert!(c.script().is_err());
    }
    c.headers.clear();
    c.method = "bad method".into();
    assert!(c.script().is_err());
}

#[test]
fn security_details_read_as_labelled_lines() {
    let summary = security_summary(&json!({
        "state": "Secure",
        "protocol_version": "TLSv1_3",
        "cipher_suite": null,
        "alpn_protocol": "h2",
        "hsts": true,
        "used_ech": false,
        "certificate_chain_der": [[1, 2], [3]],
        "weakness_reasons": ["SHA-1 signature"],
        "unknown_future_field": 1
    }));
    assert_eq!(
        summary,
        "Connection: Secure\nProtocol: TLSv1_3\nApplication protocol: h2\n\
         Strict transport security (HSTS): Yes\nEncrypted Client Hello: No\n\
         Certificates in chain: 2\nWeaknesses: SHA-1 signature"
    );
    assert!(!security_summary(&json!({"weakness_reasons": []})).contains("Weaknesses"));
    assert_eq!(
        security_summary(&json!({"kea_group_name": 29})),
        "Key exchange: 29"
    );
    assert!(security_summary(&Value::Null).starts_with("Not an HTTPS connection"));
}
