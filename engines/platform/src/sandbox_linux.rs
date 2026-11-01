//! Enter before creating any worker thread. No path grants: fonts, files and
//! storage arrive through the inherited broker; frame bytes use an inherited FD.
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};
#[repr(C)]
struct Ruleset {
    filesystem: u64,
    network: u64,
    scoped: u64,
}
fn deny(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
pub(super) fn enter() -> io::Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        return Err(deny("Elevated content workers are forbidden"));
    }
    #[repr(C)]
    struct CapabilityHeader {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CapabilityData {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    let mut header = CapabilityHeader {
        version: 0x20080522,
        pid: 0,
    };
    let mut capabilities = [CapabilityData::default(); 2];
    // SAFETY: Linux capability ABI v3 writes exactly two data records for this
    // process. Reject inherited/file capabilities rather than retaining them.
    if unsafe { libc::syscall(libc::SYS_capget, &mut header, capabilities.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if capabilities
        .iter()
        .any(|cap| cap.effective != 0 || cap.permitted != 0 || cap.inheritable != 0)
    {
        return Err(deny("Content workers must not retain Linux capabilities"));
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return Err(deny("Unsupported content-sandbox syscall architecture"));
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        // SAFETY: Landlock syscalls accept the kernel ABI's scalar arguments and
        // this exactly sized C-layout attribute. The returned FD is owned once.
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<Ruleset>(),
                0_usize,
                1_u32,
            )
        };
        if abi < 6 {
            return Err(deny("Content sandbox requires Landlock ABI 6 or later"));
        }
        let rules = Ruleset {
            filesystem: (1 << 16) - 1,
            network: 3,
            scoped: 3,
        };
        let fd = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &rules,
                std::mem::size_of::<Ruleset>(),
                0_u32,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let ruleset = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // No allow rules: existing private descriptors remain usable, while new
        // file opens, execution, mutation and TCP connection/bind are denied.
        if unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0_u32) }
            != 0
        {
            return Err(io::Error::last_os_error());
        }
        drop(ruleset);
        let core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &core) } != 0
            || unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0
        {
            return Err(io::Error::last_os_error());
        }
        install_filter()
    }
}
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn install_filter() -> io::Result<()> {
    // seccomp_data: nr at 0, arch at 4, args at 16. Architecture and x32 guards
    // prevent numbers from another syscall ABI acquiring unintended authority.
    const LD: u16 = 0x20;
    const JEQ: u16 = 0x15;
    const JGE: u16 = 0x35;
    const AND: u16 = 0x54;
    const RET: u16 = 0x06;
    const ALLOW: u32 = 0x7fff0000;
    const KILL: u32 = 0x80000000;
    const EPERM: u32 = 0x00050000 | libc::EPERM as u32;
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000003e;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc00000b7;
    let instruction = |code, jt, jf, k| libc::sock_filter { code, jt, jf, k };
    let mut filter = vec![
        instruction(LD, 0, 0, 4),
        instruction(JEQ, 1, 0, ARCH),
        instruction(RET, 0, 0, KILL),
        instruction(LD, 0, 0, 0),
        instruction(JGE, 0, 1, 0x40000000),
        instruction(RET, 0, 0, KILL),
    ];
    // clone3's pointed-to flags cannot be inspected by classic BPF. ENOSYS lets
    // pthread implementations fall back to clone; fork/vfork/exec are never allowed.
    filter.extend([
        instruction(JEQ, 0, 1, libc::SYS_clone3 as u32),
        instruction(RET, 0, 0, 0x00050000 | libc::ENOSYS as u32),
    ]);
    let thread_flags = (libc::CLONE_VM
        | libc::CLONE_FS
        | libc::CLONE_FILES
        | libc::CLONE_SIGHAND
        | libc::CLONE_THREAD
        | libc::CLONE_SYSVSEM
        | libc::CLONE_SETTLS
        | libc::CLONE_PARENT_SETTID
        | libc::CLONE_CHILD_CLEARTID
        | libc::CLONE_CHILD_SETTID
        | libc::CLONE_DETACHED) as u32;
    filter.extend([
        instruction(JEQ, 0, 9, libc::SYS_clone as u32),
        instruction(LD, 0, 0, 16),
        instruction(AND, 0, 0, libc::CLONE_THREAD as u32),
        instruction(JEQ, 1, 0, libc::CLONE_THREAD as u32),
        instruction(RET, 0, 0, EPERM),
        instruction(LD, 0, 0, 16),
        instruction(AND, 0, 0, !thread_flags),
        instruction(JEQ, 1, 0, 0),
        instruction(RET, 0, 0, EPERM),
        instruction(RET, 0, 0, ALLOW),
    ]);
    // Threads may signal only their own process (V8 termination/profiling).
    let pid = unsafe { libc::getpid() } as u32;
    filter.extend([
        instruction(JEQ, 0, 4, libc::SYS_tgkill as u32),
        instruction(LD, 0, 0, 16),
        instruction(JEQ, 1, 0, pid),
        instruction(RET, 0, 0, EPERM),
        instruction(RET, 0, 0, ALLOW),
    ]);
    // send() maps to sendto() on Linux. Only an already-connected descriptor with
    // a NULL destination may send; creating sockets/connect/bind is denied below.
    filter.extend([
        instruction(JEQ, 0, 6, libc::SYS_sendto as u32),
        instruction(LD, 0, 0, 48),
        instruction(JEQ, 0, 3, 0),
        instruction(LD, 0, 0, 52),
        instruction(JEQ, 0, 1, 0),
        instruction(RET, 0, 0, ALLOW),
        instruction(RET, 0, 0, EPERM),
    ]);
    filter.extend([
        instruction(JEQ, 0, 5, libc::SYS_prlimit64 as u32),
        instruction(LD, 0, 0, 16),
        instruction(JEQ, 2, 0, 0),
        instruction(JEQ, 1, 0, pid),
        instruction(RET, 0, 0, EPERM),
        instruction(RET, 0, 0, ALLOW),
    ]);
    // F_SETOWN/F_SETSIG on an inherited socket could redirect asynchronous
    // signals into the privileged parent. Only descriptor/stream flags and
    // duplication are needed; O_ASYNC is never authorized.
    let mut commands = vec![instruction(LD, 0, 0, 24)];
    for command in [
        libc::F_DUPFD,
        libc::F_DUPFD_CLOEXEC,
        libc::F_GETFD,
        libc::F_SETFD,
        libc::F_GETFL,
    ] {
        commands.extend([
            instruction(JEQ, 0, 1, command as u32),
            instruction(RET, 0, 0, ALLOW),
        ]);
    }
    commands.extend([
        instruction(JEQ, 0, 4, libc::F_SETFL as u32),
        instruction(LD, 0, 0, 32),
        instruction(AND, 0, 0, libc::O_ASYNC as u32),
        instruction(JEQ, 0, 1, 0),
        instruction(RET, 0, 0, ALLOW),
        instruction(RET, 0, 0, EPERM),
    ]);
    filter.push(instruction(
        JEQ,
        0,
        u8::try_from(commands.len()).map_err(|_| deny("Invalid descriptor policy"))?,
        libc::SYS_fcntl as u32,
    ));
    filter.extend(commands);
    let allowed: &[libc::c_long] = &[
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_readv,
        libc::SYS_writev,
        libc::SYS_close,
        libc::SYS_close_range,
        // recv() uses recvfrom() on Linux; these operate on explicit inherited
        // sockets. Socket creation/connect/bind and descriptor transfer stay denied.
        libc::SYS_recvfrom,
        libc::SYS_shutdown,
        libc::SYS_getsockopt,
        libc::SYS_setsockopt,
        libc::SYS_getpeername,
        libc::SYS_getsockname,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_lseek,
        libc::SYS_pread64,
        libc::SYS_pwrite64,
        libc::SYS_fsync,
        libc::SYS_fdatasync,
        libc::SYS_ftruncate,
        libc::SYS_dup,
        libc::SYS_dup3,
        libc::SYS_pipe2,
        libc::SYS_mmap,
        libc::SYS_mprotect,
        libc::SYS_munmap,
        libc::SYS_mremap,
        libc::SYS_madvise,
        libc::SYS_brk,
        libc::SYS_memfd_create,
        libc::SYS_membarrier,
        libc::SYS_futex,
        libc::SYS_set_robust_list,
        libc::SYS_rseq,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_restart_syscall,
        libc::SYS_clock_gettime,
        libc::SYS_clock_getres,
        libc::SYS_nanosleep,
        libc::SYS_clock_nanosleep,
        libc::SYS_sched_yield,
        libc::SYS_sched_getaffinity,
        libc::SYS_sched_getparam,
        libc::SYS_sched_getscheduler,
        libc::SYS_getcpu,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_getppid,
        libc::SYS_getuid,
        libc::SYS_geteuid,
        libc::SYS_getgid,
        libc::SYS_getegid,
        libc::SYS_getresuid,
        libc::SYS_getresgid,
        libc::SYS_uname,
        libc::SYS_sysinfo,
        libc::SYS_getrusage,
        libc::SYS_times,
        libc::SYS_getrlimit,
        libc::SYS_setrlimit,
        libc::SYS_getcwd,
        libc::SYS_statx,
        libc::SYS_readlinkat,
        libc::SYS_gettimeofday,
        libc::SYS_getrandom,
        libc::SYS_eventfd2,
        libc::SYS_epoll_create1,
        libc::SYS_epoll_ctl,
        libc::SYS_epoll_pwait,
        libc::SYS_ppoll,
        libc::SYS_exit,
        libc::SYS_exit_group,
        // Landlock mediates these path opens; no path rule was granted.
        libc::SYS_openat,
    ];
    for syscall in allowed {
        filter.extend([
            instruction(JEQ, 0, 1, *syscall as u32),
            instruction(RET, 0, 0, ALLOW),
        ]);
    }
    #[cfg(target_arch = "x86_64")]
    for syscall in [
        // V8's memory protection keys only affect this process's mappings.
        libc::SYS_pkey_alloc,
        libc::SYS_pkey_free,
        libc::SYS_pkey_mprotect,
        libc::SYS_arch_prctl,
        libc::SYS_poll,
        libc::SYS_epoll_wait,
        libc::SYS_dup2,
    ] {
        filter.extend([
            instruction(JEQ, 0, 1, syscall as u32),
            instruction(RET, 0, 0, ALLOW),
        ]);
    }
    filter.push(instruction(RET, 0, 0, EPERM));
    let program = libc::sock_fprog {
        len: u16::try_from(filter.len()).map_err(|_| deny("Sandbox filter too large"))?,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: the kernel copies the BPF program during this call; its storage
    // remains live. This happens before thread creation, so descendants inherit it.
    if unsafe { libc::prctl(libc::PR_SET_SECCOMP, 2, &program) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
