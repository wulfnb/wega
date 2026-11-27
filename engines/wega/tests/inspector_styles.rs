mod support;
use support::inspector_worker::Worker;
use wega_adapter_blitz::{BlitzDocument, DocumentSetup, ToNodeId};
use wega_dom::inspect::StyleInspect;
use wega_protocol::{
    self as protocol, DocumentContext, DocumentId, DocumentRevision,
    inspector::{styles::*, *},
};
fn scope() -> StyleScope {
    StyleScope {
        target: TargetId {
            tab: protocol::identity::TabId(1),
            frame: protocol::identity::FrameId(1),
            process: protocol::identity::ProcessSessionId([1; 16]),
        },
        session: InspectorSessionId(1),
        document: DocumentContext {
            document: DocumentId(1),
            revision: DocumentRevision(1),
        },
        epoch: 0,
        generation: 0,
    }
}
#[test]
fn engine_rule_refs_are_bound_to_every_authority_and_epoch_dimension() {
    let mut d = BlitzDocument::from_html(
        "<!doctype html><style>p{color:red}</style><p>x</p>",
        DocumentSetup::default(),
    );
    let scope = scope();
    let page =
        wega_engine::inspector::styles::inspect(&d, scope, &StyleQuery::Sheets { offset: 0 })
            .unwrap();
    let StyleItem::Sheet { reference, .. } = &page.items[0] else {
        panic!()
    };
    let original = reference.clone();
    for (dimension, code) in [
        (0, InspectorErrorCode::Unauthorized),
        (1, InspectorErrorCode::Unauthorized),
        (2, InspectorErrorCode::StaleDocument),
        (3, InspectorErrorCode::StaleMutation),
        (4, InspectorErrorCode::Unauthorized),
    ] {
        let mut r = original.clone();
        match dimension {
            0 => r.scope.target.tab = protocol::identity::TabId(99),
            1 => r.scope.session = InspectorSessionId(99),
            2 => {
                r.scope.document.revision = DocumentRevision(99);
                r.owner.document = r.scope.document;
            }
            3 => r.scope.generation += 1,
            _ => r.scope.target.process = protocol::identity::ProcessSessionId([99; 16]),
        };
        assert_eq!(
            wega_engine::inspector::styles::inspect(
                &d,
                scope,
                &StyleQuery::Rules {
                    parent: r,
                    offset: 0
                }
            )
            .unwrap_err()
            .code,
            code
        );
    }
    let owner = wega_dom::NodeKey::from_u64(original.owner.key).node_id();
    d.stylesheet_insert_rule(owner, &[], "p{color:blue}", 0)
        .unwrap();
    assert_eq!(
        wega_engine::inspector::styles::inspect(
            &d,
            scope,
            &StyleQuery::Rules {
                parent: original,
                offset: 0
            }
        )
        .unwrap_err()
        .code,
        InspectorErrorCode::StaleMutation
    );
    assert!(d.style_generation() > 0);
}
#[test]
fn real_process_styles_have_existing_session_authorization_and_navigation_staleness() {
    let mut w = Worker::start();
    let s = w.attach();
    w.evaluate("document.head.innerHTML='<style>p{color:red!important}</style>';document.body.innerHTML='<p id=x>text</p>';true").unwrap();
    let call = |w: &mut Worker, s, q| {
        w.call_with(
            s,
            InspectorCommand::InspectStyle,
            InspectorArgs::Style { query: q },
        )
    };
    let InspectorResponse::Style(sheets) =
        call(&mut w, s, StyleQuery::Sheets { offset: 0 }).unwrap()
    else {
        panic!()
    };
    assert!(sheets.is_bounded());
    let StyleItem::Sheet { reference, .. } = &sheets.items[0] else {
        panic!()
    };
    let reference = reference.clone();
    let InspectorResponse::Style(rules) = call(
        &mut w,
        s,
        StyleQuery::Rules {
            parent: reference.clone(),
            offset: 0,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert!(rules.is_bounded());
    let StyleItem::Rule {
        reference: rule, ..
    } = &rules.items[0]
    else {
        panic!()
    };
    let InspectorResponse::Style(decls) = call(
        &mut w,
        s,
        StyleQuery::Declarations {
            rule: rule.clone(),
            offset: 0,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert!(decls.items.iter().any(|d| matches!(
        d,
        StyleItem::Declaration {
            important: Some(true),
            ..
        }
    )));
    let mut forged = reference.clone();
    forged.scope.session = InspectorSessionId(999);
    assert_eq!(
        call(
            &mut w,
            s,
            StyleQuery::Rules {
                parent: forged,
                offset: 0
            }
        ),
        Err("Unauthorized".into())
    );
    w.navigate(1, "about:blank");
    w.until(|e| {
        matches!(
            e,
            protocol::Event::State {
                serial: 1,
                loading: false,
                ..
            }
        )
    });
    assert_eq!(
        call(
            &mut w,
            s,
            StyleQuery::Rules {
                parent: reference,
                offset: 0
            }
        ),
        Err("StaleDocument".into())
    );
    w.call(s, InspectorCommand::Detach).unwrap();
    assert_eq!(
        call(&mut w, s, StyleQuery::Sheets { offset: 0 }),
        Err("StaleSession".into())
    );
}
#[test]
fn full_response_byte_budget_and_single_group_reads_are_bounded() {
    let css=format!("@media(all){{{}}}",".a{--x:xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx;}".repeat(1000));
    let d = BlitzDocument::from_html(
        &format!("<!doctype html><style>{css}</style>"),
        DocumentSetup::default(),
    );
    let p = wega_engine::inspector::styles::inspect(&d, scope(), &StyleQuery::Sheets { offset: 0 })
        .unwrap();
    let StyleItem::Sheet { reference, .. } = &p.items[0] else {
        panic!()
    };
    let p = wega_engine::inspector::styles::inspect(
        &d,
        scope(),
        &StyleQuery::Rules {
            parent: reference.clone(),
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(p.items.len(), 1);
    assert!(p.is_bounded());
    let StyleItem::Rule { children, .. } = &p.items[0] else {
        panic!()
    };
    assert_eq!(*children, 1000);
}
#[test]
fn real_pipe_refuses_foreign_targets_and_replacement_process_rule_replay() {
    use protocol::{
        Command,
        identity::{FrameId, ProcessSessionId, TabId},
    };
    let mut w = Worker::start();
    let s = w.attach();
    w.evaluate("document.head.innerHTML='<style>p{color:red}</style>';Object.defineProperty(document,'styleSheets',{get(){throw Error('page inspector hook')}});true").unwrap();
    let InspectorResponse::Style(page) = w
        .call_with(
            s,
            InspectorCommand::InspectStyle,
            InspectorArgs::Style {
                query: StyleQuery::Sheets { offset: 0 },
            },
        )
        .unwrap()
    else {
        panic!()
    };
    let StyleItem::Sheet { reference, .. } = &page.items[0] else {
        panic!()
    };
    let reference = reference.clone();
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
                command: InspectorCommand::InspectStyle,
                args: InspectorArgs::Style {
                    query: StyleQuery::Sheets { offset: 0 },
                },
            }),
        });
        assert_eq!(w.reply(context), Err("UnknownTarget".into()));
    }
    let mut replacement = Worker::start_with(7, 8, protocol::VERSION);
    let fresh = replacement.attach();
    assert_eq!(
        replacement.call_with(
            fresh,
            InspectorCommand::InspectStyle,
            InspectorArgs::Style {
                query: StyleQuery::Rules {
                    parent: reference,
                    offset: 0
                }
            }
        ),
        Err("Unauthorized".into())
    );
    assert!(w.alive() && replacement.alive());
}
#[test]
fn json_escaped_declaration_names_are_paged_by_response_bytes() {
    let decls = (0..40)
        .map(|i| format!("--n{i}{}:{};", "\\1 ".repeat(500), "v".repeat(512)))
        .collect::<String>();
    let d = BlitzDocument::from_html(
        &format!("<!doctype html><style>p{{{decls}}}</style><p>x</p>"),
        DocumentSetup::default(),
    );
    let p = wega_engine::inspector::styles::inspect(&d, scope(), &StyleQuery::Sheets { offset: 0 })
        .unwrap();
    let StyleItem::Sheet { reference, .. } = &p.items[0] else {
        panic!()
    };
    let mut rule = reference.clone();
    rule.path = vec![0];
    let mut q = StyleQuery::Declarations { rule, offset: 0 };
    let mut count = 0;
    loop {
        let mut response = None;
        for _ in 0..5 {
            match wega_engine::inspector::styles::inspect(&d, scope(), &q) {
                Ok(p) => {
                    response = Some(p);
                    break;
                }
                Err(e) if e.code == InspectorErrorCode::Timeout => std::thread::yield_now(),
                Err(e) => panic!("{e}"),
            }
        }
        let p = response.expect("bounded retry");
        assert!(p.is_bounded());
        assert!(serde_json::to_vec(&p).unwrap().len() <= STYLE_BYTES);
        assert!(
            p.items.len() < 32,
            "byte cap should shorten an escaped page"
        );
        if p.next.is_some() {
            assert!(p.truncated, "byte-limited continuation must be labelled");
        }
        count += p.items.len();
        if let Some(next) = p.next {
            q = q.at(next);
        } else {
            break;
        }
    }
    assert_eq!(count, 40);
}
