// ---------------------------------------------------------------------------
// wega-webidl generated bindings - family "event-target"
// Do not edit by hand. Regenerate with:
//   cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- generate
// Pinned inputs (tools/webidl/idls.json):
//   vendor/servo-script-bindings/simplified-idls/EventTarget.webidl sha256=f9837ef5b6d98eb236bce527888f29d41ba3f2696c0ad224935ea81b9fac3b74
// Hook manifest: engines/wega/src/dom/generated/event-target.hooks.json
// ---------------------------------------------------------------------------
((dom) => {
  const INTERNAL = dom.internal;
  function op(name, ...args) {
    try {
      return dom.hook(name, ...args);
    } catch (error) {
      const message = String(error && error.message !== undefined ? error.message : error);
      const cut = message.indexOf(": ");
      if (cut > 0) {
        const name = message.slice(0, cut);
        if (/^[A-Z][A-Za-z]*Error$/.test(name)) dom.fail(name, message.slice(cut + 2));
      }
      throw error;
    }
  }
  function toDomString(value) {
    return String(value);
  }
  class EventTarget extends dom.base.Node {
    constructor(type, internal, info) {
      let id, nodeInfo;
      if (internal === INTERNAL) {
        id = type;
        nodeInfo = info;
      } else {
        const created = op("evtTarget.construct.eventtarget", type === undefined ? "" : toDomString(type));
        id = created.id;
        nodeInfo = created.info;
      }
      super(id, nodeInfo);
      dom.brands([EventTarget], this);
    }
    addEventListener(type) {
      dom.checkBrand(EventTarget, this);
      op("evtTarget.addEventListener", this._id, toDomString(type));
    }
    removeEventListener(type) {
      dom.checkBrand(EventTarget, this);
      op("evtTarget.removeEventListener", this._id, toDomString(type));
    }
    dispatchEvent(type) {
      dom.checkBrand(EventTarget, this);
      op("evtTarget.dispatchEvent", this._id, toDomString(type));
    }
  }
  dom.classes.EventTarget = EventTarget;
  dom.globals.EventTarget = EventTarget;
})(globalThis.__wega_dom);
