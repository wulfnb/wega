//! Runtime services attached to Wega's single authoritative flat tree.
//! Side tables contain resource/editor state, never parent/child/name copies.
mod cssom;
mod inline;
mod resources;
mod selectors;
use blitz_traits::net::{AbortSignal, NetProvider};
pub use cssom::{CssRuleInfo, CssomError};
use html5ever::QualName;
use resources::{Image, Message};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, mpsc},
};
use style::{
    servo_arc::Arc as StyleArc, shared_lock::SharedRwLock, stylesheets::DocumentStyleSheet,
};
use url::Url;
use wega_dom::input::{FocusState, HoverChange};
use wega_dom::{
    Document, DocumentMutation, DocumentView, MutationClock, MutationEpoch, MutationError, Mutator,
    NewAttribute, NodeKey, NodeView,
};
use wega_native::{controls::ControlView, layout::LayoutBox, resources::NativeResources};

#[derive(Default)]
pub struct DocumentSetup {
    pub base_url: Option<String>,
    pub fonts: Option<Arc<Vec<Vec<u8>>>>,
    pub abort_signal: Option<AbortSignal>,
    pub net_provider: Option<Arc<dyn NetProvider>>,
}

pub struct TextInput {
    pub editor: parley::PlainEditor<()>,
    pub multiline: bool,
    dirty_value: bool,
}
impl TextInput {
    fn new(value: &str, multiline: bool) -> Self {
        let mut editor = parley::PlainEditor::new(16.0);
        editor.set_text(value);
        Self {
            editor,
            multiline,
            dirty_value: false,
        }
    }
}

pub struct WegaDocument {
    tree: Document,
    fonts: Arc<Mutex<parley::FontContext>>,
    focused: Option<NodeKey>,
    hovered: Option<NodeKey>,
    active: Option<NodeKey>,
    controls: HashMap<NodeKey, TextInput>,
    checked: HashMap<NodeKey, bool>,
    files: HashMap<NodeKey, Vec<std::path::PathBuf>>,
    guard: SharedRwLock,
    nodes_to_stylesheet: HashMap<NodeKey, DocumentStyleSheet>,
    inline_sources: HashMap<NodeKey, (String, u64)>,
    stylesheet_requests: HashMap<NodeKey, String>,
    stylesheet_generation: u64,
    resource_generation: u64,
    external_epoch: u64,
    images: HashMap<Url, Option<Image>>,
    net: Option<Arc<dyn NetProvider>>,
    signal: Option<AbortSignal>,
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Message>,
}
impl WegaDocument {
    pub fn empty(setup: DocumentSetup) -> Self {
        initialize_style_parsing();
        let url = setup
            .base_url
            .as_deref()
            .and_then(|u| Url::parse(u).ok())
            .unwrap_or_else(|| Url::parse("about:blank").unwrap());
        let fonts = wega_native::text::font_context(setup.fonts.as_deref().map(Vec::as_slice));
        let (tx, rx) = mpsc::channel();
        Self {
            tree: Document::new(url),
            fonts: Arc::new(Mutex::new(fonts)),
            focused: None,
            hovered: None,
            active: None,
            controls: HashMap::new(),
            checked: HashMap::new(),
            files: HashMap::new(),
            guard: SharedRwLock::new(),
            nodes_to_stylesheet: HashMap::new(),
            inline_sources: HashMap::new(),
            stylesheet_requests: HashMap::new(),
            stylesheet_generation: 0,
            resource_generation: 0,
            external_epoch: 0,
            images: HashMap::new(),
            net: setup.net_provider,
            signal: setup.abort_signal,
            tx,
            rx,
        }
    }
    pub fn url(&self) -> &Url {
        self.base_url()
    }
    pub fn set_base_url(&mut self, url: Url) {
        self.tree.set_base_url(url);
    }
    pub fn node_count(&self) -> usize {
        self.tree.node_count()
    }
    pub fn record_external_mutation(&mut self) {
        self.external_epoch = self.external_epoch.wrapping_add(1);
    }
    pub fn text_input(&self, key: NodeKey) -> Option<&TextInput> {
        self.controls.get(&key).filter(|_| self.node(key).is_some())
    }
    pub fn text_input_mut(&mut self, key: NodeKey) -> Option<&mut TextInput> {
        let input = self.controls.get_mut(&key)?;
        input.dirty_value = true;
        Some(input)
    }
    pub fn input_value(&self, key: NodeKey) -> Option<String> {
        let node = self.node(key)?;
        node.element_name()?;
        Some(
            self.text_input(key)
                .map(|i| i.editor.text().to_string())
                .unwrap_or_else(|| node.attr("value").unwrap_or_default().to_owned()),
        )
    }
    pub fn checked(&self, key: NodeKey) -> bool {
        self.node(key).is_some() && self.checked.get(&key).copied().unwrap_or(false)
    }
    pub fn set_checked(&mut self, key: NodeKey, value: bool) {
        if self.node(key).and_then(NodeView::element_name).is_some() {
            self.checked.insert(key, value);
            self.record_external_mutation();
        }
    }
    pub fn set_hover(&mut self, key: Option<NodeKey>) -> HoverChange {
        let previous = self.hovered();
        self.hovered = key.filter(|k| self.node(*k).is_some());
        HoverChange {
            previous,
            current: self.hovered,
        }
    }
    fn ensure_control(&mut self, key: NodeKey) {
        let Some(node) = self.node(key) else { return };
        let Some(name) = node.element_name() else {
            return;
        };
        let tag = name.local;
        let kind = node.attr("type").unwrap_or("text").to_ascii_lowercase();
        if tag == "input" && matches!(kind.as_str(), "checkbox" | "radio") {
            let checked = node.attr("checked").is_some();
            self.controls.remove(&key);
            self.checked.entry(key).or_insert(checked);
        } else if tag == "textarea"
            || tag == "input"
                && !matches!(
                    kind.as_str(),
                    "button" | "submit" | "reset" | "image" | "file"
                )
        {
            let multiline = tag == "textarea";
            let value = node.attr("value").map(str::to_owned).unwrap_or_else(|| {
                if multiline {
                    wega_dom::content::text_content(self, key).unwrap_or_default()
                } else {
                    String::new()
                }
            });
            self.checked.remove(&key);
            let input = self
                .controls
                .entry(key)
                .or_insert_with(|| TextInput::new(&value, multiline));
            if !input.dirty_value && input.editor.text() != value.as_str() {
                input.editor.set_text(&value);
            }
        } else {
            self.controls.remove(&key);
            self.checked.remove(&key);
        }
    }
    fn control(&self, key: NodeKey) -> Option<ControlView<'_>> {
        let node = self.node(key)?;
        let input = self.text_input(key);
        let selection = input
            .map(|i| {
                let s = i.editor.raw_selection();
                (s.anchor().index(), s.focus().index())
            })
            .unwrap_or_default();
        Some(ControlView {
            value: input.map(|i| i.editor.text().to_string().into()),
            selection,
            checked: self.checked.get(&key).copied(),
            focused: self.focused() == Some(key),
            placeholder: node.attr("placeholder"),
        })
    }
    fn after_mutation(&mut self, touched: HashSet<NodeKey>) {
        for key in touched {
            if self.node(key).is_none() {
                continue;
            }
            self.ensure_control(key);
            self.discover_resources(key);
            let mut at = Some(key);
            while let Some(key) = at {
                self.ensure_control(key);
                if self
                    .node(key)
                    .and_then(NodeView::element_name)
                    .is_some_and(|n| n.local == "style")
                {
                    self.refresh_style_element(key);
                    break;
                }
                at = self.node(key).and_then(NodeView::parent);
            }
        }
        self.controls
            .retain(|key, _| self.tree.node(*key).is_some());
        self.checked.retain(|key, _| self.tree.node(*key).is_some());
        self.files.retain(|key, _| self.tree.node(*key).is_some());
        self.nodes_to_stylesheet
            .retain(|key, _| self.tree.node(*key).is_some());
        self.inline_sources
            .retain(|key, _| self.tree.node(*key).is_some());
        self.stylesheet_requests
            .retain(|key, _| self.tree.node(*key).is_some());
        if self
            .focused
            .is_some_and(|k| self.node(k).is_none() || wega_dom::tree_root(self, k) != self.root())
        {
            self.focused = None;
        }
        if self
            .hovered
            .is_some_and(|k| self.node(k).is_none() || wega_dom::tree_root(self, k) != self.root())
        {
            self.hovered = None;
        }
        if self.active.is_some_and(|k| self.node(k).is_none()) {
            self.active = None;
        }
    }
}
impl DocumentView for WegaDocument {
    type Node<'a> = wega_dom::document::DocumentNode<'a>;
    fn node(&self, key: NodeKey) -> Option<Self::Node<'_>> {
        self.tree.node(key)
    }
    fn element_by_id(&self, id: &str) -> Option<NodeKey> {
        self.tree.element_by_id(id)
    }
    fn root(&self) -> NodeKey {
        self.tree.root()
    }
    fn base_url(&self) -> &Url {
        self.tree.base_url()
    }
}
impl MutationClock for WegaDocument {
    fn mutation_epoch(&self) -> MutationEpoch {
        MutationEpoch::from_u64(
            self.tree
                .mutation_epoch()
                .as_u64()
                .wrapping_add(self.external_epoch)
                .wrapping_add(self.stylesheet_generation),
        )
    }
}
impl FocusState for WegaDocument {
    fn focused(&self) -> Option<NodeKey> {
        self.focused.filter(|k| self.node(*k).is_some())
    }
    fn keyboard_target(&self) -> Option<NodeKey> {
        self.focused().or_else(|| {
            self.node(self.root())?
                .children()
                .find(|k| self.node(*k).and_then(NodeView::element_name).is_some())
        })
    }
    fn hovered(&self) -> Option<NodeKey> {
        self.hovered.filter(|k| self.node(*k).is_some())
    }
}
impl WegaDocument {
    pub fn set_focus(&mut self, key: NodeKey) -> Result<bool, MutationError> {
        let mut at = key;
        loop {
            let node = self.node(at).ok_or(MutationError::Stale(at))?;
            if node.element_name().is_some() {
                break;
            }
            at = node.parent().ok_or(MutationError::NotElement(key))?;
        }
        self.ensure_control(at);
        let changed = self.focused != Some(at);
        self.focused = Some(at);
        Ok(changed)
    }
    pub fn clear_focus(&mut self) {
        self.focused = None;
    }
}
impl NativeResources for WegaDocument {
    fn native_generations(&self) -> (u64, u64) {
        (self.stylesheet_generation, self.resource_generation)
    }
    fn native_fonts(&self) -> Arc<Mutex<parley::FontContext>> {
        self.fonts.clone()
    }
    fn prepare_controls(&mut self) {
        let keys: Vec<_> = wega_dom::preorder(self, self.root()).collect();
        for key in keys {
            self.ensure_control(key);
        }
    }
    fn sync_controls(&self, root: &mut LayoutBox) {
        wega_native::controls::sync(root, &|key| self.control(key));
    }
    fn decorate_controls(&self, root: &mut LayoutBox) {
        wega_native::controls::decorate(root, &|key| self.control(key));
    }
    fn native_stylesheets(
        &self,
        allow: &dyn Fn(Option<&str>, &str) -> bool,
    ) -> Vec<wega_native::css::StyleSheet> {
        let mut sheets = Vec::new();
        for key in wega_dom::preorder(self, self.root()) {
            let Some(node) = self.node(key) else { continue };
            let Some(name) = node.element_name() else {
                continue;
            };
            if self.stylesheet_disabled(key) {
                continue;
            }
            if name.local == "style" {
                if !allow(
                    node.attr("nonce"),
                    &wega_dom::content::text_content(self, key).unwrap_or_default(),
                ) {
                    continue;
                }
            } else if name.local != "link"
                || !node.attr("rel").is_some_and(|r| {
                    r.split_ascii_whitespace()
                        .any(|r| r.eq_ignore_ascii_case("stylesheet"))
                })
                || node
                    .attr("href")
                    .and_then(|v| self.url().join(v).ok())
                    .as_ref()
                    .map(Url::as_str)
                    != self.stylesheet_requests.get(&key).map(String::as_str)
            {
                continue;
            }
            let Some(css) = self.native_stylesheet_text(key) else {
                continue;
            };
            let mut sheet = wega_native::css::StyleSheet::parse(&css);
            if let Some(media) = node.attr("media").filter(|m| !m.trim().is_empty()) {
                let queries = wega_native::css::parse_media_query_list(media);
                for rule in &mut sheet.rules {
                    rule.media.push(queries.clone());
                }
            }
            sheets.push(sheet);
        }
        sheets
    }
    fn native_images(&mut self, root: &mut LayoutBox) {
        self.attach_images(root);
    }
}
impl DocumentMutation for WegaDocument {
    type Mutator<'a> = DocumentMutator<'a>;
    fn mutate(&mut self) -> Self::Mutator<'_> {
        DocumentMutator {
            document: self,
            touched: HashSet::new(),
        }
    }
    fn refresh_style_element(&mut self, key: NodeKey) {
        self.refresh_style(key);
    }
}
pub struct DocumentMutator<'a> {
    document: &'a mut WegaDocument,
    touched: HashSet<NodeKey>,
}
impl Drop for DocumentMutator<'_> {
    fn drop(&mut self) {
        self.document
            .after_mutation(std::mem::take(&mut self.touched));
    }
}

impl Mutator for DocumentMutator<'_> {
    fn parent(&self, key: NodeKey) -> Option<NodeKey> {
        self.document.node(key)?.parent()
    }
    fn last_child(&self, key: NodeKey) -> Option<NodeKey> {
        self.document.node(key)?.children().next_back()
    }
    fn previous_sibling(&self, key: NodeKey) -> Option<NodeKey> {
        wega_dom::sibling(self.document, key, false)
    }
    fn create_document(&mut self) -> NodeKey {
        self.document.tree.mutate().create_document()
    }
    fn create_element(
        &mut self,
        name: QualName,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> NodeKey {
        if &*name.local == "#document-fragment" {
            return self.document.tree.create_fragment();
        }
        self.document.tree.mutate().create_element(name, attrs)
    }
    fn create_text(&mut self, text: &str) -> NodeKey {
        self.document.tree.mutate().create_text(text)
    }
    fn create_comment(&mut self, text: &str) -> NodeKey {
        self.document.tree.mutate().create_comment(text)
    }
    fn template_contents(&mut self, key: NodeKey) -> Result<NodeKey, MutationError> {
        self.document.tree.mutate().template_contents(key)
    }
    fn clone_node(&mut self, key: NodeKey, deep: bool) -> Result<NodeKey, MutationError> {
        self.document.tree.mutate().clone_node(key, deep)
    }
    fn append_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        self.document
            .tree
            .mutate()
            .append_children(parent, children)?;
        self.touched.insert(parent);
        for child in children {
            self.touched
                .extend(wega_dom::preorder(self.document, *child));
        }
        Ok(())
    }
    fn insert_before(
        &mut self,
        anchor: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        self.document
            .tree
            .mutate()
            .insert_before(anchor, children)?;
        self.touched.insert(anchor);
        for child in children {
            self.touched
                .extend(wega_dom::preorder(self.document, *child));
        }
        Ok(())
    }
    fn replace_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        self.document
            .tree
            .mutate()
            .replace_children(parent, children)?;
        self.touched.insert(parent);
        for child in children {
            self.touched
                .extend(wega_dom::preorder(self.document, *child));
        }
        Ok(())
    }
    fn remove(&mut self, key: NodeKey) -> Result<(), MutationError> {
        self.document.tree.mutate().remove(key)?;
        self.touched.insert(key);
        Ok(())
    }
    fn remove_and_drop(&mut self, key: NodeKey) -> Result<(), MutationError> {
        self.document.tree.mutate().remove_and_drop(key)?;
        self.touched.insert(key);
        Ok(())
    }
    fn reparent_children(&mut self, from: NodeKey, to: NodeKey) -> Result<(), MutationError> {
        self.document.tree.mutate().reparent_children(from, to)?;
        self.touched.insert(from);
        self.touched.insert(to);
        Ok(())
    }
    fn set_text(&mut self, key: NodeKey, text: &str) -> Result<(), MutationError> {
        self.document.tree.mutate().set_text(key, text)?;
        self.touched.insert(key);
        Ok(())
    }
    fn set_comment(&mut self, key: NodeKey, text: &str) -> Result<(), MutationError> {
        self.document.tree.mutate().set_comment(key, text)?;
        self.touched.insert(key);
        Ok(())
    }
    fn append_text(&mut self, key: NodeKey, text: &str) -> Result<(), MutationError> {
        self.document.tree.mutate().append_text(key, text)?;
        self.touched.insert(key);
        Ok(())
    }
    fn set_attribute(
        &mut self,
        key: NodeKey,
        name: QualName,
        value: &str,
    ) -> Result<(), MutationError> {
        self.document
            .tree
            .mutate()
            .set_attribute(key, name, value)?;
        self.touched.insert(key);
        Ok(())
    }
    fn clear_attribute(&mut self, key: NodeKey, name: QualName) -> Result<(), MutationError> {
        self.document.tree.mutate().clear_attribute(key, name)?;
        self.touched.insert(key);
        Ok(())
    }
    fn add_attributes_if_missing(
        &mut self,
        key: NodeKey,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> Result<(), MutationError> {
        self.document
            .tree
            .mutate()
            .add_attributes_if_missing(key, attrs)?;
        self.touched.insert(key);
        Ok(())
    }
}

// Preserve the parser capabilities selected by the production constructor.
// Native layout itself never enters the reference stylist or thread pool.
fn initialize_style_parsing() {
    static INITIALIZE: std::sync::Once = std::sync::Once::new();
    INITIALIZE.call_once(|| {
        style_config::set_pref!("layout.grid.enabled", true);
        style_config::set_pref!("layout.flexbox.balance", true);
        style_config::set_pref!("layout.unimplemented", true);
        style_config::set_pref!("layout.columns.enabled", true);
        style_config::set_pref!("layout.css.basic-shape-shape.enabled", true);
        style_config::set_pref!("layout.css.tree-counting-functions.enabled", true);
        style_config::set_pref!("layout.css.progress-function.enabled", true);
        style_config::set_pref!("layout.variable_fonts.enabled", true);
        style_config::set_pref!("layout.container-queries.enabled", true);
    });
}
