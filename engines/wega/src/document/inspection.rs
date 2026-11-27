//! Read-only inspection mapping; no style resolution or retained CSS state.
use super::*;
use crate::native_engine::NativeDocument;
use std::ops::Deref;
use std::{fmt::Write, time::Instant};
use style::{
    properties::{PropertyDeclarationBlock, PropertyDeclarationId},
    servo_arc::Arc,
    shared_lock::{Locked, SharedRwLockReadGuard},
    stylesheets::{CssRule, CssRules, StylesheetInDocument},
};
use style_traits::{CssWriter, ToCss};
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView, inspect::*};

fn text(value: &str) -> (String, bool) {
    BoundedCss::preview(value)
}
fn declarations(
    block: &PropertyDeclarationBlock,
    offset: u32,
    start: Instant,
    page: &mut StylePage,
) -> Result<(), StyleReadError> {
    let count = block.declarations().len();
    if offset as usize > count {
        return Err(StyleReadError::Invalid);
    }
    page.total = count.min(u32::MAX as usize) as u32;
    for index in offset as usize..count.min(offset as usize + STYLE_PAGE) {
        BoundedCss::check(start)?;
        let decl = &block.declarations()[index];
        let mut name = BoundedCss::new(start);
        match decl.id() {
            PropertyDeclarationId::Longhand(id) => {
                let _ = name.write_str(id.name());
            }
            PropertyDeclarationId::Custom(n) => {
                let _ = write!(name, "--{n}");
            }
        }
        let mut value = BoundedCss::new(start);
        if decl.to_css_with_writer(&mut value).is_err() && !value.truncated {
            BoundedCss::check(start)?;
            return Err(StyleReadError::Invalid);
        }
        page.items.push(StyleItem::Declaration(Declaration {
            name: name.text,
            value: value.text,
            important: block.declarations_importance().get(index).unwrap_or(false),
            truncated: name.truncated || value.truncated,
        }));
    }
    page.next =
        (offset as usize + page.items.len() < count).then_some(offset + page.items.len() as u32);
    Ok(())
}
fn nested(rule: &CssRule, guard: &SharedRwLockReadGuard) -> Option<Arc<Locked<CssRules>>> {
    match rule {
        CssRule::Style(r) => r.read_with(guard).rules.clone(),
        CssRule::Media(r) => Some(r.rules.clone()),
        CssRule::Supports(r) => Some(r.rules.clone()),
        CssRule::LayerBlock(r) => Some(r.rules.clone()),
        _ => None,
    }
}
fn connected(doc: &DocumentRead, key: NodeKey, start: Instant) -> Result<(), StyleReadError> {
    let mut at = key;
    for _ in 0..=128 {
        BoundedCss::check(start)?;
        let node = doc.node(at).ok_or(StyleReadError::Stale)?;
        if node.kind() == NodeKind::AnonymousBlock {
            return Err(StyleReadError::Stale);
        }
        if at == doc.root() {
            return Ok(());
        }
        at = node.parent().ok_or(StyleReadError::Stale)?;
    }
    Err(StyleReadError::Depth)
}
/// Bounded scalar longhands only. Arbitrary list/custom properties may require
/// cloning unbounded vectors in the vendor computed-value serializer.
const COMPUTED: &[&str] = &[
    "color",
    "background-color",
    "display",
    "position",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "font-size",
    "font-weight",
    "line-height",
    "opacity",
    "box-sizing",
];
fn computed(
    d: &DocumentRead,
    key: NodeKey,
    offset: u32,
    start: Instant,
    page: &mut StylePage,
) -> Result<(), StyleReadError> {
    let Some(style) = d.native.and_then(|n| n.find_box(key)).map(|b| &b.style) else {
        page.availability = Availability::NotComputed;
        return Ok(());
    };
    if offset as usize > COMPUTED.len() {
        return Err(StyleReadError::Invalid);
    }
    page.total = COMPUTED.len() as u32;
    page.availability = Availability::Cached;
    for name in COMPUTED.iter().skip(offset as usize).take(STYLE_PAGE) {
        BoundedCss::check(start)?;
        let mut out = BoundedCss::new(start);
        if let Some(value) = wega_style::computed::scalar(style, name) {
            let _ = out.write_str(&value);
        }
        page.items.push(StyleItem::Declaration(Declaration {
            name: (*name).into(),
            value: out.text,
            important: false,
            truncated: out.truncated,
        }));
    }
    page.next = (offset as usize + page.items.len() < COMPUTED.len())
        .then_some(offset + page.items.len() as u32);
    Ok(())
}
impl StyleInspect for DocumentRead<'_> {
    fn style_generation(&self) -> u64 {
        self.stylesheet_generation()
    }
    fn read_style(&self, query: &StyleRead, offset: u32) -> Result<StylePage, StyleReadError> {
        let start = Instant::now();
        let mut page = StylePage {
            availability: Availability::Available,
            layout_pending: self.layout_pending(),
            generation: self.stylesheet_generation(),
            total: 0,
            next: None,
            items: vec![],
            truncated: false,
        };
        match query {
            StyleRead::Computed(key) | StyleRead::Inline(key) | StyleRead::Matched(key) => {
                connected(self, *key, start)?;
                let node = self.node(*key).ok_or(StyleReadError::Stale)?;
                if node.element_name().is_none() {
                    page.availability = Availability::Unsupported;
                    return Ok(page);
                }
                match query {
                    StyleRead::Computed(_) => computed(self, *key, offset, start, &mut page)?,
                    StyleRead::Inline(_) => {
                        let block = self
                            .doc
                            .parse_style_attr_block(node.attr("style").unwrap_or_default());
                        declarations(&block, offset, start, &mut page)?;
                    }
                    _ => page.availability = Availability::Unsupported,
                }
            }
            StyleRead::Sheets => {
                if offset as usize > STYLE_SHEETS {
                    return Err(StyleReadError::Invalid);
                }
                let guard = self.doc.guard().read();
                for (index, (owner, sheet)) in self
                    .stylesheet_owner_nodes()
                    .zip(self.author_stylesheets())
                    .enumerate()
                    .take(STYLE_SHEETS + 1)
                {
                    BoundedCss::check(start)?;
                    if index == STYLE_SHEETS {
                        page.truncated = true;
                        break;
                    }
                    page.total = index as u32 + 1;
                    if index < offset as usize || page.items.len() == STYLE_PAGE {
                        continue;
                    }
                    let contents = sheet.contents(&guard);
                    let (url, truncated) = text(contents.url_data.as_str());
                    page.items.push(StyleItem::Sheet(SheetInfo {
                        owner,
                        url,
                        disabled: self.stylesheet_disabled(owner),
                        rules: contents
                            .rules
                            .read_with(&guard)
                            .0
                            .len()
                            .min(u32::MAX as usize) as u32,
                        truncated,
                    }));
                }
                if offset > page.total {
                    return Err(StyleReadError::Invalid);
                }
                page.next = (offset + (page.items.len() as u32) < page.total)
                    .then_some(offset + page.items.len() as u32);
            }
            StyleRead::Rules { owner, path } | StyleRead::Declarations { owner, path } => {
                if path.len() > STYLE_PATH {
                    return Err(StyleReadError::Depth);
                }
                connected(self, *owner, start)?;
                // The public iterators share the same bounded owner order; never scan an
                // arbitrary number of stylesheets to resolve an untrusted owner key.
                let sheet = self
                    .stylesheet_owner_nodes()
                    .zip(self.author_stylesheets())
                    .take(STYLE_SHEETS)
                    .find(|(id, _)| *id == *owner)
                    .map(|(_, s)| s)
                    .ok_or(StyleReadError::Stale)?;
                let g = self.doc.guard().read();
                let contents = sheet.contents(&g);
                let mut list = contents.rules.clone();
                let mut last = None;
                for (i, index) in path.iter().enumerate() {
                    BoundedCss::check(start)?;
                    let rule = list
                        .read_with(&g)
                        .0
                        .get(*index as usize)
                        .cloned()
                        .ok_or(StyleReadError::Stale)?;
                    if matches!(query, StyleRead::Declarations { .. }) && i + 1 == path.len() {
                        last = Some(rule);
                        break;
                    }
                    list = nested(&rule, &g).ok_or(StyleReadError::Invalid)?;
                }
                if matches!(query, StyleRead::Declarations { .. }) {
                    match last {
                        Some(CssRule::Style(rule)) => declarations(
                            rule.read_with(&g).block.read_with(&g),
                            offset,
                            start,
                            &mut page,
                        )?,
                        _ => page.availability = Availability::Unsupported,
                    }
                } else {
                    let rules = &list.read_with(&g).0;
                    if offset as usize > rules.len() {
                        return Err(StyleReadError::Invalid);
                    }
                    page.total = rules.len().min(u32::MAX as usize) as u32;
                    for (index, rule) in rules
                        .iter()
                        .enumerate()
                        .skip(offset as usize)
                        .take(STYLE_PAGE)
                    {
                        BoundedCss::check(start)?;
                        let mut rule_path = path.clone();
                        rule_path.push(index as u32);
                        if rule_path.len() > STYLE_PATH {
                            return Err(StyleReadError::Depth);
                        }
                        let mut selector = None;
                        let mut source = None;
                        let mut declarations = 0;
                        let mut truncated = false;
                        let kind = match rule {
                            CssRule::Style(r) => {
                                let r = r.read_with(&g);
                                let mut out = BoundedCss::new(start);
                                let _ = cssparser::ToCss::to_css(&r.selectors, &mut out);
                                truncated = out.truncated;
                                selector = Some(out.text);
                                declarations = r
                                    .block
                                    .read_with(&g)
                                    .declarations()
                                    .len()
                                    .min(u32::MAX as usize)
                                    as u32;
                                let (url, t) = text(contents.url_data.as_str());
                                truncated |= t;
                                source = Some(SourceLocation {
                                    url,
                                    line: r.source_location.line + 1,
                                    column: r.source_location.column,
                                });
                                "style"
                            }
                            CssRule::Media(r) => {
                                let mut out = BoundedCss::new(start);
                                let _ = r
                                    .media_queries
                                    .read_with(&g)
                                    .to_css(&mut CssWriter::new(&mut out));
                                truncated = out.truncated;
                                selector = Some(out.text);
                                "media"
                            }
                            CssRule::Supports(_) => "supports",
                            CssRule::LayerBlock(_) => "layer",
                            CssRule::Import(_) => "import (child sheet unsupported)",
                            CssRule::FontFace(_) => "font-face (declarations unsupported)",
                            _ => "unsupported rule kind",
                        };
                        let children = nested(rule, &g)
                            .map_or(0, |r| r.read_with(&g).0.len().min(u32::MAX as usize) as u32);
                        page.items.push(StyleItem::Rule(RuleInfo {
                            path: rule_path,
                            kind: kind.into(),
                            selector,
                            children,
                            declarations,
                            source,
                            truncated,
                        }));
                    }
                    page.next = (offset + (page.items.len() as u32) < page.total)
                        .then_some(offset + page.items.len() as u32);
                }
            }
        }
        BoundedCss::check(start)?;
        Ok(page)
    }
}

pub(crate) struct DocumentRead<'a> {
    pub doc: &'a WegaDocument,
    pub native: Option<&'a NativeDocument>,
}
impl Deref for DocumentRead<'_> {
    type Target = WegaDocument;
    fn deref(&self) -> &WegaDocument {
        self.doc
    }
}
impl DocumentView for DocumentRead<'_> {
    type Node<'a>
        = wega_dom::document::DocumentNode<'a>
    where
        Self: 'a;
    fn node(&self, key: NodeKey) -> Option<Self::Node<'_>> {
        self.doc.node(key)
    }
    fn root(&self) -> NodeKey {
        self.doc.root()
    }
    fn base_url(&self) -> &Url {
        self.doc.base_url()
    }
    fn element_by_id(&self, id: &str) -> Option<NodeKey> {
        self.doc.element_by_id(id)
    }
}
impl MutationClock for DocumentRead<'_> {
    fn mutation_epoch(&self) -> MutationEpoch {
        self.doc.mutation_epoch()
    }
}
impl DocumentRead<'_> {
    fn layout_pending(&self) -> bool {
        self.native.is_none_or(|n| n.is_dirty())
    }
}
impl WegaDocument {
    fn guard(&self) -> &SharedRwLock {
        &self.guard
    }
    fn author_stylesheets(&self) -> impl Iterator<Item = &DocumentStyleSheet> + '_ {
        self.nodes_to_stylesheet.values()
    }
}
