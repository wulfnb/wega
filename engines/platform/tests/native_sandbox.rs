//! Run at the final native-validation stage, with a signed macOS probe helper or
//! a supported Linux/Windows host. Setup failures are failures, never denials.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
#[ignore = "requires a native supported sandbox host and WEGA_SANDBOX_PROBE"]
fn native_policy_denies_ambient_authority_and_preserves_an_explicit_file_grant() {
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
    let mut command = Command::new(probe);
    let launch = wega_platform::launch::PreparedLaunch::required_fixture(&mut command).unwrap();
    let grant = launch.inherit_frame_file(&mut command, &granted).unwrap();
    command
        .arg("--secret")
        .arg(&secret)
        .arg("--write")
        .arg(root.path().join("forbidden-output"))
        .arg("--address")
        .arg(listener.local_addr().unwrap().to_string())
        .arg("--grant")
        .arg(grant.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (exit, exited) = std::sync::mpsc::channel();
    let mut process = wega_platform::process::OwnedProcess::new(
        launch.spawn(&mut command).unwrap(),
        move |report| {
            let _ = exit.send(report);
        },
    );
    launch.attach(&mut process, 128 * 1024 * 1024).unwrap();
    drop(command);
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
        let result = stdout.read_to_string(&mut bytes);
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
        "private_read_denied",
        "private_write_denied",
        "tcp_denied",
        "udp_denied",
        "child_denied",
    ] {
        assert!(
            output.contains(&format!("\"{item}\":true")),
            "{item}: stdout={output}; stderr={diagnostics}; exit={report:?}"
        );
    }
    assert!(
        !report.forced,
        "Probe required forced termination: {report:?}"
    );
    assert!(
        report.status.unwrap().success(),
        "Probe failed: {diagnostics}"
    );
}
