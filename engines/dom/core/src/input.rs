//! Focus, hover and pointer tracking: the state input routing needs from a document.
//!
//! This is deliberately **not** an event system. Event dispatch (capture, target, bubble,
//! listeners, propagation control) is the script runtime's job; default actions are engine
//! policy. What a document engine must own, because text editing, selection and style
//! (`:focus`, `:hover`) depend on it, is *which node is focused or hovered and what the
//! pointer is over*. These traits expose exactly that, in terms of [`NodeKey`]: keys are
//! validated (a node that no longer exists is never reported), nothing is copied, and no
//! engine event type appears.
//!
//! Focus has one authoritative owner, the document. Callers observe it through
//! [`FocusState::focused`] before and after an operation and derive the focus transition
//! from the difference; they must not keep a second copy.
use crate::{DocumentView, MutationError, NodeKey, NodeKind, NodeView};

/// The nodes under the pointer before and after a pointer update.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HoverChange {
    pub previous: Option<NodeKey>,
    pub current: Option<NodeKey>,
}
impl HoverChange {
    pub fn changed(&self) -> bool {
        self.previous != self.current
    }
}

/// Read access to focus and hover state.
pub trait FocusState: DocumentView {
    /// The explicitly focused node. `None` when nothing is focused (the document's
    /// fallback keyboard target is not "focus").
    fn focused(&self) -> Option<NodeKey>;
    /// Where keyboard input goes: the focused node, else the root element.
    fn keyboard_target(&self) -> Option<NodeKey>;
    /// The innermost node under the pointer, if any.
    fn hovered(&self) -> Option<NodeKey>;
}

/// Write access to focus and hover state.
pub trait InputControl: FocusState {
    /// Moves focus to `key` (or its nearest non-anonymous ancestor). `Ok(true)` when focus
    /// changed. Applies style invalidation for `:focus`; dispatches nothing.
    fn set_focus(&mut self, key: NodeKey) -> Result<bool, MutationError>;
    /// Removes focus. Dispatches nothing.
    fn clear_focus(&mut self);
    /// Hit-tests at page coordinates and updates hover state.
    fn pointer_moved(&mut self, x: f32, y: f32) -> HoverChange;
    /// The pointer left the document: clears hover.
    fn pointer_left(&mut self) -> HoverChange;
}

/// `key` followed by its element ancestors, innermost first (the hover/enter/leave chain).
pub fn element_chain<D: DocumentView>(doc: &D, key: NodeKey) -> Vec<NodeKey> {
    let mut chain = Vec::with_capacity(16);
    let Some(node) = doc.node(key) else {
        return chain;
    };
    chain.push(key);
    let mut parent = node.parent();
    while let Some(key) = parent {
        let Some(node) = doc.node(key) else { break };
        if matches!(node.kind(), NodeKind::Element | NodeKind::AnonymousBlock) {
            chain.push(key);
        }
        parent = node.parent();
    }
    chain
}
