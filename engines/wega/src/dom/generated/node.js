// ---------------------------------------------------------------------------
// wega-webidl generated bindings - family "node"
// Do not edit by hand. Regenerate with:
//   cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- generate
// Pinned inputs (tools/webidl/idls.json):
//   vendor/servo-script-bindings/simplified-idls/Node.simplified.webidl sha256=66a0009432e4c0022e680ab4352c9d99ae21cd204ab67cd5cbc2c2e9cead57b7
// Hook manifest: engines/wega/src/dom/generated/node.hooks.json
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
  function toBoolean(value) {
    return Boolean(value);
  }
  class Node extends dom.event.EventTarget {
    constructor(id, internal, info) {
      if (internal !== INTERNAL) throw new TypeError("Illegal constructor");
      super(id, nodeInfo);
      dom.brands([Node], this);
    }
    static {
      Object.defineProperty(this, "ELEMENT_NODE", { value: 1, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "ATTRIBUTE_NODE", { value: 2, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "TEXT_NODE", { value: 3, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "CDATA_SECTION_NODE", { value: 4, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "ENTITY_REFERENCE_NODE", { value: 5, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "ENTITY_NODE", { value: 6, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "PROCESSING_INSTRUCTION_NODE", { value: 7, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "COMMENT_NODE", { value: 8, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "DOCUMENT_NODE", { value: 9, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "DOCUMENT_TYPE_NODE", { value: 10, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "DOCUMENT_FRAGMENT_NODE", { value: 11, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "NOTATION_NODE", { value: 12, writable: false, enumerable: true, configurable: true });
    }
    get nodeType() {
      dom.checkBrand(Node, this);
      return op("node.nodeType", this._id);
    }
    get nodeName() {
      dom.checkBrand(Node, this);
      return op("node.nodeName", this._id);
    }
    get baseURI() {
      dom.checkBrand(Node, this);
      return op("node.baseURI", this._id);
    }
    get isConnected() {
      dom.checkBrand(Node, this);
      return op("node.isConnected", this._id);
    }
    get ownerDocument() {
      dom.checkBrand(Node, this);
      return op("node.ownerDocument", this._id);
    }
    get parentNode() {
      dom.checkBrand(Node, this);
      return op("node.parentNode", this._id);
    }
    get parentElement() {
      dom.checkBrand(Node, this);
      return op("node.parentElement", this._id);
    }
    hasChildNodes() {
      dom.checkBrand(Node, this);
      return op("node.hasChildNodes", this._id);
    }
    get childNodes() {
      dom.checkBrand(Node, this);
      if (this[dom._so]) return this[dom._so].get("childNodes");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("childNodes");
      if (!_v) { _v = op("node.childNodes", this._id); _m.set("childNodes", _v); }
      return _v;
    }
    get firstChild() {
      dom.checkBrand(Node, this);
      return op("node.firstChild", this._id);
    }
    get lastChild() {
      dom.checkBrand(Node, this);
      return op("node.lastChild", this._id);
    }
    get previousSibling() {
      dom.checkBrand(Node, this);
      return op("node.previousSibling", this._id);
    }
    get nextSibling() {
      dom.checkBrand(Node, this);
      return op("node.nextSibling", this._id);
    }
    get nodeValue() {
      dom.checkBrand(Node, this);
      return op("node.nodeValue", this._id);
    }
    set nodeValue(value) {
      dom.checkBrand(Node, this);
      const result = op("node.nodeValue.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get textContent() {
      dom.checkBrand(Node, this);
      return op("node.textContent", this._id);
    }
    set textContent(value) {
      dom.checkBrand(Node, this);
      const result = op("node.textContent.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    normalize() {
      dom.checkBrand(Node, this);
      const result = op("node.normalize", this._id);
      dom.reactions.characterData(this, result);
    }
    cloneNode(deep) {
      dom.checkBrand(Node, this);
      return op("node.cloneNode", this._id, toBoolean(deep));
    }
    isEqualNode(node) {
      dom.checkBrand(Node, this);
      return op("node.isEqualNode", this._id, toDomString(node));
    }
    isSameNode(otherNode) {
      dom.checkBrand(Node, this);
      return op("node.isSameNode", this._id, toDomString(otherNode));
    }
    static {
      Object.defineProperty(this, "DOCUMENT_POSITION_DISCONNECTED", { value: 1, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "DOCUMENT_POSITION_PRECEDING", { value: 2, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "DOCUMENT_POSITION_FOLLOWING", { value: 4, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "DOCUMENT_POSITION_CONTAINS", { value: 8, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "DOCUMENT_POSITION_CONTAINED_BY", { value: 16, writable: false, enumerable: true, configurable: true });
    }
    static {
      Object.defineProperty(this, "DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC", { value: 32, writable: false, enumerable: true, configurable: true });
    }
    compareDocumentPosition(other) {
      dom.checkBrand(Node, this);
      return op("node.compareDocumentPosition", this._id, toDomString(other));
    }
    contains(other) {
      dom.checkBrand(Node, this);
      return op("node.contains", this._id, toDomString(other));
    }
    lookupPrefix(namespace) {
      dom.checkBrand(Node, this);
      return op("node.lookupPrefix", this._id, toDomString(namespace));
    }
    lookupNamespaceURI(prefix) {
      dom.checkBrand(Node, this);
      return op("node.lookupNamespaceURI", this._id, toDomString(prefix));
    }
    isDefaultNamespace(namespace) {
      dom.checkBrand(Node, this);
      return op("node.isDefaultNamespace", this._id, toDomString(namespace));
    }
    insertBefore(node, child) {
      dom.checkBrand(Node, this);
      return op("node.insertBefore", this._id, toDomString(node), toDomString(child));
    }
    appendChild(node) {
      dom.checkBrand(Node, this);
      return op("node.appendChild", this._id, toDomString(node));
    }
    replaceChild(node, child) {
      dom.checkBrand(Node, this);
      return op("node.replaceChild", this._id, toDomString(node), toDomString(child));
    }
    removeChild(child) {
      dom.checkBrand(Node, this);
      return op("node.removeChild", this._id, toDomString(child));
    }
  }
  dom.classes.Node = Node;
})(globalThis.__wega_dom);
