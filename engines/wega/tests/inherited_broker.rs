#![cfg(any(unix, windows))]
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
    run_worker(false);
}
#[test]
#[ignore = "requires WEGA_CONTENT_HELPER and a supported native sandbox host"]
fn a_required_worker_runs_jit_storage_cookies_and_frames_through_grants() {
    run_worker(true);
}
struct FixtureDeadline(mpsc::Sender<()>);
impl FixtureDeadline {
    fn start() -> Self {
        let (finished, done) = mpsc::channel();
        std::thread::spawn(move || {
            if matches!(
                done.recv_timeout(Duration::from_secs(60)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                // A deadlock during launch/teardown must fail the test process,
                // rather than hiding its diagnostics until the CI job expires.
                eprintln!("Inherited worker fixture exceeded its complete 60-second deadline");
                std::process::abort();
            }
        });
        Self(finished)
    }
}
impl Drop for FixtureDeadline {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
fn run_worker(required: bool) {
    let _deadline = FixtureDeadline::start();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let serving = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
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
    eprintln!("Inherited worker fixture: creating host broker");
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
    eprintln!("Inherited worker fixture: creating inherited channels");
    let channels = session.inherited_channels(4).unwrap();
    eprintln!("Inherited worker fixture: inherited channels ready");
    let (_, token) = session.endpoint();
    let identity = SessionIdentity {
        tab: TabId(5),
        frame: FrameId(1),
        process: ProcessSessionId([5; 16]),
        service: Some(session.identity()),
    };
    let executable = if required {
        std::env::var_os("WEGA_CONTENT_HELPER").expect("Missing native content helper")
    } else {
        env!("CARGO_BIN_EXE_wega-engine").into()
    };
    let mut command = Command::new(executable);
    let launch = if required {
        wega_platform::launch::PreparedLaunch::required_fixture(&mut command)
    } else {
        wega_platform::launch::PreparedLaunch::development_fixture(&mut command)
    }
    .unwrap();
    let descriptors = launch
        .inherit_broker_channels(&mut command, &channels)
        .unwrap();
    let frames = std::sync::Arc::new(
        wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4)
            .unwrap()
            .into_reader(),
    );
    let frame_fd = launch
        .inherit_frame_file(&mut command, &frames.try_clone_file().unwrap())
        .unwrap();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    eprintln!("Inherited worker fixture: spawning required={required}");
    let child = launch.spawn(&mut command).unwrap();
    drop(command);
    drop(channels);
    let mut child = wega_platform::process::OwnedProcess::new(child, |_| {});
    launch.attach(&mut child, 512 * 1024 * 1024).unwrap();
    eprintln!("Inherited worker fixture: containment attached");
    child.retain_until_exit(frames.clone());
    if let Some(stderr) = child.take_stderr() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stderr.take(64 * 1024));
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|length| length > 0) {
                eprint!("Content helper: {line}");
                line.clear();
            }
        });
    }
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
    eprintln!("Inherited worker fixture: initialization sent");
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
    let frame = wait(
        &|event| matches!(event, Event::SharedFrame { revision: rendered,.. } if *rendered==revision),
    );
    if let Event::SharedFrame {
        width,
        height,
        slot,
        sequence,
        ..
    } = frame
    {
        let length = protocol::frame_bytes(width, height).unwrap();
        let pixels = frames.read_slot(slot as usize, length).unwrap();
        assert!(
            pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0),
            "Native raster must publish real pixels"
        );
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0 && pixel[..3].iter().all(|channel| *channel < 128)),
            "Broker-selected fonts must paint dark text on the default light page"
        );
        protocol::write(
            &mut stdin,
            &identity.command(Request::FrameConsumed { slot, sequence }),
        )
        .unwrap();
    }
    let context = protocol::RequestContext {
        document: protocol::DocumentId(0),
        revision,
        request: protocol::RequestId(1),
    };
    protocol::write(
        &mut stdin,
        &identity.command(Request::EvaluateDocument {
            context,
            source: "document.cookie='inherited=yes; Path=/';localStorage.setItem('persistent','broker');sessionStorage.setItem('tab','isolated');[document.cookie,localStorage.getItem('persistent'),sessionStorage.getItem('tab'),(()=>{let total=0;for(let i=0;i<100000;i++)total+=i;return total})()]".into(),
        }),
    )
    .unwrap();
    assert!(
        matches!(wait(&|event| matches!(event, Event::DocumentEvaluated { .. })), Event::DocumentEvaluated { context: returned, result: Ok(value) } if returned == context && value == "[\"inherited=yes\",\"broker\",\"isolated\",4999950000]")
    );
    let mut cache_context = context;
    cache_context.request = protocol::RequestId(2);
    protocol::write(&mut stdin, &identity.command(Request::EvaluateDocument {
        context:cache_context,
        source:"globalThis.cacheVerified='pending';(async()=>{const cache=await caches.open('inherited');await cache.put('/offline',new Response('brokered'));cacheVerified=await (await cache.match('/offline')).text()})().catch(error=>cacheVerified=String(error));true".into(),
    })).unwrap();
    wait(
        &|event| matches!(event,Event::DocumentEvaluated {context,..} if context.request==cache_context.request),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cache_context.request = protocol::RequestId(cache_context.request.0 + 1);
        protocol::write(
            &mut stdin,
            &identity.command(Request::EvaluateDocument {
                context: cache_context,
                source: "cacheVerified".into(),
            }),
        )
        .unwrap();
        match wait(
            &|event| matches!(event,Event::DocumentEvaluated {context,..} if context.request==cache_context.request),
        ) {
            Event::DocumentEvaluated {
                result: Ok(value), ..
            } if value == "\"brokered\"" => break,
            Event::DocumentEvaluated {
                result: Ok(value), ..
            } if value == "\"pending\"" => {}
            event => panic!("Inherited Cache API failed: {event:?}"),
        }
        assert!(
            Instant::now() < deadline,
            "Inherited cache operation timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
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
