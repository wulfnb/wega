//! Adversarial child fixture. Run through PreparedLaunch with --content-sandbox;
//! a setup failure is distinct from a denied operation and never counts as a pass.
use std::{
    io::Read,
    net::{TcpStream, UdpSocket},
    path::PathBuf,
    process::Command,
    time::Duration,
};
fn main() {
    if let Err(error) = run() {
        eprintln!("SETUP_FAILURE: {error}");
        std::process::exit(2);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    #[cfg(target_os = "linux")]
    {
        let mut signal = 0_i32;
        // SAFETY: query this initial thread before native policy entry. A test
        // harness thread would not inherit the initial thread's PDEATHSIG.
        if unsafe { libc::prctl(libc::PR_GET_PDEATHSIG, &mut signal, 0, 0, 0) } != 0
            || signal != libc::SIGKILL
        {
            return Err("Missing parent-death policy on initial thread".into());
        }
    }
    wega_platform::launch::validate_worker_start(args.clone())?;
    let argument = |name: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
            .ok_or_else(|| format!("Missing {name}"))
    };
    let secret = PathBuf::from(argument("--secret")?);
    let output = PathBuf::from(argument("--write")?);
    let address = argument("--address")?.parse()?;
    let grant = argument("--grant")?.parse::<u64>()?;
    let mut file = wega_platform::inherited::duplicate_file(grant)?;
    wega_platform::sandbox::finish_grants()?;
    let mut allowed = String::new();
    file.read_to_string(&mut allowed)?;
    let read_denied = std::fs::read(&secret).is_err();
    let write_denied = std::fs::write(&output, b"escape").is_err();
    let tcp_denied = TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_err();
    let udp_denied = UdpSocket::bind("127.0.0.1:0").is_err();
    let child = Command::new(executable)
        .arg("--should-never-execute")
        .spawn();
    let child_denied = match child {
        Err(_) => true,
        Ok(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            false
        }
    };
    let granted = allowed == "explicit-grant";
    println!(
        "{{\"granted_file\":{granted},\"private_read_denied\":{read_denied},\"private_write_denied\":{write_denied},\"tcp_denied\":{tcp_denied},\"udp_denied\":{udp_denied},\"child_denied\":{child_denied}}}"
    );
    if !(granted && read_denied && write_denied && tcp_denied && udp_denied && child_denied) {
        std::process::exit(1);
    }
    Ok(())
}
