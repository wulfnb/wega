//! Resources and control state supplied by the single document owner.
//! Fetch authorization and document lifecycle remain embedding-browser policy.
use crate::{css::StyleSheet, layout::LayoutBox};
pub use parley::FontContext;
use wega_dom::{DocumentView, MutationClock, input::FocusState};
pub trait NativeResources: DocumentView + MutationClock + FocusState {
    fn native_generations(&self) -> (u64, u64);
    fn native_fonts(&self) -> std::sync::Arc<std::sync::Mutex<FontContext>>;
    fn native_stylesheets(
        &self,
        allow_inline: &dyn Fn(Option<&str>, &str) -> bool,
    ) -> Vec<StyleSheet>;
    fn native_images(&mut self, root: &mut LayoutBox);
    fn prepare_controls(&mut self);
    fn sync_controls(&self, root: &mut LayoutBox);
    fn decorate_controls(&self, root: &mut LayoutBox);
}
