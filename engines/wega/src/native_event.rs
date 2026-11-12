//! Rust -> script routing of native input events.
//!
//! A [`NativeEvent`] names a node target by [`NodeKey`] and carries the event's init values;
//! [`NativeEvent::script`] is the single place that turns it into the call the script runtime
//! understands (`__wega_native_event(type, nodeId, init)`). Dispatch itself (capture, target,
//! bubble, listeners, propagation control) is the script runtime's job and is not
//! reimplemented here.
use serde_json::{Value, json};
use std::borrow::Cow;
use wega_dom::NodeKey;

pub(crate) struct NativeEvent {
    kind: Cow<'static, str>,
    target: NodeKey,
    init: Value,
}

impl NativeEvent {
    pub fn new(kind: impl Into<Cow<'static, str>>, target: NodeKey) -> Self {
        Self {
            kind: kind.into(),
            target,
            init: json!({}),
        }
    }
    /// The event's init dictionary (`MouseEventInit`, `InputEventInit`, ...).
    pub fn init(mut self, init: Value) -> Self {
        self.init = init;
        self
    }
    /// `relatedTarget` (focus, mouseover/out, enter/leave).
    pub fn related(mut self, related: Option<NodeKey>) -> Self {
        if let (Some(related), Value::Object(map)) = (related, &mut self.init) {
            map.insert("relatedTarget".into(), json!(related.to_string()));
        }
        self
    }
    /// The script that dispatches this event; evaluates to `true` when it was cancelled.
    pub fn script(&self) -> String {
        format!(
            "__wega_native_event({}, {}, {})",
            json!(self.kind),
            json!(self.target.to_string()),
            self.init
        )
    }
}

/// Several events dispatched in order by one script (e.g. `input` then `change`).
pub(crate) fn script_all(events: &[NativeEvent]) -> String {
    events
        .iter()
        .map(|event| event.script() + ";")
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_names_the_target_by_its_decimal_key_and_carries_the_init() {
        let key = NodeKey::from_u64(4294967298);
        let event = NativeEvent::new("click", key)
            .init(json!({"button": 0}))
            .related(Some(NodeKey::from_u64(7)));
        assert_eq!(
            event.script(),
            r#"__wega_native_event("click", "4294967298", {"button":0,"relatedTarget":"7"})"#
        );
    }

    #[test]
    fn related_is_omitted_when_absent_and_events_run_in_order() {
        let a = NativeEvent::new("input", NodeKey::from_u64(1)).related(None);
        let b = NativeEvent::new("change", NodeKey::from_u64(1));
        assert_eq!(
            script_all(&[a, b]),
            r#"__wega_native_event("input", "1", {}); __wega_native_event("change", "1", {});"#
        );
    }
}
