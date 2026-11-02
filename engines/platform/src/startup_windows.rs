//! Trusted native DLL startup precedes job UI lockdown; VM/document startup does
//! not. Two anonymous, direction-limited events keep that boundary off the UI
//! thread. AppContainer, child policy and job memory/process limits apply first.
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::SYNCHRONIZE,
    System::{JobObjects::*, Threading::*},
};
const READY: &str = "--content-entry-ready";
const RELEASE: &str = "--content-entry-release";
const TIMEOUT: u32 = 20_000;
pub(super) struct Gates {
    ready: OwnedHandle,
    release: OwnedHandle,
    child_ready: OwnedHandle,
    child_release: OwnedHandle,
}
fn event() -> io::Result<OwnedHandle> {
    // SAFETY: an anonymous auto-reset event, initially nonsignaled; unique owner.
    let raw = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(raw.cast()) })
}
fn duplicate(raw: HANDLE, access: u32, inherit: bool) -> io::Result<OwnedHandle> {
    let mut output = std::ptr::null_mut();
    // SAFETY: duplicates in this process without adopting the caller's handle.
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            raw,
            GetCurrentProcess(),
            &mut output,
            access,
            i32::from(inherit),
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(output.cast()) })
}
impl Gates {
    pub fn new() -> io::Result<Self> {
        let ready = event()?;
        let release = event()?;
        let child_ready = duplicate(ready.as_raw_handle().cast(), EVENT_MODIFY_STATE, true)?;
        let child_release = duplicate(release.as_raw_handle().cast(), SYNCHRONIZE, true)?;
        Ok(Self {
            ready,
            release,
            child_ready,
            child_release,
        })
    }
    pub fn arguments(&self) -> [String; 4] {
        [
            READY.into(),
            (self.child_ready.as_raw_handle() as usize).to_string(),
            RELEASE.into(),
            (self.child_release.as_raw_handle() as usize).to_string(),
        ]
    }
    pub fn child_handles(&self) -> [HANDLE; 2] {
        [
            self.child_ready.as_raw_handle().cast(),
            self.child_release.as_raw_handle().cast(),
        ]
    }
    pub fn watch(
        self: &Arc<Self>,
        job: &OwnedHandle,
        process: &OwnedHandle,
    ) -> io::Result<std::thread::JoinHandle<()>> {
        let gates = self.clone();
        let job = super::duplicate(job.as_raw_handle().cast(), false)?;
        let process = super::duplicate(process.as_raw_handle().cast(), false)?;
        std::thread::Builder::new()
            .name("wega-native-entry".into())
            .spawn(move || {
                let handles = [
                    gates.ready.as_raw_handle().cast(),
                    process.as_raw_handle().cast(),
                ];
                // SAFETY: all independently owned handles remain live through the
                // wait and UI policy application. No stored PID is used for killing.
                let ready = unsafe {
                    WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, TIMEOUT)
                };
                if ready == WAIT_OBJECT_0 + 1 {
                    return;
                }
                let result = if ready == WAIT_OBJECT_0 {
                    crate::launch::windows::restrict_ui(&job).and_then(|_| {
                        if unsafe { SetEvent(gates.release.as_raw_handle().cast()) } == 0 {
                            Err(io::Error::last_os_error())
                        } else {
                            Ok(())
                        }
                    })
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Native startup gate did not open",
                    ))
                };
                if let Err(error) = result {
                    eprintln!("Wega native entry policy failed: {error}");
                    unsafe {
                        TerminateProcess(process.as_raw_handle().cast(), 1);
                    }
                }
            })
    }
}
/// Required workers must wait for the host; explicit legacy debug fixtures with
/// no inherited gates remain supported. The child cannot signal its release event.
pub(super) fn enter(required: bool) -> io::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let argument = |name: &str| -> io::Result<Option<OwnedHandle>> {
        let Some(pair) = args.windows(2).find(|pair| pair[0] == name) else {
            return Ok(None);
        };
        let raw = pair[1]
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0 && *value <= isize::MAX as usize)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "Invalid native startup event")
            })? as HANDLE;
        let access = if name == READY {
            EVENT_MODIFY_STATE
        } else {
            SYNCHRONIZE
        };
        duplicate(raw, access, false).map(Some)
    };
    let (ready, release) = match (argument(READY)?, argument(RELEASE)?) {
        (Some(ready), Some(release)) => (ready, release),
        (None, None) if !required => return Ok(()),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Missing native startup policy gate",
            ));
        }
    };
    if unsafe { SetEvent(ready.as_raw_handle().cast()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { WaitForSingleObject(release.as_raw_handle().cast(), TIMEOUT) } != WAIT_OBJECT_0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native UI policy was not applied",
        ));
    }
    let mut ui: JOBOBJECT_BASIC_UI_RESTRICTIONS = unsafe { std::mem::zeroed() };
    if unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectBasicUIRestrictions,
            (&mut ui as *mut JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
            std::mem::size_of_val(&ui) as u32,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let required = JOB_OBJECT_UILIMIT_DESKTOP
        | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
        | JOB_OBJECT_UILIMIT_EXITWINDOWS
        | JOB_OBJECT_UILIMIT_GLOBALATOMS
        | JOB_OBJECT_UILIMIT_HANDLES
        | JOB_OBJECT_UILIMIT_READCLIPBOARD
        | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
        | JOB_OBJECT_UILIMIT_WRITECLIPBOARD;
    if ui.UIRestrictionsClass & required != required {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Incomplete native UI policy",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_can_signal_readiness_but_cannot_release_itself() {
        let gates = Gates::new().unwrap();
        // Kernel handle rights apply even to this unrestricted test process.
        assert_eq!(
            unsafe { SetEvent(gates.child_release.as_raw_handle().cast()) },
            0
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(ERROR_ACCESS_DENIED as i32)
        );
        assert_eq!(
            unsafe { WaitForSingleObject(gates.child_release.as_raw_handle().cast(), 0) },
            WAIT_TIMEOUT
        );
        assert_ne!(
            unsafe { SetEvent(gates.child_ready.as_raw_handle().cast()) },
            0
        );
        assert_eq!(
            unsafe { WaitForSingleObject(gates.ready.as_raw_handle().cast(), 0) },
            WAIT_OBJECT_0
        );
        assert_ne!(unsafe { SetEvent(gates.release.as_raw_handle().cast()) }, 0);
        assert_eq!(
            unsafe { WaitForSingleObject(gates.child_release.as_raw_handle().cast(), 0) },
            WAIT_OBJECT_0
        );
    }
}
