//! CSS behaviour through the JavaScript API: inline styles, the cascade, computed values,
//! stylesheet changes, invalidation after DOM mutation and the current Shadow DOM limits.
//! The engine reaches styles only through `wega_dom::style`; these tests pin what script sees.
use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url, View};

fn page(html: &str) -> View {
    let mut view = Engine::new(
        Limits::default(),
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .create_view(Url::parse("about:blank").unwrap())
    .unwrap();
    view.set_html(&format!("<!doctype html>{html}")).unwrap();
    view.poll();
    view.paint(200, 200, 1.0, 1.0, false).unwrap();
    view
}
/// Runs `body` (statements ending in a `return`) with `$` = getElementById and
/// `cs(element, property)` = the computed value.
fn js(view: &mut View, body: &str) -> String {
    view.evaluate_json(&format!(
        "(()=>{{const $=id=>document.getElementById(id);const cs=(e,p)=>getComputedStyle(e).getPropertyValue(p);{body}}})()"
    ))
    .unwrap()
}
const BASE: &str = "<style id=s>div{color:red}.a{color:blue}#x{color:green}</style>\
    <div id=x class=a style='width: 10px; COLOR: Purple; bogus: 1'><p id=p>t</p></div><div id=y class=a></div>";

#[test]
fn inline_style_reads_are_canonical_and_drop_invalid_declarations() {
    let mut v = page(BASE);
    assert_eq!(
        js(
            &mut v,
            "const s=$('x').style;return [s.cssText,s.length,s.item(0),s[1],s.color,s.getPropertyValue('width'),s.backgroundColor,s.getPropertyValue('nonsense')]"
        ),
        r#"["width: 10px; color: purple;",2,"width","color","purple","10px","",""]"#
    );
    assert_eq!(
        js(
            &mut v,
            "const e=$('y');e.setAttribute('style','color:blue;;;width:;height:4px');return [e.style.cssText,e.getAttribute('style')]"
        ),
        r#"["color: blue; height: 4px;","color:blue;;;width:;height:4px"]"#,
        "the attribute keeps its text; the declaration block recovers from the invalid parts"
    );
    assert_eq!(
        js(
            &mut v,
            "const e=$('y');e.style.cssText='color:blue;bogus:1;width:3px';return [e.getAttribute('style'),e.style.cssText,e.style.length]"
        ),
        r#"["color:blue;bogus:1;width:3px","color: blue; width: 3px;",2]"#
    );
}

#[test]
fn set_property_serializes_canonically_and_handles_priority_shorthands_and_removal() {
    let mut v = page(BASE);
    assert_eq!(
        js(
            &mut v,
            "const e=$('y');e.style.setProperty('width','5px');e.style.height='7px';return [e.getAttribute('style'),e.style.cssText]"
        ),
        r#"["width: 5px; height: 7px;","width: 5px; height: 7px;"]"#
    );
    assert_eq!(
        js(
            &mut v,
            "const e=$('y');e.style.setProperty('color','red','important');return [e.getAttribute('style'),e.style.getPropertyPriority('color'),e.style.getPropertyPriority('width'),cs(e,'color')]"
        ),
        r#"["width: 5px; height: 7px; color: red !important;","important","","rgb(255, 0, 0)"]"#
    );
    assert_eq!(
        js(
            &mut v,
            "const e=$('y');e.style.margin='1px 2px';return [e.style.marginLeft,e.style.margin,cs(e,'margin-top')]"
        ),
        r#"["2px","1px 2px","1px"]"#
    );
    assert_eq!(
        js(
            &mut v,
            "const e=$('y');return [e.style.removeProperty('width'),e.style.removeProperty('margin'),e.style.removeProperty('width'),e.getAttribute('style')]"
        ),
        r#"["5px","1px 2px","","height: 7px; color: red !important;"]"#
    );
    assert_eq!(
        js(
            &mut v,
            "const e=$('y');e.style.setProperty('color','blue','bogus-priority');e.style.color=null;return e.getAttribute('style')"
        ),
        r#""height: 7px;""#,
        "an unknown priority is ignored; null removes"
    );
}

#[test]
fn an_inline_write_is_seen_by_computed_style_geometry_and_paint() {
    let mut v = page(
        "<style>body{margin:0}div{width:20px;height:10px;background:rgb(255,0,0)}</style><div id=a></div><div id=b></div>",
    );
    assert_eq!(
        js(
            &mut v,
            "const a=$('a'),b=$('b'),r=[cs(a,'height'),b.getBoundingClientRect().top];a.style.height='30px';r.push(cs(a,'height'),a.getBoundingClientRect().height,b.getBoundingClientRect().top,a.offsetHeight);\
            a.style.setProperty('background-color','rgb(0, 255, 0)');a.style.removeProperty('height');r.push(cs(a,'height'),b.getBoundingClientRect().top,cs(a,'background-color'));return r"
        ),
        r#"["10px",10,"30px",30,30,30,"10px",10,"rgb(0, 255, 0)"]"#
    );
    let frame = v.paint(200, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(
        &frame.rgba[(5 * 200 + 5) * 4..(5 * 200 + 5) * 4 + 4],
        &[0, 255, 0, 255],
        "#a repainted"
    );
    assert_eq!(
        &frame.rgba[(15 * 200 + 5) * 4..(15 * 200 + 5) * 4 + 4],
        &[255, 0, 0, 255],
        "#b back at y=10"
    );
    v.evaluate("document.getElementById('a').style.height='30px'")
        .unwrap();
    assert!(v.needs_paint(), "an inline write requests a frame");
    let frame = v.paint(200, 200, 1.0, 1.0, false).unwrap();
    assert_eq!(
        &frame.rgba[(25 * 200 + 5) * 4..(25 * 200 + 5) * 4 + 4],
        &[0, 255, 0, 255]
    );
}

#[test]
fn invalid_values_are_ignored_and_queue_no_mutation_record() {
    let mut v = page(BASE);
    assert_eq!(
        js(
            &mut v,
            "const e=$('y');e.style.width='5px';const seen=[];const o=new MutationObserver(r=>seen.push(...r));o.observe(e,{attributes:true,attributeOldValue:true});\
            e.style.setProperty('color','notacolor');e.style.width='5';e.style.setProperty('nonsense','1');e.style.color='red !important';e.style.width='5px';\
            const quiet=o.takeRecords().length;e.style.width='6px';const records=o.takeRecords();\
            return [e.getAttribute('style'),e.style.cssText,cs(e,'color'),quiet,records.length,records[0].attributeName,records[0].oldValue]"
        ),
        r#"["width: 6px;","width: 6px;","rgb(0, 0, 255)",0,1,"style","width: 5px;"]"#,
        "four rejected writes and one unchanged write queue nothing; the real change queues one record"
    );
}

#[test]
fn custom_properties_are_set_read_inherited_and_substituted() {
    let mut v = page(
        "<style>#o{--c: 4px}#i{margin-left:var(--c);padding-left:var(--missing, 3px)}</style><div id=o><div id=i></div></div>",
    );
    assert_eq!(
        js(
            &mut v,
            "return [cs($('o'),'--c'),cs($('i'),'--c'),cs($('i'),'margin-left'),cs($('i'),'padding-left')]"
        ),
        r#"["4px","4px","4px","3px"]"#
    );
    assert_eq!(
        js(
            &mut v,
            "const o=$('o');o.style.setProperty('--c',' 9px ');return [o.getAttribute('style'),o.style.getPropertyValue('--c'),cs($('i'),'margin-left')]"
        ),
        r#"["--c: 9px;","9px","9px"]"#
    );
    assert_eq!(
        js(
            &mut v,
            "$('o').style.removeProperty('--c');return [$('o').getAttribute('style'),cs($('i'),'margin-left')]"
        ),
        r#"["","4px"]"#
    );
}

#[test]
fn cascade_specificity_importance_inheritance_and_source_order() {
    let mut v = page(
        "<style>p{color:rgb(1,1,1)}.c{color:rgb(2,2,2)}#q{color:rgb(3,3,3)}p.c{background-color:rgb(4,4,4)}.c{background-color:rgb(5,5,5)}\
         .imp{color:rgb(6,6,6)!important}section{color:rgb(7,7,7);border:1px solid red}.late{outline-width:1px}.late{outline-width:2px;outline-style:solid}</style>\
         <p id=a>a</p><p id=b class=c>b</p><p id=q class=c>q</p><p id=i class='c imp' style='color:rgb(9,9,9)'>i</p>\
         <section id=sec><span id=child class=late>c</span></section>",
    );
    assert_eq!(
        js(
            &mut v,
            "return [cs($('a'),'color'),cs($('b'),'color'),cs($('q'),'color'),cs($('b'),'background-color'),cs($('i'),'color'),cs($('child'),'color'),cs($('child'),'border-top-width'),cs($('child'),'outline-width')]"
        ),
        r#"["rgb(1, 1, 1)","rgb(2, 2, 2)","rgb(3, 3, 3)","rgb(4, 4, 4)","rgb(6, 6, 6)","rgb(7, 7, 7)","0px","2px"]"#,
        "type < class < id; p.c beats the later .c; !important beats inline; color inherits, border does not; later rule wins"
    );
    assert_eq!(
        js(
            &mut v,
            "$('i').style.setProperty('color','rgb(8,8,8)','important');return cs($('i'),'color')"
        ),
        r#""rgb(8, 8, 8)""#,
        "inline !important beats author !important"
    );
}

#[test]
fn stylesheets_follow_insertion_removal_and_text_changes() {
    let mut v = page(
        "<style id=one>#t{color:rgb(1,1,1);opacity:0.5}</style><style id=two>#t{color:rgb(2,2,2)}</style><div id=t>t</div>",
    );
    assert_eq!(
        js(&mut v, "return [cs($('t'),'color'),cs($('t'),'opacity')]"),
        r#"["rgb(2, 2, 2)","0.5"]"#,
        "parsed sheets cascade in document order"
    );
    assert_eq!(
        js(
            &mut v,
            "const s=document.createElement('style');s.textContent='#t{color:rgb(3,3,3)}';const detached=cs($('t'),'color');\
            document.head.appendChild(s);const last=cs($('t'),'color');s.remove();return [detached,last,cs($('t'),'color')]"
        ),
        r#"["rgb(2, 2, 2)","rgb(3, 3, 3)","rgb(2, 2, 2)"]"#,
        "a sheet applies only while its element is in the document"
    );
    assert_eq!(
        js(
            &mut v,
            "$('two').textContent='#t{color:rgb(4,4,4)}';const changed=cs($('t'),'color');$('two').remove();$('one').remove();return [changed,cs($('t'),'color'),cs($('t'),'opacity')]"
        ),
        r#"["rgb(4, 4, 4)","rgb(0, 0, 0)","1"]"#
    );
}

#[test]
fn style_elements_outside_the_document_have_no_stylesheet() {
    let mut v = page("<span id=o>o</span>");
    assert_eq!(
        js(
            &mut v,
            "const o=$('o'),r=[];const s=document.createElement('style');s.textContent='#o{color:rgb(8,8,8)}';r.push(cs(o,'color'));\
            const d=document.createElement('div');d.appendChild(s);r.push(cs(o,'color'));d.innerHTML='<style>#o{color:rgb(7,7,7)}</style>';r.push(cs(o,'color'));\
            const t=document.createElement('template');t.innerHTML='<style>#o{color:rgb(6,6,6)}</style>';r.push(cs(o,'color'));\
            document.body.appendChild(d);r.push(cs(o,'color'));d.remove();r.push(cs(o,'color'));\
            document.body.appendChild(t.content.cloneNode(true));r.push(cs(o,'color'));return r"
        ),
        r#"["rgb(0, 0, 0)","rgb(0, 0, 0)","rgb(0, 0, 0)","rgb(0, 0, 0)","rgb(7, 7, 7)","rgb(0, 0, 0)","rgb(6, 6, 6)"]"#,
        "never-inserted, detached-parent, fragment and template styles apply only once connected"
    );
}

/// Known deviation (Blitz orders author sheets by node id, i.e. creation order, not tree
/// order): a script-created `<style>` inserted *before* existing sheets still cascades last.
/// This pins the current behaviour so a fix is noticed; the correct value is `rgb(2, 2, 2)`.
#[test]
fn known_gap_a_script_inserted_stylesheet_cascades_in_creation_order() {
    let mut v = page(
        "<style id=one>#t{color:rgb(1,1,1)}</style><style id=two>#t{color:rgb(2,2,2)}</style><div id=t>t</div>",
    );
    assert_eq!(
        js(
            &mut v,
            "const s=document.createElement('style');s.textContent='#t{color:rgb(3,3,3)}';document.head.insertBefore(s,$('one'));return cs($('t'),'color')"
        ),
        r#""rgb(3, 3, 3)""#
    );
}

#[test]
fn computed_style_follows_dom_mutations() {
    let mut v = page(
        "<style>.on{color:rgb(1,1,1)}[data-k=v]{opacity:0.5}#box>span{color:rgb(2,2,2)}div:empty{height:9px}</style><div id=box></div><span id=s>s</span><div id=other></div>",
    );
    assert_eq!(
        js(
            &mut v,
            "const s=$('s'),b=$('box');const r=[cs(s,'color'),cs(b,'height')];s.className='on';r.push(cs(s,'color'));s.setAttribute('data-k','v');r.push(cs(s,'opacity'));\
            b.appendChild(s);r.push(cs(s,'color'),cs(b,'height')!=='9px');s.classList.remove('on');s.removeAttribute('data-k');r.push(cs(s,'color'),cs(s,'opacity'));\
            $('other').appendChild(s);r.push(cs(s,'color'),cs(b,'height'));return r"
        ),
        r#"["rgb(0, 0, 0)","9px","rgb(1, 1, 1)","0.5","rgb(2, 2, 2)",true,"rgb(2, 2, 2)","1","rgb(0, 0, 0)","9px"]"#,
        "class, attribute, parent and :empty changes all re-cascade (#box>span outranks .on)"
    );
    assert_eq!(
        js(
            &mut v,
            "const s=$('s');s.id='renamed';const st=document.createElement('style');st.textContent='#renamed{color:rgb(5,5,5)}';document.head.appendChild(st);return cs(s,'color')"
        ),
        r#""rgb(5, 5, 5)""#
    );
}

#[test]
fn computed_style_is_read_only_live_and_empty_outside_the_document() {
    let mut v = page(BASE);
    assert_eq!(
        js(
            &mut v,
            "const e=document.createElement('div');e.style.color='red';const c=getComputedStyle(e);const detached=[c.color,c.getPropertyValue('display'),c.cssText,c.length];\
            document.body.appendChild(e);const attached=c.color;e.remove();return [detached,attached,c.color,e.style.color]"
        ),
        r#"[["","","",0],"rgb(255, 0, 0)","","red"]"#,
        "a computed declaration never answers from the inline style and is live"
    );
    assert_eq!(
        js(
            &mut v,
            "const c=getComputedStyle($('x'));const r=[];for(const f of [()=>{c.color='red'},()=>c.setProperty('color','red'),()=>{c.cssText='color:red'},()=>c.removeProperty('color')]){try{f();r.push('no error')}catch(e){r.push(e.name)}}return [r,c.color,cs($('x'),'width'),cs($('x'),'display'),cs($('x'),'nonsense'),c.fontSize,c.cssFloat]"
        ),
        r#"[["NoModificationAllowedError","NoModificationAllowedError","NoModificationAllowedError","NoModificationAllowedError"],"rgb(128, 0, 128)","10px","block","","16px","none"]"#
    );
}

#[test]
fn invalid_css_is_recovered_from_rule_by_rule() {
    let mut v = page(
        "<style>#t{opacity:0.75;;color:} } } @bogus { x } #t{z-index:5;position:relative;width:nope;height:3px} #t{top:1px</style><div id=t>t</div>",
    );
    assert_eq!(
        js(
            &mut v,
            "const t=$('t');return [cs(t,'opacity'),cs(t,'z-index'),cs(t,'position'),cs(t,'height'),cs(t,'top'),cs(t,'color')]"
        ),
        r#"["0.75","5","relative","3px","1px","rgb(0, 0, 0)"]"#
    );
}

#[test]
fn css_supports_and_match_media() {
    let mut v = page(BASE);
    assert_eq!(
        js(
            &mut v,
            "return [CSS.supports('display','grid'),CSS.supports('display','nope'),CSS.supports('nope','1'),CSS.supports('--x','1'),CSS.supports('(display:grid) and (not (color:nope))'),CSS.supports('color: red'),CSS.supports('garbage('),\
            matchMedia('(min-width:1px)').matches,matchMedia('(max-width:1px)').matches,matchMedia('print').matches,matchMedia('screen').matches,matchMedia('garbage(').matches]"
        ),
        "[true,false,false,true,true,true,false,true,false,false,true,false]"
    );
    assert_eq!(
        js(
            &mut v,
            "try{CSS.supports();return 'no error'}catch(e){return e.name}"
        ),
        r#""TypeError""#
    );
}

#[test]
fn a_replaced_document_keeps_no_styles_from_the_old_one() {
    let mut v = page("<style>#a{color:rgb(3,3,3)}</style><div id=a style='width:5px'>a</div>");
    assert_eq!(
        js(
            &mut v,
            "globalThis.old=getComputedStyle($('a'));return [old.color,old.width]"
        ),
        r#"["rgb(3, 3, 3)","5px"]"#
    );
    v.set_html("<!doctype html><div id=a>a</div>").unwrap();
    v.poll();
    assert_eq!(
        js(
            &mut v,
            "return [cs($('a'),'color'),$('a').style.cssText,typeof globalThis.old]"
        ),
        r#"["rgb(0, 0, 0)","","undefined"]"#
    );
}

// --- Shadow DOM: current behaviour (parity), not conformance. Shadow trees are not styled,
// laid out or painted; see docs/architecture/dom-round9-report.md for the missing semantics.

const SHADOW: &str = "<style>span{color:rgb(1,1,1)}.o{color:rgb(2,2,2)}</style>\
    <div id=h><span id=l class=o>light</span></div><span id=out>o</span>\
    <script>globalThis.root=document.getElementById('h').attachShadow({mode:'open'});\
    root.innerHTML='<style>span{color:rgb(5,5,5)}:host{color:rgb(6,6,6);display:inline-block}:host-context(body){opacity:0.5}::slotted(span){color:rgb(8,8,8)}#out{color:rgb(9,9,9)}</style><span id=in style=\"width:3px\">in</span><slot></slot>'</script>";

#[test]
fn shadow_styles_do_not_leak_into_the_document() {
    let mut v = page(SHADOW);
    assert_eq!(
        js(
            &mut v,
            "return [cs($('out'),'color'),$('out').matches('#out'),root.querySelector('style')!==null,document.querySelectorAll('style').length]"
        ),
        r#"["rgb(1, 1, 1)",true,true,1]"#,
        "a rule in a shadow <style> does not style a document element"
    );
}

#[test]
fn shadow_tree_styling_is_not_implemented_and_is_reported() {
    let mut v = page(SHADOW);
    assert_eq!(
        js(
            &mut v,
            "const i=root.getElementById('in');const b=i.getBoundingClientRect();return [cs(i,'color'),cs(i,'display'),i.style.cssText,b.width,b.height]"
        ),
        r#"["","","width: 3px;",0,0]"#,
        "shadow-tree elements have an inline style but no computed style and no box"
    );
    assert_eq!(
        js(
            &mut v,
            "const h=$('h'),l=$('l');return [cs(h,'color'),cs(h,'display'),cs(h,'opacity'),cs(l,'color'),l.getBoundingClientRect().height>0,typeof root.adoptedStyleSheets,typeof document.adoptedStyleSheets]"
        ),
        r#"["rgb(0, 0, 0)","block","1","rgb(2, 2, 2)",true,"undefined","undefined"]"#,
        ":host, :host-context and ::slotted have no effect; light children render unflattened with document styles; no adopted stylesheets"
    );
    assert_eq!(
        js(
            &mut v,
            "const i=root.getElementById('in');i.style.color='rgb(4,4,4)';const s=document.createElement('style');s.textContent='span{color:red}';root.appendChild(s);return [i.style.color,cs(i,'color'),cs($('out'),'color')]"
        ),
        r#"["rgb(4, 4, 4)","","rgb(1, 1, 1)"]"#,
        "dynamic updates inside a shadow tree change inline state only and still do not leak"
    );
    assert_eq!(
        v.compatibility.as_deref(),
        Some("Shadow DOM scoped style and flattened layout are not implemented by Wega")
    );
}
