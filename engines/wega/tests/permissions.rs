//! Integration tests for Step 21: Permissions, Notifications, and Device Capabilities.

use std::{sync::Arc, time::Duration};
use url::Url;
use wega_engine::{Engine, Limits, NetworkPolicy, PageEvent, View};
use wega_protocol::permissions::{PermissionFeature, PermissionState};

fn test_view(html: &str, url_str: &str, policy: Option<NetworkPolicy>) -> View {
    let temp = std::env::temp_dir().join(format!("wega-perm-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp);
    let policy = policy.unwrap_or_default();
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(5),
            ..Limits::default()
        },
        policy,
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(temp);
    let url = Url::parse(url_str).unwrap();
    let mut view = engine.create_view(url).unwrap();
    view.set_html(html).unwrap();
    view.poll();
    view
}

#[test]
fn permissions_query_and_opaque_origins() {
    let mut view = test_view(
        r#"<!doctype html><html><body><script>
            window.__status = null;
            navigator.permissions.query({ name: "geolocation" }).then(s => {
                window.__status = s.state;
                window.__name = s.name;
            });
        </script></body></html>"#,
        "https://example.com/index.html",
        None,
    );
    view.poll();

    assert_eq!(view.evaluate_json("window.__status").unwrap(), "\"prompt\"");
    assert_eq!(
        view.evaluate_json("window.__name").unwrap(),
        "\"geolocation\""
    );

    // Opaque origin check: data: URL
    let mut opaque_view = test_view(
        r#"<!doctype html><html><body><script>
            window.__opaqueStatus = null;
            navigator.permissions.query({ name: "camera" }).then(s => {
                window.__opaqueStatus = s.state;
            });
        </script></body></html>"#,
        "data:text/html,<html></html>",
        None,
    );
    opaque_view.poll();
    assert_eq!(
        opaque_view.evaluate_json("window.__opaqueStatus").unwrap(),
        "\"denied\""
    );

    // Blocked host check via NetworkPolicy
    let blocked_policy = NetworkPolicy {
        blocked_hosts: "blocked.com".into(),
        ..Default::default()
    };
    let mut blocked_view = test_view(
        r#"<!doctype html><html><body><script>
            window.__blockedStatus = null;
            navigator.permissions.query({ name: "camera" }).then(s => {
                window.__blockedStatus = s.state;
            });
        </script></body></html>"#,
        "https://blocked.com/index.html",
        Some(blocked_policy),
    );
    blocked_view.poll();
    assert_eq!(
        blocked_view
            .evaluate_json("window.__blockedStatus")
            .unwrap(),
        "\"denied\""
    );
}

#[test]
fn notification_lifecycle_request_and_prompt() {
    let mut view = test_view(
        r#"<!doctype html><html><body><script>
            window.__initialPerm = Notification.permission;
            window.__permResult = null;
            window.__threwBeforeGrant = false;
            try {
                new Notification("Blocked");
            } catch (e) {
                window.__threwBeforeGrant = true;
            }
            Notification.requestPermission().then(res => {
                window.__permResult = res;
            });
        </script></body></html>"#,
        "https://example.com/index.html",
        None,
    );
    view.poll();

    assert_eq!(
        view.evaluate_json("window.__initialPerm").unwrap(),
        "\"default\""
    );
    assert_eq!(
        view.evaluate_json("window.__threwBeforeGrant").unwrap(),
        "true"
    );

    // Verify a PermissionPrompt event was issued
    let prompt_event = view
        .events
        .iter()
        .find(|e| matches!(e, PageEvent::PermissionPrompt { feature, .. } if *feature == PermissionFeature::Notifications));
    assert!(
        prompt_event.is_some(),
        "Expected Notification permission prompt"
    );
    let prompt_id = match prompt_event.unwrap() {
        PageEvent::PermissionPrompt { id, .. } => *id,
        _ => unreachable!(),
    };

    // Answer permission
    view.answer_permission(prompt_id, true);
    view.poll();

    assert_eq!(
        view.evaluate_json("window.__permResult").unwrap(),
        "\"granted\""
    );
    assert_eq!(
        view.evaluate_json("Notification.permission").unwrap(),
        "\"granted\""
    );

    // Create a real notification after grant
    view.evaluate_json(
        r#"
        window.__notifShown = false;
        window.__notifClosed = false;
        const n = new Notification("Hello World", { body: "Test notification body" });
        n.onshow = () => { window.__notifShown = true; };
        n.onclose = () => { window.__notifClosed = true; };
        window.__testNotif = n;
        "#,
    )
    .unwrap();
    view.poll();

    let show_event = view
        .events
        .iter()
        .find(|e| matches!(e, PageEvent::NotificationShow { title, .. } if title == "Hello World"));
    assert!(show_event.is_some(), "Expected NotificationShow event");

    // Close notification
    view.evaluate_json("window.__testNotif.close()").unwrap();
    view.poll();

    let close_event = view
        .events
        .iter()
        .find(|e| matches!(e, PageEvent::NotificationClose { .. }));
    assert!(close_event.is_some(), "Expected NotificationClose event");
    assert_eq!(view.evaluate_json("window.__notifClosed").unwrap(), "true");
}

#[test]
fn mediastream_device_capture_and_acknowledged_revocation() {
    let mut view = test_view(
        r#"<!doctype html><html><body><script>
            window.__streamAcquired = false;
            window.__trackEnded = false;
            window.__cloneEnded = false;
            window.__permChanged = false;

            navigator.permissions.query({ name: "camera" }).then(status => {
                window.__cameraStatus = status;
                status.onchange = () => { window.__permChanged = true; };
            });

            function startStream() {
                return navigator.mediaDevices.getUserMedia({ video: true, audio: true }).then(stream => {
                    window.__stream = stream;
                    window.__vtrack = stream.getVideoTracks()[0];
                    window.__atrack = stream.getAudioTracks()[0];
                    window.__vclone = window.__vtrack.clone();

                    window.__vtrack.onended = () => { window.__trackEnded = true; };
                    window.__vclone.onended = () => { window.__cloneEnded = true; };
                    window.__streamAcquired = true;
                });
            }
        </script></body></html>"#,
        "https://example.com/index.html",
        None,
    );
    view.poll();

    // Without permission, getUserMedia fails
    view.evaluate_json(
        r#"
        window.__failName = null;
        startStream().catch(e => { window.__failName = e.name; });
        "#,
    )
    .unwrap();
    view.poll();
    assert_eq!(
        view.evaluate_json("window.__failName").unwrap(),
        "\"NotAllowedError\""
    );

    // Grant Camera and Microphone
    view.permissions.borrow_mut().set_grant(
        "https://example.com".into(),
        PermissionFeature::Camera,
        PermissionState::Granted,
    );
    view.permissions.borrow_mut().set_grant(
        "https://example.com".into(),
        PermissionFeature::Microphone,
        PermissionState::Granted,
    );

    // Re-query camera permission status now that it is granted
    view.evaluate_json(
        r#"
        navigator.permissions.query({ name: "camera" }).then(status => {
            window.__cameraStatus = status;
            status.onchange = () => { window.__permChanged = true; };
        });
        "#,
    )
    .unwrap();
    view.poll();
    assert_eq!(
        view.evaluate_json("window.__cameraStatus.state").unwrap(),
        "\"granted\""
    );

    // Call startStream again with permission granted
    view.evaluate_json("startStream()").unwrap();
    view.poll();

    assert_eq!(
        view.evaluate_json("window.__streamAcquired").unwrap(),
        "true"
    );
    assert_eq!(
        view.evaluate_json("window.__vtrack.readyState").unwrap(),
        "\"live\""
    );
    assert_eq!(
        view.evaluate_json("window.__vclone.readyState").unwrap(),
        "\"live\""
    );
    assert_eq!(
        view.evaluate_json("window.__atrack.readyState").unwrap(),
        "\"live\""
    );

    // Embedder performs acknowledged revocation for Camera
    view.revoke_permission("https://example.com", PermissionFeature::Camera);
    view.poll();

    // Verify: video track and cloned track are ended, event listeners triggered
    assert_eq!(
        view.evaluate_json("window.__vtrack.readyState").unwrap(),
        "\"ended\""
    );
    assert_eq!(
        view.evaluate_json("window.__vclone.readyState").unwrap(),
        "\"ended\""
    );
    assert_eq!(view.evaluate_json("window.__trackEnded").unwrap(), "true");
    assert_eq!(view.evaluate_json("window.__cloneEnded").unwrap(), "true");

    // Verify: PermissionStatus fired onchange and its state updated to prompt
    assert_eq!(view.evaluate_json("window.__permChanged").unwrap(), "true");
    assert_eq!(
        view.evaluate_json("window.__cameraStatus.state").unwrap(),
        "\"prompt\""
    );

    // Audio track remains live because Microphone was not revoked
    assert_eq!(
        view.evaluate_json("window.__atrack.readyState").unwrap(),
        "\"live\""
    );
}

#[test]
fn wake_lock_lifecycle_and_revocation() {
    let mut view = test_view(
        r#"<!doctype html><html><body><script>
            window.__lockAcquired = false;
            window.__lockReleased = false;

            navigator.wakeLock.request("screen").then(sentinel => {
                window.__sentinel = sentinel;
                window.__lockAcquired = true;
                sentinel.onrelease = () => {
                    window.__lockReleased = true;
                };
            });
        </script></body></html>"#,
        "https://example.com/index.html",
        None,
    );
    view.poll();

    assert_eq!(view.evaluate_json("window.__lockAcquired").unwrap(), "true");
    assert_eq!(
        view.evaluate_json("window.__sentinel.released").unwrap(),
        "false"
    );

    // Embedder revokes ScreenWakeLock
    view.revoke_permission("https://example.com", PermissionFeature::ScreenWakeLock);
    view.poll();

    assert_eq!(
        view.evaluate_json("window.__sentinel.released").unwrap(),
        "true"
    );
    assert_eq!(view.evaluate_json("window.__lockReleased").unwrap(), "true");
}

#[test]
fn navigation_cancels_pending_permission_prompts() {
    let mut view = test_view(
        r#"<!doctype html><html><body><script>
            window.__pendingResult = null;
            Notification.requestPermission().then(res => {
                window.__pendingResult = res;
            });
        </script></body></html>"#,
        "https://site-a.com/index.html",
        None,
    );
    view.poll();

    let prompt_event = view
        .events
        .iter()
        .find(|e| matches!(e, PageEvent::PermissionPrompt { .. }));
    assert!(prompt_event.is_some());
    let prompt_id = match prompt_event.unwrap() {
        PageEvent::PermissionPrompt { id, .. } => *id,
        _ => unreachable!(),
    };

    // Navigating to about:blank cancels site-a's pending prompt
    view.load(Url::parse("about:blank").unwrap()).unwrap();
    view.poll();

    // Answering old prompt ID has no effect on site-b
    view.answer_permission(prompt_id, true);
    view.poll();

    let state = view.permissions.borrow().query(
        &wega_protocol::authority::Origin::Tuple("https://site-a.com".into()),
        PermissionFeature::Notifications,
        &NetworkPolicy::default(),
    );
    assert_eq!(state, PermissionState::Prompt);
}

#[test]
fn fullscreen_requires_activation_and_lifecycle() {
    let mut view = test_view(
        r#"<!doctype html><html><body><div id="target">Content</div><script>
            window.__fsChanged = 0;
            document.addEventListener("fullscreenchange", () => {
                window.__fsChanged++;
            });
        </script></body></html>"#,
        "https://example.com/index.html",
        None,
    );
    view.poll();

    // Without user activation, requestFullscreen fails
    view.evaluate_json(
        r#"
        window.__fsError = null;
        document.getElementById("target").requestFullscreen().catch(e => { window.__fsError = e.name; });
        "#,
    )
    .unwrap();
    view.poll();
    assert_eq!(
        view.evaluate_json("window.__fsError").unwrap(),
        "\"TypeError\""
    );

    // With user activation, requestFullscreen succeeds
    view.activate();
    view.evaluate_json(
        r#"
        document.getElementById("target").requestFullscreen();
        "#,
    )
    .unwrap();
    view.poll();

    assert_eq!(
        view.evaluate_json("document.fullscreenElement === document.getElementById('target')")
            .unwrap(),
        "true"
    );
    let fs_event = view
        .events
        .iter()
        .find(|e| matches!(e, PageEvent::Fullscreen(true)));
    assert!(fs_event.is_some(), "Expected Fullscreen(true) event");

    // Exit fullscreen
    view.exit_fullscreen();
    view.poll();

    assert_eq!(
        view.evaluate_json("document.fullscreenElement === null")
            .unwrap(),
        "true"
    );
    let fs_exit_event = view
        .events
        .iter()
        .find(|e| matches!(e, PageEvent::Fullscreen(false)));
    assert!(fs_exit_event.is_some(), "Expected Fullscreen(false) event");
}

#[test]
fn protocol_handler_and_screen_orientation() {
    let mut view = test_view(
        r#"<!doctype html><html><body><script>
            navigator.registerProtocolHandler("web+custom", "https://example.com/handler?q=%s");
            window.__orientType = screen.orientation.type;
            window.__orientAngle = screen.orientation.angle;
            screen.orientation.lock("portrait-primary").then(() => {
                window.__lockWorked = true;
            });
            screen.orientation.unlock();
        </script></body></html>"#,
        "https://example.com/index.html",
        None,
    );
    view.poll();

    let reg_event = view
        .events
        .iter()
        .find(|e| matches!(e, PageEvent::ProtocolRegister(r) if r.scheme == "web+custom"));
    assert!(reg_event.is_some(), "Expected ProtocolRegister event");

    assert_eq!(
        view.evaluate_json("window.__orientType").unwrap(),
        "\"landscape-primary\""
    );
    assert_eq!(view.evaluate_json("window.__orientAngle").unwrap(), "0");
    assert_eq!(view.evaluate_json("window.__lockWorked").unwrap(), "true");
}
