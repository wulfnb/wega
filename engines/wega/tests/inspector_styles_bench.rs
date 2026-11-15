//! Bounded read latency and RSS; setup rendering is outside timed inspection.
use std::time::Instant;
use wega_adapter_blitz::{BlitzDocument, DocumentSetup};
use wega_dom::DocumentView;
use wega_protocol::{
    DocumentContext, DocumentId, DocumentRevision,
    identity::{FrameId, ProcessSessionId, TabId},
    inspector::{InspectorSessionId, TargetId, dom::InspectorNodeId, styles::*},
};
#[test]
#[ignore = "BENCH: read-only CSS latency and retained RSS"]
fn style_reader_latency_memory() {
    let scope = StyleScope {
        target: TargetId {
            tab: TabId(1),
            frame: FrameId(1),
            process: ProcessSessionId([1; 16]),
        },
        session: InspectorSessionId(1),
        document: DocumentContext {
            document: DocumentId(1),
            revision: DocumentRevision(1),
        },
        epoch: 0,
        generation: 0,
    };
    let rss = || wega_platform::resident_bytes().unwrap_or(0) as f64 / 1048576.;
    for(name,html,render)in[
  ("small","<!doctype html><style>p{color:red}body{font-size:17px}@media(all){p{width:42px}}</style><p id=x style='height:19px'>text</p>".to_owned(),true),
  ("large",format!("<!doctype html><style>{}</style><p id=x>text</p>",(0..20000).map(|i|format!(".n{i}{{color:red;width:19px}}")).collect::<String>()),true),
  ("huge-value",format!("<!doctype html><style>p{{--huge:{};color:red}}</style><p id=x>text</p>","x".repeat(1_000_000)),false),
  ("deep",format!("<!doctype html><style>div{{color:red}}</style><div id=x>{}x{}</div>","<div>".repeat(1000),"</div>".repeat(1000)),false),
 ]{
  let before=rss();let mut d=BlitzDocument::from_html(&html,DocumentSetup::default());if render{d.resolve(0.);}
  let built=rss();let node=InspectorNodeId{document:scope.document,key:d.element_by_id("x").unwrap().as_u64()};
  let sheets=wega_engine::inspector::styles::inspect(&d,scope,&StyleQuery::Sheets{offset:0}).unwrap();
  let StyleItem::Sheet{reference,..}=&sheets.items[0]else{panic!()};let mut rule=reference.clone();rule.path=vec![0];
  for(label,query)in[("sheets",StyleQuery::Sheets{offset:0}),("rules",StyleQuery::Rules{parent:reference.clone(),offset:if name=="large"{19968}else{0}}),("declarations",StyleQuery::Declarations{rule,offset:0}),("computed",StyleQuery::Computed{node,offset:0})]{
   let retained=rss();let mut samples=Vec::new();let mut bytes=0;let mut refused=0;
   for _ in 0..1000{let at=Instant::now();match wega_engine::inspector::styles::inspect(&d,scope,&query){Ok(p)=>{assert!(p.is_bounded());bytes=bytes.max(serde_json::to_vec(&p).unwrap().len());},Err(e)=>{assert_eq!(e.code,wega_protocol::inspector::InspectorErrorCode::Timeout);refused+=1;}};samples.push(at.elapsed().as_secs_f64()*1e6);}
   samples.sort_by(f64::total_cmp);println!("STYLE_BENCH {name} {label}: median_us={:.3} p95_us={:.3} max_us={:.3} bytes={bytes} refused={refused} rss_growth_mib={:.3}",samples[500],samples[950],samples[999],rss()-retained);
  }
  println!("STYLE_BENCH {name}: setup_rss_mib={:.3} retained_reads_rss_mib={:.3}",built-before,rss()-built);
 }
}
