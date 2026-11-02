//! Document-owned custom-element and shadow-root state. Bindings retain callable
//! JS values; validation, upgrade states and reaction admission belong to Rust.
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Definition {
    pub id: u64,
    pub name: String,
    pub observed: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementState {
    Constructing(u64),
    Custom(u64),
    Failed,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Reaction {
    pub definition: u64,
    pub node: String,
    pub callback: &'static str,
    pub args: Vec<serde_json::Value>,
}
#[derive(Default)]
pub(crate) struct Registry {
    next: u64,
    definitions: BTreeMap<String, Definition>,
    constructors: BTreeSet<u64>,
    elements: BTreeMap<u64, ElementState>,
    reactions: VecDeque<Reaction>,
}
fn name_start(c: char) -> bool {
    c.is_ascii_lowercase()
}
fn name_char(c: char) -> bool {
    c.is_ascii_lowercase()
        || c.is_ascii_digit()
        || matches!(c, '-' | '.' | '_' | '\u{b7}'
        | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{37d}' | '\u{37f}'..='\u{1fff}'
        | '\u{200c}'..='\u{200d}' | '\u{203f}'..='\u{2040}' | '\u{2070}'..='\u{218f}'
        | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}'
        | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
}
pub(crate) fn valid_name(name: &str) -> bool {
    name.len() <= 1024
        && name.chars().next().is_some_and(name_start)
        && name.contains('-')
        && name.chars().all(name_char)
        && ![
            "annotation-xml",
            "color-profile",
            "font-face",
            "font-face-src",
            "font-face-uri",
            "font-face-format",
            "font-face-name",
            "missing-glyph",
        ]
        .contains(&name)
}
impl Registry {
    pub fn preflight(
        &self,
        name: &str,
        constructor: u64,
        extends: Option<&str>,
    ) -> Result<(), String> {
        if !valid_name(name) {
            return Err("SyntaxError: invalid custom element name".into());
        }
        if constructor == 0 {
            return Err("TypeError: invalid custom element constructor".into());
        }
        if self.definitions.contains_key(name) || self.constructors.contains(&constructor) {
            return Err(
                "NotSupportedError: custom element name or constructor already registered".into(),
            );
        }
        if extends.is_some() {
            return Err("NotSupportedError: customized built-in elements are not enabled".into());
        }
        Ok(())
    }
    pub fn define(
        &mut self,
        name: &str,
        constructor: u64,
        observed: Vec<String>,
        extends: Option<&str>,
    ) -> Result<Definition, String> {
        self.preflight(name, constructor, extends)?;
        if self.definitions.len() >= 1024
            || observed.len() > 256
            || observed.iter().any(|name| name.len() > 1024)
        {
            return Err("Custom element definition budget exceeded".into());
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or("Custom element IDs exhausted")?;
        let definition = Definition {
            id: self.next,
            name: name.into(),
            observed,
        };
        self.constructors.insert(constructor);
        self.definitions.insert(name.into(), definition.clone());
        Ok(definition)
    }
    pub fn get(&self, name: &str) -> Option<&Definition> {
        self.definitions.get(name)
    }
    pub fn by_id(&self, id: u64) -> Option<&Definition> {
        self.definitions
            .values()
            .find(|definition| definition.id == id)
    }
    pub fn begin(&mut self, node: u64, id: u64) -> Result<bool, String> {
        if self.by_id(id).is_none() {
            return Err("NotSupportedError: definition does not exist".into());
        }
        if self.elements.contains_key(&node) {
            return Ok(false);
        }
        self.elements.insert(node, ElementState::Constructing(id));
        Ok(true)
    }
    pub fn complete(&mut self, node: u64, success: bool) -> Result<(), String> {
        let Some(ElementState::Constructing(id)) = self.elements.get(&node).copied() else {
            return Err("Invalid custom element construction state".into());
        };
        self.elements.insert(
            node,
            if success {
                ElementState::Custom(id)
            } else {
                ElementState::Failed
            },
        );
        if !success {
            self.reactions
                .retain(|reaction| reaction.node != node.to_string());
        }
        Ok(())
    }
    pub fn custom(&self, node: u64) -> bool {
        matches!(self.elements.get(&node), Some(ElementState::Custom(_)))
    }
    pub fn needs_upgrade(&self, node: u64, name: &str) -> Option<u64> {
        (!self.elements.contains_key(&node))
            .then(|| self.get(name).map(|definition| definition.id))
            .flatten()
    }
    pub fn attribute(
        &mut self,
        node: u64,
        name: &str,
        old: Option<&str>,
        value: Option<&str>,
        namespace: Option<&str>,
    ) {
        let Some(ElementState::Custom(id) | ElementState::Constructing(id)) =
            self.elements.get(&node).copied()
        else {
            return;
        };
        if self.by_id(id).is_some_and(|definition| {
            definition
                .observed
                .iter()
                .any(|attribute| attribute == name)
        }) {
            self.enqueue(
                node,
                "attributeChangedCallback",
                vec![name.into(), old.into(), value.into(), namespace.into()],
            );
        }
    }
    pub fn lifecycle(&mut self, node: u64, connected: bool) {
        if matches!(
            self.elements.get(&node),
            Some(ElementState::Custom(_) | ElementState::Constructing(_))
        ) {
            self.enqueue(
                node,
                if connected {
                    "connectedCallback"
                } else {
                    "disconnectedCallback"
                },
                vec![],
            );
        }
    }
    pub fn adopted(&mut self, node: u64, old: u64, new: u64) {
        if self.custom(node) && old != new {
            self.enqueue(
                node,
                "adoptedCallback",
                vec![old.to_string().into(), new.to_string().into()],
            );
        }
    }
    fn enqueue(&mut self, node: u64, callback: &'static str, args: Vec<serde_json::Value>) {
        // DOM limits bound live elements; a task cannot retain unbounded reaction
        // history. Overflow fails explicitly at the binding checkpoint.
        if self.reactions.len() <= 65536 {
            let Some(ElementState::Custom(definition) | ElementState::Constructing(definition)) =
                self.elements.get(&node).copied()
            else {
                return;
            };
            self.reactions.push_back(Reaction {
                definition,
                node: node.to_string(),
                callback,
                args,
            });
        }
    }
    pub fn drain(&mut self) -> Result<Vec<Reaction>, String> {
        if self.reactions.len() > 65536 {
            self.reactions.clear();
            return Err("Custom element reaction budget exceeded".into());
        }
        Ok(self.reactions.drain(..).collect())
    }
    pub fn release(&mut self, node: u64) {
        self.elements.remove(&node);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum ShadowMode {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "closed")]
    Closed,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ShadowRoot {
    pub root: String,
    pub host: String,
    pub mode: ShadowMode,
    pub delegates_focus: bool,
}
#[derive(Default)]
pub(crate) struct ShadowTree {
    by_host: BTreeMap<u64, ShadowRoot>,
    by_root: BTreeMap<u64, u64>,
}
impl ShadowTree {
    pub fn attach(
        &mut self,
        host: u64,
        root: u64,
        mode: &str,
        delegates_focus: bool,
        tag: &str,
    ) -> Result<ShadowRoot, String> {
        let mode = match mode {
            "open" => ShadowMode::Open,
            "closed" => ShadowMode::Closed,
            _ => return Err("TypeError: invalid shadow root mode".into()),
        };
        if ![
            "article",
            "aside",
            "blockquote",
            "body",
            "div",
            "footer",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "header",
            "main",
            "nav",
            "p",
            "section",
            "span",
        ]
        .contains(&tag)
            && !valid_name(tag)
        {
            return Err("NotSupportedError: element cannot host a shadow root".into());
        }
        if self.by_host.contains_key(&host) {
            return Err("NotSupportedError: element already has a shadow root".into());
        }
        let entry = ShadowRoot {
            root: root.to_string(),
            host: host.to_string(),
            mode,
            delegates_focus,
        };
        self.by_root.insert(root, host);
        self.by_host.insert(host, entry.clone());
        Ok(entry)
    }
    pub fn root(&self, host: u64, internal: bool) -> Option<&ShadowRoot> {
        self.by_host
            .get(&host)
            .filter(|root| internal || root.mode == ShadowMode::Open)
    }
    pub fn host(&self, root: u64) -> Option<u64> {
        self.by_root.get(&root).copied()
    }
    pub fn release(&mut self, host: u64) -> Option<ShadowRoot> {
        let root = self.by_host.remove(&host)?;
        self.by_root.remove(&root.root.parse().ok()?);
        Some(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_follow_pcen_ranges_without_case_folding() {
        for name in ["no-hyphen", "x-é", "x-\u{10000}"] {
            assert!(valid_name(name), "{name}");
        }
        for name in [
            "X-widget",
            "myElement",
            "1-x",
            "font-face",
            "x-\u{f0000}",
            "x- ",
        ] {
            assert!(!valid_name(name), "{name}");
        }
    }
    #[test]
    fn upgrades_do_not_retry_failed_constructors_and_observations_are_frozen() {
        let mut registry = Registry::default();
        let definition = registry
            .define("x-test", 1, vec!["value".into()], None)
            .unwrap();
        assert!(registry.define("x-other", 1, vec![], None).is_err());
        assert!(registry.begin(7, definition.id).unwrap());
        registry.attribute(7, "value", None, Some("initial"), None);
        registry.attribute(7, "other", None, Some("ignore"), None);
        registry.lifecycle(7, true);
        let reactions = registry.drain().unwrap();
        assert_eq!(reactions.len(), 2);
        assert_eq!(reactions[0].callback, "attributeChangedCallback");
        assert_eq!(reactions[1].callback, "connectedCallback");
        registry.attribute(7, "value", Some("initial"), Some("next"), None);
        registry.complete(7, false).unwrap();
        assert!(registry.drain().unwrap().is_empty());
        assert!(!registry.begin(7, definition.id).unwrap());
        assert_eq!(registry.needs_upgrade(7, "x-test"), None);
        registry.release(7);
        assert_eq!(registry.needs_upgrade(7, "x-test"), Some(definition.id));
    }
    #[test]
    fn closed_roots_are_internal_and_releasing_hosts_removes_reverse_links() {
        let mut tree = ShadowTree::default();
        tree.attach(1, 2, "closed", false, "div").unwrap();
        assert!(tree.root(1, false).is_none());
        assert!(tree.root(1, true).is_some());
        assert_eq!(tree.host(2), Some(1));
        assert!(tree.attach(1, 3, "open", false, "div").is_err());
        assert!(tree.attach(4, 5, "open", false, "input").is_err());
        tree.release(1);
        assert!(tree.host(2).is_none());
    }
}
