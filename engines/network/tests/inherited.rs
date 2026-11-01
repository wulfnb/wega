#![cfg(unix)]
use std::sync::Arc;
use url::Url;
use wega_network::{Broker, Policy, Remote};

#[test]
fn inherited_sessions_reuse_bounded_channels_and_cannot_fall_back_to_tcp() {
    let broker = Broker::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let session = broker
        .open_bound_session(directory.path().into(), Policy::default())
        .unwrap();
    let channels = session.inherited_channels(4).unwrap();
    assert!(session.inherited_channels(1).is_err());
    let (address, token) = session.endpoint();
    let remote = Arc::new(Remote::inherited(channels, token.clone(), session.identity()).unwrap());
    let url = Url::parse("https://channel.example/").unwrap();
    remote
        .cookie_set(&url, "channel=owned; Path=/", true)
        .unwrap();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let remote = remote.clone();
        let url = url.clone();
        tasks.push(std::thread::spawn(move || {
            for _ in 0..10 {
                assert_eq!(remote.cookie_get(&url, true).unwrap(), "channel=owned");
            }
        }));
    }
    for task in tasks {
        task.join().unwrap();
    }
    assert!(
        Remote::bound(&address, token, session.identity())
            .unwrap()
            .cookie_get(&url, true)
            .unwrap_err()
            .contains("endpoint/session mismatch")
    );
    session.revoke();
    assert!(remote.cookie_get(&url, true).is_err());
    assert!(
        remote.cookie_get(&url, true).is_err(),
        "Closed inherited channels must never reconnect via TCP"
    );
}

#[test]
fn a_stolen_token_cannot_change_the_session_owning_an_inherited_endpoint() {
    let broker = Broker::new().unwrap();
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first = broker
        .open_bound_session(first_dir.path().into(), Policy::default())
        .unwrap();
    let second = broker
        .open_bound_session(second_dir.path().into(), Policy::default())
        .unwrap();
    let first_channels = first.inherited_channels(1).unwrap();
    let (_, stolen) = second.endpoint();
    let forged = Remote::inherited(first_channels, stolen, second.identity()).unwrap();
    let url = Url::parse("https://channel.example/").unwrap();
    assert!(
        forged
            .cookie_set(&url, "foreign=denied", true)
            .unwrap_err()
            .contains("endpoint/session mismatch")
    );
    let second_channels = second.inherited_channels(1).unwrap();
    let (_, token) = second.endpoint();
    assert_eq!(
        Remote::inherited(second_channels, token, second.identity())
            .unwrap()
            .cookie_get(&url, true)
            .unwrap(),
        ""
    );
}
