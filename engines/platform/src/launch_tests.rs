use super::*;
#[test]
fn release_builds_and_implicit_development_launches_cannot_bypass_sandbox_setup() {
    assert_eq!(Mode::select(true, false).unwrap(), Mode::Required);
    assert_eq!(Mode::select(false, false).unwrap(), Mode::Required);
    assert_eq!(Mode::select(true, true).unwrap(), Mode::Development);
    assert_eq!(
        Mode::select(false, true).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    for transport in [BrokerTransport::Tcp, BrokerTransport::InheritedEndpoint] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        assert!(PreparedLaunch::prepare(&mut command, transport, Mode::Required).is_err());
    }
    assert!(validate_worker_start(Vec::<String>::new()).is_err());
}
#[test]
fn a_development_launch_scrubs_credentials_injection_and_profile_environment() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    for name in [
        "AWS_SECRET_ACCESS_KEY",
        "HOME",
        "PATH",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
        "NODE_OPTIONS",
        "HTTP_PROXY",
        "V8_FLAGS",
    ] {
        command.env(name, "must-not-reach-worker");
    }
    let launch =
        PreparedLaunch::prepare(&mut command, BrokerTransport::Tcp, Mode::Development).unwrap();
    assert_eq!(command.get_current_dir(), Some(launch.directory.path()));
    assert!(
        command
            .get_args()
            .any(|argument| argument == DEVELOPMENT_ARGUMENT)
    );
    let keys: Vec<_> = command
        .get_envs()
        .filter_map(|(key, value)| value.map(|_| key.to_string_lossy().into_owned()))
        .collect();
    assert!(!keys.iter().any(|name| {
        [
            "AWS_SECRET_ACCESS_KEY",
            "HOME",
            "PATH",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "NODE_OPTIONS",
            "HTTP_PROXY",
            "V8_FLAGS",
        ]
        .contains(&name.as_str())
    }));
    assert_eq!(
        command
            .get_envs()
            .find(|(key, _)| *key == "TMPDIR")
            .unwrap()
            .1,
        Some(launch.directory.path().as_os_str())
    );
    let path = launch.directory.path().to_owned();
    assert!(path.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    }
    drop(launch);
    assert!(!path.exists());
}
