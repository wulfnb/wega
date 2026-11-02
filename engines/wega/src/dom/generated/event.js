// ---------------------------------------------------------------------------
// wega-webidl generated bindings - family "event"
// Do not edit by hand. Regenerate with:
//   cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- generate
// Pinned inputs (tools/webidl/idls.json):
//   vendor/servo-script-bindings/simplified-idls/Event.simplified.webidl sha256=4008b8e640f1461e692cd8097e3ee4d864e919ed575fa4599d353c6a4fff3211
// Hook manifest: engines/wega/src/dom/generated/event.hooks.json
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
  function toUnsignedShort(value) {
    let number = Number(value);
    if (Number.isNaN(number)) number = 0;
    number = Math.trunc(number);
    return ((number % 65536) + 65536) % 65536;
  }
  function toDouble(value) {
    const number = Number(value);
    return Number.isNaN(number) ? 0 : number;
  }
  function toBoolean(value) {
    return Boolean(value);
  }
  class Event extends dom.base.Node {
    constructor(type, internal, info) {
      let id, nodeInfo;
      if (internal === INTERNAL) {
        id = type;
        nodeInfo = info;
      } else {
        const created = op("event.construct.event", toDomString(type));
        id = created.id;
        nodeInfo = created.info;
      }
      super(id, nodeInfo);
      dom.brands([Event], this);
    }
    get type() {
      dom.checkBrand(Event, this);
      return op("event.type", this._id);
    }
    static {
      Object.defineProperty(this, "NONE", { value: 0, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "CAPTURING_PHASE", { value: 1, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "AT_TARGET", { value: 2, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "BUBBLING_PHASE", { value: 3, writable: false, enumerable: true, configurable: true });
    }
    get eventPhase() {
      dom.checkBrand(Event, this);
      return op("event.eventPhase", this._id);
    }
    stopPropagation() {
      dom.checkBrand(Event, this);
      const result = op("event.stopPropagation", this._id);
      dom.reactions.stopPropagation(this, result);
    }
    get cancelBubble() {
      dom.checkBrand(Event, this);
      return op("event.cancelBubble", this._id);
    }
    set cancelBubble(value) {
      dom.checkBrand(Event, this);
      const result = op("event.cancelBubble.set", this._id, toBoolean(value));
      dom.reactions.stopPropagation(this, result);
    }
    stopImmediatePropagation() {
      dom.checkBrand(Event, this);
      const result = op("event.stopImmediatePropagation", this._id);
      dom.reactions.stopImmediatePropagation(this, result);
    }
    get bubbles() {
      dom.checkBrand(Event, this);
      return op("event.bubbles", this._id);
    }
    get cancelable() {
      dom.checkBrand(Event, this);
      return op("event.cancelable", this._id);
    }
    get returnValue() {
      dom.checkBrand(Event, this);
      return op("event.returnValue", this._id);
    }
    set returnValue(value) {
      dom.checkBrand(Event, this);
      const result = op("event.returnValue.set", this._id, toBoolean(value));
    }
    preventDefault() {
      dom.checkBrand(Event, this);
      const result = op("event.preventDefault", this._id);
      dom.reactions.preventDefault(this, result);
    }
    get defaultPrevented() {
      dom.checkBrand(Event, this);
      return op("event.defaultPrevented", this._id);
    }
    get composed() {
      dom.checkBrand(Event, this);
      return op("event.composed", this._id);
    }
    get isTrusted() {
      dom.checkBrand(Event, this);
      return op("event.isTrusted", this._id);
    }
    get timeStamp() {
      dom.checkBrand(Event, this);
      return op("event.timeStamp", this._id);
    }
    initEvent(type) {
      dom.checkBrand(Event, this);
      const result = op("event.initEvent", this._id, toDomString(type));
      dom.reactions.initEvent(this, result);
    }
  }
  dom.classes.Event = Event;
  dom.globals.Event = Event;
})(globalThis.__wega_dom);
