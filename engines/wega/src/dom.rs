//! Checked native operations over Blitz's single DOM, used by the V8 host.
use crate::Limits;
use blitz_dom::node::NodeData;
use blitz_dom::{BaseDocument, NodeId};
use html5ever::{LocalName, QualName, ns};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};
pub(crate) struct Dom {
    pub network: crate::network::Network,
    pub document: Rc<RefCell<BaseDocument>>,
    pub limits: Limits,
    pub navigate: Rc<RefCell<Vec<url::Url>>>,
    pub errors: Rc<RefCell<Vec<String>>>,
    allocated: Cell<usize>,
    used: Cell<usize>,
    start: Instant,
    retained: RefCell<HashSet<NodeId>>,
}
impl Dom {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        limits: &Limits,
        navigate: Rc<RefCell<Vec<url::Url>>>,
        initial_bytes: usize,
        _network: crate::network::Network,
    ) -> Self {
        let allocated =
            Cell::new(blitz_dom::traversal::TreeTraverser::new(&document.borrow()).count());
        Self {
            network: _network,
            document,
            limits: limits.clone(),
            navigate,
            errors: Default::default(),
            allocated,
            used: Cell::new(initial_bytes),
            start: Instant::now(),
            retained: Default::default(),
        }
    }
    pub fn call(&self, args: &[Value]) -> Result<Value, String> {
        let text = |index: usize| args.get(index).and_then(Value::as_str).unwrap_or_default();
        if text(0) == "error" {
            let mut reports = self.errors.borrow_mut();
            if reports.len() < 16 {
                reports.push(text(1).chars().take(4096).collect());
            }
            return Ok(Value::Null);
        }
        let charge = match text(0) {
            "setText" => text(2).len() + 32,
            "setAttr" => text(2).len() + text(3).len() + 32,
            "create" | "createText" | "createComment" | "createFragment" => text(1).len() + 32,
            "createNS" => text(1).len() + text(2).len() + 32,
            "innerHTML" => text(2).len() + 32,
            "styleSet" => text(2).len() + text(3).len() + 32,
            _ => 0,
        };
        let next = self.used.get().saturating_add(charge);
        if next > self.limits.dom_text_bytes {
            return Err("DOM text allocation budget exceeded".into());
        }
        self.used.set(next);
        let id = |index: usize| {
            text(index)
                .parse::<u64>()
                .map(NodeId::from_u64)
                .map_err(|_| "Invalid node identity".to_string())
        };
        let mut doc = self
            .document
            .try_borrow_mut()
            .map_err(|_| "DOM is currently busy".to_string())?;
        let node_id = || -> Result<NodeId, String> {
            let id = id(1)?;
            if doc.get_node(id).is_none() {
                Err("Node no longer exists".into())
            } else {
                Ok(id)
            }
        };
        let value = match text(0) {
            "cookie"=>json!(self.network.cookies.get(doc.url(),true)),
            "setCookie"=>{self.network.cookies.set(doc.url(),text(1),true);Value::Null},
            "templateContent"=>{let target=node_id()?;let content=doc.mutate().template_contents(target);json!(content.as_u64().to_string())},
            "viewport"=>{let viewport=doc.viewport();let scale=(viewport.hidpi_scale*viewport.zoom) as f64;let scroll=doc.viewport_scroll();json!({"width":viewport.window_size.0 as f64/scale,"height":viewport.window_size.1 as f64/scale,"dpr":scale,"scrollX":scroll.x,"scrollY":scroll.y})},
            "scroll"=>{let node=doc.get_node(node_id()?).unwrap();let scroll=node.scroll_offset();json!({"x":scroll.x,"y":scroll.y})},
            "scrollViewport"=>{let x=text(1).parse::<f64>().map_err(|_|"Invalid scroll offset")?;let y=text(2).parse::<f64>().map_err(|_|"Invalid scroll offset")?;if x.is_finite()&&y.is_finite(){doc.set_viewport_scroll(blitz_dom::Point{x,y});}Value::Null},
            "scrollNode"=>{let target=node_id()?;let x=text(2).parse::<f64>().map_err(|_|"Invalid scroll offset")?;let y=text(3).parse::<f64>().map_err(|_|"Invalid scroll offset")?;if x.is_finite()&&y.is_finite(){doc.scroll_to(target,x,y,blitz_dom::ScrollBehavior::Instant);}Value::Null},
            "scrollIntoView"=>{let target=node_id()?;doc.scroll_into_view(target,blitz_dom::ScrollBehavior::Instant,blitz_dom::ScrollLogicalPosition::Start,blitz_dom::ScrollLogicalPosition::Nearest);Value::Null},
            "active"=>json!(doc.get_focussed_node_id().map(|id|id.as_u64().to_string())),
            "history"=>{let url=doc.url().join(text(1)).map_err(|e|e.to_string())?;if url.origin()!=doc.url().origin()||!matches!(url.scheme(),"http"|"https"){return Err("SecurityError: history URL must have the same tuple origin".into());}doc.set_base_url(url.as_str());Value::Null},
            "media"=>{
                let url_data=style::stylesheets::UrlExtraData(style::servo_arc::Arc::new(doc.url().clone()));
                let mut context=style::parser::ParserContext::new(style::stylesheets::Origin::Author,&url_data,Some(style::stylesheets::CssRuleType::Media),style_traits::ParsingMode::DEFAULT,selectors::matching::QuirksMode::NoQuirks,std::borrow::Cow::Owned(Default::default()),None,None,Default::default());
                let mut input=cssparser::Parser::new(text(1));let media=style::media_queries::MediaList::parse(&mut context,&mut input);
                json!(media.evaluate(doc.stylist_device(),selectors::matching::QuirksMode::NoQuirks,&mut style::stylesheets::CustomMediaEvaluator::none()))
            },
            "subtree"=>json!(Self::subtree(&doc,node_id()?).iter().map(|id|id.as_u64().to_string()).collect::<Vec<_>>()),
            "resolveURL"=>{let base=url::Url::parse(text(2)).map_err(|e|e.to_string())?;let url=base.join(text(1)).map_err(|e|e.to_string())?;json!({"href":url.as_str(),"origin":url.origin().ascii_serialization(),"protocol":format!("{}:",url.scheme()),"host":url.host_str().map_or(String::new(),|host|url.port().map_or_else(||host.to_owned(),|port|format!("{host}:{port}"))),"hostname":url.host_str().unwrap_or_default(),"port":url.port().map(|p|p.to_string()).unwrap_or_default(),"pathname":url.path(),"search":url.query().map(|q|format!("?{q}")).unwrap_or_default(),"hash":url.fragment().map(|f|format!("#{f}")).unwrap_or_default()})},
            "connected"=>{let mut root=node_id()?;while let Some(parent)=doc.get_node(root).and_then(|n|n.parent){root=parent;}json!(root==doc.root_node().id)},
            "retain" => {let id=node_id()?;self.retained.borrow_mut().insert(id);Value::Null},
            "release" => {let id=id(1)?;self.retained.borrow_mut().remove(&id);self.collect_detached(&mut doc,id);Value::Null},
            "info"=>{
                let node=doc.get_node(node_id()?).unwrap();
                let (kind,name,local,namespace,prefix)=match &node.data {
                    NodeData::Document(_)=>(9,"#document".to_owned(),None,None,None),
                    NodeData::Text(_)=>(3,"#text".to_owned(),None,None,None),
                    NodeData::Comment{..}=>(8,"#comment".to_owned(),None,None,None),
                    NodeData::Element(e)|NodeData::AnonymousBlock(e)=>{
                        let local=e.name.local.to_string();let fragment=local=="#document-fragment";
                        let name=if e.name.ns==ns!(html)&&!fragment {local.to_uppercase()}else{local.clone()};
                        (if fragment{11}else{1},name,Some(local),Some(e.name.ns.to_string()),e.name.prefix.as_ref().map(|p|p.to_string()))
                    }
                };
                json!({"nodeType":kind,"nodeName":name,"localName":local,"namespaceURI":namespace,"prefix":prefix})
            },
            "attributes"=>json!(doc.get_node(node_id()?).unwrap().attrs().unwrap_or_default().iter().map(|a|json!({"name":a.name.prefix.as_ref().map_or_else(||a.name.local.to_string(),|p|format!("{p}:{}",a.name.local)),"localName":a.name.local.to_string(),"namespaceURI":if a.name.ns==ns!(){None}else{Some(a.name.ns.to_string())},"value":a.value})).collect::<Vec<_>>()),
            "queryIn"=>json!(doc.query_selector_in(node_id()?,text(2)).map_err(|_|"Invalid selector")?.map(|id|id.as_u64().to_string())),
            "queryAllIn"=>json!(doc.query_selector_all_in(node_id()?,text(2)).map_err(|_|"Invalid selector")?.iter().map(|id|id.as_u64().to_string()).collect::<Vec<_>>()),
            "matches"=>json!(doc.matches_selector(node_id()?,text(2)).map_err(|_|"Invalid selector")?),
            "closest"=>json!(doc.closest(node_id()?,text(2)).map_err(|_|"Invalid selector")?.map(|id|id.as_u64().to_string())),
            "getById"=>json!(doc.get_element_by_id(text(1)).map(|id|id.as_u64().to_string())),
            "outerHTML"=>json!(doc.get_node(node_id()?).unwrap().outer_html()),
            "innerHTML"=>{
                let target=node_id()?;
                if text(2).len()>self.limits.html_bytes{return Err("HTML fragment exceeds the size limit".into());}
                // Parse into a detached same-context element first; reject budgets before
                // replacing live children, and retain removed nodes held by JS wrappers.
                let element=doc.get_node(target).unwrap().element_data().ok_or("innerHTML needs an element")?;
                let name=element.name.clone();let staging=doc.mutate().create_element(name,vec![]);
                {let mut mutation=doc.mutate();blitz_html::DocumentHtmlParser::parse_inner_html_into_mutator(&mut mutation,staging,text(2));}
                let parsed=Self::subtree(&doc,staging);
                if self.allocated.get().saturating_add(parsed.len())>self.limits.dom_nodes {doc.mutate().remove_and_drop_node(staging);return Err("DOM node limit exceeded".into());}
                let old=doc.get_node(target).unwrap().children.to_vec();let children=doc.get_node(staging).unwrap().children.to_vec();
                doc.mutate().replace_children(target,&children);doc.mutate().remove_and_drop_node(staging);
                self.allocated.set(self.allocated.get()+parsed.len()-1);
                for id in old {self.collect_detached(&mut doc,id);}
                Value::Null
            },
            "clone"=>{
                let source=node_id()?;let deep=text(2)=="true";let count=if deep{Self::subtree(&doc,source).len()}else{1};
                if self.allocated.get().saturating_add(count)>self.limits.dom_nodes{return Err("DOM node limit exceeded".into());}
                let bytes=doc.get_node(source).unwrap().outer_html().len();
                if self.used.get().saturating_add(bytes)>self.limits.dom_text_bytes{return Err("DOM text allocation budget exceeded".into());}
                let cloned=if deep{doc.mutate().deep_clone_node(source)}else{
                    match &doc.get_node(source).unwrap().data {
                        NodeData::Element(e)|NodeData::AnonymousBlock(e)=>{let name=e.name.clone();let attrs=e.attrs.to_vec();doc.mutate().create_element(name,attrs)},
                        NodeData::Text(t)=>{let text=t.content.to_string();doc.mutate().create_text_node(&text)},
                        NodeData::Comment{contents}=>{let text=contents.clone();doc.mutate().create_comment_node(&text)},
                        _=>return Err("This node cannot be cloned".into())
                    }
                };
                self.allocated.set(self.allocated.get()+count);self.used.set(self.used.get()+bytes);json!(cloned.as_u64().to_string())
            },
            "insert"=>{
                let parent=node_id()?;let child=id(2)?;
                if doc.get_node(child).is_none(){return Err("Node no longer exists".into());}
                let anchor=if text(3).is_empty(){None}else{Some(id(3)?)};
                if anchor.is_some_and(|id|doc.get_node(id).is_none_or(|n|n.parent!=Some(parent))){return Err("NotFoundError: reference is not a child".into());}
                let mut ancestor=Some(parent);while let Some(id)=ancestor {if id==child{return Err("HierarchyRequestError: DOM cycle".into());}ancestor=doc.get_node(id).and_then(|n|n.parent);}
                if anchor==Some(child){return Ok(json!(child.as_u64().to_string()));}
                if let Some(anchor)=anchor{doc.mutate().insert_nodes_before(anchor,&[child]);}else{doc.mutate().append_children(parent,&[child]);}
                json!(child.as_u64().to_string())
            },
            "createNS"|"createComment"|"createFragment"|"createDocument"=>{
                if self.allocated.get()>=self.limits.dom_nodes{return Err("DOM node limit exceeded".into());}
                let created=match text(0){
                    "createDocument"=>doc.create_node(NodeData::Document(Box::default())),
                    "createComment"=>doc.mutate().create_comment_node(text(1)),
                    "createFragment"=>doc.mutate().create_element(QualName::new(None,ns!(html),LocalName::from("#document-fragment")),vec![]),
                    _=>{let parts=text(2).split_once(':');let (prefix,local)=parts.map_or((None,text(2)),|(p,l)|(Some(html5ever::Prefix::from(p)),l));doc.mutate().create_element(QualName::new(prefix,html5ever::Namespace::from(text(1)),LocalName::from(local)),vec![])}
                };self.allocated.set(self.allocated.get()+1);json!(created.as_u64().to_string())
            },
            "attrNS"=>json!(doc.get_node(node_id()?).unwrap().attrs().unwrap_or_default().iter().find(|a|a.name.ns.as_str()==text(2)&&a.name.local.as_str()==text(3)).map(|a|a.value.clone())),
            "setAttrNS"=>{
                let target=node_id()?;let payload=[text(3).to_owned(),text(4).to_owned()];
                let (prefix,local)=payload[0].split_once(':').map_or((None,payload[0].as_str()),|(p,l)|(Some(html5ever::Prefix::from(p)),l));
                let charge=payload[0].len()+payload[1].len();if self.used.get().saturating_add(charge)>self.limits.dom_text_bytes{return Err("DOM text allocation budget exceeded".into());}self.used.set(self.used.get()+charge);
                doc.mutate().set_attribute(target,QualName::new(prefix,html5ever::Namespace::from(text(2)),LocalName::from(local)),&payload[1]);Value::Null
            },
            "inlineCSS"=>{let target=node_id()?;let mut css=String::new();if let Some(block)=doc.get_node(target).unwrap().element_data().and_then(|e|e.style_attribute.as_ref()){let guard=doc.guard().read();let _=block.read_with(&guard).to_css(&mut css);}json!(css)},
            "inlineProperty"=>{let target=node_id()?;let mut css=String::new();if let Ok(property)=style::properties::PropertyId::parse_enabled_for_all_content(text(2)) && let Some(block)=doc.get_node(target).unwrap().element_data().and_then(|e|e.style_attribute.as_ref()){let guard=doc.guard().read();let _=block.read_with(&guard).property_value_to_css(&property,&mut css);}json!(css)},
            "styleSet"=>{
                let target=node_id()?;doc.mutate().remove_style_property(target,text(2));
                let mut css=String::new();if let Some(block)=doc.get_node(target).unwrap().element_data().and_then(|e|e.style_attribute.as_ref()){let guard=doc.guard().read();let _=block.read_with(&guard).to_css(&mut css);}
                if !text(3).is_empty(){css.push_str(&format!("{}: {};",text(2),text(3)));}
                doc.mutate().set_attribute(target,QualName::new(None,ns!(),LocalName::from("style")),&css);Value::Null
            },
            "inputValue"=>{let target=node_id()?;let element=doc.get_node(target).unwrap().element_data().ok_or("Input needs an element")?;json!(element.text_input_data().map(|data|data.editor.text().to_string()).or_else(||element.attr(LocalName::from("value")).map(str::to_owned)).unwrap_or_default())},
            "checked"=>json!(doc.get_node(node_id()?).unwrap().element_data().and_then(|e|e.checkbox_input_checked()).unwrap_or(false)),
            // Blitz creates native text editors during layout. Initialize a
            // newly inserted input before focus so immediate key/IME commands
            // cannot be silently dropped while waiting for the next paint.
            "focus"=>{let target=node_id()?;doc.resolve(self.start.elapsed().as_secs_f64());doc.set_focus_to(target);Value::Null},
            "serialize"=>{let mut html=String::new();Self::serialize(&doc,node_id()?,&mut html);json!(html)},
            "computed"=>{
                let target=node_id()?;doc.resolve(self.start.elapsed().as_secs_f64());
                let property=style::properties::PropertyId::parse_enabled_for_all_content(text(2));
                let value=doc.get_node(target).unwrap().primary_styles().and_then(|computed|match property {
                    Ok(style::properties::PropertyId::Custom(ref name))=>Some(computed.computed_value_to_string(style::properties::PropertyDeclarationId::Custom(name))),
                    Ok(ref id)=>id.longhand_id().map(|id|computed.computed_value_to_string(style::properties::PropertyDeclarationId::Longhand(id))),
                    _=>None,
                }).unwrap_or_default();json!(value)
            },
            "limits" => json!({"resourceBytes":self.limits.resource_bytes}),
            "root" => json!(doc.root_node().id.as_u64().to_string()),
            "query" => json!(
                doc.query_selector(text(1))
                    .map_err(|_| "Invalid selector")?
                    .map(|id| id.as_u64().to_string())
            ),
            "queryAll" => json!(
                doc.query_selector_all(text(1))
                    .map_err(|_| "Invalid selector")?
                    .iter()
                    .map(|id| id.as_u64().to_string())
                    .collect::<Vec<_>>()
            ),
            "text" => {let node=doc.get_node(node_id()?).unwrap();match &node.data {NodeData::Document(_)=>Value::Null,NodeData::Comment{contents}=>json!(contents),_=>json!(node.text_content())}},
            "tag" => json!(
                doc.get_node(node_id()?).unwrap().element_data().map(|e| e
                    .name
                    .local
                    .to_string()
                    .to_uppercase())
            ),
            "children" => json!(
                doc.get_node(node_id()?)
                    .unwrap()
                    .children
                    .iter()
                    .map(|id| id.as_u64().to_string())
                    .collect::<Vec<_>>()
            ),
            "parent" => json!(
                doc.get_node(node_id()?)
                    .unwrap()
                    .parent
                    .map(|id| id.as_u64().to_string())
            ),
            "box"=>{
                let target=node_id()?;doc.resolve(self.start.elapsed().as_secs_f64());let node=doc.get_node(target).unwrap();let layout=node.final_layout();let offset=node.offset_top_left();
                json!({"hasBoxes":node.has_boxes(),"clientWidth":node.client_width(),"clientHeight":node.client_height(),"clientLeft":layout.border.left,"clientTop":layout.border.top,"offsetWidth":layout.size.width,"offsetHeight":layout.size.height,"offsetLeft":offset.x,"offsetTop":offset.y,"offsetParent":node.offset_parent().map(|node|node.id.as_u64().to_string()),"scrollWidth":node.scroll_width(),"scrollHeight":node.scroll_height(),"contentWidth":layout.content_box_width(),"contentHeight":layout.content_box_height(),"paddingLeft":layout.padding.left,"paddingTop":layout.padding.top})
            },
            "rect" => {
                let id = node_id()?;
                doc.resolve(self.start.elapsed().as_secs_f64());
                let node = doc.get_node(id).unwrap();
                if node.element_data().is_none() {return Ok(json!({"x":0,"y":0,"width":0,"height":0,"left":0,"right":0,"top":0,"bottom":0}));}
                let p = node.absolute_position(0.0, 0.0);
                let scroll = doc.viewport_scroll();
                let size = node.final_layout().size;
                let x = p.x + node.scroll_offset().x as f32 - scroll.x as f32;
                let y = p.y + node.scroll_offset().y as f32 - scroll.y as f32;
                json!({"x":x,"y":y,"width":size.width,"height":size.height,"left":x,"top":y,"right":x+size.width,"bottom":y+size.height})
            }
            "attr" => json!(
                doc.get_node(node_id()?)
                    .unwrap()
                    .element_data()
                    .and_then(|e| e.attr(LocalName::from(text(2))))
                    .map(str::to_owned)
            ),
            "setText" => {
                let id = node_id()?;
                if let NodeData::Comment{contents}=&mut doc.get_node_mut(id).unwrap().data {*contents=text(2).to_owned();return Ok(Value::Null);}
                if matches!(doc.get_node(id).unwrap().data,NodeData::Document(_)){return Ok(Value::Null);}
                if text(2).len() > 1024 * 1024 || self.allocated.get() >= self.limits.dom_nodes {
                    return Err("DOM text or node budget exceeded".into());
                }
                self.allocated.set(self.allocated.get() + 1);
                if matches!(
                    doc.get_node(id).unwrap().data,
                    blitz_dom::node::NodeData::Text(_)
                ) {
                    doc.mutate().set_node_text(id, text(2));
                } else {
                    let mut mutation = doc.mutate();
                    if text(2).is_empty(){mutation.replace_children(id,&[]);}else{let child=mutation.create_text_node(text(2));mutation.replace_children(id,&[child]);}
                }
                Value::Null
            }
            "setAttr" => {
                let id = node_id()?;
                if text(3).len() > 1024 * 1024 {
                    return Err("DOM attribute budget exceeded".into());
                }
                doc.mutate().set_attribute(
                    id,
                    QualName::new(None, ns!(), LocalName::from(text(2))),
                    text(3),
                );
                Value::Null
            }
            "removeAttr" => {
                let id = node_id()?;
                doc.mutate()
                    .clear_attribute(id, QualName::new(None, ns!(), LocalName::from(text(2))));
                Value::Null
            }
            "create" | "createText" => {
                if self.allocated.get() >= self.limits.dom_nodes || text(1).len() > 1024 * 1024 {
                    return Err("DOM node budget exceeded".into());
                }
                self.allocated.set(self.allocated.get() + 1);
                let id = if text(0) == "create" {
                    doc.mutate().create_element(
                        QualName::new(None, ns!(html), LocalName::from(text(1))),
                        vec![],
                    )
                } else {
                    doc.mutate().create_text_node(text(1))
                };
                json!(id.as_u64().to_string())
            }
            "append" => {
                let parent = node_id()?;
                let child = id(2)?;
                if doc.get_node(child).is_none() {
                    return Err("Node no longer exists".into());
                }
                // Reject cycles, including putting a node below itself.
                let mut ancestor = Some(parent);
                while let Some(id) = ancestor {
                    if id == child {
                        return Err("Cannot create a DOM cycle".into());
                    }
                    ancestor = doc.get_node(id).and_then(|n| n.parent);
                }
                doc.mutate().append_children(parent, &[child]);
                json!(child.as_u64().to_string())
            }
            "remove" => {
                let id = node_id()?;
                doc.mutate().remove_node(id);
                Value::Null
            }
            "url" => json!(doc.url().as_str()),
            "navigate" => {
                let target = doc.url().join(text(1)).map_err(|e| e.to_string())?;
                if !matches!(target.scheme(), "http" | "https")
                    && !(target.scheme() == "file" && doc.url().scheme() == "file")
                {
                    return Err("Navigation scheme is not allowed".into());
                }
                if self.navigate.borrow().len() >= 8 {
                    return Err("Too many pending navigations".into());
                }
                self.navigate.borrow_mut().push(target);
                Value::Null
            }
            "now" => json!(self.start.elapsed().as_secs_f64() * 1000.0),
            "log" => {
                eprintln!(
                    "Wega JavaScript: {}",
                    text(1).chars().take(4096).collect::<String>()
                );
                Value::Null
            }
            _ => return Err("This Web API is not available; use Servo for this page".into()),
        };
        Ok(value)
    }
    fn subtree(doc: &BaseDocument, root: NodeId) -> Vec<NodeId> {
        let mut nodes = vec![];
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if let Some(node) = doc.get_node(id) {
                nodes.push(id);
                pending.extend(node.children.iter().copied());
            }
        }
        nodes
    }
    fn collect_detached(&self, doc: &mut BaseDocument, id: NodeId) {
        let mut root = id;
        while let Some(parent) = doc.get_node(root).and_then(|n| n.parent) {
            root = parent;
        }
        if root == doc.root_node().id || doc.get_node(root).is_none() {
            return;
        }
        let nodes = Self::subtree(doc, root);
        if nodes.iter().any(|id| self.retained.borrow().contains(id)) {
            return;
        }
        doc.mutate().remove_and_drop_node(root);
        self.allocated
            .set(self.allocated.get().saturating_sub(nodes.len()));
    }
    fn serialize(doc: &BaseDocument, id: NodeId, out: &mut String) {
        let Some(node) = doc.get_node(id) else {
            return;
        };
        match &node.data {
            NodeData::Text(text) => {
                let raw = node
                    .parent
                    .and_then(|id| doc.get_node(id))
                    .and_then(|n| n.element_data())
                    .is_some_and(|e| matches!(e.name.local.as_str(), "script" | "style"));
                if raw {
                    out.push_str(&text.content);
                } else {
                    out.push_str(
                        &text
                            .content
                            .replace('&', "&amp;")
                            .replace('<', "&lt;")
                            .replace('>', "&gt;"),
                    );
                }
            }
            NodeData::Comment { contents } => {
                out.push_str("<!--");
                out.push_str(contents);
                out.push_str("-->");
            }
            NodeData::Document(_) => {
                for child in &node.children {
                    Self::serialize(doc, *child, out);
                }
            }
            NodeData::Element(element) | NodeData::AnonymousBlock(element) => {
                if element.name.local.as_str() == "#document-fragment" {
                    for child in &node.children {
                        Self::serialize(doc, *child, out);
                    }
                    return;
                }
                let name = element.name.prefix.as_ref().map_or_else(
                    || element.name.local.to_string(),
                    |p| format!("{p}:{}", element.name.local),
                );
                out.push('<');
                out.push_str(&name);
                for attr in element.attrs.iter() {
                    out.push(' ');
                    if let Some(prefix) = &attr.name.prefix {
                        out.push_str(prefix);
                        out.push(':');
                    }
                    out.push_str(&attr.name.local);
                    out.push_str("=\"");
                    out.push_str(&attr.value.replace('&', "&amp;").replace('"', "&quot;"));
                    out.push('"');
                }
                out.push('>');
                let void = element.name.ns == ns!(html)
                    && matches!(
                        element.name.local.as_str(),
                        "area"
                            | "base"
                            | "br"
                            | "col"
                            | "embed"
                            | "hr"
                            | "img"
                            | "input"
                            | "link"
                            | "meta"
                            | "param"
                            | "source"
                            | "track"
                            | "wbr"
                    );
                if !void {
                    for child in &node.children {
                        Self::serialize(doc, *child, out);
                    }
                    out.push_str("</");
                    out.push_str(&name);
                    out.push('>');
                }
            }
        }
    }
}
