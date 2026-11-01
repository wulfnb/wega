//! A separately signed App Sandbox helper, with JIT and no ambient network/user
//! file entitlements. No deprecated sandbox_init/private Seatbelt policy API.
use std::{
    ffi::{CStr, c_char, c_void},
    io,
};
type Ref = *const c_void;
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCodeCopySelf(flags: u32, code: *mut Ref) -> i32;
    fn SecCodeCopySigningInformation(code: Ref, flags: u32, information: *mut Ref) -> i32;
    static kSecCodeInfoEntitlementsDict: Ref;
    static kSecCodeInfoFlags: Ref;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFDictionaryGetCount(value: Ref) -> isize;
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFDictionaryGetKeysAndValues(dictionary: Ref, keys: *mut Ref, values: *mut Ref);
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetCString(value: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> bool;
    fn CFNumberGetValue(value: Ref, kind: isize, output: *mut c_void) -> bool;
    static kCFBooleanTrue: Ref;
    static kCFBooleanFalse: Ref;
}
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}
fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
pub(super) fn verify() -> io::Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        return Err(denied("Elevated content workers are forbidden"));
    }
    // SAFETY: Security/CoreFoundation return retained immutable objects. They
    // stay owned for every borrowed lookup and are released exactly once.
    unsafe {
        let mut code = Owned(std::ptr::null());
        if SecCodeCopySelf(0, &mut code.0) != 0 {
            return Err(denied("Cannot inspect content-helper signature"));
        }
        let mut information = Owned(std::ptr::null());
        if SecCodeCopySigningInformation(code.0, 2, &mut information.0) != 0
            || information.0.is_null()
        {
            return Err(denied("Content helper must be signed"));
        }
        let flags = CFDictionaryGetValue(information.0, kSecCodeInfoFlags);
        let mut signature_flags: i32 = 0;
        if flags.is_null()
            || !CFNumberGetValue(flags, 3, (&mut signature_flags as *mut i32).cast())
            || signature_flags & 0x10000 == 0
        {
            return Err(denied("Content helper requires hardened runtime signing"));
        }
        let entitlements = CFDictionaryGetValue(information.0, kSecCodeInfoEntitlementsDict);
        if entitlements.is_null() || CFGetTypeID(entitlements) != CFDictionaryGetTypeID() {
            return Err(denied("Missing App Sandbox entitlements"));
        }
        let count = CFDictionaryGetCount(entitlements);
        if !(1..=128).contains(&count) {
            return Err(denied("Invalid helper entitlement count"));
        }
        let mut keys = vec![std::ptr::null(); count as usize];
        let mut values = keys.clone();
        CFDictionaryGetKeysAndValues(entitlements, keys.as_mut_ptr(), values.as_mut_ptr());
        let mut sandbox = false;
        let mut jit = false;
        for (key, value) in keys.into_iter().zip(values) {
            if key.is_null() || CFGetTypeID(key) != CFStringGetTypeID() {
                return Err(denied("Invalid entitlement name"));
            }
            let mut bytes = [0_i8; 1024];
            if !CFStringGetCString(key, bytes.as_mut_ptr(), bytes.len() as isize, 0x08000100) {
                return Err(denied("Invalid entitlement encoding"));
            }
            let name = CStr::from_ptr(bytes.as_ptr())
                .to_str()
                .map_err(|_| denied("Invalid entitlement encoding"))?;
            match name {
                "com.apple.security.app-sandbox" => sandbox = value == kCFBooleanTrue,
                "com.apple.security.cs.allow-jit" => jit = value == kCFBooleanTrue,
                "com.apple.application-identifier"
                | "application-identifier"
                | "com.apple.developer.team-identifier" => {}
                _ if value == kCFBooleanFalse => {}
                _ => return Err(denied("Content helper has an unapproved entitlement")),
            }
        }
        if !sandbox || !jit {
            return Err(denied("Content helper needs App Sandbox and allow-jit"));
        }
        let children = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let cores = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::setrlimit(libc::RLIMIT_NPROC, &children) != 0
            || libc::setrlimit(libc::RLIMIT_CORE, &cores) != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

unsafe extern "C" {
    static mach_task_self_: u32;
    fn task_get_special_port(task: u32, which: i32, port: *mut u32) -> i32;
    fn task_set_special_port(task: u32, which: i32, port: u32) -> i32;
    fn mach_port_destroy(task: u32, port: u32) -> i32;
}
/// Remove the ambient launchd namespace after signature inspection and explicit
/// grants. Destroying this child's name also invalidates libSystem's cached name;
/// it does not destroy the launchd service or the parent's send right.
pub(super) fn finish_grants() -> io::Result<()> {
    // SAFETY: these public Mach APIs operate only on this task's special port
    // and a send right obtained from it, before worker/VM threads exist.
    unsafe {
        let task = mach_task_self_;
        let mut bootstrap = 0;
        if task_get_special_port(task, 4, &mut bootstrap) != 0
            || task_set_special_port(task, 4, 0) != 0
            || (bootstrap != 0 && mach_port_destroy(task, bootstrap) != 0)
        {
            return Err(denied("Cannot remove ambient bootstrap service namespace"));
        }
    }
    Ok(())
}
