//! Native AppContainer launch with an explicit inherited-handle list. The child
//! stays suspended until its job is attached; all native ownership is RAII.
use std::{
    ffi::{OsStr, OsString},
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, Isolation::*, *},
    System::{Com::CoTaskMemFree, Threading::*},
};
fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in process configuration",
        ));
    }
    value.push(0);
    Ok(value)
}
fn string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}
pub(crate) fn duplicate(raw: HANDLE, inherit: bool) -> io::Result<OwnedHandle> {
    let mut copied = std::ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            raw,
            GetCurrentProcess(),
            &mut copied,
            0,
            i32::from(inherit),
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(copied.cast()) })
}
struct Profile {
    name: Vec<u16>,
    sid: PSID,
    folder: PathBuf,
}
// The allocated immutable SID is process-independent Windows memory, freed once
// after the process owner/reaper has observed exit. No thread-affine API owns it.
unsafe impl Send for Profile {}
impl Profile {
    fn create() -> io::Result<Self> {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|error| io::Error::other(error.to_string()))?;
        let name = string(&format!(
            "Wega.Content.{}",
            nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
        let mut sid = std::ptr::null_mut();
        let status = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                std::ptr::null(),
                0,
                &mut sid,
            )
        };
        if status < 0 {
            return Err(io::Error::other(format!(
                "Cannot create AppContainer profile: {status:#x}"
            )));
        }
        let mut profile = Self {
            name,
            sid,
            folder: PathBuf::new(),
        };
        let mut sid_text = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(profile.sid, &mut sid_text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut folder = std::ptr::null_mut();
        let status = unsafe { GetAppContainerFolderPath(sid_text, &mut folder) };
        unsafe {
            LocalFree(sid_text.cast());
        }
        if status < 0 || folder.is_null() {
            return Err(io::Error::other("Cannot resolve AppContainer folder"));
        }
        let mut length = 0;
        while unsafe { *folder.add(length) } != 0 {
            length += 1;
        }
        use std::os::windows::ffi::OsStringExt;
        profile.folder = PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(folder, length)
        }));
        unsafe {
            CoTaskMemFree(folder.cast());
        }
        Ok(profile)
    }
    fn executable(&self, source: &Path) -> io::Result<PathBuf> {
        std::fs::create_dir_all(&self.folder)?;
        let destination = self.folder.join("wega-engine.exe");
        std::fs::copy(source, &destination)?;
        let mut sid_text = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(self.sid, &mut sid_text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while unsafe { *sid_text.add(length) } != 0 {
            length += 1;
        }
        let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(sid_text, length) })
            .map_err(io::Error::other);
        unsafe {
            LocalFree(sid_text.cast());
        }
        // The worker may read/execute this selected image, never the host's
        // profile/project directory. Owner/system retain normal cleanup access.
        let dacl = string(&format!("D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;GRGX;;;{})", sid?));
        let mut security = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                dacl.as_ptr(),
                1,
                &mut security,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let path = wide(destination.as_os_str())?;
        let updated = unsafe {
            SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                security,
            )
        };
        unsafe {
            LocalFree(security.cast());
        }
        if updated == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(destination)
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        unsafe {
            if !self.sid.is_null() {
                FreeSid(self.sid);
            }
            DeleteAppContainerProfile(self.name.as_ptr());
        }
    }
}
struct Attributes {
    storage: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), count, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut this = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        if unsafe { InitializeProcThreadAttributeList(this.pointer(), count, 0, &mut bytes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        this.initialized = true;
        Ok(this)
    }
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    fn set<T>(&mut self, key: u32, value: &T) -> io::Result<()> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                key as usize,
                (value as *const T).cast_mut().cast(),
                size_of::<T>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::new(
                io::Error::last_os_error().kind(),
                format!("Process attribute {key:#x}: {}", io::Error::last_os_error()),
            ));
        }
        Ok(())
    }
    fn handles(&mut self, handles: &mut [HANDLE]) -> io::Result<()> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_mut_ptr().cast(),
                std::mem::size_of_val(handles),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }
}
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    if unsafe {
        windows_sys::Win32::System::Pipes::CreatePipe(
            &mut read,
            &mut write,
            std::ptr::null(),
            65536,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read.cast()),
            OwnedHandle::from_raw_handle(write.cast()),
        )
    })
}
fn quote(argument: &OsStr) -> io::Result<Vec<u16>> {
    let argument = wide(argument)?;
    let mut output = vec![34];
    let mut slashes = 0;
    for code in argument.into_iter().take_while(|code| *code != 0) {
        if code == 92 {
            slashes += 1;
            continue;
        }
        if code == 34 {
            output.extend(std::iter::repeat_n(92, slashes * 2 + 1));
            output.push(34);
        } else {
            output.extend(std::iter::repeat_n(92, slashes));
            output.push(code);
        }
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(92, slashes * 2));
    output.push(34);
    Ok(output)
}
pub fn spawn(command: &Command, required: bool, grants: &[OwnedHandle]) -> io::Result<NativeChild> {
    let profile = if required {
        Some(Profile::create().map_err(|error| {
            io::Error::new(error.kind(), format!("AppContainer profile setup: {error}"))
        })?)
    } else {
        None
    };
    let program = if let Some(profile) = &profile {
        profile
            .executable(Path::new(command.get_program()))
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("AppContainer executable setup: {error}"),
                )
            })?
    } else {
        PathBuf::from(command.get_program())
    };
    let executable = wide(program.as_os_str())?;
    let mut line = quote(program.as_os_str())?;
    for argument in command.get_args() {
        line.push(32);
        line.extend(quote(argument)?);
    }
    line.push(0);
    let mut environment: Vec<_> = command
        .get_envs()
        .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))
        .collect();
    environment.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (key, value) in environment {
        let mut pair = key;
        pair.push("=");
        pair.push(value);
        block.extend(wide(&pair)?);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    let directory = profile
        .as_ref()
        .map(|profile| profile.folder.as_path())
        .or_else(|| command.get_current_dir())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing worker directory"))?;
    let directory = wide(directory.as_os_str())?;
    let (input, stdin) = pipe()?;
    let (stdout, output) = pipe()?;
    let (stderr, error) = pipe()?;
    let input = duplicate(input.as_raw_handle().cast(), true)?;
    let output = duplicate(output.as_raw_handle().cast(), true)?;
    let error = duplicate(error.as_raw_handle().cast(), true)?;
    let mut handles: Vec<HANDLE> = vec![
        input.as_raw_handle().cast(),
        output.as_raw_handle().cast(),
        error.as_raw_handle().cast(),
    ];
    handles.extend(grants.iter().map(|handle| handle.as_raw_handle().cast()));
    let mut attributes = Attributes::new(if required { 3 } else { 1 })?;
    attributes.handles(&mut handles)?;
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: profile
            .as_ref()
            .map(|profile| profile.sid)
            .unwrap_or(std::ptr::null_mut()),
        Capabilities: std::ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let restricted_children = 1_u32;
    if required {
        attributes.set(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
            &restricted_children,
        )?;
    }
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    // USER32 imports must initialize against the standard low-integrity desktop,
    // rather than inheriting a CI/service window station that rejects AppContainer
    // tokens. The attached job denies clipboard, foreign USER handles, desktop
    // changes and global atoms before the suspended process is resumed. No host
    // window-station ACL is modified to grant the worker access.
    let mut desktop = string("WinSta0\\Default");
    startup.StartupInfo.lpDesktop = desktop.as_mut_ptr();
    startup.StartupInfo.hStdInput = input.as_raw_handle().cast();
    startup.StartupInfo.hStdOutput = output.as_raw_handle().cast();
    startup.StartupInfo.hStdError = error.as_raw_handle().cast();
    startup.lpAttributeList = attributes.pointer();
    let mut result: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        CreateProcessW(
            executable.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_NO_WINDOW
                | EXTENDED_STARTUPINFO_PRESENT,
            block.as_ptr().cast(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut result,
        )
    } == 0
    {
        return Err(io::Error::new(
            io::Error::last_os_error().kind(),
            format!(
                "CreateProcessW content helper: {}",
                io::Error::last_os_error()
            ),
        ));
    }
    Ok(unsafe {
        NativeChild {
            process: OwnedHandle::from_raw_handle(result.hProcess.cast()),
            thread: OwnedHandle::from_raw_handle(result.hThread.cast()),
            pid: result.dwProcessId,
            stdin: Some(stdin.into()),
            stdout: Some(stdout.into()),
            stderr: Some(stderr.into()),
            _profile: profile,
        }
    })
}
pub struct NativeChild {
    process: OwnedHandle,
    thread: OwnedHandle,
    pid: u32,
    pub(crate) stdin: Option<ChildStdin>,
    pub(crate) stdout: Option<ChildStdout>,
    pub(crate) stderr: Option<ChildStderr>,
    _profile: Option<Profile>,
}
impl NativeChild {
    pub fn id(&self) -> u32 {
        self.pid
    }
    pub fn resume(&self) -> io::Result<()> {
        if unsafe { ResumeThread(self.thread.as_raw_handle().cast()) } == u32::MAX {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = unsafe { WaitForSingleObject(self.process.as_raw_handle().cast(), 0) };
        if status == WAIT_TIMEOUT {
            return Ok(None);
        }
        if status != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.process.as_raw_handle().cast(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        use std::os::windows::process::ExitStatusExt;
        Ok(Some(ExitStatus::from_raw(code)))
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        if unsafe { WaitForSingleObject(self.process.as_raw_handle().cast(), u32::MAX) }
            != WAIT_OBJECT_0
        {
            return Err(io::Error::last_os_error());
        }
        self.try_wait()?
            .ok_or_else(|| io::Error::other("Process wait lost exit status"))
    }
    pub fn kill(&mut self) -> io::Result<()> {
        if unsafe { TerminateProcess(self.process.as_raw_handle().cast(), 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
impl AsRawHandle for NativeChild {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.process.as_raw_handle()
    }
}
impl Drop for NativeChild {
    fn drop(&mut self) {
        if self.try_wait().ok().flatten().is_none() {
            let _ = self.kill();
            let _ = self.wait();
        }
    }
}

pub(crate) fn verify() -> io::Result<()> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token.cast()) };
    let mut container = 0_u32;
    let mut returned = 0;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenIsAppContainer,
            (&mut container as *mut u32).cast(),
            4,
            &mut returned,
        )
    } == 0
        || container != 1
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Worker is not an AppContainer",
        ));
    }
    let mut required = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenCapabilities,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
    }
    if required == 0 || required > 65536 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Invalid AppContainer capabilities",
        ));
    }
    let mut capabilities = vec![0_usize; (required as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenCapabilities,
            capabilities.as_mut_ptr().cast(),
            required,
            &mut returned,
        )
    } == 0
        || unsafe { *capabilities.as_ptr().cast::<u32>() } != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Content AppContainer has unapproved capabilities",
        ));
    }
    let mut in_job = 0;
    if unsafe {
        windows_sys::Win32::System::JobObjects::IsProcessInJob(
            GetCurrentProcess(),
            std::ptr::null_mut(),
            &mut in_job,
        )
    } == 0
        || in_job == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Content job was not attached before execution",
        ));
    }
    Ok(())
}
