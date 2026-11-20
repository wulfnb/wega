//! Paint recording: turning a laid-out document into a display list for a rasteriser.
//!
//! The document engine owns painting (backgrounds, borders, text, images, clips, transforms,
//! opacity and scroll offsets). Wega owns the *orchestration*: when a frame is recorded, with
//! which viewport, and where the recording goes. This trait is the seam between the two; it
//! names no painter, display-list or graphics type, because the recording is an associated
//! type that only the rasteriser and the implementation need to agree on.
//!
//! # Conventions
//!
//! The viewport is [`ViewportConfig`]: width and height are **device pixels**; `scale` is device
//! pixels per CSS pixel and `zoom` multiplies it, so the painter scale is `scale * zoom` and the
//! CSS viewport is `size / (scale * zoom)` (see [`GeometryAccess::viewport`]). The recording is
//! in device pixels of that size, origin top-left, already offset by the viewport scroll.
//!
//! # Lifecycle
//!
//! 1. [`PaintSource::prepare_frame`]: apply the viewport and flush style and layout.
//! 2. (the caller may run frame callbacks that change the document)
//! 3. [`LayoutControl::flush_layout`] again if anything changed.
//! 4. [`PaintSource::record_scene`]: record the current layout. It does **not** flush; it paints
//!    the layout as of the last flush, and records nothing while the document is not ready to
//!    paint (for example while render-blocking stylesheets load).
//!
//! The recording is owned and moves to the rasteriser without copying; no state is shared with
//! the document afterwards.
use crate::layout::{LayoutControl, ViewportConfig};

/// A rectangle in device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A document that can record its painting.
pub trait PaintSource: LayoutControl {
    /// The owned recording handed to the rasteriser.
    type Scene: Send + 'static;
    /// Applies the viewport and flushes style and layout for it. `animation_time` is seconds
    /// since the document started.
    fn prepare_frame(&mut self, viewport: &ViewportConfig, animation_time: f64);
    /// Records the current layout for `viewport`. Never flushes.
    fn record_scene(&mut self, viewport: &ViewportConfig) -> Self::Scene;
    /// Records the current layout for `viewport`, integrating child scenes at their corresponding
    /// element stacking positions in the display list.
    fn record_scene_with_children(
        &mut self,
        viewport: &ViewportConfig,
        children: Vec<(crate::NodeKey, Self::Scene)>,
    ) -> Self::Scene {
        let mut scene = self.record_scene(viewport);
        for (_, child) in children {
            Self::compose_child(
                &mut scene,
                child,
                DeviceRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            );
        }
        scene
    }
    /// Composites a nested document's recording into `scene`: `child` was recorded at the
    /// size of `rect` (device pixels, origin top-left) and is translated to `rect`'s origin and
    /// clipped to `rect`. Nothing is flattened into the parent document; the child keeps its own
    /// layout. Frames are painted after the parent's own content (see the iframe design, L-1).
    fn compose_child(scene: &mut Self::Scene, child: Self::Scene, rect: DeviceRect);
    /// Whether the document needs another frame without any further input (running CSS
    /// animations or transitions, scroll animation, scrollbar fades, canvas content).
    fn is_animating(&self) -> bool;
}
