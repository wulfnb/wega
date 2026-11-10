//! Monotonic timeline tracker, readyState, and A/V playback clock.
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkState {
    Empty = 0,
    Idle = 1,
    Loading = 2,
    NoSource = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadyState {
    HaveNothing = 0,
    HaveMetadata = 1,
    HaveCurrentData = 2,
    HaveFutureData = 3,
    HaveEnoughData = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaErrorCode {
    Aborted = 1,
    Network = 2,
    Decode = 3,
    SrcNotSupported = 4,
}

#[derive(Clone, Debug)]
pub struct MediaError {
    pub code: MediaErrorCode,
    pub message: String,
}

#[derive(Debug)]
pub struct MediaClock {
    pub paused: bool,
    pub playback_rate: f64,
    pub current_time: f64,
    pub duration: f64,
    pub looping: bool,
    pub volume: f64,
    pub muted: bool,
    pub autoplay: bool,
    pub ready_state: ReadyState,
    pub network_state: NetworkState,
    pub error: Option<MediaError>,
    pub seeking: bool,
    pub ended: bool,
    last_tick: Option<Instant>,
}

impl Default for MediaClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaClock {
    pub fn new() -> Self {
        Self {
            paused: true,
            playback_rate: 1.0,
            current_time: 0.0,
            duration: 0.0,
            looping: false,
            volume: 1.0,
            muted: false,
            autoplay: false,
            ready_state: ReadyState::HaveNothing,
            network_state: NetworkState::Empty,
            error: None,
            seeking: false,
            ended: false,
            last_tick: None,
        }
    }

    pub fn play(&mut self) -> Result<(), &'static str> {
        if self.error.is_some() {
            return Err("Media error present");
        }
        if self.ended {
            self.current_time = 0.0;
            self.ended = false;
        }
        self.paused = false;
        self.last_tick = Some(Instant::now());
        if self.ready_state == ReadyState::HaveNothing {
            self.ready_state = ReadyState::HaveMetadata;
        }
        Ok(())
    }

    pub fn pause(&mut self) {
        self.update_clock();
        self.paused = true;
        self.last_tick = None;
    }

    pub fn seek(&mut self, time: f64) {
        self.seeking = true;
        let clamped = if self.duration > 0.0 {
            time.max(0.0).min(self.duration)
        } else {
            time.max(0.0)
        };
        self.current_time = clamped;
        self.ended = false;
        if !self.paused {
            self.last_tick = Some(Instant::now());
        }
        self.seeking = false;
    }

    pub fn set_playback_rate(&mut self, rate: f64) {
        self.playback_rate = rate;
    }

    pub fn set_volume(&mut self, vol: f64) -> Result<(), &'static str> {
        if !(0.0..=1.0).contains(&vol) {
            return Err("Volume out of bounds [0.0, 1.0]");
        }
        self.volume = vol;
        Ok(())
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn set_loop(&mut self, looping: bool) {
        self.looping = looping;
    }

    pub fn step_explicit(&mut self, delta_seconds: f64) {
        if self.paused || self.ended {
            return;
        }
        let advance = delta_seconds * self.playback_rate;
        let next_time = self.current_time + advance;

        if self.duration > 0.0 && next_time >= self.duration {
            if self.looping {
                self.current_time = (next_time % self.duration).max(0.0);
            } else {
                self.current_time = self.duration;
                self.ended = true;
                self.paused = true;
                self.last_tick = None;
            }
        } else {
            self.current_time = next_time.max(0.0);
        }
    }

    pub fn update_clock(&mut self) {
        if self.paused || self.ended {
            return;
        }
        if let Some(last) = self.last_tick {
            let now = Instant::now();
            let elapsed = now.duration_since(last).as_secs_f64();
            self.last_tick = Some(now);
            self.step_explicit(elapsed);
        }
    }

    pub fn current_position(&mut self) -> f64 {
        self.update_clock();
        self.current_time
    }
}
