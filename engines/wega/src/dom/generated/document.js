// ---------------------------------------------------------------------------
// wega-webidl generated bindings - family "document"
// Do not edit by hand. Regenerate with:
//   cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- generate
// Pinned inputs (tools/webidl/idls.json):
//   vendor/servo-script-bindings/simplified-idls/Document.simplified.webidl sha256=2bea70dcb3e694310f06b3223de4efd4f0c0da74cdf7ecbc63537106b737aa6d
// Hook manifest: engines/wega/src/dom/generated/document.hooks.json
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
  function toBoolean(value) {
    return Boolean(value);
  }
  class Document extends dom.base.EventTarget {
    constructor(unused, internal, info) {
      let id, nodeInfo;
      if (internal === INTERNAL) {
        id = unused;
        nodeInfo = info;
      } else {
        const created = op("document.construct.document", toDomString(unused));
        id = created.id;
        nodeInfo = created.info;
      }
      super(id, nodeInfo);
      dom.brands([Document], this);
    }
    get implementation() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("implementation");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("implementation");
      if (!_v) { _v = op("document.implementation", this._id); _m.set("implementation", _v); }
      return _v;
    }
    get URL() {
      dom.checkBrand(Document, this);
      return op("document.URL", this._id);
    }
    get documentURI() {
      dom.checkBrand(Document, this);
      return op("document.documentURI", this._id);
    }
    get compatMode() {
      dom.checkBrand(Document, this);
      return op("document.compatMode", this._id);
    }
    get characterSet() {
      dom.checkBrand(Document, this);
      return op("document.characterSet", this._id);
    }
    get charset() {
      dom.checkBrand(Document, this);
      return op("document.charset", this._id);
    }
    get inputEncoding() {
      dom.checkBrand(Document, this);
      return op("document.inputEncoding", this._id);
    }
    get contentType() {
      dom.checkBrand(Document, this);
      return op("document.contentType", this._id);
    }
    get doctype() {
      dom.checkBrand(Document, this);
      return op("document.doctype", this._id);
    }
    get documentElement() {
      dom.checkBrand(Document, this);
      return op("document.documentElement", this._id);
    }
    getElementsByTagName(qualifiedName) {
      dom.checkBrand(Document, this);
      return op("document.getElementsByTagName", this._id, toDomString(qualifiedName));
    }
    getElementsByTagNameNS(namespace, qualifiedName) {
      dom.checkBrand(Document, this);
      return op("document.getElementsByTagNameNS", this._id, toDomString(namespace), toDomString(qualifiedName));
    }
    getElementsByClassName(classNames) {
      dom.checkBrand(Document, this);
      return op("document.getElementsByClassName", this._id, toDomString(classNames));
    }
    createElement(localName, options) {
      dom.checkBrand(Document, this);
      return op("document.createElement", this._id, toDomString(localName), options === undefined ? "" : toDomString(options));
    }
    createElementNS(namespace, qualifiedName, options) {
      dom.checkBrand(Document, this);
      return op("document.createElementNS", this._id, toDomString(namespace), toDomString(qualifiedName), options === undefined ? "" : toDomString(options));
    }
    createDocumentFragment() {
      dom.checkBrand(Document, this);
      const result = op("document.createDocumentFragment", this._id);
      dom.reactions.characterData(this, result);
    }
    createTextNode(data) {
      dom.checkBrand(Document, this);
      op("document.createTextNode", this._id, toDomString(data));
    }
    createCDATASection(data) {
      dom.checkBrand(Document, this);
      op("document.createCDATASection", this._id, toDomString(data));
    }
    createComment(data) {
      dom.checkBrand(Document, this);
      op("document.createComment", this._id, toDomString(data));
    }
    createProcessingInstruction(target, data) {
      dom.checkBrand(Document, this);
      const result = op("document.createProcessingInstruction", this._id, toDomString(target), toDomString(data));
      dom.reactions.characterData(this, result);
    }
    importNode(node, options) {
      dom.checkBrand(Document, this);
      op("document.importNode", this._id, toDomString(node), options === undefined ? false : toBoolean(options));
    }
    adoptNode(node) {
      dom.checkBrand(Document, this);
      op("document.adoptNode", this._id, toDomString(node));
    }
    createAttribute(localName) {
      dom.checkBrand(Document, this);
      op("document.createAttribute", this._id, toDomString(localName));
    }
    createAttributeNS(namespace, qualifiedName) {
      dom.checkBrand(Document, this);
      op("document.createAttributeNS", this._id, toDomString(namespace), toDomString(qualifiedName));
    }
    createEvent(interface_) {
      dom.checkBrand(Document, this);
      op("document.createEvent", this._id, toDomString(interface_));
    }
    createRange() {
      dom.checkBrand(Document, this);
      op("document.createRange", this._id);
    }
    createNodeIterator(root, whatToShow, filter) {
      dom.checkBrand(Document, this);
      op("document.createNodeIterator", this._id, toDomString(root), whatToShow === undefined ? 4294967295 : toUnsignedLong(whatToShow), filter === undefined ? "" : toDomString(filter));
    }
    createTreeWalker(root, whatToShow, filter) {
      dom.checkBrand(Document, this);
      op("document.createTreeWalker", this._id, toDomString(root), whatToShow === undefined ? 4294967295 : toUnsignedLong(whatToShow), filter === undefined ? "" : toDomString(filter));
    }
    get domain() {
      dom.checkBrand(Document, this);
      return op("document.domain", this._id);
    }
    set domain(value) {
      dom.checkBrand(Document, this);
      const result = op("document.domain.set", this._id, toDomString(value));
    }
    get referrer() {
      dom.checkBrand(Document, this);
      return op("document.referrer", this._id);
    }
    get cookie() {
      dom.checkBrand(Document, this);
      return op("document.cookie", this._id);
    }
    set cookie(value) {
      dom.checkBrand(Document, this);
      const result = op("document.cookie.set", this._id, toDomString(value));
    }
    get lastModified() {
      dom.checkBrand(Document, this);
      return op("document.lastModified", this._id);
    }
    get readyState() {
      dom.checkBrand(Document, this);
      return op("document.readyState", this._id);
    }
    get location() {
      dom.checkBrand(Document, this);
      return op("document.location", this._id);
    }
    namedItem(name) {
      dom.checkBrand(Document, this);
      return op("document.namedItem", this._id, toDomString(name));
    }
    get title() {
      dom.checkBrand(Document, this);
      return op("document.title", this._id);
    }
    set title(value) {
      dom.checkBrand(Document, this);
      const result = op("document.title.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get dir() {
      dom.checkBrand(Document, this);
      return op("document.dir", this._id);
    }
    set dir(value) {
      dom.checkBrand(Document, this);
      const result = op("document.dir.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get body() {
      dom.checkBrand(Document, this);
      return op("document.body", this._id);
    }
    set body(value) {
      dom.checkBrand(Document, this);
      const result = op("document.body.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get head() {
      dom.checkBrand(Document, this);
      return op("document.head", this._id);
    }
    get images() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("images");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("images");
      if (!_v) { _v = op("document.images", this._id); _m.set("images", _v); }
      return _v;
    }
    get embeds() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("embeds");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("embeds");
      if (!_v) { _v = op("document.embeds", this._id); _m.set("embeds", _v); }
      return _v;
    }
    get plugins() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("plugins");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("plugins");
      if (!_v) { _v = op("document.plugins", this._id); _m.set("plugins", _v); }
      return _v;
    }
    get links() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("links");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("links");
      if (!_v) { _v = op("document.links", this._id); _m.set("links", _v); }
      return _v;
    }
    get forms() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("forms");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("forms");
      if (!_v) { _v = op("document.forms", this._id); _m.set("forms", _v); }
      return _v;
    }
    get scripts() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("scripts");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("scripts");
      if (!_v) { _v = op("document.scripts", this._id); _m.set("scripts", _v); }
      return _v;
    }
    getElementsByName(elementName) {
      dom.checkBrand(Document, this);
      return op("document.getElementsByName", this._id, toDomString(elementName));
    }
    get currentScript() {
      dom.checkBrand(Document, this);
      return op("document.currentScript", this._id);
    }
    open() {
      dom.checkBrand(Document, this);
      return op("document.open", this._id);
    }
    close() {
      dom.checkBrand(Document, this);
      const result = op("document.close", this._id);
      dom.reactions.characterData(this, result);
    }
    write(text) {
      dom.checkBrand(Document, this);
      const result = op("document.write", this._id, Array.isArray(text) ? text.map(v => toDomString(v)) : ( text !== undefined ? [toDomString(text)] : []));
      dom.reactions.characterData(this, result);
    }
    writeln(text) {
      dom.checkBrand(Document, this);
      const result = op("document.writeln", this._id, Array.isArray(text) ? text.map(v => toDomString(v)) : ( text !== undefined ? [toDomString(text)] : []));
      dom.reactions.characterData(this, result);
    }
    get defaultView() {
      dom.checkBrand(Document, this);
      return op("document.defaultView", this._id);
    }
    hasFocus() {
      dom.checkBrand(Document, this);
      return op("document.hasFocus", this._id);
    }
    execCommand(commandId, showUI, value) {
      dom.checkBrand(Document, this);
      return op("document.execCommand", this._id, toDomString(commandId), showUI === undefined ? false : toBoolean(showUI), value === undefined ? "" : toDomString(value));
    }
    queryCommandEnabled(commandId) {
      dom.checkBrand(Document, this);
      return op("document.queryCommandEnabled", this._id, toDomString(commandId));
    }
    queryCommandIndeterm(commandId) {
      dom.checkBrand(Document, this);
      return op("document.queryCommandIndeterm", this._id, toDomString(commandId));
    }
    queryCommandState(commandId) {
      dom.checkBrand(Document, this);
      return op("document.queryCommandState", this._id, toDomString(commandId));
    }
    queryCommandSupported(commandId) {
      dom.checkBrand(Document, this);
      return op("document.queryCommandSupported", this._id, toDomString(commandId));
    }
    queryCommandValue(commandId) {
      dom.checkBrand(Document, this);
      return op("document.queryCommandValue", this._id, toDomString(commandId));
    }
    get hidden() {
      dom.checkBrand(Document, this);
      return op("document.hidden", this._id);
    }
    get visibilityState() {
      dom.checkBrand(Document, this);
      return op("document.visibilityState", this._id);
    }
    get onreadystatechange() {
      dom.checkBrand(Document, this);
      return op("document.onreadystatechange", this._id);
    }
    set onreadystatechange(value) {
      dom.checkBrand(Document, this);
      const result = op("document.onreadystatechange.set", this._id, toDomString(value));
    }
    get fgColor() {
      dom.checkBrand(Document, this);
      return op("document.fgColor", this._id);
    }
    set fgColor(value) {
      dom.checkBrand(Document, this);
      const result = op("document.fgColor.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get bgColor() {
      dom.checkBrand(Document, this);
      return op("document.bgColor", this._id);
    }
    set bgColor(value) {
      dom.checkBrand(Document, this);
      const result = op("document.bgColor.set", this._id, toDomString(value));
      dom.reactions.characterData(this, result);
    }
    get anchors() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("anchors");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("anchors");
      if (!_v) { _v = op("document.anchors", this._id); _m.set("anchors", _v); }
      return _v;
    }
    get applets() {
      dom.checkBrand(Document, this);
      if (this[dom._so]) return this[dom._so].get("applets");
      let _m = this[dom._so];
      if (!_m) { _m = new Map(); this[dom._so] = _m; }
      let _v = _m.get("applets");
      if (!_v) { _v = op("document.applets", this._id); _m.set("applets", _v); }
      return _v;
    }
    clear() {
      dom.checkBrand(Document, this);
      op("document.clear", this._id);
    }
    captureEvents() {
      dom.checkBrand(Document, this);
      op("document.captureEvents", this._id);
    }
    releaseEvents() {
      dom.checkBrand(Document, this);
      op("document.releaseEvents", this._id);
    }
    get fullscreenEnabled() {
      dom.checkBrand(Document, this);
      return op("document.fullscreenEnabled", this._id);
    }
    get fullscreen() {
      dom.checkBrand(Document, this);
      return op("document.fullscreen", this._id);
    }
    exitFullscreen() {
      dom.checkBrand(Document, this);
      op("document.exitFullscreen", this._id);
    }
    get onfullscreenchange() {
      dom.checkBrand(Document, this);
      return op("document.onfullscreenchange", this._id);
    }
    set onfullscreenchange(value) {
      dom.checkBrand(Document, this);
      const result = op("document.onfullscreenchange.set", this._id, toDomString(value));
    }
    get onfullscreenerror() {
      dom.checkBrand(Document, this);
      return op("document.onfullscreenerror", this._id);
    }
    set onfullscreenerror(value) {
      dom.checkBrand(Document, this);
      const result = op("document.onfullscreenerror.set", this._id, toDomString(value));
    }
    get scrollingElement() {
      dom.checkBrand(Document, this);
      return op("document.scrollingElement", this._id);
    }
    getSelection() {
      dom.checkBrand(Document, this);
      return op("document.getSelection", this._id);
    }
    get timeline() {
      dom.checkBrand(Document, this);
      return op("document.timeline", this._id);
    }
    parseHTML(html, options) {
      dom.checkBrand(Document, this);
      return op("document.parseHTML", this._id, toDomString(html), options === undefined ? "" : toDomString(options));
    }
  }
  dom.classes.Document = Document;
  dom.globals.Document = Document;
})(globalThis.__wega_dom);
