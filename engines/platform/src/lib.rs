//! Small safe interfaces around process metrics and private frame mappings.
//! The DOM/VM crate remains `forbid(unsafe_code)`; native calls live here.
#![deny(unsafe_op_in_unsafe_fn)]
use std::{fs::File, io, path::Path};
#[cfg(unix)]
pub mod inherited;
pub mod launch;
pub mod process;
pub mod supervision;

pub(crate) fn resident_for_child(child: &std::process::Child) -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v0>::uninit();
        // SAFETY: the supervisor retains an unreaped child while querying its PID;
        // the kernel initializes the correctly sized v0 buffer on success.
        let result = unsafe {
            libc::proc_pid_rusage(
                child.id() as libc::c_int,
                libc::RUSAGE_INFO_V0,
                usage.as_mut_ptr().cast(),
            )
        };
        (result == 0).then(|| unsafe { usage.assume_init() }.ri_resident_size)
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string(format!("/proc/{}/status", child.id())).ok()?;
        let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
        line.split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        let mut counters = std::mem::MaybeUninit::<PROCESS_MEMORY_COUNTERS>::zeroed();
        // SAFETY: Child retains the live process handle and the writable output
        // has the exact PROCESS_MEMORY_COUNTERS size requested by the API.
        let result = unsafe {
            GetProcessMemoryInfo(
                child.as_raw_handle().cast(),
                counters.as_mut_ptr(),
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        (result != 0).then(|| unsafe { counters.assume_init() }.WorkingSetSize as u64)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = child;
        None
    }
}

/// Current resident/working-set bytes, never peak usage or the JS heap.
pub fn resident_bytes() -> Option<u64> {
    resident()
}

#[cfg(target_os = "macos")]
fn resident() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v0>::uninit();
    // SAFETY: proc_pid_rusage initializes this correctly sized v0 output on success.
    let result = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V0,
            usage.as_mut_ptr().cast(),
        )
    };
    (result == 0).then(|| unsafe { usage.assume_init() }.ri_resident_size)
}

#[cfg(target_os = "linux")]
fn resident() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

#[cfg(windows)]
fn resident() -> Option<u64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };
    let mut counters = std::mem::MaybeUninit::<PROCESS_MEMORY_COUNTERS>::zeroed();
    // SAFETY: the pseudo-handle refers to this process, the writable output is
    // exactly PROCESS_MEMORY_COUNTERS, and the API writes it before success.
    let result = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            counters.as_mut_ptr(),
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    (result != 0).then(|| unsafe { counters.assume_init() }.WorkingSetSize as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn resident() -> Option<u64> {
    None
}

/// Return unused glibc heap pages after a tab has released renderer/VM caches.
pub fn trim_allocator() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim has no pointer arguments and is thread-safe in glibc.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Two bounded frame slots. The private pipe grants a slot to the producer and
/// acknowledges the sequence after the reader copies it. A slot must never be
/// reused before that acknowledgement; mappings are not a synchronization device.
pub struct FrameStore {
    file: File,
    mapping: Option<memmap2::MmapMut>,
    capacity: usize,
    owner: Option<tempfile::NamedTempFile>,
}
/// A browser consumer has no mapping/producer API. Host copies use bounded
/// positional reads even if a compromised renderer resizes its own backing.
pub struct FrameReader(FrameStore);
impl FrameReader {
    pub fn path(&self) -> Option<&Path> {
        self.0.path()
    }
    pub fn try_clone_file(&self) -> io::Result<File> {
        self.0.try_clone_file()
    }
    pub fn read_slot(&self, slot: usize, bytes: usize) -> io::Result<Vec<u8>> {
        self.0.read_slot(slot, bytes)
    }
}
impl FrameStore {
    pub fn into_reader(mut self) -> FrameReader {
        self.release_pages();
        FrameReader(self)
    }
    pub fn create(capacity: usize) -> io::Result<Self> {
        Self::validate_capacity(capacity)?;
        let owner = tempfile::Builder::new().prefix("wega-frames-").tempfile()?;
        owner.as_file().set_len((capacity * 2) as u64)?;
        Ok(Self {
            file: owner.as_file().try_clone()?,
            mapping: None,
            capacity,
            owner: Some(owner),
        })
    }
    pub fn open(path: &Path, capacity: usize) -> io::Result<Self> {
        let file = File::options().read(true).write(true).open(path)?;
        Self::from_file(file, capacity)
    }
    pub fn from_file(file: File, capacity: usize) -> io::Result<Self> {
        Self::validate_capacity(capacity)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != (capacity * 2) as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid shared frame backing size",
            ));
        }
        Ok(Self {
            file,
            mapping: None,
            capacity,
            owner: None,
        })
    }
    fn validate_capacity(capacity: usize) -> io::Result<()> {
        if capacity == 0 || capacity > 4096 * 2160 * 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid shared frame capacity",
            ));
        }
        Ok(())
    }
    pub fn path(&self) -> Option<&Path> {
        self.owner.as_ref().map(|owner| owner.path())
    }
    pub fn try_clone_file(&self) -> io::Result<File> {
        self.file.try_clone()
    }
    /// Browser-side copy: never mmap a file a hostile producer can truncate.
    /// A short or resized backing becomes an ordinary I/O error, not SIGBUS.
    pub fn read_slot(&self, slot: usize, bytes: usize) -> io::Result<Vec<u8>> {
        if slot >= 2
            || bytes > self.capacity
            || self.file.metadata()?.len() != (self.capacity * 2) as u64
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid shared frame read",
            ));
        }
        let mut output = vec![0; bytes];
        let mut done = 0;
        while done < bytes {
            #[cfg(unix)]
            let read = {
                use std::os::unix::fs::FileExt;
                self.file
                    .read_at(&mut output[done..], (slot * self.capacity + done) as u64)
            };
            #[cfg(windows)]
            let read = {
                use std::os::windows::fs::FileExt;
                self.file
                    .seek_read(&mut output[done..], (slot * self.capacity + done) as u64)
            };
            #[cfg(not(any(unix, windows)))]
            let read: io::Result<usize> = Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Positional frame reads are unsupported",
            ));
            match read {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                Ok(read) => done += read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(output)
    }
    pub fn slot(&mut self, slot: usize, bytes: usize) -> io::Result<&mut [u8]> {
        if slot >= 2 || bytes > self.capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid shared frame slot",
            ));
        }
        if self.mapping.is_none() {
            // SAFETY: producer mappings have a validated size and slot credits
            // coordinate access. Production browser consumers use read_slot,
            // so truncating this backing can fault only the content producer.
            self.mapping = Some(unsafe {
                memmap2::MmapOptions::new()
                    .len(self.capacity * 2)
                    .map_mut(&self.file)?
            });
        }
        let start = slot * self.capacity;
        Ok(&mut self.mapping.as_mut().unwrap()[start..start + bytes])
    }
    /// Unmap page tables while frozen. The sparse backing and contents survive.
    pub fn release_pages(&mut self) {
        self.mapping = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consumer_frame_reads_reject_a_backing_truncated_by_the_producer() {
        let owner = FrameStore::create(1024).unwrap();
        let producer = owner.try_clone_file().unwrap();
        let owner = owner.into_reader();
        let mut peer = FrameStore::from_file(producer, 1024).unwrap();
        peer.slot(1, 4).unwrap().copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(owner.read_slot(1, 4).unwrap(), [1, 2, 3, 4]);
        assert!(owner.read_slot(2, 4).is_err());
        assert!(owner.read_slot(0, 1025).is_err());
        peer.release_pages();
        peer.file.set_len(0).unwrap();
        assert!(owner.read_slot(1, 4).is_err());
        assert!(FrameStore::from_file(peer.file.try_clone().unwrap(), 1024).is_err());
    }
    #[test]
    fn shared_slots_are_bounded_and_survive_mapping_release() {
        let mut owner = FrameStore::create(1024).unwrap();
        let mut peer = FrameStore::open(owner.path().unwrap(), 1024).unwrap();
        assert!(FrameStore::open(owner.path().unwrap(), 512).is_err());
        owner.slot(1, 4).unwrap().copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(peer.slot(1, 4).unwrap(), &[1, 2, 3, 4]);
        owner.release_pages();
        peer.release_pages();
        assert_eq!(peer.slot(1, 4).unwrap(), &[1, 2, 3, 4]);
        assert!(owner.slot(2, 1).is_err());
        assert!(owner.slot(0, 1025).is_err());
    }
    #[test]
    fn current_residency_is_available_on_supported_desktops() {
        #[cfg(any(target_os = "macos", target_os = "linux", windows))]
        assert!(resident_bytes().is_some_and(|bytes| bytes > 0));
        trim_allocator();
    }
}
