//! Wega-owned flat document storage. No layout, VM, network, or alternate tree.
use crate::{
    DocumentMutation, DocumentView, ElementName, MutationClock, MutationEpoch, MutationError,
    Mutator, NewAttribute, NodeKey, NodeKind, NodeView,
};
use html5ever::QualName;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use url::Url;

// Keys are not reused, even across documents, so a stale/foreign wrapper cannot
// alias a new allocation. Exhaustion fails before reuse instead of wrapping.
static NEXT_KEY: AtomicU64 = AtomicU64::new(1);
fn key() -> NodeKey {
    NodeKey::from_u64(
        NEXT_KEY
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("document identity space exhausted"),
    )
}
#[derive(Clone)]
enum Data {
    Document,
    Fragment,
    Element(QualName, Vec<NewAttribute>),
    Text(String),
    Comment(String),
}
struct Node {
    data: Data,
    parent: Option<NodeKey>,
    children: Vec<NodeKey>,
    template: Option<NodeKey>,
    // Ownership is separate from observable DOM parentage for template contents.
    owner: Option<NodeKey>,
}
impl Node {
    fn new(data: Data) -> Self {
        Self {
            data,
            parent: None,
            children: Vec::new(),
            template: None,
            owner: None,
        }
    }
}
/// Coalesced invalidations, consumed by the eventual style/layout owner.
#[derive(Default, Debug)]
pub struct Invalidations {
    pub structure: bool,
    pub style: bool,
    pub layout: bool,
    pub nodes: HashSet<NodeKey>,
}
/// One authoritative tree, with no recursively owned nodes. Detached nodes remain
/// addressable until explicit collection; template fragments are owned by keys.
pub struct Document {
    nodes: HashMap<NodeKey, Node>,
    root: NodeKey,
    url: Url,
    epoch: u64,
    dirty: Invalidations,
}
impl Document {
    pub fn new(url: Url) -> Self {
        let root = key();
        Self {
            nodes: HashMap::from([(root, Node::new(Data::Document))]),
            root,
            url,
            epoch: 0,
            dirty: Invalidations::default(),
        }
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_fragment(&self, key: NodeKey) -> bool {
        self.nodes
            .get(&key)
            .is_some_and(|n| matches!(n.data, Data::Fragment))
    }
    pub fn create_fragment(&mut self) -> NodeKey {
        self.allocate(Data::Fragment)
    }
    pub fn take_invalidations(&mut self) -> Invalidations {
        std::mem::take(&mut self.dirty)
    }
    fn allocate(&mut self, data: Data) -> NodeKey {
        let key = key();
        self.nodes.insert(key, Node::new(data));
        key
    }
    fn get(&self, key: NodeKey) -> Result<&Node, MutationError> {
        self.nodes.get(&key).ok_or(MutationError::Stale(key))
    }
    fn changed(&mut self, key: NodeKey, structure: bool) {
        self.epoch = self.epoch.wrapping_add(1);
        self.dirty.structure |= structure;
        self.dirty.style = true;
        self.dirty.layout = true;
        if self.nodes.contains_key(&key) {
            self.dirty.nodes.insert(key);
        }
    }
    fn container(&self, key: NodeKey) -> Result<(), MutationError> {
        match self.get(key)?.data {
            Data::Text(_) | Data::Comment(_) => Err(MutationError::InvalidParent(key)),
            _ => Ok(()),
        }
    }
    fn element(&self, key: NodeKey) -> Result<(), MutationError> {
        match self.get(key)?.data {
            Data::Element(..) => Ok(()),
            _ => Err(MutationError::NotElement(key)),
        }
    }
    fn validate(&self, parent: NodeKey, children: &[NodeKey]) -> Result<(), MutationError> {
        self.container(parent)?;
        for &child in children {
            self.get(child)?;
            let mut at = Some(parent);
            while let Some(k) = at {
                if k == child {
                    return Err(MutationError::HierarchyCycle { parent, child });
                }
                let n = self.get(k)?;
                at = n.parent.or(n.owner);
            }
            if child == self.root {
                return Err(MutationError::InvalidParent(child));
            }
        }
        Ok(())
    }
    fn expand_fragments(&self, children: &[NodeKey]) -> Result<Vec<NodeKey>, MutationError> {
        let mut result = Vec::new();
        for &key in children {
            let n = self.get(key)?;
            if matches!(n.data, Data::Fragment) {
                result.extend(&n.children);
            } else {
                result.push(key);
            }
        }
        Ok(result)
    }
    fn detach(&mut self, key: NodeKey) {
        let parent = self.nodes.get_mut(&key).and_then(|n| n.parent.take());
        if let Some(parent) = parent {
            self.nodes
                .get_mut(&parent)
                .unwrap()
                .children
                .retain(|k| *k != key);
            self.dirty.nodes.insert(parent);
        }
    }
    // Inputs already validated. Repeated children move to their last position.
    fn insert(&mut self, parent: NodeKey, anchor: Option<NodeKey>, children: &[NodeKey]) {
        for &child in children {
            if Some(child) == anchor {
                continue;
            }
            self.detach(child);
            let p = self.nodes.get_mut(&parent).unwrap();
            let index = anchor
                .and_then(|a| p.children.iter().position(|k| *k == a))
                .unwrap_or(p.children.len());
            p.children.insert(index, child);
            self.nodes.get_mut(&child).unwrap().parent = Some(parent);
        }
    }
}
#[derive(Clone, Copy)]
pub struct DocumentNode<'a>(&'a Node);
impl<'a> NodeView<'a> for DocumentNode<'a> {
    fn kind(self) -> NodeKind {
        match self.0.data {
            // The current engine-neutral contract represents fragment containers
            // as document-kind nodes, as does the existing parser adapter.
            Data::Document | Data::Fragment => NodeKind::Document,
            Data::Element(..) => NodeKind::Element,
            Data::Text(_) => NodeKind::Text,
            Data::Comment(_) => NodeKind::Comment,
        }
    }
    fn text(self) -> Option<&'a str> {
        match &self.0.data {
            Data::Text(s) | Data::Comment(s) => Some(s),
            _ => None,
        }
    }
    fn qual_name(self) -> Option<&'a QualName> {
        match &self.0.data {
            Data::Element(n, _) => Some(n),
            _ => None,
        }
    }
    fn element_name(self) -> Option<ElementName<'a>> {
        self.qual_name().map(|n| ElementName {
            local: &n.local,
            namespace: &n.ns,
            prefix: n.prefix.as_deref(),
        })
    }
    fn attr(self, name: &str) -> Option<&'a str> {
        self.attributes()
            .find(|(n, _)| &*n.local == name)
            .map(|(_, v)| v)
    }
    fn parent(self) -> Option<NodeKey> {
        self.0.parent
    }
    fn children(self) -> impl ExactSizeIterator<Item = NodeKey> + DoubleEndedIterator + 'a {
        self.0.children.iter().copied()
    }
    fn child_at(self, index: usize) -> Option<NodeKey> {
        self.0.children.get(index).copied()
    }
    fn attributes(self) -> impl Iterator<Item = (&'a QualName, &'a str)> + 'a {
        let attrs = match &self.0.data {
            Data::Element(_, attrs) => attrs.as_slice(),
            _ => &[],
        };
        attrs.iter().map(|a| (&a.name, a.value.as_str()))
    }
}
impl DocumentView for Document {
    type Node<'a> = DocumentNode<'a>;
    fn node(&self, key: NodeKey) -> Option<Self::Node<'_>> {
        self.nodes.get(&key).map(DocumentNode)
    }
    fn root(&self) -> NodeKey {
        self.root
    }
    fn base_url(&self) -> &Url {
        &self.url
    }
    fn element_by_id(&self, id: &str) -> Option<NodeKey> {
        let mut stack = vec![self.root];
        while let Some(k) = stack.pop() {
            let n = self.node(k)?;
            if n.kind() == NodeKind::Element && n.attr("id") == Some(id) {
                return Some(k);
            }
            stack.extend(n.children().rev());
        }
        None
    }
}
impl MutationClock for Document {
    fn mutation_epoch(&self) -> MutationEpoch {
        MutationEpoch::from_u64(self.epoch)
    }
}
impl DocumentMutation for Document {
    type Mutator<'a> = DocumentMutator<'a>;
    fn mutate(&mut self) -> Self::Mutator<'_> {
        DocumentMutator(self)
    }
    fn refresh_style_element(&mut self, key: NodeKey) {
        if self
            .node(key)
            .and_then(NodeView::element_name)
            .is_some_and(|n| n.local == "style")
        {
            self.dirty.style = true;
            self.dirty.layout = true;
            self.dirty.nodes.insert(key);
        }
    }
}
pub struct DocumentMutator<'a>(&'a mut Document);
impl Mutator for DocumentMutator<'_> {
    fn parent(&self, key: NodeKey) -> Option<NodeKey> {
        self.0.node(key)?.parent()
    }
    fn last_child(&self, key: NodeKey) -> Option<NodeKey> {
        self.0.node(key)?.children().next_back()
    }
    fn previous_sibling(&self, key: NodeKey) -> Option<NodeKey> {
        crate::sibling(self.0, key, false)
    }
    fn create_element(
        &mut self,
        name: QualName,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> NodeKey {
        let mut unique: Vec<NewAttribute> = Vec::new();
        for a in attrs {
            if !unique
                .iter()
                .any(|b| b.name.ns == a.name.ns && b.name.local == a.name.local)
            {
                unique.push(a);
            }
        }
        self.0.allocate(Data::Element(name, unique))
    }
    fn create_text(&mut self, text: &str) -> NodeKey {
        self.0.allocate(Data::Text(text.into()))
    }
    fn create_comment(&mut self, text: &str) -> NodeKey {
        self.0.allocate(Data::Comment(text.into()))
    }
    fn template_contents(&mut self, key: NodeKey) -> Result<NodeKey, MutationError> {
        self.0.element(key)?;
        if let Some(fragment) = self.0.get(key)?.template {
            return Ok(fragment);
        }
        let fragment = self.0.create_fragment();
        self.0.nodes.get_mut(&key).unwrap().template = Some(fragment);
        self.0.nodes.get_mut(&fragment).unwrap().owner = Some(key);
        Ok(fragment)
    }
    fn append_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        self.0.validate(parent, children)?;
        let expanded = self.0.expand_fragments(children)?;
        let children = expanded.as_slice();
        self.0.validate(parent, children)?;
        if children.is_empty() {
            return Ok(());
        }
        let old = self.0.get(parent)?.children.clone();
        self.0.insert(parent, None, children);
        if self.0.get(parent)?.children != old {
            self.0.changed(parent, true);
        }
        Ok(())
    }
    fn insert_before(
        &mut self,
        anchor: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        let parent = self
            .0
            .get(anchor)?
            .parent
            .ok_or(MutationError::Detached(anchor))?;
        self.0.validate(parent, children)?;
        let expanded = self.0.expand_fragments(children)?;
        let children = expanded.as_slice();
        self.0.validate(parent, children)?;
        let old = self.0.get(parent)?.children.clone();
        self.0.insert(parent, Some(anchor), children);
        if self.0.get(parent)?.children != old {
            self.0.changed(parent, true);
        }
        Ok(())
    }
    fn replace_children(
        &mut self,
        parent: NodeKey,
        children: &[NodeKey],
    ) -> Result<(), MutationError> {
        self.0.validate(parent, children)?;
        let expanded = self.0.expand_fragments(children)?;
        let children = expanded.as_slice();
        self.0.validate(parent, children)?;
        let old = self.0.get(parent)?.children.clone();
        // Detach first; a retained descendant stays live and is then moved.
        for k in &old {
            self.0.detach(*k);
        }
        self.0.insert(parent, None, children);
        if self.0.get(parent)?.children != old {
            self.0.changed(parent, true);
        }
        Ok(())
    }
    fn remove(&mut self, key: NodeKey) -> Result<(), MutationError> {
        if self.0.get(key)?.parent.is_some() {
            self.0.detach(key);
            self.0.changed(key, true);
        }
        Ok(())
    }
    fn remove_and_drop(&mut self, key: NodeKey) -> Result<(), MutationError> {
        self.0.get(key)?;
        if key == self.0.root {
            return Err(MutationError::InvalidParent(key));
        }
        self.0.detach(key);
        let mut stack = vec![key];
        while let Some(k) = stack.pop() {
            if let Some(n) = self.0.nodes.remove(&k) {
                if let Some(owner) = n.owner.and_then(|k| self.0.nodes.get_mut(&k)) {
                    owner.template = None;
                }
                stack.extend(n.children);
                stack.extend(n.template);
                self.0.dirty.nodes.remove(&k);
            }
        }
        self.0.changed(key, true);
        Ok(())
    }
    fn reparent_children(&mut self, from: NodeKey, to: NodeKey) -> Result<(), MutationError> {
        self.0.container(from)?;
        self.0.container(to)?;
        let children = self.0.get(from)?.children.clone();
        self.0.validate(to, &children)?;
        if from != to {
            self.append_children(to, &children)?;
        }
        Ok(())
    }
    fn clone_node(&mut self, key: NodeKey, deep: bool) -> Result<NodeKey, MutationError> {
        let data = self.0.get(key)?.data.clone();
        if matches!(data, Data::Document) {
            return Err(MutationError::NotCloneable(key));
        }
        let clone = self.0.allocate(data);
        if !deep {
            return Ok(clone);
        }
        let mut stack = vec![(key, clone)];
        while let Some((source, dest)) = stack.pop() {
            let n = self.0.get(source)?;
            let (children, template) = (n.children.clone(), n.template);
            for child in children {
                let child_clone = self.0.allocate(self.0.get(child)?.data.clone());
                self.0
                    .nodes
                    .get_mut(&dest)
                    .unwrap()
                    .children
                    .push(child_clone);
                self.0.nodes.get_mut(&child_clone).unwrap().parent = Some(dest);
                stack.push((child, child_clone));
            }
            if let Some(template) = template {
                let copy = self.0.create_fragment();
                self.0.nodes.get_mut(&dest).unwrap().template = Some(copy);
                self.0.nodes.get_mut(&copy).unwrap().owner = Some(dest);
                stack.push((template, copy));
            }
        }
        Ok(clone)
    }
    fn set_text(&mut self, key: NodeKey, text: &str) -> Result<(), MutationError> {
        let n = self
            .0
            .nodes
            .get_mut(&key)
            .ok_or(MutationError::Stale(key))?;
        let Data::Text(current) = &mut n.data else {
            return Err(MutationError::NotText(key));
        };
        if current != text {
            *current = text.into();
            self.0.changed(key, false);
        }
        Ok(())
    }
    fn append_text(&mut self, key: NodeKey, text: &str) -> Result<(), MutationError> {
        let n = self
            .0
            .nodes
            .get_mut(&key)
            .ok_or(MutationError::Stale(key))?;
        let Data::Text(current) = &mut n.data else {
            return Err(MutationError::NotText(key));
        };
        if !text.is_empty() {
            current.push_str(text);
            self.0.changed(key, false);
        }
        Ok(())
    }
    fn set_attribute(
        &mut self,
        key: NodeKey,
        name: QualName,
        value: &str,
    ) -> Result<(), MutationError> {
        self.0.element(key)?;
        let Data::Element(_, attrs) = &mut self.0.nodes.get_mut(&key).unwrap().data else {
            unreachable!()
        };
        if let Some(a) = attrs
            .iter_mut()
            .find(|a| a.name.ns == name.ns && a.name.local == name.local)
        {
            if a.value == value && a.name == name {
                return Ok(());
            }
            a.name = name;
            a.value = value.into();
        } else {
            attrs.push(NewAttribute {
                name,
                value: value.into(),
            });
        }
        self.0.changed(key, false);
        Ok(())
    }
    fn clear_attribute(&mut self, key: NodeKey, name: QualName) -> Result<(), MutationError> {
        self.0.element(key)?;
        let Data::Element(_, attrs) = &mut self.0.nodes.get_mut(&key).unwrap().data else {
            unreachable!()
        };
        let old = attrs.len();
        attrs.retain(|a| a.name.ns != name.ns || a.name.local != name.local);
        if old != attrs.len() {
            self.0.changed(key, false);
        }
        Ok(())
    }
    fn add_attributes_if_missing(
        &mut self,
        key: NodeKey,
        attrs: impl IntoIterator<Item = NewAttribute>,
    ) -> Result<(), MutationError> {
        self.0.element(key)?;
        let mut changed = false;
        let Data::Element(_, current) = &mut self.0.nodes.get_mut(&key).unwrap().data else {
            unreachable!()
        };
        for a in attrs {
            if !current
                .iter()
                .any(|b| b.name.ns == a.name.ns && b.name.local == a.name.local)
            {
                current.push(a);
                changed = true;
            }
        }
        if changed {
            self.0.changed(key, false);
        }
        Ok(())
    }
}
