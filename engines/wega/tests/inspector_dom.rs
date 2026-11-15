//! Real authoritative adapter reads, followed by real-pipe authorization checks.
mod support;
use support::inspector_worker::Worker;
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::{DocumentMutation, DocumentView, MutationClock, Mutator, NodeKey, NodeView};
use wega_engine::inspector::dom::inspect;
use wega_protocol::{
    DocumentContext, DocumentId, DocumentRevision,
    inspector::{dom::*, *},
};

fn stamp() -> DocumentContext {
    DocumentContext {
        document: DocumentId(1),
        revision: DocumentRevision(2),
    }
}
fn id(key: NodeKey) -> InspectorNodeId {
    InspectorNodeId {
        document: stamp(),
        key: key.as_u64(),
    }
}
fn doc(html: &str) -> BlitzDocument {
    BlitzDocument::from_html(html, DocumentSetup::default())
}
fn describe(doc: &BlitzDocument, key: NodeKey) -> Result<DomPage, InspectorError> {
    inspect(
        doc,
        stamp(),
        InspectorCommand::DescribeDomNode,
        &InspectorArgs::DomNode { node: id(key) },
    )
}
fn children(doc: &BlitzDocument, key: NodeKey, offset: u32, limit: u32) -> DomPage {
    inspect(
        doc,
        stamp(),
        InspectorCommand::GetDomChildren,
        &InspectorArgs::DomChildren {
            node: id(key),
            epoch: doc.mutation_epoch().as_u64(),
            offset,
            limit,
        },
    )
    .unwrap()
}
#[test]
fn metadata_attributes_own_text_and_comments_are_read_without_layout_or_mutation() {
    let d = doc(
        "<!doctype html><body><div id=a data-x='&quot;'><span>child</span>text<!--comment--></div><svg><circle id=c/></svg>",
    );
    let before = d.mutation_epoch();
    let a = d.element_by_id("a").unwrap();
    let value = &describe(&d, a).unwrap().nodes[0];
    assert_eq!(value.name, "div");
    assert_eq!(value.text, "");
    assert_eq!(value.attributes[1].value, "\"");
    let page = children(&d, a, 0, 64);
    assert_eq!(
        page.nodes.iter().map(|n| n.node_type).collect::<Vec<_>>(),
        vec![1, 3, 8]
    );
    assert_eq!(page.nodes[1].text, "text");
    assert_eq!(page.nodes[2].text, "comment");
    assert_eq!(d.mutation_epoch(), before);
    assert!(wega_dom::layout::GeometryAccess::layout_pending(&d));
}
#[test]
fn wide_pages_use_direct_offsets_and_json_byte_limits_even_with_escaped_attributes() {
    let attrs = (0..32)
        .map(|i| format!(" a{i}='{}'", "&#1;".repeat(256)))
        .collect::<String>();
    let html = format!(
        "<!doctype html><body><div id=a>{}</div>",
        format!("<i{attrs}>text</i>").repeat(1000)
    );
    let d = doc(&html);
    let a = d.element_by_id("a").unwrap();
    let mut offset = 0;
    let mut found = 0;
    loop {
        let p = children(&d, a, offset, 64);
        assert!(p.is_bounded());
        assert!(serde_json::to_vec(&p).unwrap().len() <= MAX_DOM_BYTES);
        assert!(!p.nodes.is_empty());
        found += p.nodes.len();
        if let Some(next) = p.next_offset {
            assert!(next > offset);
            offset = next;
        } else {
            break;
        }
    }
    assert_eq!(found, 1000);
    assert_eq!(children(&d, a, 999, 64).nodes.len(), 1);
}
#[test]
fn mutation_preserves_live_keys_but_invalidates_pagination_and_rejects_detached_and_reused_slots() {
    let mut d = doc("<!doctype html><body><div id=a>x</div><div id=b></div>");
    let a = d.element_by_id("a").unwrap();
    let b = d.element_by_id("b").unwrap();
    let epoch = d.mutation_epoch().as_u64();
    let old = id(a);
    d.mutate().append_children(b, &[a]).unwrap();
    assert_eq!(describe(&d, a).unwrap().nodes[0].id, old);
    assert_eq!(
        inspect(
            &d,
            stamp(),
            InspectorCommand::GetDomChildren,
            &InspectorArgs::DomChildren {
                node: old,
                epoch,
                offset: 0,
                limit: 64
            }
        )
        .unwrap_err()
        .code,
        InspectorErrorCode::StaleMutation
    );
    d.mutate().remove(a).unwrap();
    assert_eq!(
        describe(&d, a).unwrap_err().code,
        InspectorErrorCode::StaleNode
    );
    d.mutate().append_children(b, &[a]).unwrap();
    d.mutate().remove_and_drop(a).unwrap();
    let fresh = d.mutate().create_text("fresh");
    d.mutate().append_children(b, &[fresh]).unwrap();
    assert_ne!(a, fresh);
    assert_eq!(
        describe(&d, a).unwrap_err().code,
        InspectorErrorCode::StaleNode
    );
}
#[test]
fn document_stamps_and_disconnected_template_content_cannot_be_bypassed() {
    let d = doc(
        "<!doctype html><body><template id=t><i>secret</i></template><iframe src='https://foreign.test/'></iframe>",
    );
    let mut foreign = id(d.root());
    foreign.document.revision = DocumentRevision(99);
    assert_eq!(
        inspect(
            &d,
            stamp(),
            InspectorCommand::DescribeDomNode,
            &InspectorArgs::DomNode { node: foreign }
        )
        .unwrap_err()
        .code,
        InspectorErrorCode::StaleDocument
    );
    let t = d.element_by_id("t").unwrap();
    assert_eq!(children(&d, t, 0, 64).nodes.len(), 0);
    assert_eq!(
        describe(&d, NodeKey::from_u64(u64::MAX)).unwrap_err().code,
        InspectorErrorCode::StaleNode
    );
}
#[test]
fn deeply_nested_tree_is_bounded_and_no_inspector_read_flushes_it() {
    let d = doc(&format!(
        "<!doctype html><body>{}x{}",
        "<div>".repeat(1000),
        "</div>".repeat(1000)
    ));
    let mut at = d.root();
    for _ in 0..140 {
        at = d.node(at).unwrap().children().last().unwrap();
    }
    assert_eq!(
        describe(&d, at).unwrap_err().code,
        InspectorErrorCode::BudgetExceeded
    );
    assert!(wega_dom::layout::GeometryAccess::layout_pending(&d));
    assert!(
        inspect(
            &d,
            stamp(),
            InspectorCommand::GetDomRoot,
            &InspectorArgs::None
        )
        .unwrap()
        .is_bounded()
    );
}
#[test]
fn real_process_dom_is_session_scoped_and_never_runs_page_hooks() {
    let mut w = Worker::start();
    let s = w.attach();
    w.evaluate("document.body.innerHTML='<div id=inspect>text</div>';Object.defineProperty(document,'childNodes',{get(){throw Error('page hook')}});true").unwrap();
    let InspectorResponse::Dom(root) = w.call(s, InspectorCommand::GetDomRoot).unwrap() else {
        panic!()
    };
    assert!(root.is_bounded());
    let n = root.nodes[0].id;
    let args = InspectorArgs::DomChildren {
        node: n,
        epoch: root.epoch,
        offset: 0,
        limit: 64,
    };
    assert!(matches!(
        w.call_with(s, InspectorCommand::GetDomChildren, args),
        Ok(InspectorResponse::Dom(_))
    ));
    let mut wrong = n;
    wrong.document.revision = DocumentRevision(u64::MAX);
    assert_eq!(
        w.call_with(
            s,
            InspectorCommand::DescribeDomNode,
            InspectorArgs::DomNode { node: wrong }
        ),
        Err("StaleDocument".into())
    );
    w.call(s, InspectorCommand::Detach).unwrap();
    assert_eq!(
        w.call(s, InspectorCommand::GetDomRoot),
        Err("StaleSession".into())
    );
    assert!(w.alive());
}

#[test]
fn real_process_rejects_cross_tab_frame_process_and_pre_navigation_dom_ids() {
    use wega_protocol::{
        Command, Event,
        identity::{FrameId, ProcessSessionId, TabId},
    };
    let mut w = Worker::start();
    let s = w.attach();
    let InspectorResponse::Dom(root) = w.call(s, InspectorCommand::GetDomRoot).unwrap() else {
        panic!()
    };
    for target in [
        TargetId {
            tab: TabId(99),
            ..w.target()
        },
        TargetId {
            frame: FrameId(99),
            ..w.target()
        },
        TargetId {
            process: ProcessSessionId([99; 16]),
            ..w.target()
        },
    ] {
        let context = w.context();
        w.send(Command::Inspector {
            context,
            request: InspectorPayload::new(&InspectorRequest {
                version: INSPECTOR_VERSION,
                session: s,
                target,
                command: InspectorCommand::GetDomRoot,
                args: InspectorArgs::None,
            }),
        });
        assert_eq!(w.reply(context), Err("UnknownTarget".into()));
    }
    let old_context = w.context();
    w.navigate(1, "about:blank");
    w.until(|e| {
        matches!(
            e,
            Event::State {
                serial: 1,
                loading: false,
                ..
            }
        )
    });
    w.request_in(old_context, s, InspectorCommand::GetDomRoot);
    assert_eq!(w.reply(old_context), Err("failed:StaleDocument".into()));
    assert_eq!(
        w.call_with(
            s,
            InspectorCommand::DescribeDomNode,
            InspectorArgs::DomNode {
                node: root.nodes[0].id
            }
        ),
        Err("StaleDocument".into())
    );
    let mut replacement = Worker::start_with(7, 8, wega_protocol::VERSION);
    assert_eq!(
        replacement.call(s, InspectorCommand::GetDomRoot),
        Err("Unauthorized".into())
    );
}
