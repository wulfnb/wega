use super::*;
use wega::engine_protocol::{DocumentId, DocumentRevision};
fn doc() -> DocumentContext {
    DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(2),
    }
}
fn node(key: u64) -> DomNode {
    DomNode {
        id: InspectorNodeId {
            document: doc(),
            key,
        },
        node_type: if key == 1 { 9 } else { 1 },
        name: if key == 1 { "#document" } else { "div" }.into(),
        namespace: String::new(),
        prefix: String::new(),
        text: String::new(),
        child_count: 1000,
        attributes: vec![],
        truncated: false,
    }
}
fn complete(m: &mut ElementsView, read: Read, nodes: Vec<DomNode>, next: Option<u32>) {
    let (parent, offset) = match read {
        Read::Children { node, offset, .. } => (Some(node), offset),
        _ => (None, 0),
    };
    m.begin(read, Instant::now());
    m.complete(
        read.request().0,
        Ok(InspectorResponse::Dom(Box::new(DomPage {
            document: doc(),
            epoch: m.epoch.unwrap(),
            parent,
            offset,
            next_offset: next,
            nodes,
        }))),
    );
}
fn model() -> ElementsView {
    let mut m = ElementsView::default();
    m.observe(doc(), 5);
    complete(&mut m, Read::Root, vec![node(1)], None);
    m
}
#[test]
fn pages_expand_select_and_refresh_only_visible_branches_on_mutation() {
    let mut m = model();
    let read = m.next().unwrap();
    complete(&mut m, read, vec![node(2), node(3)], Some(2));
    m.rows.get_mut(&2).unwrap().expanded = true;
    m.select(node(3).id);
    assert!(matches!(m.next(), Some(Read::Describe(_))));
    complete(&mut m, Read::Describe(node(3).id), vec![node(3)], None);
    m.status_changed(doc(), 6);
    assert_eq!(m.selected, Some(node(3).id));
    assert!(matches!(m.next(), Some(Read::Describe(_))));
    // No subtree is copied, cleared or eagerly queued on an epoch change.
    assert_eq!(m.rows.len(), 3);
    assert_eq!(m.visible().len(), 3);
    assert!(m.pending.is_none());
    complete(&mut m, Read::Describe(node(3).id), vec![node(3)], None);
    assert!(matches!(m.next(), Some(Read::Root)));
}
#[test]
fn navigation_resets_ids_cache_selection_and_pending() {
    let mut m = model();
    m.select(node(1).id);
    m.begin(Read::Describe(node(1).id), Instant::now());
    let mut new = doc();
    new.revision = DocumentRevision(3);
    m.document(new);
    assert!(m.rows.is_empty() && m.selected.is_none() && m.pending.is_none());
    assert_eq!(m.document, Some(new));
}
#[test]
fn row_byte_edge_and_pending_limits_survive_repeated_pagination() {
    let mut m = model();
    for i in 0..20 {
        let read = Read::Children {
            node: node(1).id,
            epoch: 5,
            offset: i * 64,
        };
        let nodes = (2 + i as u64 * 64..66 + i as u64 * 64).map(node).collect();
        complete(&mut m, read, nodes, Some((i + 1) * 64));
        assert!(m.rows.len() <= CACHE_NODES && m.bytes <= CACHE_BYTES);
        assert!(m.rows.values().map(|r| r.children.len()).sum::<usize>() <= CACHE_NODES);
    }
    assert!(m.next().is_none());
    assert!(m.notice.contains("limit"));
    m.clear_cache();
    assert!(matches!(m.next(), Some(Read::Root)));
    let now = Instant::now();
    m.begin(Read::Root, now);
    assert!(m.next().is_none());
    m.expire(now + Duration::from_secs(3));
    assert!(m.pending.is_none());
    assert!(m.next().is_none());
    m.status_changed(doc(), 5);
    assert!(m.next().is_some());
}
#[test]
fn mutation_race_foreign_responses_and_removed_selection_fail_safely() {
    let mut m = model();
    let read = m.next().unwrap();
    m.begin(read, Instant::now());
    m.complete(
        read.request().0,
        Err(InspectorError::new(
            InspectorErrorCode::StaleMutation,
            "changed",
        )),
    );
    assert!(m.next().is_none());
    m.status_changed(doc(), 6);
    assert!(m.next().is_some());
    m.select(node(2).id);
    m.begin(Read::Describe(node(2).id), Instant::now());
    m.complete(
        InspectorCommand::DescribeDomNode,
        Err(InspectorError::new(
            InspectorErrorCode::StaleNode,
            "removed",
        )),
    );
    assert!(m.selected.is_none());
    assert!(m.rows.is_empty());
    m.begin(Read::Root, Instant::now());
    let mut foreign = doc();
    foreign.document = DocumentId(99);
    m.complete(
        InspectorCommand::GetDomRoot,
        Ok(InspectorResponse::Dom(Box::new(DomPage {
            document: foreign,
            epoch: 6,
            parent: None,
            offset: 0,
            next_offset: None,
            nodes: vec![node(1)],
        }))),
    );
    assert!(m.rows.is_empty());
}
#[test]
fn panel_draws_expansion_selection_previews_and_limits_without_html_execution() {
    let mut m = model();
    let read = m.next().unwrap();
    let mut child = node(2);
    child.text = "<script>evil()</script>".into();
    child.truncated = true;
    child.attributes.push(DomAttribute {
        name: "id".into(),
        namespace: String::new(),
        value: "selected".into(),
    });
    complete(&mut m, read, vec![child], None);
    m.select(node(2).id);
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        crate::devtools_elements_ui::draw(ui, &mut m)
    });
    let texts = out
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some(t.galley.job.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        texts.contains("Read-only DOM")
            && texts.contains("Selected:")
            && texts.contains("Preview truncated")
    );
    out.textures_delta.clear();
}
