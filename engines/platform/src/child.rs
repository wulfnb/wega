//! The process owner has one lifecycle for std children and native suspended
//! AppContainer launches. Only platform backends touch raw process handles.
use std::{
    io,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, ExitStatus},
};
pub enum ChildOwner {
    Standard(Child),
    #[cfg(windows)]
    Native(crate::windows_spawn::NativeChild),
}
impl From<Child> for ChildOwner {
    fn from(child: Child) -> Self {
        Self::Standard(child)
    }
}
impl ChildOwner {
    pub fn id(&self) -> u32 {
        match self {
            Self::Standard(child) => child.id(),
            #[cfg(windows)]
            Self::Native(child) => child.id(),
        }
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match self {
            Self::Standard(child) => child.try_wait(),
            #[cfg(windows)]
            Self::Native(child) => child.try_wait(),
        }
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        match self {
            Self::Standard(child) => child.wait(),
            #[cfg(windows)]
            Self::Native(child) => child.wait(),
        }
    }
    pub fn kill(&mut self) -> io::Result<()> {
        match self {
            Self::Standard(child) => child.kill(),
            #[cfg(windows)]
            Self::Native(child) => child.kill(),
        }
    }
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        match self {
            Self::Standard(child) => child.stdin.take(),
            #[cfg(windows)]
            Self::Native(child) => child.stdin.take(),
        }
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        match self {
            Self::Standard(child) => child.stdout.take(),
            #[cfg(windows)]
            Self::Native(child) => child.stdout.take(),
        }
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        match self {
            Self::Standard(child) => child.stderr.take(),
            #[cfg(windows)]
            Self::Native(child) => child.stderr.take(),
        }
    }
    #[cfg(windows)]
    pub(crate) fn resume(&self) -> io::Result<()> {
        if let Self::Native(child) = self {
            child.resume()?;
        }
        Ok(())
    }
}
#[cfg(windows)]
impl std::os::windows::io::AsRawHandle for ChildOwner {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        match self {
            Self::Standard(child) => child.as_raw_handle(),
            Self::Native(child) => child.as_raw_handle(),
        }
    }
}
