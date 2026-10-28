// Track edits, rather than treating every loaded document as unsaved work.
(() => {
  if(globalThis.__wega_page_state_document===document) return;
  Object.defineProperty(globalThis,'__wega_page_state_document',{value:document,configurable:true});
  const baseline = new WeakMap(), edited = new Set();
  const value = e => {
    if (e.localName === 'input' && /^(checkbox|radio)$/.test(e.type)) return e.checked;
    if (e.localName === 'select') return Array.from(e.options).map(o => o.selected).join(',');
    if (e.localName === 'input' || e.localName === 'textarea') return e.value;
    return e.textContent;
  };
  const initial = e => {
    if (e.localName === 'input' && /^(checkbox|radio)$/.test(e.type)) return e.defaultChecked;
    if (e.localName === 'select') {
      const options=Array.from(e.options), hasDefault=options.some(o=>o.defaultSelected);
      return options.map((o,i)=>o.defaultSelected || (!hasDefault && !e.multiple && i===0)).join(',');
    }
    return e.defaultValue ?? e.textContent;
  };
  const target = event => {
    const e=event.composedPath?.()[0] || event.target;
    return e?.closest?.('input,textarea,select,[contenteditable=""],[contenteditable="true"],[role="textbox"]');
  };
  for(const type of ['focus','focusin']) document.addEventListener(type, event => {
    const e=target(event);
    if(e && !baseline.has(e)) baseline.set(e,value(e));
  },true);
  for(const type of ['input','change']) document.addEventListener(type,event=>{
    const e=target(event);
    if(e) {if(!baseline.has(e)) baseline.set(e,initial(e)); edited.add(e);}
  },true);
  document.addEventListener('reset',event=>{
    queueMicrotask(()=>{
      if(event.defaultPrevented) return;
      for(const e of edited) if(e.form===event.target) {edited.delete(e);baseline.delete(e);}
    });
  },true);
  Object.defineProperty(globalThis,'__wega_page_has_edits',{configurable:true,value:()=>{
    if(globalThis.__wega_page_state_document!==document) return true;
    for(const e of edited) if(e.isConnected && value(e)!==baseline.get(e)) return true;
    // Same-origin frame edits also belong to this tab. Inaccessible frames use
    // their own engine unload prompts; do not prompt for untouched embeds.
    for(const frame of document.querySelectorAll('iframe,frame')) {
      try {if(frame.contentWindow?.__wega_page_has_edits?.()) return true;} catch (_) {}
    }
    return false;
  }});
})();
