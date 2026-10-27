use super::*;
#[test]
fn real_handoff_reuses_identity_rejects_untrusted_requests_and_cleans_endpoint() {
    let directory = std::env::temp_dir().join(format!(
        "wega-container-launch-{}",
        crate::containers::random_id().unwrap()
    ));
    let Claim::Primary(mut server) = Server::claim(&directory, vec![], true).unwrap() else {
        panic!("new profile must own its listener")
    };
    let (send, received) = mpsc::channel();
    server.start(move |urls| send.send(urls).is_ok()).unwrap();
    assert!(server.start(|_| true).is_err());
    let urls = vec!["https://example.test/账户?x=1".to_owned()];
    assert!(matches!(
        Server::claim(&directory, urls.clone(), true).unwrap(),
        Claim::Forwarded
    ));
    assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), urls);
    assert!(matches!(
        Server::claim(&directory, vec![], true).unwrap(),
        Claim::Forwarded
    ));
    assert!(
        received
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    assert!(Server::claim(&directory, vec![], false).is_err());
    for request in [
        Request {
            token: "wrong".into(),
            urls: vec![],
        },
        Request {
            token: server.endpoint.token.clone(),
            urls: vec!["javascript:alert(1)".into()],
        },
    ] {
        let mut stream = TcpStream::connect(server.endpoint.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        stream.write_all(&bytes).unwrap();
        let mut ack = [0; 3];
        assert!(stream.read_exact(&mut ack).is_err());
        assert!(received.try_recv().is_err());
    }
    assert!(Server::claim(&directory, vec!["wega://settings".into()], true).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&server.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    drop(server);
    assert!(!directory.join("container-launch.json").exists());
    assert!(matches!(
        Server::claim(&directory, vec![], true).unwrap(),
        Claim::Primary(_)
    ));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_oversized_requests_and_non_loopback_endpoints_are_rejected() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    for bytes in [
        b"invalid\n".to_vec(),
        b"{}".to_vec(),
        vec![b'x'; LIMIT as usize + 1],
    ] {
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        client.write_all(&bytes).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(read_request(&server).is_err());
    }
    let directory = std::env::temp_dir().join(format!(
        "wega-container-invalid-endpoint-{}",
        crate::containers::random_id().unwrap()
    ));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("endpoint.json");
    assert!(forward(&path, &[]).is_err());
    fs::write(&path, b"broken").unwrap();
    assert!(forward(&path, &[]).is_err());
    for (address, token) in [
        ("192.0.2.1:10", "a".repeat(64)),
        ("127.0.0.1:10", "short".into()),
    ] {
        fs::write(
            &path,
            serde_json::to_vec(&Endpoint {
                address: address.parse().unwrap(),
                token,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(forward(&path, &[]).is_err());
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_live_but_unavailable_container_never_falls_back_to_another_profile() {
    let directory = std::env::temp_dir().join(format!(
        "wega-container-unavailable-{}",
        crate::containers::random_id().unwrap()
    ));
    let Claim::Primary(mut server) = Server::claim(&directory, vec![], true).unwrap() else {
        panic!("new container owns its listener")
    };
    server.start(|_| false).unwrap();
    let error = match Server::claim(&directory, vec![], true) {
        Err(error) => error,
        Ok(_) => panic!("unavailable container must fail closed"),
    };
    assert!(error.contains("already open"));
    assert!(!directory.join("browser.json").exists());
    drop(server);
    fs::remove_dir_all(directory).unwrap();
}
