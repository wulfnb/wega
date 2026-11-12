//! Per-profile launch handoff on the current Windows desktop. No listening socket.
use crate::engine::Wake;
use std::{
    ptr,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
    },
    System::{
        DataExchange::COPYDATASTRUCT, LibraryLoader::GetModuleHandleW, Threading::CreateMutexW,
    },
    UI::WindowsAndMessaging::*,
};
use winit::event_loop::EventLoopProxy;

const MAGIC: usize = 0x57454741;
const LIMIT: usize = 32 * 8192 + 1024;
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub struct Claim {
    mutex: HANDLE,
    name: Vec<u16>,
}
impl Drop for Claim {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.mutex);
        }
    }
}

/// None means the existing process acknowledged the launch. Diagnostic/test
/// options must never silently attach to another browser or alter its settings.
pub fn claim(
    options: &wega::options::Options,
) -> Result<Option<Claim>, Box<dyn std::error::Error>> {
    let directory = options
        .profile_dir
        .clone()
        .unwrap_or_else(wega::profile::Profile::default_dir);
    std::fs::create_dir_all(&directory)?;
    let path = directory.canonicalize()?.to_string_lossy().to_lowercase();
    let hash = path.as_bytes().iter().fold(0xcbf29ce484222325_u64, |n, b| {
        (n ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    let name = wide(&format!("Wega.Launch.{hash:016x}"));
    let mutex_name = wide(&format!("Local\\Wega.Profile.{hash:016x}"));
    let mutex = unsafe { CreateMutexW(ptr::null(), 0, mutex_name.as_ptr()) };
    if mutex.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let claim = Claim { mutex, name };
    if !existing {
        return Ok(Some(claim));
    }
    if options.engine.is_some()
        || options.test_mode()
        || options.capture.is_some()
        || options.webdriver_port.is_some()
    {
        return Err(
            "This profile is already running; choose its engine in the browser, or use an isolated profile for diagnostics".into(),
        );
    }
    let urls: Vec<String> = options
        .url
        .as_deref()
        .map(wega::profile::resolve_address)
        .transpose()?
        .into_iter()
        .map(|url| url.to_string())
        .collect();
    if wega::options::external_launch_urls(&urls).len() != urls.len() {
        return Err("Only web and file URLs can be handed to a running browser".into());
    }
    let payload = serde_json::to_vec(&urls)?;
    if payload.len() > LIMIT {
        return Err("Launch URL is too long".into());
    }
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        let hwnd = unsafe {
            FindWindowExW(
                HWND_MESSAGE,
                ptr::null_mut(),
                claim.name.as_ptr(),
                claim.name.as_ptr(),
            )
        };
        if !hwnd.is_null() {
            let packet = COPYDATASTRUCT {
                dwData: MAGIC,
                cbData: payload.len() as u32,
                lpData: payload.as_ptr().cast_mut().cast(),
            };
            let mut accepted = 0;
            let sent = unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    WM_COPYDATA,
                    0,
                    (&packet as *const COPYDATASTRUCT) as isize,
                    SMTO_ABORTIFHUNG | SMTO_BLOCK,
                    2000,
                    &mut accepted,
                )
            };
            if sent != 0 && accepted == 1 {
                return Ok(None);
            }
            return Err("The running browser did not accept this launch".into());
        }
        if Instant::now() >= until {
            return Err("Another browser is starting with this profile; try again shortly".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }
    if message == WM_COPYDATA && lparam != 0 {
        let packet = unsafe { &*(lparam as *const COPYDATASTRUCT) };
        if packet.dwData != MAGIC || packet.lpData.is_null() || packet.cbData as usize > LIMIT {
            return 0;
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(packet.lpData.cast::<u8>(), packet.cbData as usize)
        };
        let Ok(urls) = serde_json::from_slice::<Vec<String>>(bytes) else {
            return 0;
        };
        if urls.len() > 32 || wega::options::external_launch_urls(&urls).len() != urls.len() {
            return 0;
        }
        let proxy =
            unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const EventLoopProxy<Wake> };
        if proxy.is_null() {
            return 0;
        }
        return isize::from(unsafe { &*proxy }.send_event(Wake::OpenUrls(urls)).is_ok());
    }
    if message == WM_DESTROY {
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub struct Receiver {
    _claim: Claim,
    hwnd: HWND,
    thread: Option<thread::JoinHandle<()>>,
}
impl Claim {
    pub fn listen(
        self,
        proxy: EventLoopProxy<Wake>,
    ) -> Result<Receiver, Box<dyn std::error::Error>> {
        let name = self.name.clone();
        let (sender, ready) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let proxy = Box::new(proxy); // Stable until the window/message loop is destroyed.
            unsafe {
                let instance = GetModuleHandleW(ptr::null());
                let class = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance,
                    lpszClassName: name.as_ptr(),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    let _ = sender.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                let hwnd = CreateWindowExW(
                    0,
                    name.as_ptr(),
                    name.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    ptr::null_mut(),
                    instance,
                    (&*proxy as *const EventLoopProxy<Wake>).cast(),
                );
                if hwnd.is_null() {
                    let _ = sender.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                if sender.send(Ok(hwnd as usize)).is_err() {
                    DestroyWindow(hwnd);
                    return;
                }
                let mut message: MSG = std::mem::zeroed();
                while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        });
        let hwnd = ready
            .recv()
            .map_err(|_| "Launch receiver stopped during startup")?
            .map_err(std::io::Error::other)? as HWND;
        Ok(Receiver {
            _claim: self,
            hwnd,
            thread: Some(thread),
        })
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
