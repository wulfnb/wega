// ---------------------------------------------------------------------------
// wega-webidl generated bindings - family "element"
// Do not edit by hand. Regenerate with:
//   cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- generate
// Pinned inputs (tools/webidl/idls.json):
//   vendor/servo-script-bindings/simplified-idls/Element.simplified.webidl sha256=e00e3cef5ee8c57970f68d21272930fb0a8455ac67dd3e0b929c8c5eb92f115e
// Hook manifest: engines/wega/src/dom/generated/element.hooks.json
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
  function toUnsignedLong(value) {
    let number = Number(value);
    if (Number.isNaN(number)) number = 0;
    number = Math.trunc(number);
    return ((number % 4294967296) + 4294967296) % 4294967296;
  }
  function toDouble(value) {
    const number = Number(value);
    return Number.isNaN(number) ? 0 : number;
  }
  function toBoolean(value) {
    return Boolean(value);
  }
  class Element extends dom.base.EventTarget {
    constructor(localName, internal, info) {
      let id, nodeInfo;
      if (internal === INTERNAL) {
        id = localName;
        nodeInfo = info;
      } else {
        const created = op("element.construct.element", toDomString(localName));
        id = created.id;
        nodeInfo = created.info;
      }
      super(id, nodeInfo);
      dom.brands([Element], this);
    }
    get namespaceURI() {
      dom.checkBrand(Element, this);
      return op("element.namespaceURI", this._id);
    }
    get prefix() {
      dom.checkBrand(Element, this);
      return op("element.prefix", this._id);
    }
    get localName() {
      dom.checkBrand(Element, this);
      return op("element.localName", this._id);
    }
    get tagName() {
      dom.checkBrand(Element, this);
      return op("element.tagName", this._id);
    }
    get id() {
      dom.checkBrand(Element, this);
      return op("element.id", this._id);
    }
    set id(value) {
      dom.checkBrand(Element, this);
      const result = op("element.id.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get className() {
      dom.checkBrand(Element, this);
      return op("element.className", this._id);
    }
    set className(value) {
      dom.checkBrand(Element, this);
      const result = op("element.className.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get classList() {
      dom.checkBrand(Element, this);
      if (this[dom._so]) return this[dom._so].get("classList");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("classList");
      if (!_v) { _v = op("element.classList", this._id); _m.set("classList", _v); }
      return _v;
    }
    get slot() {
      dom.checkBrand(Element, this);
      return op("element.slot", this._id);
    }
    set slot(value) {
      dom.checkBrand(Element, this);
      const result = op("element.slot.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    hasAttributes() {
      dom.checkBrand(Element, this);
      return op("element.hasAttributes", this._id);
    }
    get attributes() {
      dom.checkBrand(Element, this);
      if (this[dom._so]) return this[dom._so].get("attributes");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("attributes");
      if (!_v) { _v = op("element.attributes", this._id); _m.set("attributes", _v); }
      return _v;
    }
    getAttributeNames() {
      dom.checkBrand(Element, this);
      return op("element.getAttributeNames", this._id);
    }
    getAttribute(name) {
      dom.checkBrand(Element, this);
      return op("element.getAttribute", this._id, toDomString(name));
    }
    getAttributeNS(namespace, localName) {
      dom.checkBrand(Element, this);
      return op("element.getAttributeNS", this._id, toDomString(namespace), toDomString(localName));
    }
    toggleAttribute(name, force) {
      dom.checkBrand(Element, this);
      return op("element.toggleAttribute", this._id, toDomString(name), force === undefined ? false : toBoolean(force));
    }
    setAttribute(name, value) {
      dom.checkBrand(Element, this);
      const result = op("element.setAttribute", this._id, toDomString(name), toDomString(value));
      dom.reactions.characterData(this, result);
    }
    setAttributeNS(namespace, name, value) {
      dom.checkBrand(Element, this);
      const result = op("element.setAttributeNS", this._id, toDomString(namespace), toDomString(name), toDomString(value));
      dom.reactions.characterData(this, result);
    }
    removeAttribute(name) {
      dom.checkBrand(Element, this);
      const result = op("element.removeAttribute", this._id, toDomString(name));
      dom.reactions.characterData(this, result);
    }
    removeAttributeNS(namespace, name) {
      dom.checkBrand(Element, this);
      const result = op("element.removeAttributeNS", this._id, toDomString(namespace), toDomString(name));
      dom.reactions.characterData(this, result);
    }
    hasAttribute(name) {
      dom.checkBrand(Element, this);
      return op("element.hasAttribute", this._id, toDomString(name));
    }
    hasAttributeNS(namespace, name) {
      dom.checkBrand(Element, this);
      return op("element.hasAttributeNS", this._id, toDomString(namespace), toDomString(name));
    }
    closest(selectors) {
      dom.checkBrand(Element, this);
      return op("element.closest", this._id, toDomString(selectors));
    }
    matches(selectors) {
      dom.checkBrand(Element, this);
      return op("element.matches", this._id, toDomString(selectors));
    }
    webkitMatchesSelector(selectors) {
      dom.checkBrand(Element, this);
      return op("element.webkitMatchesSelector", this._id, toDomString(selectors));
    }
    getElementsByTagName(localName) {
      dom.checkBrand(Element, this);
      return op("element.getElementsByTagName", this._id, toDomString(localName));
    }
    getElementsByTagNameNS(namespace, localName) {
      dom.checkBrand(Element, this);
      return op("element.getElementsByTagNameNS", this._id, toDomString(namespace), toDomString(localName));
    }
    getElementsByClassName(classNames) {
      dom.checkBrand(Element, this);
      return op("element.getElementsByClassName", this._id, toDomString(classNames));
    }
    insertAdjacentElement(where_, element) {
      dom.checkBrand(Element, this);
      return op("element.insertAdjacentElement", this._id, toDomString(where_), toDomString(element));
    }
    insertAdjacentText(where_, data) {
      dom.checkBrand(Element, this);
      op("element.insertAdjacentText", this._id, toDomString(where_), toDomString(data));
    }
    insertAdjacentHTML(position, string) {
      dom.checkBrand(Element, this);
      const result = op("element.insertAdjacentHTML", this._id, toDomString(position), toDomString(string));
      dom.reactions.characterData(this, result);
    }
    attachShadow(init) {
      dom.checkBrand(Element, this);
      op("element.attachShadow", this._id, toDomString(init));
    }
    get shadowRoot() {
      dom.checkBrand(Element, this);
      return op("element.shadowRoot", this._id);
    }
    get customElementRegistry() {
      dom.checkBrand(Element, this);
      return op("element.customElementRegistry", this._id);
    }
    getClientRects() {
      dom.checkBrand(Element, this);
      return op("element.getClientRects", this._id);
    }
    getBoundingClientRect() {
      dom.checkBrand(Element, this);
      const result = op("element.getBoundingClientRect", this._id);
      const next = dom.wrap(result.id, this.ownerDocument || this);
      return next;
    }
    scrollIntoView(arg) {
      dom.checkBrand(Element, this);
      op("element.scrollIntoView", this._id, arg === undefined ? "" : toDomString(arg));
    }
    scroll(options) {
      dom.checkBrand(Element, this);
      op("element.scroll", this._id, options === undefined ? "" : toDomString(options));
    }
    scrollTo(options) {
      dom.checkBrand(Element, this);
      op("element.scrollTo", this._id, options === undefined ? "" : toDomString(options));
    }
    scrollBy(options) {
      dom.checkBrand(Element, this);
      op("element.scrollBy", this._id, options === undefined ? "" : toDomString(options));
    }
    get scrollTop() {
      dom.checkBrand(Element, this);
      return op("element.scrollTop", this._id);
    }
    set scrollTop(value) {
      dom.checkBrand(Element, this);
      const result = op("element.scrollTop.set", this._id, toDouble(value));
    }
    get scrollLeft() {
      dom.checkBrand(Element, this);
      return op("element.scrollLeft", this._id);
    }
    set scrollLeft(value) {
      dom.checkBrand(Element, this);
      const result = op("element.scrollLeft.set", this._id, toDouble(value));
    }
    get scrollWidth() {
      dom.checkBrand(Element, this);
      return op("element.scrollWidth", this._id);
    }
    get scrollHeight() {
      dom.checkBrand(Element, this);
      return op("element.scrollHeight", this._id);
    }
    get clientTop() {
      dom.checkBrand(Element, this);
      return op("element.clientTop", this._id);
    }
    get clientLeft() {
      dom.checkBrand(Element, this);
      return op("element.clientLeft", this._id);
    }
    get clientWidth() {
      dom.checkBrand(Element, this);
      return op("element.clientWidth", this._id);
    }
    get clientHeight() {
      dom.checkBrand(Element, this);
      return op("element.clientHeight", this._id);
    }
    get currentCSSZoom() {
      dom.checkBrand(Element, this);
      return op("element.currentCSSZoom", this._id);
    }
    setHTMLUnsafe(html) {
      dom.checkBrand(Element, this);
      const result = op("element.setHTMLUnsafe", this._id, toDomString(html));
      dom.reactions.characterData(this, result);
    }
    getHTML(options) {
      dom.checkBrand(Element, this);
      return op("element.getHTML", this._id, options === undefined ? "" : toDomString(options));
    }
    get innerHTML() {
      dom.checkBrand(Element, this);
      return op("element.innerHTML", this._id);
    }
    set innerHTML(value) {
      dom.checkBrand(Element, this);
      const result = op("element.innerHTML.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get outerHTML() {
      dom.checkBrand(Element, this);
      return op("element.outerHTML", this._id);
    }
    set outerHTML(value) {
      dom.checkBrand(Element, this);
      const result = op("element.outerHTML.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
  }
  dom.classes.Element = Element;
  dom.globals.Element = Element;
})(globalThis.__wega_dom);
