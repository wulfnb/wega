use super::*;
fn page_update() -> accesskit::TreeUpdate {
    let mut root = accesskit::Node::new(accesskit::Role::RootWebArea);
    root.set_children(vec![accesskit::NodeId(2)]);
    let mut button = accesskit::Node::new(accesskit::Role::Button);
    button.set_label("Submit form");
    button.add_action(accesskit::Action::Click);
    accesskit::TreeUpdate {
        nodes: vec![(accesskit::NodeId(1), root), (accesskit::NodeId(2), button)],
        tree: Some(accesskit::TreeInfo::new(accesskit::NodeId(1))),
        tree_id: accesskit::TreeId::ROOT,
        focus: accesskit::NodeId(2),
    }
}
#[test]
fn page_accessibility_updates_are_grafted_and_scoped_to_document_and_process() {
    use wega::engine_protocol::{
        DocumentContext, DocumentId, DocumentRevision,
        authority::{Origin, Principal, TrustedRequest},
        identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
    };
    let mut state = State {
        active: true,
        ..Default::default()
    };
    let mut requester = TrustedRequest {
        session: SessionIdentity {
            tab: TabId(1),
            frame: FrameId(1),
            process: ProcessSessionId([1; 16]),
            service: None,
        },
        document: DocumentContext {
            document: DocumentId(0),
            revision: DocumentRevision(1),
        },
        principal: Principal {
            origin: Origin::Tuple("https://page.test".into()),
            top_level_site: None,
        },
    };
    let json = serde_json::to_string(&page_update()).unwrap();
    state.queue_wega(1, &requester, &json).unwrap();
    let first = state.wega.as_ref().unwrap().id;
    assert_ne!(first, accesskit::TreeId::ROOT);
    let tree = state.wega.as_ref().unwrap().pending.as_ref().unwrap();
    assert_eq!(tree.tree_id, first);
    assert_eq!(tree.nodes[1].1.label(), Some("Submit form"));
    assert!(tree.nodes[1].1.supports_action(accesskit::Action::Click));
    state.queue_wega(1, &requester, &json).unwrap();
    assert_eq!(state.wega.as_ref().unwrap().id, first);
    requester.document.revision = DocumentRevision(2);
    state.queue_wega(1, &requester, &json).unwrap();
    let second = state.wega.as_ref().unwrap().id;
    assert_ne!(first, second);
    requester.session.process = ProcessSessionId([2; 16]);
    state.queue_wega(1, &requester, &json).unwrap();
    assert_ne!(second, state.wega.as_ref().unwrap().id);
}
#[test]
fn malformed_page_accessibility_trees_are_rejected_before_reaching_the_adapter() {
    let mut update = page_update();
    assert!(validate_wega_tree(&update).is_ok());
    update.nodes[1].1.set_children(vec![accesskit::NodeId(1)]);
    assert!(validate_wega_tree(&update).is_err());
    update = page_update();
    update.nodes[0].1.push_child(accesskit::NodeId(99));
    assert!(validate_wega_tree(&update).is_err());
    update = page_update();
    update.nodes.push(update.nodes[1].clone());
    assert!(validate_wega_tree(&update).is_err());
    update = page_update();
    update.focus = accesskit::NodeId(99);
    assert!(validate_wega_tree(&update).is_err());
}
#[test]
fn grafted_page_focus_and_navigation_preserve_the_browser_tree() {
    use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
    use accesskit_consumer::{NodeRef, Tree, TreeChangeHandler as ChangeHandler};
    struct Changes;
    impl ChangeHandler for Changes {
        fn node_added(&mut self, _: &NodeRef) {}
        fn node_updated(&mut self, _: &NodeRef, _: &NodeRef) {}
        fn focus_moved(&mut self, _: Option<&NodeRef>, _: Option<&NodeRef>) {}
        fn node_removed(&mut self, _: &NodeRef) {}
    }
    let parent = |subtree: Option<TreeId>, focus: u64| {
        let mut window = Node::new(Role::Window);
        window.set_children(vec![NodeId(2), NodeId(3)]);
        let mut address = Node::new(Role::TextInput);
        address.set_label("Address bar");
        let mut graft = Node::new(Role::GenericContainer);
        if let Some(id) = subtree {
            graft.set_tree_id(id);
        }
        TreeUpdate {
            nodes: vec![
                (NodeId(1), window),
                (NodeId(2), address),
                (NodeId(3), graft),
            ],
            tree: Some(TreeInfo::new(NodeId(1))),
            tree_id: TreeId::ROOT,
            focus: NodeId(focus),
        }
    };
    let mut native = Tree::new(parent(None, 2), true);
    let mut state = State::default();
    let owner = (
        1,
        wega::engine_protocol::identity::SessionIdentity {
            tab: wega::engine_protocol::identity::TabId(1),
            frame: wega::engine_protocol::identity::FrameId(1),
            process: wega::engine_protocol::identity::ProcessSessionId([3; 16]),
            service: None,
        },
        wega::engine_protocol::DocumentContext {
            document: wega::engine_protocol::DocumentId(0),
            revision: wega::engine_protocol::DocumentRevision(1),
        },
    );
    let id = state.wega_tree(owner).unwrap().id;
    // Platform adapters require the parent first, with focus held outside the
    // graft until the child's initial complete tree exists.
    native.update_and_process_changes(parent(Some(id), 2), &mut Changes);
    let mut page = page_update();
    page.tree_id = id;
    native.update_and_process_changes(page, &mut Changes);
    native.update_and_process_changes(parent(Some(id), 3), &mut Changes);
    assert_eq!(
        native.state().focus().unwrap().label().as_deref(),
        Some("Submit form")
    );
    assert_eq!(
        native
            .state()
            .node_by_tree_local_id(NodeId(2), TreeId::ROOT)
            .unwrap()
            .label()
            .as_deref(),
        Some("Address bar")
    );
    let next = state.wega_tree((2, owner.1, owner.2)).unwrap().id;
    native.update_and_process_changes(parent(Some(next), 2), &mut Changes);
    assert!(
        native
            .state()
            .node_by_tree_local_id(NodeId(2), id)
            .is_none()
    );
    let mut page = page_update();
    page.tree_id = next;
    native.update_and_process_changes(page, &mut Changes);
    native.update_and_process_changes(parent(Some(next), 3), &mut Changes);
    assert_eq!(native.state().focus().unwrap().role(), Role::Button);
}
#[test]
fn servo_subtree_and_actions_preserve_semantics_across_versions() {
    let mut node = servo::accesskit::Node::new(servo::accesskit::Role::Button);
    node.set_label("Submit form");
    node.add_action(servo::accesskit::Action::Click);
    let update = servo::accesskit::TreeUpdate {
        nodes: vec![(servo::accesskit::NodeId(1), node)],
        tree: Some(servo::accesskit::Tree::new(servo::accesskit::NodeId(1))),
        tree_id: servo::accesskit::TreeId::ROOT,
        focus: servo::accesskit::NodeId(1),
    };
    let converted = tree(update).unwrap();
    assert_eq!(converted.nodes[0].1.label(), Some("Submit form"));
    assert!(
        converted.nodes[0]
            .1
            .supports_action(accesskit::Action::Click)
    );
    let request = accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(1),
        data: None,
    };
    let converted: servo::accesskit::ActionRequest =
        serde_json::from_value(serde_json::to_value(request).unwrap()).unwrap();
    assert_eq!(converted.action, servo::accesskit::Action::Click);
}
