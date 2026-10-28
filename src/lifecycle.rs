//! Engine-independent tab lifecycle rules: which background tabs may be unloaded
//! ("discarded") to release their engine, script and GPU resources.
use std::time::{Duration, Instant};

pub const DEFAULT_DISCARD_MINUTES: u32 = 30;
pub const MAX_DISCARD_MINUTES: u32 = 24 * 60;
/// Ineligible tabs (playing media, open dialogs, …) are re-examined at most this often.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Loaded website/local documents need an edit-state check before a manual
/// destructive action. Automatic idle discarding remains disabled for these
/// documents: a probe cannot acknowledge saves or inspect every embedded editor.
pub fn requires_state_check(loaded: bool, address: &str) -> bool {
    loaded && !url::Url::parse(address).is_ok_and(|url| url.scheme() == "wega")
}

/// Idle timeout for a policy value; `0` disables discarding.
pub fn timeout(minutes: u32) -> Option<Duration> {
    (minutes > 0).then(|| Duration::from_secs(u64::from(minutes.min(MAX_DISCARD_MINUTES)) * 60))
}

/// What the desktop layer knows about one tab when deciding whether to unload it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Activity {
    /// The selected tab is never unloaded.
    pub active: bool,
    /// Already unloaded tabs have nothing left to release.
    pub loaded: bool,
    /// Media playback, pending dialogs, attached developer tools, loads in progress,
    /// automation, script-opened popups and other state that unloading would lose.
    pub busy: bool,
    /// Time since the tab was last selected.
    pub idle: Duration,
}

/// Conservative: only loaded, unselected and quiescent tabs past the timeout qualify.
pub fn may_discard(tab: Activity, timeout: Option<Duration>) -> bool {
    timeout.is_some_and(|timeout| tab.loaded && !tab.active && !tab.busy && tab.idle >= timeout)
}

/// When the next discard check is due. `last_used` lists when each loaded background
/// tab was last selected. `None` means no timer is needed (nothing could be unloaded).
pub fn next_check(
    last_used: impl IntoIterator<Item = Instant>,
    timeout: Option<Duration>,
    last_check: Instant,
) -> Option<Instant> {
    let due = last_used.into_iter().min()?.checked_add(timeout?)?;
    Some(due.max(last_check + CHECK_INTERVAL))
}

/// On session restore only the selected tab gets a live engine view.
pub fn restore_loaded(index: usize, active: usize) -> bool {
    index == active
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
