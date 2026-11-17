// Supported owner-backed CSSOM. Native code validates every access and edit.
(() => {
  const host = __wega_call, semantics = __wega_semantics;
  const data = new WeakMap(), sheets = new Map();
  const get = Function.prototype.call.bind(WeakMap.prototype.get);
  const set = Function.prototype.call.bind(WeakMap.prototype.set);
  const token = {};
  function call(op, ...args) {
    try { return host(op, ...args.map(String)); }
    catch (error) {
      const match = /^([A-Za-z]+Error):\s*(.*)/.exec(error.message || "");
      if (match) throw new DOMException(match[2], match[1]);
      throw error;
    }
  }
  function owner(value) {
    const id = get(data, value);
    if (!id) throw new TypeError("Illegal invocation");
    return id;
  }
  function list(read) {
    return new Proxy(Object.create(null), {
      get(_, name) {
        if (name === "length") return read().length;
        if (name === "item") return index => read()[Number(index)] ?? null;
        if (name === Symbol.iterator) return function* () { yield* read(); };
        if (typeof name === "string" && /^(0|[1-9]\d*)$/.test(name)) return read()[Number(name)];
      },
    });
  }
  class CSSStyleSheet {
    constructor(key, id) {
      if (key !== token) throw new TypeError("Constructed stylesheets are unsupported");
      set(data, this, id);
    }
    get ownerNode() { return semantics.wrap(owner(this)); }
    get href() { return this.ownerNode.localName === "link" ? this.ownerNode.href : null; }
    get disabled() { return call("sheetDisabled", owner(this)); }
    set disabled(value) { call("sheetDisabledSet", owner(this), !!value); }
    get cssRules() {
      const id = owner(this);
      call("sheetRules", id); // Throw SecurityError at access time too.
      return list(() => call("sheetRules", id).map(rule => Object.freeze(rule)));
    }
    insertRule(rule, index = 0) {
      const id = owner(this);
      if (!arguments.length) throw new TypeError("insertRule requires a rule");
      return call("sheetInsert", id, String(rule), Number(index) >>> 0);
    }
    deleteRule(index) {
      const id = owner(this);
      if (!arguments.length) throw new TypeError("deleteRule requires an index");
      call("sheetDelete", id, Number(index) >>> 0);
    }
  }
  function sheet(id) {
    if (!call("sheetHas", id)) return null;
    if (!sheets.has(id)) sheets.set(id, new CSSStyleSheet(token, id));
    return sheets.get(id);
  }
  for (const name of ["HTMLStyleElement", "HTMLLinkElement"]) {
    Object.defineProperties(globalThis[name].prototype, {
      sheet: { configurable: true, get() { return sheet(semantics.ids(this)[0]); } },
      disabled: { configurable: true,
        get() { return call("sheetDisabled", semantics.ids(this)[0]); },
        set(value) { call("sheetDisabledSet", semantics.ids(this)[0], !!value); } },
    });
  }
  const lists = new WeakMap();
  Object.defineProperty(Document.prototype, "styleSheets", { configurable: true, get() {
    const id = semantics.ids(this)[0];
    if (!id) throw new TypeError("Illegal invocation");
    if (!get(lists, this)) set(lists, this, list(() => call("sheetOwners", id).map(sheet)));
    return get(lists, this);
  }});
  globalThis.CSSStyleSheet = CSSStyleSheet;
})();
