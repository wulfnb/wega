//! Explicit native-test diagnostic runner. Follows the fixture's content child
//! without changing its token/job/policy and reports bounded loader output.
#[cfg(not(windows))]
fn main() {
    eprintln!("Native loader diagnostics require Windows");
    std::process::exit(2);
}
#[cfg(windows)]
fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Native debugger setup failed: {error}");
            // Windows' default debugger kill-on-exit policy terminates debuggees.
            std::process::exit(2);
        }
    }
}
#[cfg(windows)]
fn run() -> std::io::Result<i32> {
    use std::{
        collections::HashMap,
        io,
        os::windows::{
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
            process::CommandExt,
        },
        process::Command,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Storage::FileSystem::GetFinalPathNameByHandleW,
        System::{Diagnostics::Debug::*, Threading::DEBUG_PROCESS},
    };
    let mut args = std::env::args_os().skip(1);
    let executable = args
        .next()
        .ok_or_else(|| io::Error::other("Expected native test executable"))?;
    let mut root = Command::new(executable)
        .args(args)
        .creation_flags(DEBUG_PROCESS)
        .spawn()?;
    let mut processes: HashMap<u32, OwnedHandle> = HashMap::new();
    let mut root_exit = None;
    let mut thread_local = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut output_bytes = 0_usize;
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Native debug deadline",
            ));
        }
        let mut event: DEBUG_EVENT = unsafe { std::mem::zeroed() };
        // SAFETY: the OS fills this correctly sized event on the creating thread.
        if unsafe { WaitForDebugEvent(&mut event, 1000) } == 0 {
            if io::Error::last_os_error().raw_os_error() == Some(ERROR_SEM_TIMEOUT as i32) {
                continue;
            }
            return Err(io::Error::last_os_error());
        }
        let mut status = DBG_CONTINUE;
        // SAFETY: dwDebugEventCode selects the documented union arm. OS-provided
        // handles are independently owned by this debugger and closed exactly once.
        unsafe {
            match event.dwDebugEventCode {
                CREATE_PROCESS_DEBUG_EVENT => {
                    let info = event.u.CreateProcessInfo;
                    if !info.hFile.is_null() {
                        CloseHandle(info.hFile);
                    }
                    if !info.hThread.is_null() {
                        CloseHandle(info.hThread);
                    }
                    processes.insert(
                        event.dwProcessId,
                        OwnedHandle::from_raw_handle(info.hProcess.cast()),
                    );
                    thread_local.insert(
                        (event.dwProcessId, event.dwThreadId),
                        info.lpThreadLocalBase as usize,
                    );
                    eprintln!("DEBUG process {} created", event.dwProcessId);
                }
                CREATE_THREAD_DEBUG_EVENT => {
                    let info = event.u.CreateThread;
                    thread_local.insert(
                        (event.dwProcessId, event.dwThreadId),
                        info.lpThreadLocalBase as usize,
                    );
                    CloseHandle(info.hThread);
                }
                LOAD_DLL_DEBUG_EVENT => {
                    let info = event.u.LoadDll;
                    if !info.hFile.is_null() {
                        let mut path = [0_u16; 2048];
                        let count = GetFinalPathNameByHandleW(
                            info.hFile,
                            path.as_mut_ptr(),
                            path.len() as u32,
                            0,
                        ) as usize;
                        if count > 0 && count < path.len() {
                            eprintln!(
                                "DEBUG DLL {}: {}",
                                event.dwProcessId,
                                String::from_utf16_lossy(&path[..count])
                            );
                        }
                        CloseHandle(info.hFile);
                    }
                }
                OUTPUT_DEBUG_STRING_EVENT => {
                    let info = event.u.DebugString;
                    if let Some(process) = processes.get(&event.dwProcessId) {
                        let length = usize::from(info.nDebugStringLength)
                            .saturating_mul(if info.fUnicode != 0 { 2 } else { 1 })
                            .min(16384);
                        let mut bytes = vec![0_u8; length];
                        let mut read = 0;
                        if ReadProcessMemory(
                            process.as_raw_handle().cast(),
                            info.lpDebugStringData.cast(),
                            bytes.as_mut_ptr().cast(),
                            length,
                            &mut read,
                        ) != 0
                            && read <= length
                        {
                            bytes.truncate(read);
                            output_bytes = output_bytes.saturating_add(read);
                            if output_bytes > 512 * 1024 {
                                return Err(io::Error::other("Native debug output budget"));
                            }
                            let text = if info.fUnicode != 0 {
                                String::from_utf16_lossy(
                                    &bytes
                                        .as_chunks::<2>()
                                        .0
                                        .iter()
                                        .map(|pair| u16::from_le_bytes(*pair))
                                        .collect::<Vec<_>>(),
                                )
                            } else {
                                String::from_utf8_lossy(&bytes).into_owned()
                            };
                            eprintln!(
                                "DEBUG loader {}: {}",
                                event.dwProcessId,
                                text.trim_end_matches('\0')
                            );
                            // Diagnostic only: Windows x64's TEB LastErrorValue
                            // layout. Never used for policy or a pass/fail result.
                            #[cfg(target_arch = "x86_64")]
                            if text.contains("ERROR")
                                && let Some(base) =
                                    thread_local.get(&(event.dwProcessId, event.dwThreadId))
                            {
                                let mut error = [0_u8; 4];
                                let mut read = 0;
                                if ReadProcessMemory(
                                    process.as_raw_handle().cast(),
                                    (base + 0x68) as *const _,
                                    error.as_mut_ptr().cast(),
                                    4,
                                    &mut read,
                                ) != 0
                                    && read == 4
                                {
                                    eprintln!(
                                        "DEBUG last error {}: {} ({:#x})",
                                        event.dwProcessId,
                                        u32::from_le_bytes(error),
                                        u32::from_le_bytes(error)
                                    );
                                }
                            }
                        }
                    }
                }
                EXCEPTION_DEBUG_EVENT => {
                    let info = event.u.Exception;
                    eprintln!(
                        "DEBUG exception {}: {:#x}; first={}",
                        event.dwProcessId,
                        info.ExceptionRecord.ExceptionCode as u32,
                        info.dwFirstChance
                    );
                    if info.ExceptionRecord.ExceptionCode as u32 != 0x80000003 {
                        status = DBG_EXCEPTION_NOT_HANDLED;
                    }
                }
                EXIT_PROCESS_DEBUG_EVENT => {
                    let code = event.u.ExitProcess.dwExitCode;
                    eprintln!("DEBUG process {} exited {code:#x}", event.dwProcessId);
                    if event.dwProcessId == root.id() {
                        root_exit = Some(code as i32);
                    }
                    processes.remove(&event.dwProcessId);
                }
                _ => {}
            }
            if ContinueDebugEvent(event.dwProcessId, event.dwThreadId, status) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        if let Some(code) = root_exit
            && processes.is_empty()
        {
            let _ = root.wait();
            return Ok(code);
        }
    }
}
