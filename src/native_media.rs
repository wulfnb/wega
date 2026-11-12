//! macOS Now Playing and remote commands. Worker events are marshalled to winit.
#[derive(Debug, Default)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub rate: f64,
}
impl MediaInfo {
    pub fn update(&mut self, event: wega::engine_protocol::media::MediaSessionEvent) {
        match event {
            wega::engine_protocol::media::MediaSessionEvent::SetMetadata(m) => {
                self.title = m.title;
                self.artist = m.artist;
                self.album = m.album;
            }
            wega::engine_protocol::media::MediaSessionEvent::PlaybackStateChange(state) => {
                self.playing = matches!(
                    state,
                    wega::engine_protocol::media::MediaSessionPlaybackState::Playing
                );
            }
            wega::engine_protocol::media::MediaSessionEvent::SetPositionState(p) => {
                self.position = p.position;
                self.duration = p.duration;
                self.rate = p.playback_rate;
            }
        }
    }
}
#[cfg(target_os = "macos")]
mod platform {
    use super::MediaInfo;
    use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::{NSDictionary, NSNumber, NSString};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    #[link(name = "MediaPlayer", kind = "framework")]
    unsafe extern "C" {
        static MPMediaItemPropertyTitle: *const NSString;
        static MPMediaItemPropertyArtist: *const NSString;
        static MPMediaItemPropertyAlbumTitle: *const NSString;
        static MPMediaItemPropertyPlaybackDuration: *const NSString;
        static MPNowPlayingInfoPropertyElapsedPlaybackTime: *const NSString;
        static MPNowPlayingInfoPropertyPlaybackRate: *const NSString;
    }
    pub struct NativeMedia {
        center: Retained<AnyObject>,
        targets: Vec<(Retained<AnyObject>, Retained<AnyObject>)>,
        active: Arc<AtomicBool>,
    }
    impl NativeMedia {
        pub fn new(proxy: winit::event_loop::EventLoopProxy<crate::engine::Wake>) -> Self {
            use crate::engine::Wake;
            use wega::engine_protocol::media::MediaSessionAction as Action;
            unsafe {
                let center: Retained<AnyObject> =
                    msg_send![class!(MPNowPlayingInfoCenter), defaultCenter];
                let commands: Retained<AnyObject> =
                    msg_send![class!(MPRemoteCommandCenter), sharedCommandCenter];
                let mut targets = Vec::new();
                let active = Arc::new(AtomicBool::new(false));
                let entries: Vec<(Retained<AnyObject>, Wake)> = vec![
                    (
                        msg_send![&*commands, playCommand],
                        Wake::Media(Action::Play),
                    ),
                    (
                        msg_send![&*commands, pauseCommand],
                        Wake::Media(Action::Pause),
                    ),
                    (
                        msg_send![&*commands, stopCommand],
                        Wake::Media(Action::Stop),
                    ),
                    (
                        msg_send![&*commands, nextTrackCommand],
                        Wake::Media(Action::NextTrack),
                    ),
                    (
                        msg_send![&*commands, previousTrackCommand],
                        Wake::Media(Action::PreviousTrack),
                    ),
                    (
                        msg_send![&*commands, togglePlayPauseCommand],
                        Wake::ToggleMedia,
                    ),
                ];
                for (command, event) in entries {
                    let proxy = proxy.clone();
                    let active = active.clone();
                    let block = block2::RcBlock::new(move |_: *mut AnyObject| -> isize {
                        if !active.load(Ordering::Relaxed) {
                            return 110;
                        }
                        if proxy.send_event(event.clone()).is_ok() {
                            0
                        } else {
                            200
                        }
                    });
                    let token: Retained<AnyObject> =
                        msg_send![&*command,addTargetWithHandler:&*block];
                    targets.push((command, token));
                }
                Self {
                    center,
                    targets,
                    active,
                }
            }
        }
        pub fn update(&self, info: &MediaInfo) {
            unsafe {
                let title = NSString::from_str(&info.title);
                let artist = NSString::from_str(&info.artist);
                let album = NSString::from_str(&info.album);
                let duration = NSNumber::numberWithDouble(info.duration.max(0.0));
                let position = NSNumber::numberWithDouble(info.position.max(0.0));
                let rate = NSNumber::numberWithDouble(if info.playing {
                    if info.rate > 0.0 { info.rate } else { 1.0 }
                } else {
                    0.0
                });
                let keys = [
                    &*MPMediaItemPropertyTitle,
                    &*MPMediaItemPropertyArtist,
                    &*MPMediaItemPropertyAlbumTitle,
                    &*MPMediaItemPropertyPlaybackDuration,
                    &*MPNowPlayingInfoPropertyElapsedPlaybackTime,
                    &*MPNowPlayingInfoPropertyPlaybackRate,
                ];
                let objects: [&AnyObject; 6] =
                    [&title, &artist, &album, &duration, &position, &rate];
                let values = NSDictionary::from_slices(&keys, &objects);
                let _: () = msg_send![&*self.center,setNowPlayingInfo:&*values];
                let _: () =
                    msg_send![&*self.center,setPlaybackState:if info.playing{1usize}else{2usize}];
                self.active.store(true, Ordering::Relaxed);
            }
        }
        pub fn clear(&self) {
            unsafe {
                self.active.store(false, Ordering::Relaxed);
                let _: () =
                    msg_send![&*self.center,setNowPlayingInfo:std::ptr::null::<AnyObject>()];
                let _: () = msg_send![&*self.center,setPlaybackState:0usize];
            }
        }
    }
    impl Drop for NativeMedia {
        fn drop(&mut self) {
            self.clear();
            unsafe {
                for (command, token) in &self.targets {
                    let _: () = msg_send![&**command,removeTarget:&**token];
                }
            }
        }
    }
}
#[cfg(not(target_os = "macos"))]
mod platform {
    pub struct NativeMedia;
    impl NativeMedia {
        pub fn new(_: winit::event_loop::EventLoopProxy<crate::engine::Wake>) -> Self {
            Self
        }
        pub fn update(&self, _: &super::MediaInfo) {}
        pub fn clear(&self) {}
    }
}
pub use platform::NativeMedia;
