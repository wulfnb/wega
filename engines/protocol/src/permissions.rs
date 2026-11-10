//! Typed permissions, state descriptors and native device contracts.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PermissionFeature {
    Geolocation,
    Notifications,
    Camera,
    Microphone,
    Midi,
    ScreenWakeLock,
    ClipboardRead,
    ClipboardWrite,
}

impl PermissionFeature {
    pub fn parse(s: &str) -> Option<Self> {
        let norm = s.to_ascii_lowercase().replace('_', "-");
        match norm.as_str() {
            "geolocation" => Some(Self::Geolocation),
            "notifications" | "notification" => Some(Self::Notifications),
            "camera" | "video_capture" | "video-capture" => Some(Self::Camera),
            "microphone" | "audio_capture" | "audio-capture" => Some(Self::Microphone),
            "midi" => Some(Self::Midi),
            "screen-wake-lock" | "screenwakelock" | "wake-lock" | "wakelock" | "screen" => {
                Some(Self::ScreenWakeLock)
            }
            "clipboard-read" | "clipboardread" => Some(Self::ClipboardRead),
            "clipboard-write" | "clipboardwrite" => Some(Self::ClipboardWrite),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Geolocation => "geolocation",
            Self::Notifications => "notifications",
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::Midi => "midi",
            Self::ScreenWakeLock => "screen-wake-lock",
            Self::ClipboardRead => "clipboard-read",
            Self::ClipboardWrite => "clipboard-write",
        }
    }
}

impl std::fmt::Display for PermissionFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionState {
    Granted,
    Denied,
    Prompt,
}

impl PermissionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Denied => "denied",
            Self::Prompt => "prompt",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPromptRequest {
    pub id: u64,
    pub session: crate::identity::SessionIdentity,
    pub document: crate::DocumentContext,
    pub origin: crate::authority::Origin,
    pub feature: PermissionFeature,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationOptions {
    pub body: Option<String>,
    pub icon: Option<String>,
    pub tag: Option<String>,
    pub lang: Option<String>,
    pub dir: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolHandlerRegistration {
    pub scheme: String,
    pub url: String,
}
