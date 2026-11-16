//! D3 inspection of iframe documents: every frame is its own target with its own document
//! stamp, node handles and authority. Uses the real `Backend` and real child views.
mod support;
use std::{
    thread,
    time::{Duration, Instant},
};
use support::{Reply, Server, engine, settle};
use wega_engine::{
    View,
    inspector::{Backend, Sampled},
};
use wega_protocol::{
    DocumentContext,
    identity::{FrameId, ProcessSessionId, TabId},
    inspector::{dom::*, edit::MutationOp, *},
};

fn pump(view: &mut View, mut done: impl FnMut(&mut View) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        view.poll();
        if done(view) {
            return true;
        }
        thread::sleep(Duration::from_millis(2));
    }
    false
}
fn target(frame: u64) -> TargetId {
    TargetId {
        tab: TabId(1),
        frame: FrameId(frame),
        process: ProcessSessionId([5; 16]),
    }
}
fn call(
    b: &mut Backend,
    t: TargetId,
    port: &mut View,
    ctx: DocumentContext,
    cmd: InspectorCommand,
    args: InspectorArgs,
) -> Result<InspectorResponse, InspectorError> {
    let r = InspectorRequest {
        version: INSPECTOR_VERSION,
        session: InspectorSessionId(1),
        target: t,
        command: cmd,
        args,
    };
    b.handle(&r, (ctx, false), Sampled::default(), port)
}
fn find(
    b: &mut Backend,
    t: TargetId,
    port: &mut View,
    ctx: DocumentContext,
    id_attr: &str,
) -> Option<DomNode> {
    let InspectorResponse::Dom(root) = call(
        b,
        t,
        port,
        ctx,
        InspectorCommand::GetDomRoot,
        InspectorArgs::None,
    )
    .ok()?
    else {
        return None;
    };
    let mut stack = vec![(root.nodes[0].clone(), root.epoch)];
    while let Some((node, epoch)) = stack.pop() {
        if node
            .attributes
            .iter()
            .any(|a| a.name == "id" && a.value == id_attr)
        {
            return Some(node);
        }
        if node.child_count == 0 {
            continue;
        }
        let Ok(InspectorResponse::Dom(page)) = call(
            b,
            t,
            port,
            ctx,
            InspectorCommand::GetDomChildren,
            InspectorArgs::DomChildren {
                node: node.id,
                epoch,
                offset: 0,
                limit: 64,
            },
        ) else {
            continue;
        };
        for child in page.nodes {
            stack.push((child, page.epoch));
        }
    }
    None
}

#[test]
fn each_frame_is_an_independent_inspector_target_with_its_own_authority() {
    let server = Server::new(|seen| match seen.path.as_str() {
        "/p" => Reply::html(
            "<p id=pp>parent</p><iframe id=f1 src=/c1></iframe><iframe id=f2 src=/c2></iframe>",
        ),
        "/c1" => Reply::html("<p id=x>one</p>"),
        "/c2" => Reply::html("<p id=x>two</p>"),
        "/c3" => Reply::html("<p id=y>three</p>"),
        _ => Reply::html("").status("404 Not Found"),
    });
    let engine = engine();
    let mut view = engine
        .create_view(wega_engine::Url::parse(&server.url("localhost", "/p")).unwrap())
        .unwrap();
    settle(&mut view);
    assert!(pump(&mut view, |v| v.frame_tree().len() == 2
        && v.frame_tree().iter().all(|f| f.url.contains("/c"))));
    let tree = view.frame_tree();
    assert_eq!(tree[0].origin, format!("http://localhost:{}", server.port));
    assert!(tree.iter().all(|f| f.parent.is_none() && !f.sandboxed));
    let (k1, k2) = (tree[0].key, tree[1].key);

    // The parent's tree shows the iframe elements.
    let parent_ctx = DocumentContext {
        document: wega_protocol::DocumentId(1),
        revision: wega_protocol::DocumentRevision(1),
    };
    let mut parent_backend = Backend::new(target(1), 25);
    call(
        &mut parent_backend,
        target(1),
        &mut view,
        parent_ctx,
        InspectorCommand::Attach,
        InspectorArgs::None,
    )
    .unwrap();
    // (the parent port reads its own document; the stamp is the browser's)
    assert!(find(&mut parent_backend, target(1), &mut view, parent_ctx, "f1").is_some());

    // Each child has its own backend and stamp; handles do not cross frames.
    let mut b1 = Backend::new(target(100 + k1), 25);
    let mut b2 = Backend::new(target(100 + k2), 25);
    let (x1, ctx1) = {
        let mut t = view.frame_target(k1).unwrap();
        call(
            &mut b1,
            target(100 + k1),
            t.port,
            t.context,
            InspectorCommand::Attach,
            InspectorArgs::None,
        )
        .unwrap();
        (
            find(&mut b1, target(100 + k1), t.port, t.context, "x").expect("child one DOM"),
            t.context,
        )
    };
    let (x2, ctx2) = {
        let mut t = view.frame_target(k2).unwrap();
        call(
            &mut b2,
            target(100 + k2),
            t.port,
            t.context,
            InspectorCommand::Attach,
            InspectorArgs::None,
        )
        .unwrap();
        (
            find(&mut b2, target(100 + k2), t.port, t.context, "x").expect("child two DOM"),
            t.context,
        )
    };
    assert_ne!(ctx1, ctx2);
    assert_ne!(
        x1.id, x2.id,
        "identical markup in two frames yields distinct node identities"
    );
    {
        // A handle minted by frame one is refused by frame two.
        let mut t = view.frame_target(k2).unwrap();
        let refused = call(
            &mut b2,
            target(100 + k2),
            t.port,
            t.context,
            InspectorCommand::DescribeDomNode,
            InspectorArgs::DomNode { node: x1.id },
        );
        assert!(refused.is_err(), "cross-frame node handle must be refused");
    }
    // A target resolved for one frame is not accepted by another frame's backend.
    {
        let mut t = view.frame_target(k1).unwrap();
        let wrong = call(
            &mut b2,
            target(100 + k1),
            t.port,
            t.context,
            InspectorCommand::GetDomRoot,
            InspectorArgs::None,
        );
        assert!(wrong.is_err());
    }

    // Mutation authority is per frame: a grant on frame one does not allow editing frame two.
    let origin = Some(format!("http://localhost:{}", server.port));
    let grant = InspectorArgs::Grant {
        token: GrantToken([7; 16]),
        opaque_override: false,
        origin: origin.clone(),
    };
    {
        let mut t = view.frame_target(k1).unwrap();
        call(
            &mut b1,
            target(100 + k1),
            t.port,
            t.context,
            InspectorCommand::GrantMutation,
            grant,
        )
        .unwrap();
        let epoch = epoch_of(&mut b1, target(100 + k1), t.port, t.context);
        let ok = call(
            &mut b1,
            target(100 + k1),
            t.port,
            t.context,
            InspectorCommand::Mutate,
            InspectorArgs::Mutate {
                grant: GrantToken([7; 16]),
                node: x1.id,
                epoch,
                op: MutationOp::SetAttribute {
                    name: "data-edited".into(),
                    value: "1".into(),
                },
            },
        );
        assert!(ok.is_ok(), "{ok:?}");
    }
    {
        let mut t = view.frame_target(k2).unwrap();
        let denied = call(
            &mut b2,
            target(100 + k2),
            t.port,
            t.context,
            InspectorCommand::Mutate,
            InspectorArgs::Mutate {
                grant: GrantToken([7; 16]),
                node: x2.id,
                epoch: 0,
                op: MutationOp::SetAttribute {
                    name: "data-edited".into(),
                    value: "1".into(),
                },
            },
        );
        assert!(denied.is_err(), "frame two never received a grant");
    }
    let edited = |view: &mut View, i: usize| {
        let key = view.child_frames()[i].0;
        view.child_by_key(key)
            .unwrap()
            .evaluate_json("document.getElementById('x').dataset.edited ?? 'none'")
            .unwrap()
    };
    assert_eq!(edited(&mut view, 0), "\"1\"");
    assert_eq!(edited(&mut view, 1), "\"none\"");
    assert_eq!(
        view.evaluate_json("document.getElementById('pp').dataset.edited ?? 'none'")
            .unwrap(),
        "\"none\""
    );

    // Navigating frame one changes its revision: the old stamp, handles and grant are stale.
    view.evaluate_json(&format!(
        "document.getElementById('f1').src = '{}'; 0",
        server.url("localhost", "/c3")
    ))
    .unwrap();
    assert!(pump(&mut view, |v| v
        .frame_tree()
        .iter()
        .find(|f| f.key == k1)
        .is_some_and(
            |f| f.revision != ctx1.revision.0 && f.url.ends_with("/c3")
        )));
    pump(&mut view, |v| {
        v.frame_target(k1).is_some_and(|t| !t.loading)
    });
    {
        let mut t = view.frame_target(k1).unwrap();
        assert_ne!(t.context, ctx1);
        let old = call(
            &mut b1,
            target(100 + k1),
            t.port,
            ctx1,
            InspectorCommand::Mutate,
            InspectorArgs::Mutate {
                grant: GrantToken([7; 16]),
                node: x1.id,
                epoch: 0,
                op: MutationOp::SetAttribute {
                    name: "data-edited".into(),
                    value: "2".into(),
                },
            },
        );
        assert!(old.is_err(), "stale document and grant");
        let stale_node = call(
            &mut b1,
            target(100 + k1),
            t.port,
            t.context,
            InspectorCommand::DescribeDomNode,
            InspectorArgs::DomNode { node: x1.id },
        );
        assert!(stale_node.is_err());
        assert!(find(&mut b1, target(100 + k1), t.port, t.context, "y").is_some());
    }
    // Removal makes the target unresolvable; its key is never reissued.
    view.evaluate_json("document.getElementById('f2').remove(); 0")
        .unwrap();
    assert!(pump(&mut view, |v| v.frame_target(k2).is_none()));
}
fn epoch_of(b: &mut Backend, t: TargetId, port: &mut View, ctx: DocumentContext) -> u64 {
    let InspectorResponse::Dom(root) = call(
        b,
        t,
        port,
        ctx,
        InspectorCommand::GetDomRoot,
        InspectorArgs::None,
    )
    .unwrap() else {
        panic!()
    };
    root.epoch
}
