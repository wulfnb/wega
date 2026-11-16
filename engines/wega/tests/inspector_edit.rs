//! Real-process tests of D3 geometry, picking and authorized editing: a real `wega-engine`
//! worker driven over the real pipe, with the browser's own session types on the host side.
mod support;
use support::inspector_worker::Worker;
use wega_protocol::inspector::{
    dom::{DomNode, InspectorNodeId},
    edit::MutationOp,
    geometry::{GeometryState, PaintMapping, UnsupportedPaint, to_milli},
    *,
};

type Detail = Result<InspectorResponse, (String, String)>;
const MUT: GrantToken = GrantToken([0x4D; 16]);
const EVAL: GrantToken = GrantToken([0x45; 16]);

fn code(result: &Detail) -> String {
    match result {
        Ok(_) => "Ok".into(),
        Err((code, _)) => code.clone(),
    }
}
fn grant(
    w: &mut Worker,
    s: InspectorSessionId,
    command: InspectorCommand,
    token: GrantToken,
) -> Detail {
    w.call_detail(
        s,
        command,
        InspectorArgs::Grant {
            token,
            opaque_override: true,
            origin: None,
        },
    )
}
fn page(html: &str) -> (Worker, InspectorSessionId) {
    let mut w = Worker::start();
    let s = w.attach();
    w.evaluate(&format!("document.body.innerHTML={html:?};true"))
        .unwrap();
    (w, s)
}
fn children(w: &mut Worker, s: InspectorSessionId, node: InspectorNodeId) -> Vec<DomNode> {
    let epoch = match w.call(s, InspectorCommand::GetDomRoot) {
        Ok(InspectorResponse::Dom(p)) => p.epoch,
        other => panic!("{other:?}"),
    };
    match w.call_with(
        s,
        InspectorCommand::GetDomChildren,
        InspectorArgs::DomChildren {
            node,
            epoch,
            offset: 0,
            limit: 64,
        },
    ) {
        Ok(InspectorResponse::Dom(p)) => p.nodes,
        other => panic!("{other:?}"),
    }
}
/// Breadth-first search by `id` attribute through the inspector's own DOM reads.
fn find(w: &mut Worker, s: InspectorSessionId, id: &str) -> DomNode {
    let root = match w.call(s, InspectorCommand::GetDomRoot) {
        Ok(InspectorResponse::Dom(p)) => p.nodes[0].id,
        other => panic!("{other:?}"),
    };
    let mut queue = vec![root];
    while let Some(next) = queue.pop() {
        for child in children(w, s, next) {
            if child
                .attributes
                .iter()
                .any(|a| a.name == "id" && a.value == id)
            {
                return child;
            }
            if child.child_count > 0 && child.node_type == 1 {
                queue.push(child.id);
            }
        }
    }
    panic!("no #{id}")
}
fn geometry(
    w: &mut Worker,
    s: InspectorSessionId,
    node: InspectorNodeId,
) -> geometry::GeometryReport {
    match w.call_with(
        s,
        InspectorCommand::GetGeometry,
        InspectorArgs::DomNode { node },
    ) {
        Ok(InspectorResponse::Geometry(r)) => {
            assert!(r.is_bounded());
            *r
        }
        other => panic!("{other:?}"),
    }
}
fn epoch(w: &mut Worker, s: InspectorSessionId) -> u64 {
    w.snapshot(s).mutation_epoch
}
fn mutate(
    w: &mut Worker,
    s: InspectorSessionId,
    grant: GrantToken,
    node: InspectorNodeId,
    epoch: u64,
    op: MutationOp,
) -> Detail {
    w.call_detail(
        s,
        InspectorCommand::Mutate,
        InspectorArgs::Mutate {
            grant,
            node,
            epoch,
            op,
        },
    )
}
fn enable_editing(w: &mut Worker, s: InspectorSessionId) {
    assert_eq!(
        grant(w, s, InspectorCommand::GrantMutation, MUT),
        Ok(InspectorResponse::MutationGranted)
    );
}

const BOX: &str = "<div id=box style='margin:10px;padding:5px;border:2px solid;width:100px;height:50px'>hello</div>\
<div id=tr style='transform:translate(5px,5px);width:10px;height:10px'></div>\
<div id=clip style='width:30px;height:10px;overflow:hidden'><div id=wide style='width:99px;height:5px'></div></div>";

#[test]
fn geometry_is_pending_or_stale_until_the_page_lays_out_and_is_never_forced_by_the_inspector() {
    let (mut w, s) = page(BOX);
    let node = find(&mut w, s, "box");
    let before = geometry(&mut w, s, node.id);
    assert_ne!(
        before.state,
        GeometryState::Current,
        "nothing laid out the new content yet"
    );
    assert!(before.boxes.is_none() || before.state == GeometryState::Stale);
    // Reading again does not change it: the inspector never flushes.
    assert_eq!(geometry(&mut w, s, node.id).state, before.state);
    // The page's own script reads layout; now it is current.
    w.evaluate("document.getElementById('box').getBoundingClientRect().width")
        .unwrap();
    let after = geometry(&mut w, s, node.id);
    assert_eq!(after.state, GeometryState::Current);
    let boxes = after.boxes.unwrap();
    assert_eq!(boxes.border_box.width, 100.0 + 10.0 + 4.0);
    assert_eq!(boxes.margin.top, 10.0);
    assert_eq!(boxes.content_box().width, 100.0);
    assert_eq!(after.label.id, "box");
    assert_eq!(after.label.tag, "div");
    assert!(matches!(after.paint, PaintMapping::Exact { clip: None }));
    assert!(w.alive());
}

#[test]
fn transforms_and_clips_are_reported_explicitly_never_as_accurate_geometry() {
    let (mut w, s) = page(BOX);
    w.evaluate("document.body.getBoundingClientRect().height")
        .unwrap();
    let tr = find(&mut w, s, "tr");
    assert_eq!(
        geometry(&mut w, s, tr.id).paint,
        PaintMapping::Unsupported {
            reason: UnsupportedPaint::Transform
        }
    );
    let wide = find(&mut w, s, "wide");
    let report = geometry(&mut w, s, wide.id);
    let PaintMapping::Exact { clip: Some(clip) } = report.paint else {
        panic!("{:?}", report.paint)
    };
    assert_eq!(clip.width, 30.0);
    assert!(report.boxes.unwrap().border_box.width > clip.width);
}

#[test]
fn picking_hits_the_topmost_element_without_running_page_script_or_flushing() {
    let (mut w, s) = page(BOX);
    w.evaluate("document.body.getBoundingClientRect().height;window.__moved=0;document.addEventListener('mousemove',()=>window.__moved++);true").unwrap();
    let node = find(&mut w, s, "box");
    let boxes = geometry(&mut w, s, node.id).boxes.unwrap();
    let (cx, cy) = (
        boxes.border_box.x + boxes.border_box.width / 2.0,
        boxes.border_box.y + boxes.border_box.height / 2.0,
    );
    let result = w.call_with(
        s,
        InspectorCommand::PickNode,
        InspectorArgs::Pick {
            x_milli: to_milli(cx as f32),
            y_milli: to_milli(cy as f32),
        },
    );
    let Ok(InspectorResponse::Picked(pick)) = result else {
        panic!("{result:?}")
    };
    assert!(pick.is_bounded());
    assert_eq!(
        pick.target.node, node.id,
        "the pick selects the element under the point"
    );
    assert_eq!(pick.target.state, GeometryState::Current);
    assert!(!pick.ancestors.is_empty(), "root-first ancestors");
    // The pick dispatched no page event and ran no script.
    assert_eq!(w.evaluate("window.__moved").unwrap(), "0");
    // A stale document is refused.
    let mut wrong = node.id;
    wrong.document.revision = wega_protocol::DocumentRevision(u64::MAX);
    assert_eq!(
        code(&w.call_detail(
            s,
            InspectorCommand::GetGeometry,
            InspectorArgs::DomNode { node: wrong }
        )),
        "StaleDocument"
    );
}

#[test]
fn editing_needs_its_own_grant_an_evaluation_grant_does_not_authorise_it() {
    let (mut w, s) = page(BOX);
    let node = find(&mut w, s, "box");
    let op = || MutationOp::SetAttribute {
        name: "data-x".into(),
        value: "1".into(),
    };
    let e = epoch(&mut w, s);
    // No grant at all.
    assert_eq!(
        code(&mutate(&mut w, s, MUT, node.id, e, op())),
        "Unauthorized"
    );
    // An evaluation grant is not a mutation grant.
    assert!(grant(&mut w, s, InspectorCommand::GrantEvaluation, EVAL).is_ok());
    assert_eq!(
        code(&mutate(&mut w, s, EVAL, node.id, e, op())),
        "Unauthorized"
    );
    assert_eq!(
        code(&mutate(&mut w, s, MUT, node.id, e, op())),
        "Unauthorized"
    );
    // The mutation grant does not authorise evaluation either.
    enable_editing(&mut w, s);
    let result = w.call_detail(
        s,
        InspectorCommand::Evaluate,
        InspectorArgs::Evaluate {
            expression: "1".into(),
            grant: MUT,
        },
    );
    assert_eq!(code(&result), "Unauthorized");
    // A forged token is refused; the right one works.
    assert_eq!(
        code(&mutate(&mut w, s, GrantToken([9; 16]), node.id, e, op())),
        "Unauthorized"
    );
    assert!(mutate(&mut w, s, MUT, node.id, e, op()).is_ok());
    assert_eq!(
        w.evaluate("document.getElementById('box').getAttribute('data-x')")
            .unwrap(),
        "\"1\""
    );
    // Revocation is immediate.
    assert!(matches!(
        w.call(s, InspectorCommand::RevokeMutation),
        Ok(InspectorResponse::MutationRevoked)
    ));
    let e = epoch(&mut w, s);
    assert_eq!(
        code(&mutate(&mut w, s, MUT, node.id, e, op())),
        "Unauthorized"
    );
}

#[test]
fn edits_apply_to_the_authoritative_document_and_page_script_sees_them() {
    let (mut w, s) = page(BOX);
    w.evaluate("document.body.getBoundingClientRect().height")
        .unwrap();
    enable_editing(&mut w, s);
    let node = find(&mut w, s, "box");
    let text = children(&mut w, s, node.id)
        .into_iter()
        .find(|n| n.node_type == 3)
        .unwrap();
    let run = |w: &mut Worker, id, op| {
        let e = epoch(w, s);
        mutate(w, s, MUT, id, e, op)
    };
    // Attribute add, change, remove.
    let r = run(
        &mut w,
        node.id,
        MutationOp::SetAttribute {
            name: "title".into(),
            value: "t1".into(),
        },
    );
    let Ok(InspectorResponse::Mutated(out)) = r else {
        panic!("{r:?}")
    };
    assert!(out.changed);
    assert_eq!(
        w.evaluate("document.getElementById('box').title").unwrap(),
        "\"t1\""
    );
    assert!(
        run(
            &mut w,
            node.id,
            MutationOp::SetAttribute {
                name: "title".into(),
                value: "t2".into()
            }
        )
        .is_ok()
    );
    assert_eq!(
        w.evaluate("document.getElementById('box').title").unwrap(),
        "\"t2\""
    );
    assert!(
        run(
            &mut w,
            node.id,
            MutationOp::RemoveAttribute {
                name: "title".into()
            }
        )
        .is_ok()
    );
    assert_eq!(
        w.evaluate("document.getElementById('box').hasAttribute('title')")
            .unwrap(),
        "false"
    );
    // Text.
    assert!(run(&mut w, text.id, MutationOp::SetText { text: "bye".into() }).is_ok());
    assert_eq!(
        w.evaluate("document.getElementById('box').textContent")
            .unwrap(),
        "\"bye\""
    );
    // Inline style, with and without !important, and removal.
    assert!(
        run(
            &mut w,
            node.id,
            MutationOp::SetInlineProperty {
                property: "width".into(),
                value: "200px".into(),
                important: false
            }
        )
        .is_ok()
    );
    assert_eq!(
        w.evaluate("document.getElementById('box').getBoundingClientRect().width")
            .unwrap(),
        "214"
    );
    assert!(
        run(
            &mut w,
            node.id,
            MutationOp::SetInlineProperty {
                property: "color".into(),
                value: "red".into(),
                important: true
            }
        )
        .is_ok()
    );
    assert_eq!(
        w.evaluate("document.getElementById('box').style.getPropertyPriority('color')")
            .unwrap(),
        "\"important\""
    );
    let ignored = run(
        &mut w,
        node.id,
        MutationOp::SetInlineProperty {
            property: "no-such-prop".into(),
            value: "1".into(),
            important: false,
        },
    );
    let Ok(InspectorResponse::Mutated(out)) = ignored else {
        panic!("{ignored:?}")
    };
    assert!(!out.changed, "an ignored CSS write reports no change");
    assert!(
        run(
            &mut w,
            node.id,
            MutationOp::SetInlineProperty {
                property: "width".into(),
                value: String::new(),
                important: false
            }
        )
        .is_ok()
    );
    assert_eq!(
        w.evaluate("document.getElementById('box').style.width")
            .unwrap(),
        "\"\""
    );
    // Geometry follows: stale right after the edit, current once the page lays out again.
    assert_eq!(geometry(&mut w, s, node.id).state, GeometryState::Stale);
    w.evaluate("document.body.getBoundingClientRect().height")
        .unwrap();
    assert_eq!(geometry(&mut w, s, node.id).state, GeometryState::Current);
    // Removal.
    assert!(run(&mut w, node.id, MutationOp::RemoveNode).is_ok());
    assert_eq!(
        w.evaluate("document.getElementById('box')===null").unwrap(),
        "true"
    );
    // Removed nodes are stale for the next edit.
    assert_eq!(
        code(&run(
            &mut w,
            node.id,
            MutationOp::RemoveAttribute { name: "id".into() }
        )),
        "StaleNode"
    );
}

#[test]
fn stale_epochs_protected_nodes_and_code_running_edits_are_refused_without_side_effects() {
    let (mut w, s) = page(BOX);
    enable_editing(&mut w, s);
    let node = find(&mut w, s, "box");
    let old = epoch(&mut w, s);
    // A script edit moves the epoch: the stale native edit is refused and applies nothing.
    w.evaluate("document.getElementById('box').setAttribute('data-a','1')")
        .unwrap();
    let r = mutate(
        &mut w,
        s,
        MUT,
        node.id,
        old,
        MutationOp::SetAttribute {
            name: "data-b".into(),
            value: "2".into(),
        },
    );
    assert_eq!(code(&r), "StaleMutation");
    assert_eq!(
        w.evaluate("document.getElementById('box').hasAttribute('data-b')")
            .unwrap(),
        "false"
    );
    let e = epoch(&mut w, s);
    // html, head and body cannot be removed; the document cannot either.
    for tag in ["body", "html", "head"] {
        let n = {
            let root = match w.call(s, InspectorCommand::GetDomRoot) {
                Ok(InspectorResponse::Dom(p)) => p.nodes[0].id,
                o => panic!("{o:?}"),
            };
            let mut stack = vec![root];
            let mut found = None;
            while let Some(x) = stack.pop() {
                for c in children(&mut w, s, x) {
                    if c.name == tag {
                        found = Some(c.id);
                    }
                    if c.node_type == 1 {
                        stack.push(c.id);
                    }
                }
            }
            found.unwrap()
        };
        assert_eq!(
            code(&mutate(&mut w, s, MUT, n, e, MutationOp::RemoveNode)),
            "InvalidPayload",
            "{tag}"
        );
    }
    // Event handlers and javascript: URLs run code: they need the evaluation grant too.
    for (name, value) in [
        ("onclick", "window.x=1"),
        ("href", " JavaScript:alert(1)"),
        ("srcdoc", "<p>"),
    ] {
        let r = mutate(
            &mut w,
            s,
            MUT,
            node.id,
            e,
            MutationOp::SetAttribute {
                name: name.into(),
                value: value.into(),
            },
        );
        assert_eq!(code(&r), "PolicyDenied", "{name}");
    }
    assert_eq!(
        w.evaluate("document.getElementById('box').hasAttribute('onclick')")
            .unwrap(),
        "false"
    );
    assert!(grant(&mut w, s, InspectorCommand::GrantEvaluation, EVAL).is_ok());
    let e = epoch(&mut w, s);
    assert!(
        mutate(
            &mut w,
            s,
            MUT,
            node.id,
            e,
            MutationOp::SetAttribute {
                name: "onclick".into(),
                value: "window.x=1".into()
            }
        )
        .is_ok()
    );
    // Malformed and oversized operations never reach the document.
    for bad in [
        MutationOp::SetAttribute {
            name: "a b".into(),
            value: "".into(),
        },
        MutationOp::SetAttribute {
            name: "a".into(),
            value: "x".repeat(5000),
        },
        MutationOp::SetInlineProperty {
            property: "co;lor".into(),
            value: "red".into(),
            important: false,
        },
    ] {
        let e = epoch(&mut w, s);
        assert_eq!(
            code(&mutate(&mut w, s, MUT, node.id, e, bad)),
            // Rejected while decoding, like every other malformed inspector payload.
            "failed:InvalidRequest"
        );
    }
    // Element text is edited through its text node, not the element.
    let e = epoch(&mut w, s);
    assert_eq!(
        code(&mutate(
            &mut w,
            s,
            MUT,
            node.id,
            e,
            MutationOp::SetText { text: "x".into() }
        )),
        "InvalidPayload"
    );
    assert!(w.alive());
}

#[test]
fn text_budget_is_shared_with_script_and_exhausting_it_refuses_the_edit() {
    let (mut w, s) = page(BOX);
    enable_editing(&mut w, s);
    let node = find(&mut w, s, "box");
    // Script spends the document's text budget down to almost nothing.
    w.evaluate("(()=>{try{const el=document.getElementById('box');for(let i=0;i<100000;i++)el.setAttribute('data-'+(i%50),'x'.repeat(1000)+i)}catch(e){}return true})()").unwrap();
    let mut refused = false;
    for _ in 0..4000 {
        let e = epoch(&mut w, s);
        let r = mutate(
            &mut w,
            s,
            MUT,
            node.id,
            e,
            MutationOp::SetAttribute {
                name: "data-n".into(),
                value: "y".repeat(4000),
            },
        );
        if code(&r) == "BudgetExceeded" {
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "native edits charge the same text budget as script"
    );
    assert!(w.alive());
}

#[test]
fn editing_authority_dies_with_navigation_detach_and_a_replacement_session() {
    let (mut w, s) = page(BOX);
    enable_editing(&mut w, s);
    let node = find(&mut w, s, "box");
    let e = epoch(&mut w, s);
    let op = || MutationOp::SetAttribute {
        name: "data-x".into(),
        value: "1".into(),
    };
    assert!(mutate(&mut w, s, MUT, node.id, e, op()).is_ok());
    // Navigation: the old node id and the grant are both gone.
    w.navigate(1, "about:blank");
    w.pump(std::time::Duration::from_millis(200));
    let r = mutate(&mut w, s, MUT, node.id, e, op());
    assert!(
        matches!(code(&r).as_str(), "StaleDocument" | "Unauthorized"),
        "{r:?}"
    );
    let root = match w.call(s, InspectorCommand::GetDomRoot) {
        Ok(InspectorResponse::Dom(p)) => p.nodes[0].id,
        o => panic!("{o:?}"),
    };
    let e = epoch(&mut w, s);
    assert_eq!(
        code(&mutate(&mut w, s, MUT, root, e, op())),
        "Unauthorized",
        "no grant on the new document"
    );
    // A new session never inherits it.
    enable_editing(&mut w, s);
    let s2 = w.attach_replacing(s);
    assert_eq!(
        code(&mutate(&mut w, s2, MUT, root, e, MutationOp::RemoveNode)),
        "Unauthorized"
    );
    // Detach drops everything; the worker survives all of it.
    w.call(s2, InspectorCommand::Detach).unwrap();
    assert_eq!(
        code(&mutate(&mut w, s2, MUT, root, e, MutationOp::RemoveNode)),
        "StaleSession"
    );
    assert!(w.alive());
}

#[test]
fn a_page_script_cannot_reach_inspector_operations() {
    let (mut w, _s) = page(BOX);
    let names = w
        .evaluate("JSON.stringify(Object.getOwnPropertyNames(globalThis).filter(n=>/inspect|devtools|grant|mutat|__wega/i.test(n)))")
        .unwrap();
    assert!(
        !names.contains("grant") && !names.contains("mutat"),
        "{names}"
    );
}

#[test]
fn the_real_worker_refuses_a_deep_document_stays_responsive_and_renders_the_next_page() {
    let mut w = Worker::start();
    let s = w.attach();
    // 1,000 nested blocks through the page's own script, then a layout read from script.
    w.evaluate("document.body.innerHTML='<div>'.repeat(1000)+'x'+'</div>'.repeat(1000);true")
        .unwrap();
    assert_eq!(
        w.evaluate("document.body.getBoundingClientRect().height")
            .unwrap(),
        "0",
        "the refused document has no layout"
    );
    // Its geometry is reported as refused (not hung, not invented), and inspection still works.
    let root = match w.call(s, InspectorCommand::GetDomRoot) {
        Ok(InspectorResponse::Dom(p)) => p.nodes[0].id,
        other => panic!("{other:?}"),
    };
    let report = geometry(&mut w, s, root);
    assert!(
        matches!(
            report.state,
            GeometryState::Refused | GeometryState::Pending
        ),
        "{:?}",
        report.state
    );
    assert!(w.alive());
    // The same view recovers with an ordinary page.
    w.evaluate("document.body.innerHTML='<div id=ok style=\"height:77px\">ok</div>';true")
        .unwrap();
    assert_eq!(
        w.evaluate("document.getElementById('ok').getBoundingClientRect().height")
            .unwrap(),
        "77"
    );
    assert!(w.alive());
}

#[test]
fn native_edits_are_visible_to_page_script_but_do_not_produce_mutation_observer_records() {
    // Pins the documented limitation: shared state, no script-side observer records.
    let (mut w, s) = page(BOX);
    enable_editing(&mut w, s);
    let node = find(&mut w, s, "box");
    w.evaluate("window.__records=0;new MutationObserver(r=>{window.__records+=r.length}).observe(document.getElementById('box'),{attributes:true});document.getElementById('box').setAttribute('data-s','1');true").unwrap();
    w.evaluate("Promise.resolve().then(()=>0)").ok();
    assert_eq!(
        w.evaluate("window.__records").unwrap(),
        "1",
        "script edits are observed"
    );
    let e = epoch(&mut w, s);
    assert!(
        mutate(
            &mut w,
            s,
            MUT,
            node.id,
            e,
            MutationOp::SetAttribute {
                name: "data-n".into(),
                value: "1".into()
            }
        )
        .is_ok()
    );
    w.evaluate("Promise.resolve().then(()=>0)").ok();
    assert_eq!(
        w.evaluate("document.getElementById('box').getAttribute('data-n')")
            .unwrap(),
        "\"1\""
    );
    assert_eq!(
        w.evaluate("window.__records").unwrap(),
        "1",
        "no record for the native edit"
    );
}
