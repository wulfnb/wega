//! Frame invalidation: when the view needs another frame.
//!
//! Wega owns this. The document engine reports whether it is animating and script reports
//! whether a frame callback is pending; every other reason to repaint is an explicit
//! [`FrameState::invalidate`] from the code that changed something visible. There are no damage
//! rectangles: a frame repaints and rasterises the whole surface.
#[derive(Debug)]
pub struct FrameState {
    dirty: bool,
}
impl FrameState {
    /// A new view has never painted.
    pub fn new() -> Self {
        Self { dirty: true }
    }
    /// Something visible may have changed: another frame is needed.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    /// A frame was just recorded. A further frame is needed only if script has a frame callback
    /// pending or the document is animating; invalidations that arrive later set the flag again.
    pub fn recorded(&mut self, another_frame: bool) {
        self.dirty = another_frame;
    }
    /// Whether a frame should be painted; a frozen (hidden) view never paints.
    pub fn needs_paint(&self, frozen: bool) -> bool {
        self.dirty && !frozen
    }
}
impl Default for FrameState {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(test)]
mod tests {
    use super::FrameState;
    #[test]
    fn a_new_view_needs_a_frame_and_a_recorded_frame_settles_it() {
        let mut frame = FrameState::new();
        assert!(frame.needs_paint(false));
        frame.recorded(false);
        assert!(!frame.needs_paint(false));
    }
    #[test]
    fn invalidation_after_recording_requests_the_next_frame() {
        let mut frame = FrameState::new();
        frame.recorded(false);
        frame.invalidate();
        assert!(frame.needs_paint(false));
    }
    #[test]
    fn animation_or_a_pending_callback_keeps_the_view_painting() {
        let mut frame = FrameState::new();
        frame.recorded(true);
        assert!(frame.needs_paint(false));
        frame.recorded(false);
        assert!(!frame.needs_paint(false));
    }
    #[test]
    fn a_frozen_view_never_paints_but_remembers_that_it_is_dirty() {
        let mut frame = FrameState::new();
        frame.recorded(false);
        frame.invalidate();
        assert!(!frame.needs_paint(true));
        assert!(
            frame.needs_paint(false),
            "thawing paints the pending change"
        );
    }
}
