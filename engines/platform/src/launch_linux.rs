//! Baseline launch hardening, not a filesystem/network content sandbox.
use std::{io, os::unix::process::CommandExt, process::Command};

pub(super) fn prepare(command: &mut Command) {
    // SAFETY: getpid has no pointer arguments and is evaluated in the parent.
    let parent = unsafe { libc::getpid() };
    // SAFETY: after fork the closure only calls async-signal-safe native operations.
    // CLOEXEC preserves Rust's exec-error pipe until exec succeeds, and closes all
    // non-stdio descriptors on success. Future broker handles need explicit remapping.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::from_raw_os_error(libc::EPIPE));
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            let core = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &core) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::syscall(
                libc::SYS_close_range,
                3_u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            ) != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub(super) fn worker_baseline() -> io::Result<()> {
    // SAFETY: scalar-only prctl calls affect this worker. Dumpability is changed
    // after exec because an ordinary exec can reset that attribute.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
            || libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0
        {
            return Err(io::Error::last_os_error());
        }
        let core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::setrlimit(libc::RLIMIT_CORE, &core) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::OwnedProcess;
    use std::{
        io::{BufRead, BufReader},
        os::fd::{AsRawFd, FromRawFd, OwnedFd},
        process::Stdio,
        sync::mpsc,
        time::Duration,
    };
    #[test]
    fn child_probe() {
        let Ok(descriptor) = std::env::var("WEGA_LINUX_LAUNCH_PROBE") else {
            return;
        };
        let descriptor = descriptor.parse::<i32>().unwrap();
        // SAFETY: a scalar-only query must already reflect the pre-exec policy.
        assert_eq!(
            unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) },
            1
        );
        worker_baseline().unwrap();
        // SAFETY: query-only native calls use scalar values or a valid writable int.
        unsafe {
            // PDEATHSIG is per-thread and is not inherited by this Rust test
            // harness thread. The containment binary checks it on its main thread.
            assert_eq!(libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 1);
            assert_eq!(libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0), 0);
            assert_eq!(libc::fcntl(descriptor, libc::F_GETFD), -1);
        }
        println!("WEGA_LINUX_PROBE_OK");
    }
    #[test]
    fn launch_hardening_closes_an_intentionally_inheritable_descriptor_before_worker_code() {
        let file = tempfile::tempfile().unwrap();
        // SAFETY: F_DUPFD picks a free descriptor >=197, without CLOEXEC. The test
        // transfers that unique descriptor into OwnedFd and never closes the original.
        let descriptor = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 197) };
        assert!(descriptor >= 197);
        let _descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "launch::linux::tests::child_probe",
                "--nocapture",
            ])
            .env("WEGA_LINUX_LAUNCH_PROBE", descriptor.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        prepare(&mut command);
        let mut process = OwnedProcess::new(command.spawn().unwrap(), |_| {});
        let output = process.take_stdout().unwrap();
        let (result, received) = mpsc::channel();
        std::thread::spawn(move || {
            let mut ok = false;
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                ok |= line.contains("WEGA_LINUX_PROBE_OK");
            }
            let _ = result.send(ok);
        });
        assert!(received.recv_timeout(Duration::from_secs(5)).unwrap());
    }
}
