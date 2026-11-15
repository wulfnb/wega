//! A bounded presentation cache. The content document is authoritative; this cache
//! owns only protocol values and drops everything when its document/session changes.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use wega::engine_protocol::{
    DocumentContext,
    inspector::{dom::*, *},
};

pub(crate) const CACHE_NODES: usize = 512;
pub(crate) const CACHE_BYTES: usize = 512 * 1024;
#[derive(Debug)]
pub(crate) struct Row {
    pub node: DomNode,
    pub expanded: bool,
    pub children: Vec<u64>,
    pub next: Option<u32>,
    pub observed_epoch: u64,
    epoch: Option<u64>,
    bytes: usize,
}

#[cfg(test)]
#[path = "devtools_elements_tests.rs"]
mod tests;
#[derive(Clone, Copy, Debug)]
pub(crate) enum Read {
    Root,
    Children {
        node: InspectorNodeId,
        epoch: u64,
        offset: u32,
    },
    Describe(InspectorNodeId),
}
impl Read {
    pub fn request(self) -> (InspectorCommand, InspectorArgs) {
        match self {
            Self::Root => (InspectorCommand::GetDomRoot, InspectorArgs::None),
            Self::Children {
                node,
                epoch,
                offset,
            } => (
                InspectorCommand::GetDomChildren,
                InspectorArgs::DomChildren {
                    node,
                    epoch,
                    offset,
                    limit: MAX_DOM_NODES as u32,
                },
            ),
            Self::Describe(node) => (
                InspectorCommand::DescribeDomNode,
                InspectorArgs::DomNode { node },
            ),
        }
    }
}
#[derive(Debug, Default)]
pub(crate) struct ElementsView {
    pub document: Option<DocumentContext>,
    pub epoch: Option<u64>,
    pub rows: BTreeMap<u64, Row>,
    pub root: Option<u64>,
    pub selected: Option<InspectorNodeId>,
    selected_epoch: Option<u64>,
    root_epoch: Option<u64>,
    pub pending: Option<Read>,
    pending_since: Option<Instant>,
    pub notice: String,
    bytes: usize,
    blocked: bool,
    retry_on_status: bool,
    more: Option<u64>,
}
impl ElementsView {
    pub fn begin(&mut self, read: Read, now: Instant) {
        self.pending = Some(read);
        self.pending_since = Some(now);
    }
    pub fn expire(&mut self, now: Instant) {
        if self.pending_since.is_some_and(|at| {
            now.saturating_duration_since(at) >= Duration::from_millis(INSPECTOR_TIMEOUT_MS + 100)
        }) {
            if let Some(read) = self.pending {
                self.complete(
                    read.request().0,
                    Err(InspectorError::new(
                        InspectorErrorCode::Timeout,
                        "DOM request timed out",
                    )),
                );
            }
        }
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn document(&mut self, document: DocumentContext) {
        if self.document != Some(document) {
            self.reset();
            self.document = Some(document);
        }
    }
    pub fn observe(&mut self, document: DocumentContext, epoch: u64) {
        self.document(document);
        // Compare only for inequality, including wrap. Cached values retain their
        // epochs; only visible expanded pages are refreshed, one per response.
        self.epoch = Some(epoch);
    }
    pub fn clear_cache(&mut self) {
        self.rows.clear();
        self.root = None;
        self.bytes = 0;
        self.root_epoch = None;
        self.selected = None;
        self.selected_epoch = None;
        self.blocked = false;
        self.more = None;
        self.notice.clear();
        // Keep an outstanding request correlated until it completes.
    }
    pub fn select(&mut self, id: InspectorNodeId) {
        if self.document == Some(id.document) {
            self.selected = Some(id);
            self.selected_epoch = None;
        }
    }
    pub fn more(&mut self, key: u64) {
        if self.rows.contains_key(&key) {
            self.more = Some(key);
        }
    }
    pub fn next(&self) -> Option<Read> {
        if self.pending.is_some() || self.blocked {
            return None;
        }
        let epoch = self.epoch?;
        if let Some(id) = self.selected
            && self.selected_epoch != Some(epoch)
        {
            return Some(Read::Describe(id));
        }
        if self.root.is_none() || self.root_epoch != Some(epoch) {
            return Some(Read::Root);
        }
        for (key, depth) in self.visible() {
            let row = self.rows.get(&key)?;
            if row.expanded && depth < MAX_DOM_DEPTH && row.node.child_count > 0 {
                if row.epoch != Some(epoch) {
                    return Some(Read::Children {
                        node: row.node.id,
                        epoch,
                        offset: 0,
                    });
                }
                if self.more == Some(key)
                    && let Some(offset) = row.next
                {
                    return Some(Read::Children {
                        node: row.node.id,
                        epoch,
                        offset,
                    });
                }
            }
        }
        None
    }
    /// Iterative flattening, bounded by cached rows and absolute depth.
    pub fn visible(&self) -> Vec<(u64, usize)> {
        let mut out = Vec::new();
        let mut stack = self.root.map(|r| vec![(r, 0)]).unwrap_or_default();
        while let Some((key, depth)) = stack.pop() {
            if out.len() >= CACHE_NODES || depth > MAX_DOM_DEPTH {
                break;
            }
            let Some(row) = self.rows.get(&key) else {
                continue;
            };
            out.push((key, depth));
            if row.expanded {
                stack.extend(row.children.iter().rev().map(|k| (*k, depth + 1)));
            }
        }
        out
    }
    pub fn complete(
        &mut self,
        command: InspectorCommand,
        result: Result<InspectorResponse, InspectorError>,
    ) {
        let Some(read) = self.pending else { return };
        if read.request().0 != command {
            return;
        }
        self.pending = None;
        self.pending_since = None;
        let page = match result {
            Ok(InspectorResponse::Dom(page))
                if page.is_bounded() && Some(page.document) == self.document =>
            {
                page
            }
            Err(error) => {
                self.notice = error.message;
                match error.code {
                    InspectorErrorCode::StaleMutation
                    | InspectorErrorCode::Timeout
                    | InspectorErrorCode::Busy => {
                        self.blocked = true;
                        self.retry_on_status = true;
                    }
                    InspectorErrorCode::StaleNode => {
                        self.selected = None;
                        self.selected_epoch = None;
                        self.clear_cache();
                        self.notice = "Selected node was removed; refreshing the tree".into();
                    }
                    _ => self.blocked = true,
                }
                return;
            }
            _ => {
                self.notice = "Invalid or stale DOM response".into();
                self.blocked = true;
                return;
            }
        };
        // A status response may overtake a page request. Never merge pages from
        // different mutation epochs; the next poll supplies a consistent cursor.
        if Some(page.epoch) != self.epoch {
            self.notice = "DOM changed; waiting for the next status sample".into();
            self.blocked = true;
            self.retry_on_status = true;
            return;
        }
        let shape = match read {
            Read::Root => {
                page.parent.is_none() && page.nodes.len() == 1 && page.nodes[0].node_type == 9
            }
            Read::Describe(_) => page.parent.is_none() && page.nodes.len() == 1,
            Read::Children {
                node,
                epoch,
                offset,
            } => page.parent == Some(node) && page.epoch == epoch && page.offset == offset,
        };
        if !shape {
            self.notice = "Mismatched DOM response".into();
            self.blocked = true;
            return;
        }
        if let Read::Describe(node) = read
            && page.nodes[0].id != node
        {
            self.blocked = true;
            return;
        }
        if let Read::Children { node, offset, .. } = read {
            let edges: usize = self.rows.values().map(|r| r.children.len()).sum();
            let replaced = if offset == 0 {
                self.rows.get(&node.key).map_or(0, |r| r.children.len())
            } else {
                0
            };
            if edges - replaced + page.nodes.len() > CACHE_NODES {
                self.notice = "Elements cache limit reached. Reset the tree to continue.".into();
                self.blocked = true;
                return;
            }
        }
        let mut count = self.rows.len();
        let mut bytes = self.bytes;
        for node in &page.nodes {
            if let Some(old) = self.rows.get(&node.id.key) {
                bytes = bytes.saturating_sub(old.bytes);
            } else {
                count += 1;
            }
            bytes += serde_json::to_vec(node).map_or(CACHE_BYTES + 1, |s| s.len());
        }
        if count > CACHE_NODES || bytes > CACHE_BYTES {
            self.notice =
                "Elements cache limit reached. Reset the tree to inspect another branch.".into();
            self.blocked = true;
            return;
        }
        let keys: Vec<u64> = page.nodes.iter().map(|n| n.id.key).collect();
        for node in page.nodes {
            let key = node.id.key;
            let size = serde_json::to_vec(&node).map_or(0, |s| s.len());
            if let Some(row) = self.rows.get_mut(&key) {
                row.node = node;
                row.bytes = size;
                row.observed_epoch = page.epoch;
            } else {
                self.rows.insert(
                    key,
                    Row {
                        node,
                        expanded: false,
                        children: vec![],
                        next: None,
                        epoch: None,
                        observed_epoch: page.epoch,
                        bytes: size,
                    },
                );
            }
        }
        self.bytes = bytes;
        match read {
            Read::Root => {
                let first = self.root.is_none();
                self.root = keys.first().copied();
                self.root_epoch = Some(page.epoch);
                if first && let Some(row) = self.root.and_then(|k| self.rows.get_mut(&k)) {
                    row.expanded = true;
                }
            }
            Read::Describe(_) => self.selected_epoch = Some(page.epoch),
            Read::Children { node, offset, .. } => {
                if let Some(parent) = self.rows.get_mut(&node.key) {
                    if offset == 0 {
                        parent.children.clear();
                    }
                    // Total edge storage is also bounded, even for forged duplicates.
                    for key in keys {
                        if key != node.key
                            && !parent.children.contains(&key)
                            && parent.children.len() < CACHE_NODES
                        {
                            parent.children.push(key);
                        }
                    }
                    parent.next = page.next_offset;
                    parent.epoch = Some(page.epoch);
                }
                self.more = None;
            }
        }
        self.notice.clear();
    }
    pub fn status_changed(&mut self, document: DocumentContext, epoch: u64) {
        if self.epoch != Some(epoch) || self.retry_on_status {
            self.blocked = false;
            self.retry_on_status = false;
        }
        self.observe(document, epoch);
    }
}
