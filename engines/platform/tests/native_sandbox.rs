//! Run at the final native-validation stage, with a signed macOS probe helper or
//! a supported Linux/Windows host. Setup failures are failures, never denials.
use std::{
    io::{BufRead, Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
#[ignore = "requires a native supported sandbox host and WEGA_SANDBOX_PROBE"]
fn native_policy_denies_ambient_authority_and_preserves_an_explicit_file_grant() {
    let (complete, completed) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if matches!(
            completed.recv_timeout(Duration::from_secs(30)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            eprintln!("Native grant fixture exceeded its complete 30-second deadline");
            #[cfg(windows)]
            unsafe {
                use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
                // The failing fixture must not wait for DLL detach callbacks.
                TerminateProcess(GetCurrentProcess(), 124);
            }
            std::process::abort();
        }
    });
    let probe = std::env::var_os("WEGA_SANDBOX_PROBE")
        .expect("Native probe setup requires WEGA_SANDBOX_PROBE");
    let root = tempfile::tempdir().unwrap();
    let secret = root.path().join("private-profile");
    std::fs::write(&secret, "private").unwrap();
    let mut granted = tempfile::tempfile().unwrap();
    granted.write_all(b"explicit-grant").unwrap();
    use std::io::{Seek, SeekFrom};
    granted.seek(SeekFrom::Start(0)).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    // Prove delivery works before using this listener to detect sandbox escape.
    let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    sender
        .send_to(b"baseline", udp.local_addr().unwrap())
        .unwrap();
    let mut baseline = [0; 64];
    let (length, _) = udp.recv_from(&mut baseline).unwrap();
    assert_eq!(&baseline[..length], b"baseline");
    drop(sender);
    // AppContainer's WFP filters can drop UDP asynchronously after send_to
    // succeeds. Actual host reception, not the syscall result, is the verdict.
    // https://learn.microsoft.com/en-us/windows/security/operating-system-security/network-security/windows-firewall/troubleshooting-uwp-firewall
    udp.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut command = Command::new(probe);
    eprintln!("Native grant fixture: preparing launch");
    let launch = wega_platform::launch::PreparedLaunch::required_fixture(&mut command).unwrap();
    let grant = launch.inherit_frame_file(&mut command, &granted).unwrap();
    let mut channels = Vec::new();
    let mut servers = Vec::new();
    for _ in 0..4 {
        let (mut server, client) = wega_platform::ipc::Channel::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        server
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        servers.push(std::thread::spawn(move || {
            let mut request = [0; 5];
            server.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"grant");
            server.write_all(b"reply").unwrap();
        }));
        channels.push(client);
    }
    eprintln!("Native grant fixture: duplicating four channel grants");
    let channel_grants = launch
        .inherit_broker_channels(&mut command, &channels)
        .unwrap();
    command
        .arg("--channel-grants")
        .arg(
            channel_grants
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )
        .arg("--secret")
        .arg(&secret)
        .arg("--write")
        .arg(root.path().join("forbidden-output"))
        .arg("--address")
        .arg(listener.local_addr().unwrap().to_string())
        .arg("--udp-address")
        .arg(udp.local_addr().unwrap().to_string())
        .arg("--grant")
        .arg(grant.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (exit, exited) = std::sync::mpsc::channel();
    eprintln!("Native grant fixture: spawning helper");
    let mut process = wega_platform::process::OwnedProcess::new(
        launch.spawn(&mut command).unwrap(),
        move |report| {
            let _ = exit.send(report);
        },
    );
    launch.attach(&mut process, 128 * 1024 * 1024).unwrap();
    eprintln!("Native grant fixture: containment attached");
    drop(command);
    drop(channels);
    let mut stdout = process.take_stdout().unwrap();
    let mut stderr = process.take_stderr().unwrap();
    let (errors, error_output) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = String::new();
        let _ = stderr.by_ref().take(65536).read_to_string(&mut bytes);
        let _ = errors.send(bytes);
    });
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = String::new();
        // Lowbox token/object lifetime can outlive process exit. A complete
        // result line and the owned process's exit status are the completion
        // contract; waiting for pipe EOF can hang even after the probe exits.
        let result = std::io::BufReader::new(stdout.by_ref().take(65536)).read_line(&mut bytes);
        let _ = sent.send((result, bytes));
    });
    let (read, output) = received
        .recv_timeout(Duration::from_secs(10))
        .expect("Probe failed to terminate");
    read.unwrap();
    let diagnostics = error_output
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !matches!(
        process.control().sample().unwrap(),
        wega_platform::process::Sample::Exited
    ) && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(process);
    let report = exited
        .recv_timeout(Duration::from_secs(3))
        .expect("Probe was not reaped");
    eprintln!("Native probe exit: {report:?}; stderr: {diagnostics}");
    for item in [
        "granted_file",
        "granted_channels",
        "private_read_denied",
        "private_write_denied",
        "tcp_denied",
        "child_denied",
    ] {
        assert!(
            output.contains(&format!("\"{item}\":true")),
            "{item}: stdout={output}; stderr={diagnostics}; exit={report:?}"
        );
    }
    let mut packet = [0; 64];
    assert!(
        udp.recv_from(&mut packet).is_err(),
        "Sandboxed UDP reached the host listener"
    );
    assert!(
        !root.path().join("forbidden-output").exists(),
        "Sandboxed write created a host file"
    );
    assert!(
        !report.forced,
        "Probe required forced termination: {report:?}"
    );
    assert!(
        report.status.unwrap().success(),
        "Probe failed: {diagnostics}"
    );
    for server in servers {
        server.join().unwrap();
    }
    complete.send(()).unwrap();
}
