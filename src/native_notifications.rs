//! macOS UserNotifications delivery. Site permission is handled by the worker protocol.
use std::sync::OnceLock;
pub struct Notification {
    pub title: String,
    pub body: String,
    pub tag: String,
}
static RESULTS: OnceLock<(
    crossbeam_channel::Sender<String>,
    crossbeam_channel::Receiver<String>,
)> = OnceLock::new();
fn results() -> &'static (
    crossbeam_channel::Sender<String>,
    crossbeam_channel::Receiver<String>,
) {
    RESULTS.get_or_init(|| crossbeam_channel::bounded(32))
}
pub fn updates() -> Vec<String> {
    results().1.try_iter().collect()
}
#[cfg(target_os = "macos")]
#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}
#[cfg(target_os = "macos")]
pub fn show(notification: Notification, origin: &str) -> Result<(), String> {
    use objc2::{
        class, msg_send,
        rc::Retained,
        runtime::{AnyObject, Bool},
    };
    use objc2_foundation::NSString;
    // UserNotifications requires a signed/bundled application identity. Do not invoke
    // currentNotificationCenter from an unbundled test executable: AppKit raises an exception.
    unsafe {
        let bundle: Retained<AnyObject> = msg_send![class!(NSBundle), mainBundle];
        let identity: Option<Retained<NSString>> = msg_send![&*bundle, bundleIdentifier];
        if identity.is_none() {
            return Err("macOS notifications require launching Wega.app".into());
        }
        let center: Retained<AnyObject> =
            msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
        let content: Retained<AnyObject> = msg_send![class!(UNMutableNotificationContent), new];
        let title = NSString::from_str(&notification.title.chars().take(1000).collect::<String>());
        let body = NSString::from_str(&notification.body.chars().take(16000).collect::<String>());
        let _: () = msg_send![&*content,setTitle:&*title];
        let _: () = msg_send![&*content,setBody:&*body];
        let subtitle = NSString::from_str(origin);
        let _: () = msg_send![&*content,setSubtitle:&*subtitle];
        let id = NSString::from_str(&if notification.tag.is_empty() {
            format!("wega-{:?}", std::time::SystemTime::now())
        } else {
            format!(
                "{}:{}",
                origin,
                notification.tag.chars().take(512).collect::<String>()
            )
        });
        let request: Retained<AnyObject> = msg_send![class!(UNNotificationRequest),requestWithIdentifier:&*id,content:&*content,trigger:std::ptr::null::<AnyObject>()];
        let sender = results().0.clone();
        let completion = block2::RcBlock::new(move |granted: Bool, error: *mut AnyObject| {
            if !error.is_null() || !granted.as_bool() {
                let _ = sender
                    .try_send("macOS notification permission was denied or unavailable.".into());
                return;
            }
            let sender = sender.clone();
            let delivered = block2::RcBlock::new(move |error: *mut AnyObject| {
                let _ = sender.try_send(if error.is_null() {
                    "Notification delivered to macOS.".into()
                } else {
                    "macOS rejected notification delivery.".into()
                });
            });
            let _: () = msg_send![&*center,addNotificationRequest:&*request,withCompletionHandler:&*delivered];
        });
        let center: Retained<AnyObject> =
            msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
        let _: () = msg_send![&*center,requestAuthorizationWithOptions:4usize,completionHandler:&*completion];
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn show(notification: Notification, origin: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let mut cmd = std::process::Command::new("notify-send");
        cmd.arg(&notification.title)
            .arg(format!("{}\n{}", origin, notification.body));
        if cmd.spawn().is_ok() {
            let _ = results()
                .0
                .try_send("Notification delivered to Linux desktop.".into());
            return Ok(());
        }
    }
    Err(
        "System notification delivery is currently available on macOS and Linux (via notify-send)."
            .into(),
    )
}
