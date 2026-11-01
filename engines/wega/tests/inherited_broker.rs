#![cfg(unix)]
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use wega_protocol::{
    self as protocol, Command as Request, Event,
    identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
};

#[test]
fn a_real_worker_uses_inherited_broker_grants_without_a_tcp_broker_address() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let serving = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        stream.read_exact(&mut byte).unwrap();
                        header.push(byte[0]);
                        assert!(header.len() < 8192);
                    }
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<p>broker</p>").unwrap();
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until);
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("Fixture setup failed: {error}"),
            }
        }
    });
    let profile = tempfile::tempdir().unwrap();
    let broker = wega_network::Broker::new().unwrap();
    let session = broker
        .open_bound_session(
            profile.path().into(),
            wega_network::Policy {
                allow_loopback: true,
                ..Default::default()
            },
        )
        .unwrap();
    session.begin_navigation(&url.parse().unwrap());
    let channels = session.inherited_channels(4).unwrap();
    let (_, token) = session.endpoint();
    let identity = SessionIdentity {
        tab: TabId(5),
        frame: FrameId(1),
        process: ProcessSessionId([5; 16]),
        service: Some(session.identity()),
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_wega-engine"));
    let launch = wega_platform::launch::PreparedLaunch::development_fixture(&mut command).unwrap();
    let descriptors = launch
        .inherit_broker_channels(&mut command, &channels)
        .unwrap();
    let frames = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let frame_fd = launch
        .inherit_frame_file(&mut command, &frames.try_clone_file().unwrap())
        .unwrap();
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    drop(command);
    drop(channels);
    let mut child = wega_platform::process::OwnedProcess::new(child, |_| {});
    launch.attach(&mut child, 512 * 1024 * 1024).unwrap();
    child.retain_until_exit(frames);
    let mut stdin = child.take_stdin().unwrap();
    let mut stdout = child.take_stdout().unwrap();
    let (events, receiver) = mpsc::sync_channel(32);
    std::thread::spawn(move || {
        while let Ok(Some(packet)) = protocol::read::<Event>(&mut stdout) {
            let event = identity.receive_event(packet).unwrap();
            if events.send(event).is_err() {
                break;
            }
        }
    });
    protocol::write(
        &mut stdin,
        &Request::Init {
            identity: Some(Box::new(identity)),
            version: protocol::VERSION,
            url: url.clone(),
            offline: false,
            blocked_hosts: String::new(),
            allowed_hosts: String::new(),
            storage_dir: None,
            broker_address: None,
            broker_token: Some(token),
            broker_fds: descriptors,
            frame_path: None,
            frame_fd: Some(frame_fd),
        },
    )
    .unwrap();
    let wait = |predicate: &dyn Fn(&Event) -> bool| {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let event = receiver
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("Worker transport deadline");
            if predicate(&event) {
                break event;
            }
        }
    };
    wait(&|event| matches!(event, Event::Ready { .. }));
    let revision = match wait(&|event| matches!(event, Event::DocumentCommitted { .. })) {
        Event::DocumentCommitted { revision, .. } => revision,
        _ => unreachable!(),
    };
    session.take_committed_principal(Some(&url));
    wait(&|event| matches!(event, Event::State { loading: false, .. }));
    let context = protocol::RequestContext {
        document: protocol::DocumentId(0),
        revision,
        request: protocol::RequestId(1),
    };
    protocol::write(
        &mut stdin,
        &identity.command(Request::EvaluateDocument {
            context,
            source: "document.cookie='inherited=yes; Path=/'; document.cookie".into(),
        }),
    )
    .unwrap();
    assert!(
        matches!(wait(&|event| matches!(event, Event::DocumentEvaluated { .. })), Event::DocumentEvaluated { context: returned, result: Ok(value) } if returned == context && value == "\"inherited=yes\"")
    );
    session.revoke();
    protocol::write(&mut stdin, &identity.command(Request::Shutdown)).unwrap();
    wait(&|event| matches!(event, Event::ShutdownComplete));
    let until = Instant::now() + Duration::from_secs(5);
    while !matches!(
        child.control().sample().unwrap(),
        wega_platform::process::Sample::Exited
    ) {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    serving.join().unwrap();
}
