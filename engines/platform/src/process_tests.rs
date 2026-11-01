use super::*;
use std::process::Child;
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::mpsc,
};

#[test]
fn child_waits_for_control() {
    if std::env::var_os("WEGA_PROCESS_TEST_HELPER").is_none() {
        return;
    }
    let mut command = String::new();
    std::io::stdin().read_line(&mut command).unwrap();
    if command.trim() == "exit" {
        return;
    }
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

fn child() -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process::tests::child_waits_for_control",
            "--nocapture",
        ])
        .env("WEGA_PROCESS_TEST_HELPER", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn a_control_handle_can_terminate_and_sample_without_releasing_the_real_owner() {
    let (tx, rx) = mpsc::channel();
    let mut process = OwnedProcess::new(child(), move |report| {
        tx.send(report).unwrap();
    });
    let _input = process.take_stdin().unwrap();
    let control = process.control();
    assert!(matches!(control.sample().unwrap(), Sample::Alive { .. }));
    control.terminate().unwrap();
    drop(process);
    let report = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(report.forced);
    assert!(!report.status.unwrap().success());
    assert!(matches!(control.sample().unwrap(), Sample::Reaping));
}

#[test]
fn graceful_exit_is_observed_and_reaped_before_reporting_success() {
    let (tx, rx) = mpsc::channel();
    let mut process = OwnedProcess::new(child(), move |report| {
        tx.send(report).unwrap();
    });
    let pid = process.id();
    let mut input = process.take_stdin().unwrap();
    process.allow_graceful_shutdown(MAX_SHUTDOWN_GRACE);
    input.write_all(b"exit\n").unwrap();
    drop(input);
    drop(process);
    let report = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(report.pid, pid);
    assert!(!report.forced);
    assert!(report.status.unwrap().success());
}

#[test]
fn an_unresponsive_child_is_terminated_and_reaped_after_the_grace_period() {
    let (tx, rx) = mpsc::channel();
    let mut process = OwnedProcess::new(child(), move |report| {
        tx.send(report).unwrap();
    });
    process.allow_graceful_shutdown(Duration::from_millis(50));
    drop(process);
    let report = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(report.forced);
    assert!(!report.status.unwrap().success());
}

#[test]
fn normal_browser_exit_can_drain_every_previously_closed_owner() {
    let (tx, rx) = mpsc::channel();
    let process = OwnedProcess::new(child(), move |report| {
        tx.send(report).unwrap();
    });
    drop(process);
    wait_for_shutdowns();
    let report = rx.try_recv().unwrap();
    assert!(report.forced);
    assert!(!report.status.unwrap().success());
}
