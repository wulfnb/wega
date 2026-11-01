//! Job containment is additional to AppContainer/token/handle isolation, not a
//! substitute for it. The worker blocks on Init until job attachment completes.
use crate::process::OwnedProcess;
use std::{
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation, SetInformationJobObject,
};

pub(super) fn attach(process: &OwnedProcess, maximum_commit_bytes: u64) -> io::Result<OwnedHandle> {
    let limit = usize::try_from(maximum_commit_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid job memory limit"))?;
    if limit == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Empty job memory limit",
        ));
    }
    // SAFETY: null attributes make this unnamed job non-inheritable. A successful
    // handle is uniquely transferred into OwnedHandle, including every error path.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let job = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
    // SAFETY: this C aggregate permits an all-zero initial state.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_JOB_MEMORY;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.ProcessMemoryLimit = limit;
    limits.JobMemoryLimit = limit;
    // SAFETY: job and correctly sized limits buffer stay live for this call.
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle().cast(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
            | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
            | JOB_OBJECT_UILIMIT_EXITWINDOWS
            | JOB_OBJECT_UILIMIT_GLOBALATOMS
            | JOB_OBJECT_UILIMIT_HANDLES
            | JOB_OBJECT_UILIMIT_READCLIPBOARD
            | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
            | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    };
    // SAFETY: the job and correctly sized immutable restrictions stay live.
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle().cast(),
            JobObjectBasicUIRestrictions,
            (&ui as *const JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
            size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    process.with_child(|child| {
        // SAFETY: both owned kernel handles stay live under the process owner lock.
        if unsafe {
            AssignProcessToJobObject(job.as_raw_handle().cast(), child.as_raw_handle().cast())
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })?;
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Sample;
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Command, Stdio},
        sync::mpsc,
        time::{Duration, Instant},
    };
    #[test]
    fn child_probe() {
        if std::env::var_os("WEGA_JOB_PROBE").is_none() {
            return;
        }
        let mut command = String::new();
        std::io::stdin().read_line(&mut command).unwrap();
        if command.trim() == "spawn" {
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "launch::windows::tests::child_probe"])
                .env_remove("WEGA_JOB_PROBE")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            match child {
                Ok(mut child) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    println!("WEGA_JOB_CHILD_ALLOWED");
                }
                Err(error)
                    if error.raw_os_error()
                        == Some(windows_sys::Win32::Foundation::ERROR_NOT_ENOUGH_QUOTA as i32) =>
                {
                    println!("WEGA_JOB_CHILD_DENIED")
                }
                Err(error) => panic!("Child probe setup failed: {error}"),
            }
        }
    }
    fn worker() -> OwnedProcess {
        OwnedProcess::new(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "launch::windows::tests::child_probe",
                    "--nocapture",
                ])
                .env("WEGA_JOB_PROBE", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            |_| {},
        )
    }
    #[test]
    fn job_active_process_limit_denies_a_descendant_before_it_executes() {
        let mut process = worker();
        let job = attach(&process, 512 * 1024 * 1024).unwrap();
        let output = process.take_stdout().unwrap();
        let (result, received) = mpsc::channel();
        std::thread::spawn(move || {
            let mut denied = false;
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                denied |= line.contains("WEGA_JOB_CHILD_DENIED");
            }
            let _ = result.send(denied);
        });
        process.take_stdin().unwrap().write_all(b"spawn\n").unwrap();
        assert!(received.recv_timeout(Duration::from_secs(5)).unwrap());
        drop(job);
    }
    #[test]
    fn closing_the_last_job_handle_terminates_the_owned_child() {
        let mut process = worker();
        let _input = process.take_stdin().unwrap();
        let job = attach(&process, 512 * 1024 * 1024).unwrap();
        drop(job);
        let control = process.control();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if matches!(control.sample().unwrap(), Sample::Exited) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Job close failed to terminate child"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
