#![cfg(windows)]
use std::{
    io::{Read, Write},
    sync::mpsc,
    time::Duration,
};

#[test]
fn preconnected_channels_exchange_bytes_time_out_and_revoke_without_a_vm() {
    let (finished, done) = mpsc::channel();
    std::thread::spawn(move || {
        if matches!(
            done.recv_timeout(Duration::from_secs(20)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            eprintln!("Native channel regression exceeded its complete deadline");
            // SAFETY: this is a failing isolated test process. Do not enter DLL
            // teardown when a stalled thread may hold the loader lock.
            unsafe {
                use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
                TerminateProcess(GetCurrentProcess(), 124);
            }
            std::process::abort();
        }
    });
    eprintln!("Native channel: create pair");
    let (mut server, mut client) = wega_platform::ipc::Channel::pair().unwrap();
    server
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    server
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    eprintln!("Native channel: write request");
    client.write_all(b"request").unwrap();
    client.flush().unwrap();
    let mut request = [0; 7];
    server.read_exact(&mut request).unwrap();
    assert_eq!(&request, b"request");
    server.write_all(b"reply").unwrap();
    let mut reply = [0; 5];
    client.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"reply");
    eprintln!("Native channel: cancel timed-out read");
    let error = server.read(&mut request).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    // Timeout cancellation must drain its OVERLAPPED before the next operation.
    client.write_all(b"request").unwrap();
    server.read_exact(&mut request).unwrap();
    let mut clone = server.try_clone().unwrap();
    server.shutdown(std::net::Shutdown::Both).unwrap();
    assert_eq!(
        clone.read(&mut request).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    finished.send(()).unwrap();
}
