import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {fileURLToPath} from 'node:url';
const file = name => fileURLToPath(new URL('../src/' + name, import.meta.url));
const find = new vm.Script(readFileSync(file('page_actions.js'), 'utf8'), {filename: file('page_actions.js')});
const reader = new vm.Script(readFileSync(file('reader.js'), 'utf8'), {filename: file('reader.js')});
function textNode(data, options = {}) {
  return {data, parentElement: options.orphan ? null : {closest: () => options.skipped, scrollIntoView() {}, style: {display: options.display || 'block', visibility: options.visibility || 'visible'}}};
}
function findFixture(nodes) {
  const ranges = [];
  const context = vm.createContext({QUERY: '', BACKWARDS: false, NodeFilter: {SHOW_TEXT: 4}, getComputedStyle: parent => parent.style, window: {getSelection: () => ({removeAllRanges() {}, addRange: range => ranges.push(range)})}, document: {
    body: {},
    createTreeWalker: () => { let index = 0; return {nextNode: () => nodes[index++]}; },
    createRange: () => ({setStart(node, offset) {this.start = [node, offset];}, setEnd(node, offset) {this.end = [node, offset];}})
  }});
  return {ranges, run(query, backwards = false) {context.QUERY = query; context.BACKWARDS = backwards; return find.runInContext(context);}};
}

test('find skips hidden and unsafe nodes and preserves text while cycling matches', () => {
  const text = textNode('Needle then NEEDLE');
  const f = findFixture([textNode('needle', {orphan: true}), textNode('needle', {skipped: true}), textNode('needle', {display: 'none'}), textNode('needle', {visibility: 'hidden'}), text]);
  assert.equal(f.run(''), 'Enter text to find');
  assert.equal(f.run('absent'), 'No matches');
  assert.equal(f.run('needle'), '1 of 2');
  assert.equal(f.run('needle'), '2 of 2');
  assert.equal(f.ranges.at(-1).start[1], 12);
  assert.equal(f.run('needle', true), '1 of 2');
  assert.equal(f.run('NEEDLE', true), '2 of 2');
  assert.equal(text.data, 'Needle then NEEDLE');
});

test('find bounds large documents and match counts', () => {
  assert.equal(findFixture([textNode('a'.repeat(10001))]).run('a'), '1 of 10000');
  const nodes = [textNode('hit'), ...Array.from({length: 50001}, () => textNode('miss'))];
  assert.equal(findFixture(nodes).run('hit'), '1 of 1 (search limit reached)');
});

test('reader uses text content and toggles without injecting page HTML', () => {
  for (const useArticle of [true, false]) {
    const ids = new Map();
    const document = {
      title: 'Title <script>unsafe()</script>',
      body: {innerText: 'Body text', appendChild(element) {ids.set(element.id, element);}},
      querySelector: () => useArticle ? {innerText: 'Article text'} : null,
      getElementById: id => ids.get(id),
      createElement: tag => ({tag, style: {}, children: [], appendChild(child) {this.children.push(child);}, remove() {ids.delete(this.id);}})
    };
    const context = vm.createContext({document});
    reader.runInContext(context);
    const panel = ids.get('__wega_reader');
    assert.equal(panel.children[1].textContent, document.title + '\n\n' + (useArticle ? 'Article text' : 'Body text'));
    assert.equal(panel.children[1].innerHTML, undefined);
    panel.children[0].onclick(); assert.equal(ids.size, 0);
    reader.runInContext(context); reader.runInContext(context); assert.equal(ids.size, 0);
  }
});

test('media failures show element-local feedback and safe downloads; source changes clear it',()=>{
  const listeners=new Map(),panels=[];
  const element=()=>({style:{},children:[],setAttribute(k,v){this[k]=v;},append(e){this.children.push(e);},remove(){this.removed=true;}});
  const document={addEventListener:(name,fn)=>listeners.set(name,fn),createElement:element};
  new vm.Script(readFileSync(file('media_fallback.js'),'utf8'),{filename:file('media_fallback.js')}).runInNewContext({document,URL,location:{href:'https://example.test/page'}});
  const media={tagName:'VIDEO',error:{code:4},currentSrc:'https://example.test/movie.webm',after(p){panels.push(p);}};
  const error=listeners.get('error');error({target:{tagName:'DIV'}});error({target:{tagName:'AUDIO'}});error({target:{tagName:'AUDIO',error:{code:1}}});assert.equal(panels.length,0);
  error({target:media});assert.equal(panels.length,1);assert.match(panels[0].children[0].textContent,/unsupported/);assert.equal(panels[0].children[1].download,'');
  media.error.code=2;error({target:media});assert.equal(panels[0].removed,true);assert.match(panels[1].children[0].textContent,/connection/);
  listeners.get('loadstart')({target:media});assert.equal(panels[1].removed,true);listeners.get('loadeddata')({target:media});
  media.currentSrc='data:video/x,x';error({target:media});assert.equal(panels.at(-1).children.length,1);
  media.currentSrc='https://[';error({target:media});assert.equal(panels.at(-1).children.length,1);
  media.currentSrc='';media.src='';error({target:media});assert.equal(panels.at(-1).children.length,1);
  media.src='https://example.test/fallback.webm';error({target:media});assert.equal(panels.at(-1).children[1].href,media.src);
});

async function maskFixture({supports=false,loading=false,links=[{href:'/w/load.php?a=1'}],response='success',css='.vector-icon.test{mask-image:url(/icon.svg)}'}={}) {
  const styles=[],listeners=new Map();let observer,fetches=0;
  const document={readyState:loading?'loading':'complete',querySelectorAll:()=>links,head:{append(s){styles.push(s);}},addEventListener:(name,fn)=>listeners.set(name,fn),createElement:()=>({setAttribute(){}})};
  const script=new vm.Script(readFileSync(file('mask_icons.js'),'utf8'),{filename:file('mask_icons.js')});
  script.runInNewContext({document,CSS:{supports:()=>supports},URL,location:{href:'https://example.test/page',origin:'https://example.test'},MutationObserver:class{constructor(fn){observer=fn;}observe(){}},fetch:async()=>{fetches++;if(response==='error')throw Error('offline');return {ok:response!=='bad',text:async()=>css};}});
  if(loading)listeners.get('DOMContentLoaded')();
  await new Promise(setImmediate);
  return {styles,observer,fetches:()=>fetches};
}
test('Vector mask fallback retires with native support and keeps original same-origin SVG/selectors',async()=>{
  assert.equal((await maskFixture({supports:true})).fetches(),0);
  const f=await maskFixture({loading:true,links:[{href:'https://['},{href:'https://other.test/w/load.php'},{href:'/other.css'},{href:'/w/load.php?a=1'},{href:'/w/load.php?a=1'}],css:'.vector-icon.test{mask-image:url("/icon.svg")} .vector-icon.other{mask-image:url(\'/other.svg\')} .unrelated{mask-image:url(/no.svg)} .vector-icon.nope{mask-image:none} .vector-icon.remote{mask-image:url(https://other.test/i.svg)}'});
  assert.equal(f.fetches(),1);assert.equal(f.styles.length,1);assert.match(f.styles[0].textContent,/example.test\/icon.svg/);assert.match(f.styles[0].textContent,/background-color:transparent/);assert.doesNotMatch(f.styles[0].textContent,/unrelated|remote|nope/);
  f.observer([{addedNodes:[{tagName:'DIV'}]}]);f.observer([{addedNodes:[{tagName:'LINK'}]}]);await new Promise(setImmediate);assert.equal(f.fetches(),1);
  for(const response of ['bad','error']) assert.equal((await maskFixture({response})).styles.length,0);
  assert.equal((await maskFixture({css:'x'.repeat(2097153)})).styles.length,0);
  assert.equal((await maskFixture({css:'.vector-icon.x{mask-image:none}'})).styles.length,0);
  assert.equal((await maskFixture({css:'.vector-icon.x{mask-image:url(https://[)}'})).styles.length,0);
});
