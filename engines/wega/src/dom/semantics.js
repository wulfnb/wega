// Wega contributors, 2026. DOM semantic algorithms and private lifetime state.
// Public IDL descriptors are installed by bindings.js and generated family factories.
(() => {
  const NativeWeakMap = WeakMap, NativeWeakSet = WeakSet;
  const apply = Reflect.apply, freeze = Object.freeze;
  const weakGet = WeakMap.prototype.get, weakSet = WeakMap.prototype.set, weakHas = WeakMap.prototype.has;
  const weakSetAdd = WeakSet.prototype.add, weakSetHas = WeakSet.prototype.has;
  const isPrototypeOf = Object.prototype.isPrototypeOf;
  function privateWeakMap() {
    const map = new NativeWeakMap();
    return freeze({ get: key => apply(weakGet, map, [key]), set: (key, value) => apply(weakSet, map, [key, value]), has: key => apply(weakHas, map, [key]) });
  }
  function privateWeakSet() {
    const set = new NativeWeakSet();
    return freeze({ add: value => apply(weakSetAdd, set, [value]), has: value => apply(weakSetHas, set, [value]) });
  }
  const host = globalThis.__wega_call;
  const web = typeof __wega_web === "object" ? __wega_web : null;
  const missing = new Set(),
    cache = new Map(),
    connected = new Map();
  const weak =
    typeof WeakRef === "function" && typeof FinalizationRegistry === "function";
  let constructingNode = 0;
  let document;
  const nodeHandles = privateWeakMap();
  const collectionData = privateWeakMap();
  const attrData = privateWeakMap();
  const namedMapData = privateWeakMap();
  const observerData = privateWeakMap();
  const recordData = privateWeakMap();
  const tokenData = privateWeakMap();
  const locationData = privateWeakSet();
  const internalToken = {};

  const componentConstructors = new Map();
  const componentCallbacks = new Map();
  const constructorDefinitions = privateWeakMap();
  const componentWaiters = new Map();
  const constructionStack = [];
  const shadowDescriptors = privateWeakMap();
  const shadowToken = {};
  let constructorIdentity = 0;
  let flushingComponents = false;
  function component(op, a = "", b = "", c = "") {
    try { return host(op, String(a), String(b), String(c)); }
    catch (error) {
      const match = /^([A-Za-z]+Error):\s*(.*)/.exec(error.message || "");
      if (match) fail(match[1], match[2]);
      throw error;
    }
  }
  function flushComponents() {
    if (flushingComponents || constructionStack.length) return;
    flushingComponents = true;
    try {
      for (;;) {
        const reactions = component("customReactions");
        if (!reactions.length) break;
        for (const reaction of reactions) {
          const element = wrap(reaction.node);
          const args = reaction.callback === "adoptedCallback" ? reaction.args.map(id => wrap(id)) : reaction.args;
          try { componentCallbacks.get(reaction.definition)?.[reaction.callback]?.apply(element, args); } catch (error) { report(error); }
        }
      }
    } finally { flushingComponents = false; }
  }
  function shadowWrapper(descriptor) {
    if (!descriptor) return null;
    const cached = weak ? cache.get(descriptor.root)?.deref() : cache.get(descriptor.root);
    if (cached instanceof ShadowRoot) return cached;
    const root = new ShadowRoot(shadowToken, descriptor);
    cache.set(descriptor.root, weak ? new WeakRef(root) : root);
    host("retain", descriptor.root);
    finalizer?.register(root, descriptor.root);
    return root;
  }
  // DOM algorithms throw DOMException; only Web IDL's simple errors stay JS errors.
  const jsErrors = { TypeError, RangeError, ReferenceError, EvalError };
  const fail = (name, message) => {
    throw jsErrors[name] ? new jsErrors[name](message) : new DOMException(message, name);
  };
  const fatal = (error) =>
    (error?.name === "InternalError" &&
      /out of memory|stack overflow|interrupted/.test(error.message)) ||
    (error?.name === "RangeError" &&
      /Maximum call stack size exceeded/.test(error.message));
  function report(error) {
    if (fatal(error)) throw error;
    const message = String(error);
    // Undefined expando reads and feature detection are normal. Only name an
    // API as missing when the failing call actually identifies that property.
    const absent = [...missing].filter((name) => {
      const key = name.slice(name.indexOf(".") + 1)
        .replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const called = new RegExp(
        "(?:^|[^\\w$])" + key +
        "(?:['\"]\\s*\\])?\\s+is not a (?:function|constructor)",
      );
      return called.test(message);
    });
    const text = message + "\n" + String(error?.stack || "");
    host(
      "error",
      text +
        (message.includes("TypeError") && absent.length
          ? "; missing: " + absent.join(", ")
          : ""),
    );
    if (report.active) return;
    report.active = true;
    try {
      const event = web
        ? new web.ErrorEvent("error", {
            message: String(error),
            error,
            cancelable: true,
          })
        : new Event("error");
      if (!web) {
        event.message = String(error);
        event.error = error;
      }
      globalThis.dispatchEvent(event);
    } finally {
      report.active = false;
    }
  }
  Object.defineProperties(globalThis, {
    __wega_report: { value: report },
    __wega_missing: {
      value: () => {
        const result = [...missing].join(", ");
        missing.clear();
        return result;
      },
    },
  });
  class FallbackEvent {
    constructor(type, options = {}) {
      this.type = String(type);
      this.bubbles = !!options.bubbles;
      this.cancelable = !!options.cancelable;
      this.composed = !!options.composed;
      this.defaultPrevented = false;
      this.eventPhase = 0;
      this.target = null;
      this.currentTarget = null;
      this.timeStamp = host("now");
      this.isTrusted = false;
    }
    preventDefault() {
      if (this.cancelable && !this._passive) this.defaultPrevented = true;
    }
    stopPropagation() {
      this._stopped = true;
    }
    stopImmediatePropagation() {
      this._stopped = this._immediate = true;
    }
    composedPath() {
      return [...(this._path || [])];
    }
    initEvent(type, bubbles = false, cancelable = false) {
      this.type = String(type);
      this.bubbles = !!bubbles;
      this.cancelable = !!cancelable;
    }
  }
  class FallbackEventTarget {
    constructor() {
      Object.defineProperty(this, "_events", { value: new Map() });
    }
    addEventListener(type, callback, options = {}) {
      if (!callback) return;
      type = String(type);
      const capture =
        typeof options === "boolean" ? options : !!options.capture;
      let list = this._events.get(type);
      if (!list) this._events.set(type, (list = []));
      if (!list.some((x) => x.callback === callback && x.capture === capture))
        list.push({
          callback,
          capture,
          once: !!options.once,
          passive: !!options.passive,
        });
    }
    removeEventListener(type, callback, options = {}) {
      const capture =
        typeof options === "boolean" ? options : !!options.capture;
      this._events.set(
        String(type),
        (this._events.get(String(type)) || []).filter(
          (x) => x.callback !== callback || x.capture !== capture,
        ),
      );
    }
    dispatchEvent(event) {
      if (!event?.type) fail("InvalidStateError", "Event type is empty");
      const originalTarget = this;
      event.target = this;
      const path = [this];
      let curr = this;
      while (true) {
        if (curr.parentNode) {
          curr = curr.parentNode;
          path.push(curr);
        } else if (curr._host) {
          if (event.composed) {
            curr = curr._host;
            path.push(curr);
          } else {
            break;
          }
        } else {
          break;
        }
      }
      if (path.at(-1)?.nodeType === 9) path.push(globalThis);
      event._path = path;
      const invoke = (target, capture, phase) => {
        event.currentTarget = target;
        event.eventPhase = phase;
        if (originalTarget.getRootNode && target.getRootNode) {
          const targetRoot = target.getRootNode();
          if (originalTarget.getRootNode() === targetRoot) {
            event.target = originalTarget;
          } else {
            let retargeted = originalTarget;
            for (const node of path) {
              if (node.getRootNode && node.getRootNode() === targetRoot) {
                retargeted = node;
                break;
              }
            }
            event.target = retargeted;
          }
        }
        for (const entry of [...(target._events?.get(event.type) || [])]) {
          if (entry.capture !== capture) continue;
          if (entry.once)
            target.removeEventListener(
              event.type,
              entry.callback,
              entry.capture,
            );
          event._passive = entry.passive;
          try {
            typeof entry.callback === "function"
              ? entry.callback.call(target, event)
              : entry.callback.handleEvent(event);
          } catch (error) {
            report(error);
          }
          if (event._immediate) break;
        }
        event._passive = false;
      };
      for (let i = path.length - 1; i > 0 && !event._stopped; i--)
        invoke(path[i], true, 1);
      if (!event._stopped) {
        invoke(this, true, 2);
        if (!event._immediate) invoke(this, false, 2);
      }
      if (event.bubbles)
        for (let i = 1; i < path.length && !event._stopped; i++)
          invoke(path[i], false, 3);
      event.currentTarget = null;
      event.eventPhase = 0;
      event.target = originalTarget;
      event._path = [];
      return !event.defaultPrevented;
    }
  }
  const Event = web?.Event || FallbackEvent,
    BaseTarget = web?.EventTarget || FallbackEventTarget;
  const finalizer = weak
    ? new FinalizationRegistry((id) => {
        if (!cache.get(id)?.deref()) {
          cache.delete(id);
          host("release", id);
        }
      })
    : null;
  function wrap(id, owner) {
    if (id === null || id === undefined) return null;
    let result = weak ? cache.get(id)?.deref() : cache.get(id);
    if (result) return result;
    const info = host("info", id);
    if (info.nodeType === 11) {
      const owner = component("shadowHost", id);
      if (owner !== null) return shadowWrapper(component("shadowInternal", owner));
    }
    const Ctor =
      info.nodeType === 9
        ? Document
        : info.nodeType === 11
          ? DocumentFragment
          : info.nodeType === 10
            ? DocumentType
            : info.nodeType === 8
              ? Comment
              : info.nodeType === 7
                ? ProcessingInstruction
                : info.nodeType === 4
                  ? CDATASection
                  : info.nodeType === 3
                    ? Text
                    : info.namespaceURI === "http://www.w3.org/2000/svg"
                      ? SVGElement
                      : htmlConstructors[info.localName] || HTMLElement;
    let instance;
    constructingNode++;
    try { instance = new Ctor(id, info); } finally { constructingNode--; }
    result = new Proxy(instance, {
      get(target, key, receiver) {
        const value = Reflect.get(target, key, receiver);
        if (
          value === undefined &&
          typeof key === "string" &&
          !key.startsWith("_") &&
          !(key === "namespaceURI" && info.nodeType !== 1)
        ) {
          if (missing.size >= 32) missing.delete(missing.values().next().value);
          missing.add((info.nodeType === 9 ? "Document." : "Element.") + key);
        }
        return value;
      },
    });
    nodeHandles.set(result, id);
    cache.set(id, weak ? new WeakRef(result) : result);
    host("retain", id);
    finalizer?.register(result, id);
    if (owner) result._owner = owner;
    if (host("connected", id)) connected.set(id, result);
    if (info.nodeType === 1) {
      if (typeof customElements === "object") {
        const definition = component("customUpgrade", id);
        if (definition !== null) upgradeElement(result, definition);
      }
      for (const attr of host("attributes", id)) {
        if (attr.name.startsWith("on")) {
          ensureHandler(attr.name.slice(2));
          try {
            result[attr.name] = new Function("event", attr.value);
          } catch (error) {
            report(error);
          }
        }
      }
    }
    return result;
  }
  // Before replacing all children: forget them as connected, and wrap them only
  // when a MutationObserver needs removedNodes (wrappers keep nodes alive).
  function detachChildren(parent) {
    const old = mutationObservers.size ? [...parent.childNodes] : [];
    for (const id of host("subtree", parent._id))
      if (id !== parent._id) connected.delete(id);
    return old;
  }
  function retainTree(node, live) {
    for (const id of host("subtree", node._id)) {
      const value = weak ? cache.get(id)?.deref() : cache.get(id);
      if (live && value) connected.set(id, value);
      else connected.delete(id);
    }
  }
  function nodeList(
    read,
    collection = false,
    indexed = null,
    live = collection,
  ) {
    if (live && !indexed) {
      const query = read;
      let revision = -1, nodes = [];
      const entries = () => {
        const current = host("revision");
        if (revision !== current) {
          // Cache identities, not wrappers: a live collection must not retain
          // removed subtrees and their allocation budget until its next read.
          nodes = query().map((node) => [node._id, node.ownerDocument]);
          revision = current;
        }
        return nodes;
      };
      read = () => entries().map(([id, owner]) => wrap(id, owner));
      indexed = {
        length: () => entries().length,
        at: (index) => {
          const entry = entries()[index];
          return entry ? wrap(entry[0], entry[1]) : undefined;
        },
      };
    }
    const length = () => indexed ? indexed.length() : read().length;
    const at = (index) => indexed ? indexed.at(index) : read()[index];
    const target = collection ? new HTMLCollection() : new NodeList();
    const proxy = new Proxy(target, {
      get(target, key, receiver) {
        if (typeof key === "string" && /^\d+$/.test(key)) {
          return at(Number(key));
        }
        const value = Reflect.get(target, key, receiver);
        if (value === undefined && collection && typeof key === "string") {
          return read().find(
            (n) => n.id === key || n.getAttribute("name") === key,
          );
        }
        return value;
      },
      has(target, key) {
        return typeof key === "string" && /^\d+$/.test(key)
          ? Number(key) < length()
          : Reflect.has(target, key);
      },
      ownKeys() {
        return Array.from({ length: length() }, (_, i) => String(i));
      },
      getOwnPropertyDescriptor(target, key) {
        if (
          typeof key === "string" &&
          /^\d+$/.test(key) &&
          Number(key) < length()
        ) {
          return {
            enumerable: true,
            configurable: true,
            value: at(Number(key)),
          };
        }
        return Reflect.getOwnPropertyDescriptor(target, key);
      },
    });
    const state = { read, length, at };
    collectionData.set(target, state);
    collectionData.set(proxy, state);
    return proxy;
  }
  class NodeList {
    get length() {
      const state = collectionData.get(this);
      if (!state) throw new TypeError("Illegal invocation");
      return state.length();
    }
    item(index) {
      const state = collectionData.get(this);
      if (!state) throw new TypeError("Illegal invocation");
      return state.at(Number(index)) || null;
    }
  }
  class HTMLCollection {
    get length() {
      const state = collectionData.get(this);
      if (!state) throw new TypeError("Illegal invocation");
      return state.length();
    }
    item(index) {
      const state = collectionData.get(this);
      if (!state) throw new TypeError("Illegal invocation");
      return state.at(Number(index)) || null;
    }
    namedItem(name) {
      return (
        [...this].find(
          (n) => n.id === name || n.getAttribute("name") === name,
        ) || null
      );
    }
  }
  function makeCollection(target) {
    const proxy = new Proxy(target, {
      get(t, key, r) {
        if (typeof key === "string" && /^(0|[1-9]\d*)$/.test(key)) return t.item(Number(key)) ?? undefined;
        const value = Reflect.get(t, key, r);
        if (value !== undefined) return value;
        if (typeof key === "string") {
          // Legacy named properties: try namedItem first (by id/name)
          const named = t.namedItem(key);
          if (named) return named;
        }
        return value;
      },
      ownKeys(t) {
        const len = t.length;
        const keys = [];
        for (let i = 0; i < len; i++) keys.push(String(i));
        // Also add named properties by id/name
        const seen = new Set(keys);
        for (const node of [...t]) {
          if (node.id && !seen.has(node.id)) { keys.push(node.id); seen.add(node.id); }
          const name = node.getAttribute("name");
          if (name && !seen.has(name)) { keys.push(name); seen.add(name); }
        }
        return keys;
      },
      getOwnPropertyDescriptor(t, key) {
        if (typeof key === "string" && /^(0|[1-9]\d*)$/.test(key) && Number(key) < t.length)
          return { configurable: true, enumerable: true, value: t.item(Number(key)) };
        if (typeof key === "string" && t.namedItem(key))
          return { configurable: true, enumerable: true, value: t.namedItem(key) };
        return Reflect.getOwnPropertyDescriptor(t, key);
      },
      has(t, key) {
        if (typeof key === "string" && /^(0|[1-9]\d*)$/.test(key)) return Number(key) < t.length;
        if (typeof key === "string") return !!t.namedItem(key);
        return Reflect.has(t, key);
      },
    });
    collectionData.set(proxy, collectionData.get(target));
    return proxy;
  }
  for (const name of ["keys", "values", "entries", "forEach"])
    NodeList.prototype[name] = Array.prototype[name];
  NodeList.prototype[Symbol.iterator] = Array.prototype.values;
  HTMLCollection.prototype[Symbol.iterator] = Array.prototype.values;
  Object.defineProperty(NodeList.prototype, Symbol.toStringTag, {
    value: "NodeList",
  });
  Object.defineProperty(HTMLCollection.prototype, Symbol.toStringTag, {
    value: "HTMLCollection",
  });
  // DOM "ensure pre-insertion validity" and replaceChild's variant (replaced
  // is the node being replaced, else null), in specification order.
  const insertionToken = {};
  function insertionValidity(parent, node, child, replaced) {
    const hierarchy = (message) => fail("HierarchyRequestError", message);
    if (![1, 9, 11].includes(parent.nodeType)) hierarchy("Parent cannot have children");
    for (let n = parent; n; n = n.parentNode || (n.nodeType === 11 ? n.host : null)) {
      if (n === node) hierarchy("Cannot create a DOM cycle");
    }
    if (child && child.parentNode !== parent) fail("NotFoundError", "Reference is not a child");
    if (![1, 3, 4, 7, 8, 10, 11].includes(node.nodeType)) hierarchy("Node cannot be inserted");
    if ((node.nodeType === 3 && parent.nodeType === 9) || (node.nodeType === 10 && parent.nodeType !== 9)) {
      hierarchy("Node cannot be inserted here");
    }
    if (parent.nodeType !== 9) return;
    const children = [...parent.childNodes];
    const others = children.filter((n) => n !== replaced);
    const after = child ? children.slice(children.indexOf(child) + 1) : [];
    const before = child ? children.slice(0, children.indexOf(child)) : children;
    const doctypeAfter = (child && child.nodeType === 10 && child !== replaced) || after.some((n) => n.nodeType === 10);
    const elementOf = (nodes) => nodes.some((n) => n.nodeType === 1);
    if (node.nodeType === 11 || node.nodeType === 1) {
      const elements = node.nodeType === 1 ? 1 : [...node.childNodes].filter((n) => n.nodeType === 1).length;
      if (node.nodeType === 11 && (elements > 1 || [...node.childNodes].some((n) => n.nodeType === 3))) hierarchy("Document can have one element");
      if (elements === 1 && (elementOf(others) || doctypeAfter)) hierarchy("Document can have one element");
    } else if (node.nodeType === 10) {
      if (others.some((n) => n.nodeType === 10) || elementOf(before.filter((n) => n !== replaced)) || (!child && elementOf(children))) {
        hierarchy("Document can have one doctype before its element");
      }
    }
  }
  // DOM "convert nodes into a node" and ChildNode's viable next sibling.
  function nodesIntoNode(document, values) {
    const nodes = values.map((value) => value instanceof Node ? value : document.createTextNode(String(value)));
    if (nodes.length === 1) return nodes[0];
    const fragment = document.createDocumentFragment();
    for (const node of nodes) fragment.appendChild(node);
    return fragment;
  }
  function viableNextSibling(node, values) {
    let next = node.nextSibling;
    while (next && values.includes(next)) next = next.nextSibling;
    return next;
  }
  class Node extends BaseTarget {
    constructor(id, info) {
      if (!constructingNode && new.target !== Attr && new.target !== ShadowRoot && !constructorDefinitions.has(new.target)) throw new TypeError("Illegal constructor");
      super();
      // Deno allocates listener data lazily; a node must take part in event
      // paths (delegation) even before it has listeners of its own.
      web?.event.setEventTargetData(this);
      nodeHandles.set(this, id);
      Object.defineProperties(this, {
        _id: { value: id },
        _info: { value: info },
        _callbacks: { value: new Map() },
      });
    }
    get nodeType() {
      return this._info.nodeType;
    }
    get nodeName() {
      return this._info.nodeName;
    }
    get ownerDocument() {
      return this.nodeType === 9 ? null : this._owner || document;
    }
    get parentNode() {
      return wrap(host("parent", this._id));
    }
    get parentElement() {
      const parent = this.parentNode;
      return parent?.nodeType === 1 ? parent : null;
    }
    get childNodes() {
      return (
        this._childNodes ||
        (this._childNodes = nodeList(
          () =>
            host("children", this._id).map((id) =>
              wrap(id, this.ownerDocument || this)
            ),
          false,
          {
            length: () => host("childCount", this._id),
            at: (index) => {
              const id = host("childAt", this._id, String(index));
              return id === null
                ? undefined
                : wrap(id, this.ownerDocument || this);
            },
          },
        ))
      );
    }
    get children() {
      return (
        this._children ||
        (this._children = nodeList(
          () => [...this.childNodes].filter((n) => n.nodeType === 1),
          true,
        ))
      );
    }
    get firstChild() {
      return wrap(host("firstChild", this._id), this.ownerDocument || this);
    }
    get lastChild() {
      return wrap(host("lastChild", this._id), this.ownerDocument || this);
    }
    get firstElementChild() {
      return this.children[0] || null;
    }
    get lastElementChild() {
      return this.children[this.children.length - 1] || null;
    }
    get childElementCount() {
      return this.children.length;
    }
    get previousSibling() {
      return wrap(
        host("previousSibling", this._id),
        this.ownerDocument || this,
      );
    }
    get nextSibling() {
      return wrap(host("nextSibling", this._id), this.ownerDocument || this);
    }
    get previousElementSibling() {
      let node = this.previousSibling;
      while (node && node.nodeType !== 1) node = node.previousSibling;
      return node;
    }
    get nextElementSibling() {
      let node = this.nextSibling;
      while (node && node.nodeType !== 1) node = node.nextSibling;
      return node;
    }
    get textContent() {
      return host("text", this._id);
    }
    set textContent(value) {
      const text = String(value === null ? "" : value);
      const oldLength = this instanceof CharacterData ? this.length : 0;
      const children = liveRanges.size ? [...this.childNodes] : [];
      const old = detachChildren(this);
      host("setText", this._id, text);
      if (this instanceof CharacterData) {
        changedRangeData(this, 0, oldLength, text.length);
      } else replacedRangeChildren(this, children);
      if (mutationObservers.size) {
        notify(this, "childList", {
          removedNodes: old,
          addedNodes: [...this.childNodes],
        });
      }
      if (isScript(this) && this.isConnected) prepareScript(this);
    }
    get nodeValue() {
      return [3, 8].includes(this.nodeType) ? this.textContent : null;
    }
    set nodeValue(value) {
      if ([3, 8].includes(this.nodeType)) {
        const old = this.textContent;
        const text = String(value === null ? "" : value);
        host("setText", this._id, text);
        changedRangeData(this, 0, old.length, text.length);
        notify(this, "characterData", { oldValue: old });
      }
    }
    get isConnected() {
      return this.getRootNode({ composed: true }).nodeType === 9;
    }
    hasChildNodes() {
      return this.childNodes.length > 0;
    }
    getRootNode(options) {
      return wrap(component("componentRoot", this._id, !!options?.composed));
    }
    get baseURI() { return (this.ownerDocument || this).URL; }
    lookupNamespaceURI(prefix) {
      if (this.nodeType === 9) return this.documentElement?.lookupNamespaceURI(prefix) ?? null;
      if (this.nodeType !== 1) return this.parentElement?.lookupNamespaceURI(prefix) ?? null;
      if ((this.prefix || null) === (prefix || null) && this.namespaceURI) return this.namespaceURI;
      const value = this.getAttribute(prefix ? "xmlns:" + prefix : "xmlns");
      return value !== null ? value || null : this.parentElement?.lookupNamespaceURI(prefix) ?? null;
    }
    lookupPrefix(namespace) {
      if (!namespace) return null;
      if (this.nodeType === 9) return this.documentElement?.lookupPrefix(namespace) ?? null;
      if (this.namespaceURI === namespace && this.prefix) return this.prefix;
      for (const attr of this.attributes || [])
        if (attr.prefix === "xmlns" && attr.value === namespace && this.lookupNamespaceURI(attr.localName) === namespace) return attr.localName;
      return this.parentElement?.lookupPrefix(namespace) ?? null;
    }
    isDefaultNamespace(namespace) { return this.lookupNamespaceURI(null) === (namespace || null); }
    contains(other) {
      for (let node = other; node; node = node.parentNode)
        if (node === this) return true;
      return false;
    }
    isSameNode(other) {
      return this === other;
    }
    isEqualNode(other) {
      if (
        !other ||
        this.nodeType !== other.nodeType ||
        this.nodeName !== other.nodeName ||
        this.nodeValue !== other.nodeValue
      )
        return false;
      const attrs = this.nodeType === 1 ? [...this.attributes] : [];
      if (
        attrs.length !== (other.attributes?.length || 0) ||
        attrs.some(
          (a) => other.getAttributeNS(a.namespaceURI, a.localName) !== a.value,
        )
      )
        return false;
      return (
        this.childNodes.length === other.childNodes.length &&
        [...this.childNodes].every((node, i) =>
          node.isEqualNode(other.childNodes[i]),
        )
      );
    }
    compareDocumentPosition(other) {
      if (this === other) return 0;
      if (this.getRootNode() !== other.getRootNode()) return 1 | 32 | 4;
      if (this.contains(other)) return 4 | 16;
      if (other.contains(this)) return 2 | 8;
      const all = [];
      const walk = (n) => {
        all.push(n);
        for (const child of n.childNodes) walk(child);
      };
      walk(this.getRootNode());
      return all.indexOf(this) < all.indexOf(other) ? 4 : 2;
    }
    appendChild(child) {
      return this.insertBefore(child, null);
    }
    insertBefore(child, reference, validated) {
      if (!(child instanceof Node)) throw new TypeError("Expected a Node");
      if (validated !== insertionToken) insertionValidity(this, child, reference ?? null, null);
      if (child.nodeType === 11) {
        for (const node of [...child.childNodes]) {
          this.insertBefore(node, reference, insertionToken);
        }
        return child;
      }
      const oldParent = child.parentNode;
      const wasConnected = child.isConnected;
      if (child === reference) return child;
      if (oldParent) removedRangeNode(child);
      const index = reference
        ? nodeIndex(reference) -
          Number(oldParent === this && nodeIndex(child) < nodeIndex(reference))
        : this.childNodes.length - Number(oldParent === this);
      insertedRangeNode(this, index);
      host("insert", this._id, child._id, reference ? reference._id : "");
      child._owner = this.ownerDocument || this;
      retainTree(child, this.isConnected);
      if (wasConnected) triggerDisconnected(child);
      if (oldParent) {
        notify(oldParent, "childList", {
          removedNodes: [child],
          addedNodes: [],
        });
      }
      notify(this, "childList", { removedNodes: [], addedNodes: [child] });
      if (this.getRootNode() === document || this.isConnected) {
        prepareInserted(child);
        triggerConnected(child);
        if (isScript(this)) prepareScript(this);
      }
      return child;
    }
    removeChild(child) {
      if (child?.parentNode !== this) {
        fail("NotFoundError", "Node is not a child");
      }
      const wasConnected = child.isConnected;
      removedRangeNode(child);
      retainTree(child, false);
      host("remove", child._id);
      notify(this, "childList", { removedNodes: [child], addedNodes: [] });
      if (wasConnected) {
        triggerDisconnected(child);
      }
      return child;
    }
    replaceChild(child, old) {
      if (!(child instanceof Node)) throw new TypeError("Expected a Node");
      insertionValidity(this, child, old, old);
      if (child === old) return old;
      this.insertBefore(child, old, insertionToken);
      this.removeChild(old);
      return old;
    }
    replaceChildren(...values) {
      for (const child of [...this.childNodes]) this.removeChild(child);
      this.append(...values);
    }
    append(...values) {
      this.appendChild(nodesIntoNode(this.ownerDocument || this, values));
    }
    prepend(...values) {
      this.insertBefore(nodesIntoNode(this.ownerDocument || this, values), this.firstChild);
    }
    before(...values) {
      const parent = this.parentNode;
      if (!parent) return;
      let previous = this.previousSibling;
      while (previous && values.includes(previous)) previous = previous.previousSibling;
      const node = nodesIntoNode(this.ownerDocument, values);
      parent.insertBefore(node, previous ? previous.nextSibling : parent.firstChild);
    }
    after(...values) {
      const parent = this.parentNode;
      if (!parent) return;
      const next = viableNextSibling(this, values);
      parent.insertBefore(nodesIntoNode(this.ownerDocument, values), next);
    }
    replaceWith(...values) {
      const parent = this.parentNode;
      if (!parent) return;
      const next = viableNextSibling(this, values);
      const node = nodesIntoNode(this.ownerDocument, values);
      if (this.parentNode === parent) parent.replaceChild(node, this);
      else parent.insertBefore(node, next);
    }
    remove() {
      this.parentNode?.removeChild(this);
    }
    cloneNode(deep = false) {
      return wrap(host("clone", this._id, String(!!deep)), this.ownerDocument);
    }
    normalize() {
      if (mutationObservers.size === 0) {
        let previous = null;
        for (const child of [...this.childNodes]) {
          if (child.nodeType === 3) {
            if (child.data === "") child.remove();
            else if (previous) {
              previous.data += child.data;
              child.remove();
            } else previous = child;
          } else {
            previous = null;
            child.normalize();
          }
        }
        return;
      }
      // Track mutations for MutationObserver childList records.
      const removed = [], added = [];
      function walk(node) {
        if (node.childNodes?.length) node.childNodes.forEach(walk);
      }
      walk(this);
      let previous = null;
      for (const child of [...this.childNodes]) {
        if (child.nodeType === 3) {
          if (child.data === "") {
            removed.push(child);
            child.remove();
          } else if (previous) {
            const oldLen = previous.length;
            previous.data += child.data;
            const newLen = previous.length;
            removed.push(child);
            added.push(previous);
            child.remove();
          } else previous = child;
        } else {
          previous = null;
          child.normalize();
        }
      }
      if (removed.length || added.length) {
        notify(this, "childList", { removedNodes: new NodeList(removed), addedNodes: new NodeList(added) });
      }
    }
    addEventListener(type, callback, options = {}) {
      if (!callback) return;
      const capture =
        typeof options === "boolean" ? options : !!options.capture;
      const key = String(type) + ":" + capture;
      let callbacks = this._callbacks.get(key);
      if (!callbacks) this._callbacks.set(key, (callbacks = new Map()));
      if (callbacks.has(callback)) return;
      const wrapped = (event) => {
        try {
          typeof callback === "function"
            ? callback.call(this, event)
            : callback.handleEvent(event);
        } catch (error) {
          report(error);
        }
      };
      callbacks.set(callback, wrapped);
      super.addEventListener(type, wrapped, options);
      if (this.getRootNode() === document) connected.set(this._id, this);
    }
    removeEventListener(type, callback, options = {}) {
      const capture =
        typeof options === "boolean" ? options : !!options.capture;
      const wrapped = this._callbacks
        .get(String(type) + ":" + capture)
        ?.get(callback);
      if (wrapped) {
        super.removeEventListener(type, wrapped, options);
        this._callbacks.get(String(type) + ":" + capture).delete(callback);
      }
    }
  }
  for (const [name, value] of Object.entries({
    ELEMENT_NODE: 1,
    ATTRIBUTE_NODE: 2,
    TEXT_NODE: 3,
    CDATA_SECTION_NODE: 4,
    ENTITY_REFERENCE_NODE: 5,
    ENTITY_NODE: 6,
    PROCESSING_INSTRUCTION_NODE: 7,
    COMMENT_NODE: 8,
    DOCUMENT_NODE: 9,
    DOCUMENT_TYPE_NODE: 10,
    DOCUMENT_FRAGMENT_NODE: 11,
    NOTATION_NODE: 12,
    DOCUMENT_POSITION_DISCONNECTED: 1,
    DOCUMENT_POSITION_PRECEDING: 2,
    DOCUMENT_POSITION_FOLLOWING: 4,
    DOCUMENT_POSITION_CONTAINS: 8,
    DOCUMENT_POSITION_CONTAINED_BY: 16,
    DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC: 32,
  })) {
    Object.defineProperty(Node, name, { value, configurable: true, enumerable: true });
    Object.defineProperty(Node.prototype, name, { value, configurable: true, enumerable: true });
  }
  class CharacterData extends Node {
    get data() {
      return this.nodeValue;
    }
    set data(value) {
      this.nodeValue = value;
    }
    get length() {
      return this.data.length;
    }
    substringData(offset, count) {
      if (offset > this.length) fail("IndexSizeError", "Offset exceeds data");
      return this.data.substring(offset, offset + count);
    }
    appendData(value) {
      this.replaceData(this.length, 0, value);
    }
    insertData(offset, value) {
      this.replaceData(offset, 0, value);
    }
    deleteData(offset, count) {
      this.replaceData(offset, count, "");
    }
    replaceData(offset, count, value) {
      if (offset > this.length) fail("IndexSizeError", "Offset exceeds data");
      value = String(value);
      count = Math.min(Number(count) >>> 0, this.length - offset);
      const old = this.data;
      host(
        "setText",
        this._id,
        old.slice(0, offset) + value + old.slice(offset + count),
      );
      changedRangeData(this, offset, count, value.length);
      notify(this, "characterData", { oldValue: old });
    }
  }
  class Text extends CharacterData {
    splitText(offset) {
      if (offset > this.length) fail("IndexSizeError", "Offset exceeds text");
      const next = this.ownerDocument.createTextNode(this.data.slice(offset));
      const old = this.data;
      if (this.parentNode) this.parentNode.insertBefore(next, this.nextSibling);
      eachRange((range) => {
        for (const point of [range._start, range._end]) {
          if (point[0] === this && point[1] > offset) {
            point[0] = next;
            point[1] -= offset;
          } else if (
            this.parentNode && point[0] === this.parentNode &&
            point[1] === nodeIndex(this) + 1
          ) point[1]++;
        }
      });
      host("setText", this._id, old.slice(0, offset));
      notify(this, "characterData", { oldValue: old });
      return next;
    }
    get wholeText() {
      let node = this;
      while (node.previousSibling?.nodeType === 3) node = node.previousSibling;
      let result = "";
      while (node?.nodeType === 3) {
        result += node.data;
        node = node.nextSibling;
      }
      return result;
    }
  }
  class Comment extends CharacterData {}
  class CDATASection extends Text {
    constructor(token, descriptor) {
      if (token !== internalToken) throw new TypeError("Illegal constructor");
      super(descriptor, descriptor);
    }
  }
  class ProcessingInstruction extends CharacterData {
    get target() {
      const info = host("info", this._id);
      return info.localName;
    }
  }
  class DocumentType extends Node {
    constructor(token, descriptor) {
      if (token !== internalToken) throw new TypeError("Illegal constructor");
      super(descriptor.root, host("info", descriptor.root));
    }
    get name() { return this._name || ""; }
    get publicId() { return this._publicId || ""; }
    get systemId() { return this._systemId || ""; }
    cloneNode(deep = false) {
      const target = component("clone", this._id, deep);
      const doc = this.ownerDocument || document;
      return new DocumentType(internalToken, { root: target, name: this.name, publicId: this.publicId, systemId: this.systemId });
    }
  }
  class DOMImplementation {
    createDocumentType(qualifiedName, publicId, systemId) {
      const target = component("createDocumentType", qualifiedName, publicId, systemId);
      return new DocumentType(internalToken, { root: target, name: qualifiedName, publicId, systemId });
    }
    createDocument(namespace, qualifiedName, doctype) {
      const id = component("createDocument", namespace || "", qualifiedName);
      return wrap(id, this._document || document);
    }
    createHTMLDocument(title) {
      const doc = this._document || document;
      const wrapper = doc.createElement("html");
      wrapper.append(doc.createElement("head"), doc.createElement("body"));
      doc.replaceWith(wrapper);
      doc._readyState = "complete";
      return doc;
    }
    hasFeature() { return true; }
  }
  class DocumentFragment extends Node {
    querySelector(selector) {
      return wrap(
        host("queryIn", this._id, String(selector)),
        this.ownerDocument,
      );
    }
    querySelectorAll(selector) {
      const nodes = host("queryAllIn", this._id, String(selector)).map((id) =>
        wrap(id, this.ownerDocument),
      );
      return nodeList(() => nodes);
    }
    getElementById(id) {
      return (
        [...this.querySelectorAll("[id]")].find((n) => n.id === String(id)) ||
        null
      );
    }
  }
  class ShadowRoot extends DocumentFragment {
    constructor(token, descriptor) {
      if (token !== shadowToken) fail("TypeError", "Illegal constructor");
      super(descriptor.root, host("info", descriptor.root));
      shadowDescriptors.set(this, descriptor);
      this._owner = wrap(descriptor.host)?.ownerDocument || document;
    }
    get host() { return wrap(shadowDescriptors.get(this).host); }
    get mode() { return shadowDescriptors.get(this).mode; }
    get delegatesFocus() { return shadowDescriptors.get(this).delegates_focus; }
    get innerHTML() {
      return [...this.childNodes]
        .map((child) => host("serialize", child._id))
        .join("");
    }
    set innerHTML(value) {
      elementInnerHTMLSet.call(this, value);
    }
    getElementById(id) {
      return (
        [...this.querySelectorAll("[id]")].find((n) => n.id === String(id)) ||
        null
      );
    }
  }
  const cssName = (name) =>
    String(name)
      .replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())
      .replace(/^css-float$/, "float");
  class CSSStyleDeclaration {
    constructor(element, computed = false) {
      this._element = element;
      this._computed = computed;
      return new Proxy(this, {
        get(target, key, receiver) {
          if (key in target || typeof key !== "string")
            return Reflect.get(target, key, receiver);
          if (/^\d+$/.test(key)) return target.item(Number(key));
          return target.getPropertyValue(cssName(key));
        },
        set(target, key, value, receiver) {
          if (key in target || typeof key !== "string")
            return Reflect.set(target, key, value, receiver);
          target.setProperty(cssName(key), value);
          return true;
        },
      });
    }
    get cssText() {
      return host("inlineCSS", this._element._id);
    }
    set cssText(value) {
      if (this._computed)
        fail("NoModificationAllowedError", "Computed style is readonly");
      this._element.setAttribute("style", String(value));
    }
    get length() {
      return this.cssText.split(";").filter((s) => s.includes(":")).length;
    }
    item(index) {
      return (
        this.cssText
          .split(";")
          .filter((s) => s.includes(":"))
          [index]?.split(":")[0]
          .trim() || ""
      );
    }
    getPropertyValue(name) {
      return host(
        this._computed ? "computed" : "inlineProperty",
        this._element._id,
        String(name),
      );
    }
    setProperty(name, value, priority = "") {
      if (this._computed)
        fail("NoModificationAllowedError", "Computed style is readonly");
      if (priority && String(priority).toLowerCase() !== "important") return;
      const old = this._element.getAttribute("style");
      host(
        "styleSet",
        this._element._id,
        String(name),
        String(value === null ? "" : value) + (priority ? " !important" : ""),
      );
      notify(this._element, "attributes", {
        attributeName: "style",
        oldValue: old,
      });
      flushComponents();
    }
    removeProperty(name) {
      const old = this.getPropertyValue(name);
      this.setProperty(name, "");
      return old;
    }
    getPropertyPriority(name) {
      return new RegExp(
        "(?:^|;)\\s*" +
          String(name).replace(/[.*+?^${}()|[\]\\]/g, "\\$&") +
          "\\s*:[^;]*!important",
        "i",
      ).test(this.cssText)
        ? "important"
        : "";
    }
  }
  class Element extends Node {
    get tagName() {
      return this.nodeName;
    }
    get localName() {
      return this._info.localName;
    }
    get namespaceURI() {
      return this._info.namespaceURI;
    }
    get prefix() {
      return this._info.prefix;
    }
    querySelector(selector) {
      return wrap(
        host("queryIn", this._id, String(selector)),
        this.ownerDocument,
      );
    }
    querySelectorAll(selector) {
      const nodes = host("queryAllIn", this._id, String(selector)).map((id) =>
        wrap(id, this.ownerDocument),
      );
      return nodeList(() => nodes);
    }
    matches(selector) {
      return host("matches", this._id, String(selector));
    }
    closest(selector) {
      return wrap(
        host("closest", this._id, String(selector)),
        this.ownerDocument,
      );
    }
    getElementsByTagName(name) {
      return nodeList(() => [...this.querySelectorAll(String(name))], true);
    }
    getElementsByClassName(names) {
      return nodeList(
        () => [
          ...this.querySelectorAll(
            String(names)
              .trim()
              .split(/\s+/)
              .map((n) => "." + cssEscape(n))
              .join(""),
          ),
        ],
        true,
      );
    }
    getAttribute(name) {
      name = String(name);
      return host(
        "attr",
        this._id,
        this.namespaceURI === "http://www.w3.org/1999/xhtml"
          ? name.toLowerCase()
          : name,
      );
    }
    hasAttribute(name) {
      return this.getAttribute(name) !== null;
    }
    setAttribute(name, value) {
      name = String(name);
      if (this.namespaceURI === "http://www.w3.org/1999/xhtml")
        name = name.toLowerCase();
      if (!/^[^\s"'<>/=]+$/.test(name))
        fail("InvalidCharacterError", "Invalid attribute name");
      const old = this.getAttribute(name);
      host("setAttr", this._id, name, String(value));
      if (name === "src" && isScript(this) && this.isConnected)
        prepareScript(this);
      if (name.startsWith("on")) {
        ensureHandler(name.slice(2));
        this[name] = new Function("event", String(value));
      }
      notify(this, "attributes", { attributeName: name, oldValue: old });
      flushComponents();
    }
    removeAttribute(name) {
      const old = this.getAttribute(name);
      detachAttribute(this, null, String(name));
      host("removeAttr", this._id, String(name));
      if (String(name).startsWith("on")) this[name] = null;
      if (old !== null) {
        notify(this, "attributes", {
          attributeName: String(name),
          oldValue: old,
        });
        flushComponents();
      }
    }
    toggleAttribute(name, force) {
      const add = force === undefined ? !this.hasAttribute(name) : !!force;
      if (add) this.setAttribute(name, "");
      else this.removeAttribute(name);
      return add;
    }
    getAttributeNS(namespace, name) {
      return host("attrNS", this._id, String(namespace || ""), String(name));
    }
    hasAttributeNS(namespace, name) {
      return this.getAttributeNS(namespace, name) !== null;
    }
    setAttributeNS(namespace, name, value) {
      host(
        "setAttrNS",
        this._id,
        String(namespace || ""),
        String(name),
        String(value),
      );
      notify(this, "attributes", {
        attributeName: String(name),
        oldValue: null,
      });
      flushComponents();
    }
    removeAttributeNS(namespace, name) {
      const attr = [...this.attributes].find(
        (a) => a.namespaceURI === namespace && a.localName === name,
      );
      if (attr) {
        const old = attr.value;
        detachAttribute(this, attr.namespaceURI, attr.localName);
        // The native qualified-name operation removes this namespace/name pair.
        host("removeAttrNS", this._id, String(namespace || ""), String(name));
        notify(this, "attributes", { attributeName: String(name), attributeNamespace: namespace || null, oldValue: old });
      }
    }
    get attributes() {
      return this._attributes || (this._attributes = makeNamedNodeMap(this));
    }
    hasAttributes() { return this.attributes.length !== 0; }
    getAttributeNames() { return [...this.attributes].map(attr => attr.name); }
    getAttributeNode(name) { return this.attributes.getNamedItem(name); }
    getAttributeNodeNS(namespace, name) { return this.attributes.getNamedItemNS(namespace, name); }
    setAttributeNode(attr) { return this.attributes.setNamedItem(attr); }
    setAttributeNodeNS(attr) { return this.attributes.setNamedItemNS(attr); }
    removeAttributeNode(attr) {
      if (attr.ownerElement !== this) fail("NotFoundError", "Attribute does not belong to this element");
      return this.attributes.removeNamedItemNS(attr.namespaceURI, attr.localName);
    }
    getElementsByTagNameNS(namespace, localName) {
      return nodeList(() => [...this.querySelectorAll("*")].filter(node =>
        (namespace === "*" || node.namespaceURI === (namespace || null)) &&
        (localName === "*" || node.localName === localName)), true);
    }
    get id() {
      return this.getAttribute("id") || "";
    }
    set id(value) {
      this.setAttribute("id", value);
    }
    get className() {
      return this.getAttribute("class") || "";
    }
    set className(value) {
      this.setAttribute("class", value);
    }
    get classList() { return this._classList || (this._classList = makeTokenList(this, "class")); }
    get dataset() {
      if (this._dataset) return this._dataset;
      const element = this;
      const attr = (name) =>
        "data-" + String(name).replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
      return (this._dataset = new Proxy(
        {},
        {
          get(_target, key) {
            if (typeof key === "string")
              return element.getAttribute(attr(key)) ?? undefined;
          },
          set(_target, key, value) {
            element.setAttribute(attr(key), String(value));
            return true;
          },
          deleteProperty(_target, key) {
            element.removeAttribute(attr(key));
            return true;
          },
          ownKeys() {
            return [...element.attributes]
              .filter((a) => a.name.startsWith("data-"))
              .map((a) =>
                a.name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase()),
              );
          },
          getOwnPropertyDescriptor() {
            return { enumerable: true, configurable: true };
          },
        },
      ));
    }
    get style() {
      return this._style || (this._style = new CSSStyleDeclaration(this));
    }
    set style(value) {
      this.style.cssText = value;
    }
    get innerHTML() {
      return [...this.childNodes]
        .map((child) => host("serialize", child._id))
        .join("");
    }
    set innerHTML(value) {
      const children = liveRanges.size ? [...this.childNodes] : [];
      const old = detachChildren(this);
      host("innerHTML", this._id, String(value));
      replacedRangeChildren(this, children);
      if (mutationObservers.size) {
        notify(this, "childList", {
          removedNodes: old,
          addedNodes: [...this.childNodes],
        });
      }
    }
    get outerHTML() {
      return host("serialize", this._id);
    }
    set outerHTML(value) {
      if (!this.parentNode) return;
      const context = this.ownerDocument.createElement(
        this.parentElement?.localName || "body",
      );
      context.innerHTML = String(value);
      this.replaceWith(...context.childNodes);
    }
    insertAdjacentHTML(position, html) {
      position = String(position).toLowerCase();
      const outside = position === "beforebegin" || position === "afterend";
      if (!outside && position !== "afterbegin" && position !== "beforeend")
        fail("SyntaxError", "Invalid insertion position");
      let context = outside ? this.parentNode : this;
      if (!context || context.nodeType === 9)
        fail("NoModificationAllowedError", "Insertion needs a parent element");
      if (
        context.nodeType !== 1 ||
        (context.namespaceURI === "http://www.w3.org/1999/xhtml" &&
          context.localName === "html")
      )
        context = this.ownerDocument.createElement("body");
      const fragment = this.ownerDocument.createElementNS(
        context.namespaceURI,
        context.localName,
      );
      fragment.innerHTML = String(html);
      this.insertAdjacentElementList(position, [...fragment.childNodes]);
    }
    insertAdjacentElementList(position, nodes) {
      switch (String(position).toLowerCase()) {
        case "beforebegin":
          this.before(...nodes);
          break;
        case "afterend":
          this.after(...nodes);
          break;
        case "afterbegin":
          this.prepend(...nodes);
          break;
        case "beforeend":
          this.append(...nodes);
          break;
        default:
          fail("SyntaxError", "Invalid insertion position");
      }
    }
    insertAdjacentElement(position, element) {
      this.insertAdjacentElementList(position, [element]);
      return element;
    }
    insertAdjacentText(position, text) {
      this.insertAdjacentElementList(position, [
        this.ownerDocument.createTextNode(text),
      ]);
    }
    getBoundingClientRect() {
      return DOMRect.fromRect(host("rect", this._id));
    }
    getClientRects() {
      return host("clientRects", this._id).map(rect=>DOMRect.fromRect(rect));
    }
    get clientWidth() {
      return Math.round(host("box", this._id).clientWidth);
    }
    get clientHeight() {
      return Math.round(host("box", this._id).clientHeight);
    }
    get clientLeft() {
      return Math.round(host("box", this._id).clientLeft);
    }
    get clientTop() {
      return Math.round(host("box", this._id).clientTop);
    }
    get offsetWidth() {
      return Math.round(host("box", this._id).offsetWidth);
    }
    get offsetHeight() {
      return Math.round(host("box", this._id).offsetHeight);
    }
    get offsetLeft() {
      return Math.round(host("box", this._id).offsetLeft);
    }
    get offsetTop() {
      return Math.round(host("box", this._id).offsetTop);
    }
    get offsetParent() {
      return wrap(host("box", this._id).offsetParent);
    }
    get scrollWidth() {
      return Math.round(host("box", this._id).scrollWidth);
    }
    get scrollHeight() {
      return Math.round(host("box", this._id).scrollHeight);
    }
    get value() {
      if (this.localName === "select")
        return (
          [...this.options].find((n) => n.selected)?.value ||
          this.options[0]?.value ||
          ""
        );
      if (this.localName === "option")
        return this.getAttribute("value") ?? this.textContent;
      return host("inputValue", this._id);
    }
    set value(value) {
      if (this.localName === "select") {
        for (const option of this.options)
          option.selected = option.value === String(value);
      } else this.setAttribute("value", String(value));
    }
    get checked() {
      return host("checked", this._id);
    }
    set checked(value) {
      host("setChecked", this._id, String(!!value));
    }
    get defaultChecked() {
      return this.hasAttribute("checked");
    }
    set defaultChecked(value) {
      this.toggleAttribute("checked", value);
    }
    get defaultValue() {
      return this.getAttribute("value") || "";
    }
    set defaultValue(value) {
      this.setAttribute("value", String(value));
    }
    get selected() {
      return this.hasAttribute("selected");
    }
    set selected(value) {
      this.toggleAttribute("selected", value);
    }
    get selectedIndex() {
      return [...this.options].findIndex((n) => n.selected);
    }
    set selectedIndex(value) {
      [...this.options].forEach((n, i) => (n.selected = i === Number(value)));
    }
    get options() {
      return this.getElementsByTagName("option");
    }
    get type() {
      return (
        this.getAttribute("type") ||
        (["input"].includes(this.localName) ? "text" : "")
      );
    }
    set type(value) {
      this.setAttribute("type", value);
    }
    get disabled() {
      return this.hasAttribute("disabled");
    }
    set disabled(value) {
      this.toggleAttribute("disabled", value);
    }
    get name() {
      return this.getAttribute("name") || "";
    }
    set name(value) {
      this.setAttribute("name", value);
    }
    get href() {
      const value = this.getAttribute("href");
      return value === null ? "" : new URL(value, this.ownerDocument.URL).href;
    }
    set href(value) {
      this.setAttribute("href", value);
    }
    get src() {
      return new URL(this.getAttribute("src") || "", this.ownerDocument.URL)
        .href;
    }
    set src(value) {
      this.setAttribute("src", value);
    }
    focus() {
      this.ownerDocument._active = this;
      host("focus", this._id);
      this.dispatchEvent(new Event("focus"));
    }
    blur() {
      if (this.ownerDocument._active === this)
        this.ownerDocument._active = null;
      this.dispatchEvent(new Event("blur"));
    }
    click() {
      if (this._clicking || this.hasAttribute("disabled")) return;
      this._clicking = true;
      try {
        const Mouse = globalThis.MouseEvent || Event;
        const event = new Mouse("click", {
          bubbles: true,
          cancelable: true,
          composed: true,
          detail: 1,
        });
        // Activation behaviour (links, submit buttons, checkboxes) runs natively.
        if (this.dispatchEvent(event)) host("activate", this._id);
      } finally {
        this._clicking = false;
      }
    }
    attachShadow(options) {
      const descriptor = component("shadowAttach", this._id, String(options?.mode), !!options?.delegatesFocus);
      return shadowWrapper(descriptor);
    }
    get shadowRoot() { return shadowWrapper(component("shadowRoot", this._id)); }
    get assignedSlot() { return wrap(component("assignedSlot", this._id)); }
  }
  function upgradeElement(element, definition) {
    const constructor = componentConstructors.get(definition);
    if (!constructor || !component("customBegin", element._id, definition)) return;
    Object.setPrototypeOf(element, constructor.prototype);
    constructionStack.push({ element, constructor, used: false });
    let success = false;
    try {
      const result = Reflect.construct(constructor, [], constructor);
      if (result !== element) fail("TypeError", "Custom element constructor returned a different object");
      success = true;
    } catch (error) { report(error); }
    finally {
      constructionStack.pop();
      component("customComplete", element._id, success);
    }
    flushComponents();
  }
  function triggerConnected(node) {
    component("customConnected", node._id); flushComponents();
  }
  function triggerDisconnected(node) {
    component("customDisconnected", node._id); flushComponents();
  }
  class CustomElementRegistry {
    define(name, constructor, options) {
      if (typeof constructor !== "function" || !constructor.prototype) fail("TypeError", "Custom element constructor must be constructible");
      const identity = constructorDefinitions.get(constructor)?.constructorIdentity || ++constructorIdentity;
      const nameString = String(name);
      const extendsName = options?.extends === undefined ? "" : String(options.extends);
      component("customPreflight", nameString, identity, extendsName);
      const callbacks = Object.create(null);
      for (const name of ["connectedCallback", "disconnectedCallback", "adoptedCallback", "attributeChangedCallback"]) {
        const callback = constructor.prototype[name];
        if (callback !== undefined && typeof callback !== "function") fail("TypeError", name + " must be callable");
        callbacks[name] = callback;
      }
      const observed = callbacks.attributeChangedCallback ? Array.from(constructor.observedAttributes || [], String) : [];
      const definition = component("customDefine", nameString, identity, JSON.stringify({ observed, extends: extendsName || undefined }));
      constructorDefinitions.set(constructor, { ...definition, constructorIdentity: identity });
      componentConstructors.set(definition.id, constructor);
      componentCallbacks.set(definition.id, callbacks);
      componentWaiters.get(definition.name)?.resolve(constructor);
      componentWaiters.delete(definition.name);
      for (const id of component("customCandidates", document._id, definition.id)) upgradeElement(wrap(id), definition.id);
    }
    get(name) {
      const definition = component("customDefinition", String(name));
      return definition ? componentConstructors.get(definition.id) : undefined;
    }
    getName(constructor) { return constructorDefinitions.get(constructor)?.name || null; }
    whenDefined(name) {
      name = String(name);
      if (!component("customNameValid", name)) return Promise.reject(new SyntaxError("Invalid custom element name"));
      const existing = this.get(name);
      if (existing) return Promise.resolve(existing);
      if (!componentWaiters.has(name)) {
        let resolve;
        const promise = new Promise(done => { resolve = done; });
        componentWaiters.set(name, { promise, resolve });
      }
      return componentWaiters.get(name).promise;
    }
    upgrade(root) {
      if (!(root instanceof Node)) fail("TypeError", "upgrade requires a Node");
      for (const [definition] of componentConstructors) {
        for (const id of component("customCandidates", root._id, definition)) upgradeElement(wrap(id), definition);
      }
    }
  }
  const customElements = new CustomElementRegistry();
  // Captured before the generated layer brand-checks Element.prototype;
  // ShadowRoot reuses the semantic fragment-parsing setter.
  const elementInnerHTMLSet = Object.getOwnPropertyDescriptor(Element.prototype, "innerHTML").set;
  class HTMLElement extends Element {
    constructor(id, info) {
      if ((id !== undefined || info !== undefined) && !constructingNode) throw new TypeError("Illegal constructor");
      const construction = constructionStack[constructionStack.length - 1];
      if (id === undefined && info === undefined && construction?.constructor === new.target) {
        if (construction.used) fail("TypeError", "Custom element already constructed");
        construction.used = true;
        return construction.element;
      }
      if (id === undefined || info === undefined) {
        const definition = constructorDefinitions.get(new.target);
        if (!definition) fail("TypeError", "Illegal constructor");
        id = component("customConstruct", definition.id);
        info = host("info", id);
      }
      super(id, info);
      cache.set(id, weak ? new WeakRef(this) : this);
      host("retain", id);
      finalizer?.register(this, id);
    }
  }
  class SVGElement extends Element {}
  for (const key of [
    "title",
    "lang",
    "dir",
    "rel",
    "target",
    "htmlFor",
    "placeholder",
  ])
    Object.defineProperty(Element.prototype, key, {
      get() {
        return this.getAttribute(key === "htmlFor" ? "for" : key) || "";
      },
      set(value) {
        this.setAttribute(key === "htmlFor" ? "for" : key, value);
      },
    });

  function tokensChecked(tokens) {
    for (const token of tokens) {
      if (!token) fail("SyntaxError", "Token must not be empty");
      if (/[\t\n\f\r ]/.test(token)) fail("InvalidCharacterError", "Token contains ASCII whitespace");
    }
    return tokens;
  }
  const attributeCaches = privateWeakMap();
  function checkedAttributeName(namespace, name) {
    if (!/^[A-Za-z_][\w.:-]*$/.test(name) || name.split(":").length > 2)
      fail("InvalidCharacterError", "Invalid attribute name");
    const parts = name.split(":"), prefix = parts.length === 2 ? parts[0] : null;
    if ((prefix && !namespace) || (prefix === "xml" && namespace !== "http://www.w3.org/XML/1998/namespace") ||
        ((name === "xmlns" || prefix === "xmlns") !== (namespace === "http://www.w3.org/2000/xmlns/")))
      fail("NamespaceError", "Invalid attribute namespace");
    return { namespace: namespace || null, name, prefix, localName: parts.at(-1) };
  }
  function makeAttribute(ownerDocument, namespace, name, value) {
    const meta = checkedAttributeName(namespace, name);
    return new Attr(internalToken, { ...meta, value, owner: null, document: ownerDocument });
  }
  function attributesFor(element) {
    let cache = attributeCaches.get(element);
    if (!cache) attributeCaches.set(element, cache = new Map());
    return cache;
  }
  const attributeKey = (namespace, localName) => JSON.stringify([namespace || null, localName]);
  function attachedAttribute(element, meta) {
    const cache = attributesFor(element), key = attributeKey(meta.namespaceURI, meta.localName);
    let attr = cache.get(key);
    if (!attr) {
      attr = makeAttribute(element.ownerDocument, meta.namespaceURI, meta.name, meta.value);
      attrData.get(attr).owner = element; cache.set(key, attr);
    }
    const state = attrData.get(attr);
    state.name = meta.name; state.prefix = meta.name.includes(":") ? meta.name.split(":")[0] : null;
    return attr;
  }
  function detachAttribute(element, namespace, localName) {
    const cache = attributeCaches.get(element), key = attributeKey(namespace, localName), attr = cache?.get(key);
    if (!attr) return;
    const state = attrData.get(attr);
    state.value = element.getAttributeNS(state.namespace, state.localName) ?? "";
    state.owner = null; cache.delete(key);
  }
  class Attr extends Node {
    constructor(token, state) {
      if (token !== internalToken) throw new TypeError("Illegal constructor");
      super("", { nodeType: 2, nodeName: state.name });
      attrData.set(this, state); this._owner = state.document;
    }
    get namespaceURI() { return attrData.get(this).namespace; }
    get prefix() { return attrData.get(this).prefix; }
    get localName() { return attrData.get(this).localName; }
    get name() { return attrData.get(this).name; }
    get nodeName() { return this.name; }
    get specified() { return true; }
    get ownerElement() { return attrData.get(this).owner; }
    get value() { const s = attrData.get(this); return s.owner ? s.owner.getAttributeNS(s.namespace, s.localName) ?? "" : s.value; }
    set value(value) { const s = attrData.get(this); if (s.owner) s.owner.setAttributeNS(s.namespace, s.name, value); else s.value = String(value); }
    get nodeValue() { return this.value; }
    set nodeValue(value) { this.value = value ?? ""; }
    get textContent() { return this.value; }
    set textContent(value) { this.value = value ?? ""; }
    get parentNode() { return null; }
    get parentElement() { return null; }
    get firstChild() { return null; }
    get lastChild() { return null; }
    get previousSibling() { return null; }
    get nextSibling() { return null; }
    get childNodes() { return this._childNodes || (this._childNodes = nodeList(() => [])); }
    get isConnected() { return false; }
    getRootNode() { return this; }
    cloneNode() { return makeAttribute(this.ownerDocument, this.namespaceURI, this.name, this.value); }
    appendChild() { fail("HierarchyRequestError", "Attributes have no children"); }
    insertBefore() { fail("HierarchyRequestError", "Attributes have no children"); }
  }
  class NamedNodeMap {
    constructor(token, element) {
      if (token !== internalToken) throw new TypeError("Illegal constructor");
      namedMapData.set(this, { element });
    }
    _read() { const { element } = namedMapData.get(this); return host("attributes", element._id).map(meta => attachedAttribute(element, meta)); }
    get length() { return this._read().length; }
    item(index) { return this._read()[index] || null; }
    getNamedItem(name) {
      const { element } = namedMapData.get(this);
      if (element.namespaceURI === HTML_NS) name = name.toLowerCase();
      return this._read().find(attr => attr.name === name) || null;
    }
    getNamedItemNS(namespace, localName) { return this._read().find(attr => attr.namespaceURI === (namespace || null) && attr.localName === localName) || null; }
    setNamedItem(attr) { return this.setNamedItemNS(attr); }
    setNamedItemNS(attr) {
      const { element } = namedMapData.get(this), state = attrData.get(attr);
      if (state.owner && state.owner !== element) fail("InUseAttributeError", "Attribute is already attached");
      const previous = this.getNamedItemNS(state.namespace, state.localName);
      if (previous === attr) return attr;
      const old = previous?.value;
      element.setAttributeNS(state.namespace, state.name, state.value);
      if (previous) { const s = attrData.get(previous); s.owner = null; s.value = old; }
      state.owner = element; state.document = element.ownerDocument; attr._owner = state.document;
      attributesFor(element).set(attributeKey(state.namespace, state.localName), attr);
      return previous;
    }
    removeNamedItem(name) {
      const attr = this.getNamedItem(name);
      if (!attr) fail("NotFoundError", "Attribute not found");
      return this.removeNamedItemNS(attr.namespaceURI, attr.localName);
    }
    removeNamedItemNS(namespace, localName) {
      const attr = this.getNamedItemNS(namespace, localName);
      if (!attr) fail("NotFoundError", "Attribute not found");
      namedMapData.get(this).element.removeAttributeNS(namespace, localName); return attr;
    }
    [Symbol.iterator]() { return this._read()[Symbol.iterator](); }
  }
  function makeNamedNodeMap(element) {
    const target = new NamedNodeMap(internalToken, element);
    const proxy = new Proxy(target, {
      get(target, key, receiver) {
        if (typeof key === "string" && /^(0|[1-9]\d*)$/.test(key)) return target.item(Number(key)) ?? undefined;
        const value = Reflect.get(target, key, receiver);
        return value === undefined && typeof key === "string" ? target.getNamedItem(key) ?? undefined : value;
      },
      ownKeys(target) { return target._read().map((_, index) => String(index)); },
      getOwnPropertyDescriptor(target, key) {
        if (typeof key === "string" && /^(0|[1-9]\d*)$/.test(key) && Number(key) < target.length)
          return { configurable: true, enumerable: true, value: target.item(Number(key)) };
        return Reflect.getOwnPropertyDescriptor(target, key);
      },
      has(target, key) { return Reflect.has(target, key) || (typeof key === "string" && (/^(0|[1-9]\d*)$/.test(key) ? Number(key) < target.length : !!target.getNamedItem(key))); },
    });
    namedMapData.set(proxy, namedMapData.get(target)); return proxy;
  }

  class DOMTokenList {
    constructor(token, element, attribute) {
      if (token !== internalToken) throw new TypeError("Illegal constructor");
      tokenData.set(this, { element, attribute });
    }
    get length() { return this._tokens().length; }
    _tokens() {
      const state = tokenData.get(this);
      return [...new Set((state.element.getAttribute(state.attribute) || "").split(/[\t\n\f\r ]+/).filter(Boolean))];
    }
    // DOMTokenList update steps: no attribute is created for an empty set.
    _write(tokens) {
      const state = tokenData.get(this);
      if (!tokens.length && !state.element.hasAttribute(state.attribute)) return;
      state.element.setAttribute(state.attribute, tokens.join(" "));
    }
    item(index) { return this._tokens()[index] ?? null; }
    contains(token) { return this._tokens().includes(String(token)); }
    add(...tokens) { tokensChecked(tokens); this._write([...new Set([...this._tokens(), ...tokens])]); }
    remove(...tokens) { tokensChecked(tokens); this._write(this._tokens().filter(token => !tokens.includes(token))); }
    toggle(token, force) {
      tokensChecked([token]);
      const tokens = this._tokens();
      if (tokens.includes(token)) {
        if (force) return true;
        this._write(tokens.filter((t) => t !== token));
        return false;
      }
      if (force === undefined || force) { this._write([...tokens, token]); return true; }
      return false;
    }
    replace(token, replacement) {
      // replace() checks both for emptiness before either for whitespace.
      if (!token || !replacement) fail("SyntaxError", "Token must not be empty");
      tokensChecked([token, replacement]);
      const tokens = this._tokens();
      if (!tokens.includes(token)) return false;
      // Ordered-set replace: the first of either becomes replacement, others go.
      const first = tokens.findIndex((t) => t === token || t === replacement);
      this._write(tokens.filter((t, i) => i === first || (t !== token && t !== replacement)).map((t, i) => (i === first ? replacement : t)));
      return true;
    }
    supports(token) {
      const state = tokenData.get(this);
      if (state.attribute !== "sandbox") throw new TypeError("This token list has no supported-token vocabulary");
      return ["allow-downloads", "allow-forms", "allow-modals", "allow-orientation-lock", "allow-pointer-lock", "allow-popups", "allow-popups-to-escape-sandbox", "allow-presentation", "allow-same-origin", "allow-scripts", "allow-storage-access-by-user-activation", "allow-top-navigation", "allow-top-navigation-by-user-activation", "allow-top-navigation-to-custom-protocols"].includes(token.toLowerCase());
    }
    get value() { const s = tokenData.get(this); return s.element.getAttribute(s.attribute) || ""; }
    set value(value) { const s = tokenData.get(this); s.element.setAttribute(s.attribute, value); }
    toString() { return this.value; }
    [Symbol.iterator]() { return this._tokens()[Symbol.iterator](); }
  }
  function makeTokenList(element, attribute) {
    const target = new DOMTokenList(internalToken, element, attribute);
    const proxy = new Proxy(target, { get(target, key, receiver) {
      if (typeof key === "string" && /^(0|[1-9]\d*)$/.test(key)) return target._tokens()[Number(key)];
      return Reflect.get(target, key, receiver);
    }});
    tokenData.set(proxy, tokenData.get(target)); return proxy;
  }

  // Nested documents are not exposed until a browser-issued frame context is
  // bound to a real worker. A detached synthetic document is not an iframe.
  class HTMLIFrameElement extends HTMLElement {
    get contentWindow() { return null; }
    get contentDocument() { return null; }
    get src() { return this.getAttribute("src") || ""; }
    set src(value) { this.setAttribute("src", String(value)); }
    get srcdoc() { return this.getAttribute("srcdoc") || ""; }
    set srcdoc(value) { this.setAttribute("srcdoc", String(value)); }
    get name() { return this.getAttribute("name") || ""; }
    set name(value) { this.setAttribute("name", String(value)); }
    get width() { return this.getAttribute("width") || ""; }
    set width(value) { this.setAttribute("width", String(value)); }
    get height() { return this.getAttribute("height") || ""; }
    set height(value) { this.setAttribute("height", String(value)); }
    get sandbox() { return this._sandbox || (this._sandbox = makeTokenList(this, "sandbox")); }
  }
  Object.defineProperty(Element.prototype, "form", {
    get() {
      if (
        ![
          "input",
          "button",
          "select",
          "textarea",
          "fieldset",
          "output",
          "object",
        ].includes(this.localName)
      )
        return undefined;
      const owner = this.getAttribute("form");
      if (owner !== null)
        return this.ownerDocument.getElementById(owner)?.localName === "form"
          ? this.ownerDocument.getElementById(owner)
          : null;
      return this.closest("form");
    },
  });
  class HTMLInputElement extends HTMLElement {}
  class HTMLTextAreaElement extends HTMLElement {}
  class HTMLSelectElement extends HTMLElement {}
  class HTMLOptionElement extends HTMLElement {}
  class HTMLButtonElement extends HTMLElement {}
  class HTMLFormElement extends HTMLElement {
    // submit() skips the submit event and validation, per HTML.
    submit() {
      host("submitForm", this._id, "");
    }
    requestSubmit(submitter = null) {
      if (submitter !== null && submitter.form !== this)
        fail("NotFoundError", "The submitter is not owned by this form");
      if (!__wega_submit_event(this._id, submitter ? submitter._id : null))
        host("submitForm", this._id, submitter ? submitter._id : "");
    }
    reset() {
      if (
        this.dispatchEvent(
          new Event("reset", { bubbles: true, cancelable: true }),
        )
      )
        for (const control of this.elements) {
          if (["checkbox", "radio"].includes(control.type))
            control.checked = control.defaultChecked;
          else if (control.localName !== "select" && "defaultValue" in control)
            control.value = control.defaultValue;
        }
    }
    get length() {
      return this.elements.length;
    }
    get action() {
      const value = this.getAttribute("action");
      return new URL(value || "", this.ownerDocument.URL).href;
    }
    set action(value) {
      this.setAttribute("action", value);
    }
    get method() {
      return (this.getAttribute("method") || "get").toLowerCase() === "post"
        ? "post"
        : "get";
    }
    set method(value) {
      this.setAttribute("method", value);
    }
    get enctype() {
      const value = (this.getAttribute("enctype") || "").toLowerCase();
      return ["multipart/form-data", "text/plain"].includes(value)
        ? value
        : "application/x-www-form-urlencoded";
    }
    set enctype(value) {
      this.setAttribute("enctype", value);
    }
    get elements() {
      return nodeList(
        () => [
          ...this.querySelectorAll(
            "input,select,textarea,button,fieldset,object,output",
          ),
        ],
        true,
      );
    }
  }
  class HTMLAnchorElement extends HTMLElement {}
  for (const key of [
    "protocol",
    "host",
    "hostname",
    "port",
    "pathname",
    "search",
    "hash",
    "origin",
  ])
    Object.defineProperty(HTMLAnchorElement.prototype, key, {
      get() {
        return new URL(this.href || this.ownerDocument.URL)[key];
      },
      set(value) {
        const url = new URL(this.href || this.ownerDocument.URL);
        url[key] = String(value);
        this.href = url.href;
      },
    });
  class HTMLImageElement extends HTMLElement {}
  class HTMLScriptElement extends HTMLElement {
    get src() {
      const value = this.getAttribute("src");
      return value === null ? "" : new URL(value, this.ownerDocument.URL).href;
    }
    set src(value) {
      this.setAttribute("src", value);
    }
    get text() {
      return this.textContent;
    }
    set text(value) {
      this.textContent = value;
    }
    // Script-created scripts are async unless script clears `async`.
    get async() {
      return this._forceAsync === true || this.hasAttribute("async");
    }
    set async(value) {
      this._forceAsync = false;
      this.toggleAttribute("async", !!value);
    }
  }
  for (const [key, attribute] of [
    ["defer", "defer"],
    ["noModule", "nomodule"],
  ])
    Object.defineProperty(HTMLScriptElement.prototype, key, {
      get() {
        return this.hasAttribute(attribute);
      },
      set(value) {
        this.toggleAttribute(attribute, !!value);
      },
    });
  for (const [key, attribute] of [
    ["nonce", "nonce"],
    ["charset", "charset"],
    ["integrity", "integrity"],
    ["crossOrigin", "crossorigin"],
    ["referrerPolicy", "referrerpolicy"],
  ])
    Object.defineProperty(HTMLScriptElement.prototype, key, {
      get() {
        return (
          this.getAttribute(attribute) ?? (key === "crossOrigin" ? null : "")
        );
      },
      set(value) {
        this.setAttribute(attribute, value);
      },
    });
  const HTML_NS = "http://www.w3.org/1999/xhtml";
  const isScript = (node) =>
    node?.nodeType === 1 &&
    node.localName === "script" &&
    node.namespaceURI === HTML_NS;
  const scriptType = (script) => {
    const type = (script.getAttribute("type") ?? "").trim().toLowerCase();
    if (type === "module") return "module";
    return /^(|(text|application)\/(x-)?(java|ecma)script|text\/javascript1\.[0-5]|text\/(jscript|livescript))$/.test(
      type,
    )
      ? "classic"
      : null;
  };
  let currentScript = null;
  // HTML "prepare the script element" for script-inserted scripts: inline
  // classic scripts run now; external and module scripts go to the native loader.
  function prepareScript(script) {
    if (!isScript(script) || !script.isConnected) return;
    const kind = scriptType(script);
    if (!kind) return;
    if (!script.hasAttribute("src")) {
      const text = script.textContent;
      if (!text) return;
      if (kind === "module") {
        host("scriptInserted", script._id, "true");
        return;
      }
      if (!host("scriptStart", script._id)) return;
      if (!host("cspInline", script.getAttribute("nonce") || "", text)) {
        host(
          "error",
          "Refused to execute an inline script: Content Security Policy",
        );
        return;
      }
      const previous = currentScript;
      currentScript = script;
      try {
        __wega_run_script(text, host("url"));
      } catch (error) {
        report(error);
      } finally {
        currentScript = previous;
      }
      return;
    }
    host("scriptInserted", script._id, String(script._forceAsync === true));
  }
  function prepareInserted(node) {
    if (node?.nodeType !== 1) return;
    if (isScript(node)) prepareScript(node);
    else if (node.firstElementChild) {
      for (const script of node.querySelectorAll("script"))
        prepareScript(script);
    }
  }
  class HTMLTemplateElement extends HTMLElement {
    get content() {
      return wrap(host("templateContent", this._id), this.ownerDocument);
    }
  }
  class HTMLSlotElement extends HTMLElement {
    get name() {
      return this.getAttribute("name") || "";
    }
    set name(value) {
      this.setAttribute("name", value);
    }
    assignedNodes(options) {
      return component("assignedNodes", this._id, !!options?.flatten).map(id => wrap(id));
    }
    assignedElements(options) {
      return this.assignedNodes(options).filter((node) => node.nodeType === 1);
    }
  }
  class CanvasRenderingContext2D {
    constructor(canvas) {
      this._canvas = canvas;
      this.fillStyle = "#000000";
      this.strokeStyle = "#000000";
      this.lineWidth = 1;
      this.lineCap = "butt";
      this.lineJoin = "miter";
      this.miterLimit = 10;
      this.lineDashOffset = 0;
      this.globalAlpha = 1.0;
      this.globalCompositeOperation = "source-over";
      this.font = "10px sans-serif";
      this.textAlign = "start";
      this.textBaseline = "alphabetic";
      this.direction = "inherit";
      this.shadowBlur = 0;
      this.shadowColor = "rgba(0, 0, 0, 0)";
      this.shadowOffsetX = 0;
      this.shadowOffsetY = 0;
      this.imageSmoothingEnabled = true;
      this.imageSmoothingQuality = "low";
      this.filter = "none";
    }
    get canvas() {
      return this._canvas;
    }
    save() {}
    restore() {}
    reset() {}
    scale(_x, _y) {}
    rotate(_angle) {}
    translate(_x, _y) {}
    transform(_a, _b, _c, _d, _e, _f) {}
    setTransform(_a, _b, _c, _d, _e, _f) {}
    resetTransform() {}
    getTransform() {
      return {
        a: 1,
        b: 0,
        c: 0,
        d: 1,
        e: 0,
        f: 0,
        m11: 1,
        m12: 0,
        m21: 0,
        m22: 1,
        m41: 0,
        m42: 0,
      };
    }
    clearRect(_x, _y, _w, _h) {}
    fillRect(_x, _y, _w, _h) {}
    strokeRect(_x, _y, _w, _h) {}
    beginPath() {}
    fill(_fillRule) {}
    stroke() {}
    clip(_fillRule) {}
    isPointInPath(_x, _y) {
      return false;
    }
    isPointInStroke(_x, _y) {
      return false;
    }
    fillText(_text, _x, _y, _maxWidth) {}
    strokeText(_text, _x, _y, _maxWidth) {}
    measureText(text) {
      const len = String(text ?? "").length;
      return {
        width: len * 8,
        actualBoundingBoxLeft: 0,
        actualBoundingBoxRight: len * 8,
        fontBoundingBoxAscent: 10,
        fontBoundingBoxDescent: 2,
        actualBoundingBoxAscent: 10,
        actualBoundingBoxDescent: 2,
        emHeightAscent: 10,
        emHeightDescent: 2,
        hangingBaseline: 8,
        alphabeticBaseline: 0,
        ideographicBaseline: -2,
      };
    }
    drawImage() {}
    createImageData(w, h) {
      const width = Math.max(0, w | 0);
      const height = Math.max(0, h | 0);
      return {
        width,
        height,
        data: new Uint8ClampedArray(Math.max(0, (width * height * 4) | 0)),
      };
    }
    getImageData(_sx, _sy, sw, sh) {
      const width = Math.max(0, sw | 0);
      const height = Math.max(0, sh | 0);
      return {
        width,
        height,
        data: new Uint8ClampedArray(Math.max(0, (width * height * 4) | 0)),
      };
    }
    putImageData(_imagedata, _dx, _dy) {}
    createLinearGradient(_x0, _y0, _x1, _y1) {
      return { addColorStop() {} };
    }
    createRadialGradient(_x0, _y0, _r0, _x1, _y1, _r1) {
      return { addColorStop() {} };
    }
    createConicGradient(_startAngle, _x, _y) {
      return { addColorStop() {} };
    }
    createPattern(_image, _repetition) {
      return { setTransform() {} };
    }
    setLineDash(_segments) {}
    getLineDash() {
      return [];
    }
    closePath() {}
    moveTo(_x, _y) {}
    lineTo(_x, _y) {}
    quadraticCurveTo(_cpx, _cpy, _x, _y) {}
    bezierCurveTo(_cp1x, _cp1y, _cp2x, _cp2y, _x, _y) {}
    arcTo(_x1, _y1, _x2, _y2, _radius) {}
    rect(_x, _y, _w, _h) {}
    roundRect(_x, _y, _w, _h, _radii) {}
    arc(_x, _y, _radius, _startAngle, _endAngle, _counterclockwise) {}
    ellipse(
      _x,
      _y,
      _radiusX,
      _radiusY,
      _rotation,
      _startAngle,
      _endAngle,
      _counterclockwise,
    ) {}
  }
  class HTMLCanvasElement extends HTMLElement {
    get width() {
      const v = this.getAttribute("width");
      return v === null ? 300 : parseInt(v, 10) || 0;
    }
    set width(value) {
      this.setAttribute("width", String(value >>> 0));
    }
    get height() {
      const v = this.getAttribute("height");
      return v === null ? 150 : parseInt(v, 10) || 0;
    }
    set height(value) {
      this.setAttribute("height", String(value >>> 0));
    }
    getContext(type) {
      if (type === "2d") {
        if (!this._context2d) {
          this._context2d = new CanvasRenderingContext2D(this);
        }
        return this._context2d;
      }
      return null;
    }
    toDataURL() {
      return "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    }
    toBlob(callback) {
      if (typeof callback === "function") {
        setTimeout(() => callback(null), 0);
      }
    }
  }
  class Screen {
    get width() {
      try {
        return Math.round(host("viewport")?.width || 1920);
      } catch {
        return 1920;
      }
    }
    get height() {
      try {
        return Math.round(host("viewport")?.height || 1080);
      } catch {
        return 1080;
      }
    }
    get availWidth() {
      return this.width;
    }
    get availHeight() {
      return this.height;
    }
    get colorDepth() {
      return 24;
    }
    get pixelDepth() {
      return 24;
    }
    get orientation() {
      return {
        type: "landscape-primary",
        angle: 0,
        onchange: null,
      };
    }
  }
  const screen = new Screen();
  const htmlConstructors = {
    iframe: HTMLIFrameElement,
    input: HTMLInputElement,
    textarea: HTMLTextAreaElement,
    select: HTMLSelectElement,
    option: HTMLOptionElement,
    button: HTMLButtonElement,
    form: HTMLFormElement,
    a: HTMLAnchorElement,
    img: HTMLImageElement,
    canvas: HTMLCanvasElement,
    script: HTMLScriptElement,
    template: HTMLTemplateElement,
    slot: HTMLSlotElement,
  };

  // Ranges hold real DOM boundary points. Fragment parsing uses the start
  // element's HTML/SVG context and retains executable fragment scripts.
  const liveRanges = new Set();
  function eachRange(update) {
    for (const reference of liveRanges) {
      const range = weak ? reference.deref() : reference;
      if (range) update(range);
      else liveRanges.delete(reference);
    }
  }
  const nodeIndex = (node) => [...node.parentNode.childNodes].indexOf(node);
  const nodeLength = (node) =>
    node instanceof CharacterData ? node.length : node.childNodes.length;
  function boundary(node, offset) {
    if (!(node instanceof Node)) throw new TypeError("Expected a Node");
    if (node.nodeType === 10) {
      fail("InvalidNodeTypeError", "A doctype cannot be a range boundary");
    }
    offset = Number(offset) >>> 0;
    if (offset > nodeLength(node)) {
      fail("IndexSizeError", "Offset exceeds node length");
    }
    return [node, offset];
  }
  function comparePoints(a, b) {
    if (a[0] === b[0]) return Math.sign(a[1] - b[1]);
    if (a[0].contains(b[0])) {
      let child = b[0];
      while (child.parentNode !== a[0]) child = child.parentNode;
      return nodeIndex(child) < a[1] ? 1 : -1;
    }
    if (b[0].contains(a[0])) return -comparePoints(b, a);
    return a[0].compareDocumentPosition(b[0]) & 4 ? -1 : 1;
  }
  function removedRangeNode(node) {
    if (!liveRanges.size) return;
    const parent = node.parentNode, index = nodeIndex(node);
    eachRange((range) => {
      for (const key of ["_start", "_end"]) {
        const point = range[key];
        if (node.contains(point[0])) range[key] = [parent, index];
        else if (point[0] === parent && point[1] > index) point[1]--;
      }
    });
  }
  function insertedRangeNode(parent, index) {
    if (!liveRanges.size) return;
    eachRange((range) => {
      for (const point of [range._start, range._end]) {
        if (point[0] === parent && point[1] > index) point[1]++;
      }
    });
  }
  function changedRangeData(node, offset, count, length) {
    if (!liveRanges.size) return;
    eachRange((range) => {
      for (const point of [range._start, range._end]) {
        if (point[0] === node && point[1] > offset) {
          point[1] = point[1] <= offset + count
            ? offset
            : point[1] + length - count;
        }
      }
    });
  }
  function replacedRangeChildren(parent, children) {
    eachRange((range) => {
      for (const key of ["_start", "_end"]) {
        if (
          range[key][0] === parent ||
          children.some((child) => child.contains(range[key][0]))
        ) {
          range[key] = [parent, 0];
        }
      }
    });
  }
  class Range {
    constructor() {
      this._start = [document, 0];
      this._end = [document, 0];
      liveRanges.add(weak ? new WeakRef(this) : this);
    }
    get startContainer() {
      return this._start[0];
    }
    get startOffset() {
      return this._start[1];
    }
    get endContainer() {
      return this._end[0];
    }
    get endOffset() {
      return this._end[1];
    }
    get collapsed() {
      return this.startContainer === this.endContainer &&
        this.startOffset === this.endOffset;
    }
    get commonAncestorContainer() {
      let node = this.startContainer;
      while (!node.contains(this.endContainer)) node = node.parentNode;
      return node;
    }
    setStart(node, offset) {
      const point = boundary(node, offset);
      if (
        node.getRootNode() !== this.endContainer.getRootNode() ||
        comparePoints(point, this._end) > 0
      ) {
        this._end = [...point];
      }
      this._start = point;
    }
    setEnd(node, offset) {
      const point = boundary(node, offset);
      if (
        node.getRootNode() !== this.startContainer.getRootNode() ||
        comparePoints(point, this._start) < 0
      ) {
        this._start = [...point];
      }
      this._end = point;
    }
    _relative(node, after, end) {
      if (!(node instanceof Node)) throw new TypeError("Expected a Node");
      if (!node.parentNode) {
        fail("InvalidNodeTypeError", "The node needs a parent");
      }
      this[end ? "setEnd" : "setStart"](
        node.parentNode,
        nodeIndex(node) + Number(after),
      );
    }
    setStartBefore(node) {
      this._relative(node, false, false);
    }
    setStartAfter(node) {
      this._relative(node, true, false);
    }
    setEndBefore(node) {
      this._relative(node, false, true);
    }
    setEndAfter(node) {
      this._relative(node, true, true);
    }
    selectNode(node) {
      this.setStartBefore(node);
      this.setEndAfter(node);
    }
    selectNodeContents(node) {
      const end = boundary(node, nodeLength(node));
      this._start = [node, 0];
      this._end = end;
    }
    collapse(toStart = false) {
      const point = toStart ? this._start : this._end;
      this._start = [...point];
      this._end = [...point];
    }
    cloneRange() {
      const clone = new Range();
      clone._start = [...this._start];
      clone._end = [...this._end];
      return clone;
    }
    detach() {} // Specified no-op; it must not disable a live range.
    compareBoundaryPoints(how, other) {
      how = Number(how) >>> 0;
      if (how > 3) fail("NotSupportedError", "Invalid comparison mode");
      if (!(other instanceof Range)) throw new TypeError("Expected a Range");
      if (
        this.startContainer.getRootNode() !== other.startContainer.getRootNode()
      ) {
        fail("WrongDocumentError", "Ranges have different roots");
      }
      return comparePoints(
        how === 1 || how === 2 ? this._end : this._start,
        how === 2 || how === 3 ? other._end : other._start,
      );
    }
    comparePoint(node, offset) {
      const point = boundary(node, offset);
      if (node.getRootNode() !== this.startContainer.getRootNode()) {
        fail("WrongDocumentError", "Different roots");
      }
      return comparePoints(point, this._start) < 0
        ? -1
        : comparePoints(point, this._end) > 0
        ? 1
        : 0;
    }
    isPointInRange(node, offset) {
      const point = boundary(node, offset);
      return node.getRootNode() === this.startContainer.getRootNode() &&
        comparePoints(point, this._start) >= 0 &&
        comparePoints(point, this._end) <= 0;
    }
    intersectsNode(node) {
      if (!(node instanceof Node)) throw new TypeError("Expected a Node");
      if (node.getRootNode() !== this.startContainer.getRootNode()) {
        return false;
      }
      if (!node.parentNode) return true;
      const index = nodeIndex(node);
      return comparePoints([node.parentNode, index], this._end) < 0 &&
        comparePoints([node.parentNode, index + 1], this._start) > 0;
    }
    insertNode(node) {
      if (!(node instanceof Node)) throw new TypeError("Expected a Node");
      if (node.nodeType === 9 || node.contains(this.startContainer)) {
        fail("HierarchyRequestError", "Invalid range insertion");
      }
      const collapsed = this.collapsed;
      let parent = this.startContainer,
        reference = parent.childNodes[this.startOffset] || null;
      if (parent.nodeType === 3) {
        if (!parent.parentNode) {
          fail(
            "HierarchyRequestError",
            "Detached text has no insertion parent",
          );
        }
        reference = parent.splitText(this.startOffset);
        parent = reference.parentNode;
      } else if (parent instanceof CharacterData) {
        fail("HierarchyRequestError", "Cannot insert into character data");
      }
      const count = node.nodeType === 11 ? node.childNodes.length : 1;
      if (node === reference) reference = reference.nextSibling;
      if (node.parentNode) node.parentNode.removeChild(node);
      const offset = reference
        ? nodeIndex(reference)
        : parent.childNodes.length;
      parent.insertBefore(node, reference);
      if (collapsed) this._end = [parent, offset + count];
    }
    createContextualFragment(html) {
      let context = this.startContainer;
      const owner = context.ownerDocument || context;
      if (context.nodeType !== 1) context = context.parentElement;
      if (
        !context ||
        (context.namespaceURI === HTML_NS && context.localName === "html")
      ) {
        context = owner.createElement("body");
      }
      return wrap(host("contextualFragment", context._id, String(html)), owner);
    }
    toString() {
      let result = "";
      const walk = (node) => {
        if (node.nodeType === 3) {
          if (
            comparePoints([node, node.length], this._start) <= 0 ||
            comparePoints([node, 0], this._end) >= 0
          ) return;
          result += node.data.slice(
            node === this.startContainer ? this.startOffset : 0,
            node === this.endContainer ? this.endOffset : node.length,
          );
        } else for (const child of node.childNodes) walk(child);
      };
      if (!this.collapsed) walk(this.commonAncestorContainer);
      return result;
    }
  }
  for (
    const [name, value] of Object.entries({
      START_TO_START: 0,
      START_TO_END: 1,
      END_TO_END: 2,
      END_TO_START: 3,
    })
  ) {
    Object.defineProperty(Range, name, { value });
    Object.defineProperty(Range.prototype, name, { value });
  }
  Object.defineProperty(Range.prototype, Symbol.toStringTag, {
    value: "Range",
  });
  class Document extends Node {
    constructor(id, info) {
      if (id === undefined) return wrap(host("createDocument"));
      super(id, info);
    }
    get documentElement() {
      return this.children[0] || null;
    }
    get head() {
      return this.querySelector("head");
    }
    get body() {
      return this.querySelector("body");
    }
    set body(value) {
      if (this.body) this.body.replaceWith(value);
      else this.documentElement.appendChild(value);
    }
    get scripts() {
      return this.getElementsByTagName("script");
    }
    get visibilityState() {
      return this === document ? this._visibility || "visible" : "hidden";
    }
    get hidden() {
      return this.visibilityState === "hidden";
    }
    get domain() {
      return new URL(this.URL).hostname;
    }
    set domain(value) {
      // Each Wega tab is origin-keyed; legacy domain relaxation is disabled.
      // Keep the origin used by cookies, storage, fetch and other tabs intact.
      host("documentDomain", String(value));
    }
    createRange() {
      const range = new Range();
      range._start = [this, 0];
      range._end = [this, 0];
      return range;
    }
    get defaultView() {
      return this === document ? globalThis : (this._defaultView || null);
    }
    get location() {
      return this === document ? globalThis.location : (this._location || null);
    }
    set location(value) {
      if (this === document) globalThis.location.href = String(value);
      else if (this._location) this._location.href = String(value);
    }
    get URL() {
      if (this === document) return host("url");
      return this._location ? this._location.href : "about:blank";
    }
    get documentURI() {
      return this.URL;
    }
    get baseURI() {
      return this.URL;
    }
    get contentType() {
      return "text/html";
    }
    get characterSet() { return "UTF-8"; }
    get charset() { return this.characterSet; }
    get inputEncoding() { return this.characterSet; }
    get readyState() { return this._readyState || "loading"; }
    set readyState(value) { this._readyState = value; }
    get images() { return this.getElementsByTagName("img"); }
    get embeds() { return this.getElementsByTagName("embed"); }
    get plugins() { return this.embeds; }
    get forms() { return this.getElementsByTagName("form"); }
    get links() { return nodeList(() => [...this.querySelectorAll("a[href],area[href]")], true); }
    getElementsByTagNameNS(namespace, localName) {
      return nodeList(() => [...this.querySelectorAll("*")].filter(node =>
        (namespace === "*" || node.namespaceURI === (namespace || null)) &&
        (localName === "*" || node.localName === localName)), true);
    }
    createAttribute(name) { return makeAttribute(this, null, String(name).toLowerCase(), ""); }
    createAttributeNS(namespace, name) { return makeAttribute(this, namespace || null, name, ""); }
    get compatMode() {
      return "CSS1Compat";
    }
    get title() {
      return (this.querySelector("title")?.textContent || "").replace(/[\t\n\f\r ]+/g, " ").replace(/^ | $/g, "");
    }
    set title(value) {
      let title = this.querySelector("title");
      if (!title) {
        title = this.createElement("title");
        (this.head || this.documentElement).appendChild(title);
      }
      title.textContent = value;
    }
    querySelector(selector) {
      return wrap(host("queryIn", this._id, String(selector)), this);
    }
    querySelectorAll(selector) {
      const nodes = host("queryAllIn", this._id, String(selector)).map((id) =>
        wrap(id, this),
      );
      return nodeList(() => nodes);
    }
    getElementById(id) {
      return this === document
        ? wrap(host("getById", String(id)), this)
        : [...this.querySelectorAll("[id]")].find((n) => n.id === String(id)) ||
            null;
    }
    getElementsByTagName(name) {
      return nodeList(() => [...this.querySelectorAll(String(name))], true);
    }
    getElementsByClassName(names) {
      return nodeList(
        () => [
          ...this.querySelectorAll(
            String(names)
              .trim()
              .split(/\s+/)
              .map((n) => "." + cssEscape(n))
              .join(""),
          ),
        ],
        true,
      );
    }
    getElementsByName(name) {
      name = String(name);
      return nodeList(
        () =>
          [...this.querySelectorAll("[name]")].filter(
            (element) =>
              element.namespaceURI === "http://www.w3.org/1999/xhtml" &&
              element.getAttribute("name") === name,
          ),
        false,
        null,
        true,
      );
    }
    createElement(name, options) {
      if ((typeof options === "string" && options) || options?.is) fail("NotSupportedError", "Customized built-in elements are not implemented");
      name = String(name).toLowerCase();
      if (!/^[a-z_][\w.:-]*$/i.test(name))
        fail("InvalidCharacterError", "Invalid element name");
      const element = wrap(host("create", name), this);
      if (name === "script") element._forceAsync = true;
      return element;
    }
    createElementNS(namespace, name, options) {
      if ((typeof options === "string" && options) || options?.is) fail("NotSupportedError", "Customized built-in elements are not implemented");
      return wrap(
        host("createNS", String(namespace || ""), String(name)),
        this,
      );
    }
    createTextNode(text) {
      return wrap(host("createText", String(text)), this);
    }
    createComment(text) {
      return wrap(host("createComment", String(text)), this);
    }
    createDocumentFragment() {
      return wrap(host("createFragment"), this);
    }
    createEvent(name) {
      const event = new Event("uninitialized");
      if (!event.initCustomEvent)
        event.initCustomEvent = function (type, bubbles, cancelable, detail) {
          this.initEvent(type, bubbles, cancelable);
          this.detail = detail;
        };
      return event;
    }
    get implementation() {
      if (!this._implementation) {
        this._implementation = new DOMImplementation();
        this._implementation._document = this;
      }
      return this._implementation;
    }
    importNode(node, deep = false) {
      const clone = node.cloneNode(deep);
      clone._owner = this;
      return clone;
    }
    adoptNode(node) {
      const oldOwner = node.ownerDocument;
      node.remove();
      component("customAdopt", node._id, oldOwner?._id || "", this._id);
      node._owner = this;
      flushComponents();
      return node;
    }
    get activeElement() {
      return wrap(host("active"), this) || this.body;
    }
    get currentScript() {
      return this === document ? currentScript : null;
    }
    write(...text) {
      if (this === document) host("write", text.join(""));
    }
    writeln(...text) {
      this.write(...text, "\n");
    }
    open() {
      return this;
    }
    close() {}
    hasFocus() {
      return true;
    }
  }
  function defineHandler(target, name) {
    if (web) {
      web.event.defineEventHandler(
        target,
        name,
        undefined,
        name === "error" && target === globalThis,
      );
      return;
    }
    Object.defineProperty(target, "on" + name, {
      configurable: true,
      enumerable: true,
      get() {
        return this._handlers?.get(name)?.handler || null;
      },
      set(handler) {
        if (!this._handlers) this._handlers = new Map();
        let entry = this._handlers.get(name);
        if (!entry) {
          entry = { handler: null };
          this._handlers.set(name, entry);
          this.addEventListener(name, (event) => {
            if (
              typeof entry.handler === "function" &&
              (name === "error" && this === globalThis
                ? entry.handler.call(
                    this,
                    event.message,
                    "",
                    0,
                    0,
                    event.error,
                  ) === true
                : entry.handler.call(this, event) === false)
            )
              event.preventDefault();
          });
        }
        entry.handler = typeof handler === "function" ? handler : null;
      },
    });
  }
  function ensureHandler(name) {
    if (!Object.getOwnPropertyDescriptor(Element.prototype, "on" + name))
      defineHandler(Element.prototype, name);
  }
  Object.defineProperty(globalThis, "__wega_node", { value: (id) => wrap(id) });
  defineHandler(Document.prototype, "visibilitychange");
  Object.defineProperty(Document.prototype, "cookie", {
    get() {
      return host("cookie");
    },
    set(value) {
      host("setCookie", String(value));
    },
  });
  for (const name of [
    "click",
    "dblclick",
    "beforeinput",
    "compositionstart",
    "compositionupdate",
    "compositionend",
    "input",
    "change",
    "submit",
    "reset",
    "focus",
    "blur",
    "keydown",
    "keyup",
    "pointerdown",
    "pointerup",
    "mousedown",
    "mouseup",
    "load",
    "error",
  ])
    ensureHandler(name);
  const mutationObservers = new Set();
  let scheduled = false;
  class MutationRecord {
    constructor(token, record) {
      if (token !== internalToken) throw new TypeError("Illegal constructor");
      recordData.set(this, record);
    }
  }
  for (const name of ["type", "target", "addedNodes", "removedNodes", "previousSibling", "nextSibling", "attributeName", "attributeNamespace", "oldValue"])
    Object.defineProperty(MutationRecord.prototype, name, { configurable: true, enumerable: true, get() {
      const state = recordData.get(this); if (!state) throw new TypeError("Illegal invocation"); return state[name];
    }});
  function notify(target, type, detail = {}) {
    if (!mutationObservers.size) return;
    for (const observer of mutationObservers) {
      const state = observerData.get(observer);
      let interested = false, oldValue = false;
      for (const [root, options] of [...state.targets, ...state.transient]) {
        if (root !== target && !(options.subtree && root.contains(target))) continue;
        if (type === "childList" && options.subtree)
          for (const node of detail.removedNodes || []) state.transient.set(node, options);
        if (!options[type] || (type === "attributes" && options.attributeFilter && !options.attributeFilter.includes(detail.attributeName))) continue;
        interested = true;
        oldValue ||= type === "attributes" ? !!options.attributeOldValue : !!options.characterDataOldValue;
      }
      if (!interested) continue;
      const added = [...(detail.addedNodes || [])], removed = [...(detail.removedNodes || [])];
      state.records.push(new MutationRecord(internalToken, {
        type, target, previousSibling: null, nextSibling: null, attributeName: null,
        attributeNamespace: null, ...detail, oldValue: oldValue ? detail.oldValue ?? null : null,
        addedNodes: nodeList(() => added), removedNodes: nodeList(() => removed),
      }));
    }
    if (scheduled) return;
    scheduled = true;
    (globalThis.queueMicrotask || (fn => Promise.resolve().then(fn)))(() => {
      scheduled = false;
      for (const observer of [...mutationObservers]) {
        const state = observerData.get(observer), records = observer.takeRecords();
        state.transient.clear();
        if (records.length) try { Reflect.apply(state.callback, observer, [records, observer]); } catch (error) { report(error); }
      }
    });
  }
  class MutationObserver {
    constructor(callback) {
      if (typeof callback !== "function") throw new TypeError("Callback required");
      observerData.set(this, { callback, targets: new Map(), transient: new Map(), records: [] });
    }
    observe(target, options) {
      const normalized = { ...options };
      if (options.attributes === undefined && (options.attributeOldValue !== undefined || options.attributeFilter !== undefined)) normalized.attributes = true;
      if (options.characterData === undefined && options.characterDataOldValue !== undefined) normalized.characterData = true;
      if ((!normalized.childList && !normalized.attributes && !normalized.characterData) ||
          (!normalized.attributes && (options.attributeOldValue || options.attributeFilter !== undefined)) ||
          (!normalized.characterData && options.characterDataOldValue)) throw new TypeError("Invalid mutation observer options");
      const state = observerData.get(this);
      state.targets.set(target, normalized); state.transient.clear(); mutationObservers.add(this);
    }
    disconnect() { const s = observerData.get(this); s.targets.clear(); s.transient.clear(); s.records = []; mutationObservers.delete(this); }
    takeRecords() { const s = observerData.get(this), records = s.records; s.records = []; return records; }
  }
  const cssEscape = (value) =>
    String(value).replace(/[^a-zA-Z0-9_-]/g, (c) => "\\" + c);
  const timers = new Map();
  let timerId = 0;
  function schedule(fn, delay, repeat, args) {
    if (timers.size >= 256) throw new Error("Timer limit exceeded");
    const id = ++timerId;
    timers.set(id, {
      fn,
      args,
      delay: Math.max(4, Number(delay) || 0),
      due: host("now") + Math.max(4, Number(delay) || 0),
      repeat,
    });
    return id;
  }
  document = wrap(host("root"));
  document.readyState = "loading";
  connected.set(document._id, document);
  const globalEvents = new BaseTarget();
  class Window extends BaseTarget {
    constructor() {
      throw new TypeError("Illegal constructor");
    }
  }
  Object.setPrototypeOf(globalEvents, Window.prototype);
  Object.defineProperty(Window.prototype, Symbol.toStringTag, { value: "Window" });
  // HTML named properties live behind Window's own properties and remain live
  // when an element is renamed, removed, or shadowed by a script-owned property.
  const namedWindow = (key) =>
    typeof key === "string" && key.length ? host("windowNamed", key) : [];
  Object.setPrototypeOf(
    globalThis,
    new Proxy(globalEvents, {
      has(target, key) {
        return Reflect.has(target, key) || namedWindow(key).length > 0;
      },
      get(target, key, receiver) {
        if (Reflect.has(target, key)) return Reflect.get(target, key, receiver);
        const ids = namedWindow(key);
        if (!ids.length) return undefined;
        return ids.length === 1
          ? wrap(ids[0])
          : nodeList(() => namedWindow(key).map((id) => wrap(id)), true);
      },
    }),
  );
  for (const method of [
    "addEventListener",
    "removeEventListener",
    "dispatchEvent",
  ])
    Object.defineProperty(globalThis, method, {
      value: BaseTarget.prototype[method].bind(globalThis),
      writable: true,
      configurable: true,
    });
  for (const name of [
    "load",
    "beforeunload",
    "unload",
    "resize",
    "scroll",
    "popstate",
    "hashchange",
    "DOMContentLoaded",
    "error",
    "message",
  ])
    defineHandler(globalThis, name);
  Object.assign(globalThis, {
    Node,
    CharacterData,
    Attr,
    NamedNodeMap,
    MutationRecord,
    Element,
    HTMLElement,
    SVGElement,
    HTMLIFrameElement,
    DOMTokenList,
    HTMLInputElement,
    HTMLTextAreaElement,
    HTMLSelectElement,
    HTMLOptionElement,
    HTMLButtonElement,
    HTMLFormElement,
    HTMLAnchorElement,
    HTMLImageElement,
    HTMLCanvasElement,
    HTMLScriptElement,
    HTMLTemplateElement,
    HTMLSlotElement,
    ShadowRoot,
    CustomElementRegistry,
    customElements,
    CanvasRenderingContext2D,
    Document,
    HTMLDocument: Document,
    DocumentFragment,
    Range,
    Text,
    Comment,
    NodeList,
    HTMLCollection,
    CSSStyleDeclaration,
    MutationObserver,
    Event,
    EventTarget: BaseTarget,
    Window,
    Screen,
    screen,
    document,
    window: globalThis,
    self: globalThis,
    top: globalThis,
    parent: globalThis,
    frames: globalThis,
    frameElement: null,
    postMessage(message, targetOrigin = "/", transfer = []) {
      if (targetOrigin !== null && typeof targetOrigin === "object") {
        transfer = targetOrigin.transfer || [];
        targetOrigin = targetOrigin.targetOrigin === undefined ? "/" : targetOrigin.targetOrigin;
      }
      const accepts = component("messageTarget", String(targetOrigin));
      // V8/Deno owns serialization and transfer validation. Serialization errors
      // propagate synchronously; original objects are never delivered as fallback.
      const data = web.structuredClone(message, { transfer });
      const origin = new URL(host("url")).origin;
      if (accepts) setTimeout(() => {
        const event = new web.MessageEvent("message", { data, origin, source: globalThis });
        globalThis.dispatchEvent(event);
      }, 0);
    },
    navigator: {
      userAgent:
        "Mozilla/5.0 (compatible; Wega/0.2; Blitz) AppleWebKit/537.36 (KHTML, like Gecko)",
      appName: "Netscape",
      appVersion: "5.0 (compatible; Wega/0.2; Blitz)",
      product: "Gecko",
      vendor: "",
      platform: host("platform"),
      language: "en-US",
      languages: ["en-US"],
      onLine: true,
      cookieEnabled: true,
      hardwareConcurrency: 2,
      maxTouchPoints: 0,
      webdriver: false,
      javaEnabled: () => false,
      sendBeacon: (url, data) => {
        try {
          fetch(url, { method: "POST", body: data, keepalive: true }).catch(
            () => {},
          );
          return true;
        } catch {
          return false;
        }
      },
    },
    Image: function Image(width, height) {
      const image = document.createElement("img");
      if (width !== undefined) image.setAttribute("width", String(width));
      if (height !== undefined) image.setAttribute("height", String(height));
      return image;
    },
    alert: (message = "") => {
      host("dialog", "alert", String(message), "");
    },
    confirm: (message = "") =>
      host("dialog", "confirm", String(message), "") === "true",
    prompt: (message = "", value = "") =>
      host("dialog", "prompt", String(message), String(value)),
    open: (url = "", _target = "_blank", _features = "") => {
      if (url === "" || url === "about:blank") return null;
      if (!host("open", String(url))) return null;
      // The new page runs in its own engine; this handle cannot reach it.
      return {
        closed: false,
        close() {},
        focus() {},
        blur() {},
        postMessage() {},
      };
    },
    getComputedStyle: (element) => new CSSStyleDeclaration(element, true),
    CSS: {
      escape: cssEscape,
      supports: (...args) => {
        if (!args.length)
          throw new TypeError("CSS.supports requires an argument");
        return args.length === 1
          ? host("supports", String(args[0]))
          : host("supportsProperty", String(args[0]), String(args[1]));
      },
    },
    console: {
      log: (...v) => host("log", v.map(String).join(" ")),
      warn: (...v) => host("log", v.map(String).join(" "), "2"),
      error: (...v) => host("log", v.map(String).join(" "), "3"),
    },
    setTimeout: (fn, delay, ...args) => schedule(fn, delay, false, args),
    setInterval: (fn, delay, ...args) => schedule(fn, delay, true, args),
    clearTimeout: (id) => timers.delete(id),
    clearInterval: (id) => timers.delete(id),
  });
  if (!globalThis.URL) {
    class URLFallback {
      constructor(value, base) {
        const result = host(
          "resolveURL",
          String(value),
          String(base || host("url")),
        );
        Object.assign(this, result);
      }
      toString() {
        return this.href;
      }
    }
    globalThis.URL = URLFallback;
  }
  class Location {
    constructor(token) { if (token !== internalToken) throw new TypeError("Illegal constructor"); locationData.add(this); }
    get href() { return host("url"); }
    set href(value) { __wega_location_navigate(value, false); }
    get origin() { return new URL(this.href).origin; }
    assign(value) { __wega_location_navigate(value, false); }
    replace(value) { __wega_location_navigate(value, true); }
    reload() { host("navigate", host("url")); }
    toString() { return this.href; }
  }
  for (const key of ["protocol", "host", "hostname", "port", "pathname", "search", "hash"])
    Object.defineProperty(Location.prototype, key, { configurable: true, enumerable: true,
      get() { return new URL(this.href)[key]; },
      set(value) { const url = new URL(this.href); url[key] = value; __wega_location_navigate(url.href, false); },
    });
  const location = new Location(internalToken);
  // HTML Location object creation: unforgeable own valueOf and @@toPrimitive.
  Object.defineProperty(location, "valueOf", { value: Object.prototype.valueOf, writable: false, enumerable: false, configurable: false });
  Object.defineProperty(location, Symbol.toPrimitive, { value: undefined, writable: false, enumerable: false, configurable: false });
  Object.defineProperty(globalThis, "location", { configurable: true, get: () => location, set: value => { location.href = value; } });
  Object.defineProperties(globalThis, {
    __wega_dispatch: {
      value: (type, id, event) => {
        const target = wrap(id);
        if (!target) return false;
        return !target.dispatchEvent(
          event || new Event(type, { bubbles: true, cancelable: true }),
        );
      },
    },
    __wega_visibility: {
      value: (hidden) => {
        const state = hidden ? "hidden" : "visible";
        if (document.visibilityState === state) return;
        document._visibility = state;
        document.dispatchEvent(
          new Event("visibilitychange", { bubbles: true }),
        );
      },
    },
    __wega_current_script: {
      value: (id) => {
        currentScript = id === null ? null : wrap(id);
      },
    },
    __wega_script_done: {
      value: (id, ok) => {
        wrap(id)?.dispatchEvent(new Event(ok ? "load" : "error"));
      },
    },
    __wega_resources_done: {
      value: (done) => {
        const status = new Map(done);
        for (const element of document.querySelectorAll(
          "img[src], link[href]",
        )) {
          const raw = element.getAttribute(
            element.localName === "img" ? "src" : "href",
          );
          let url;
          try {
            url = new URL(raw, document.URL).href;
          } catch {
            continue;
          }
          if (!status.has(url) || element._loaded === url) continue;
          element._loaded = url;
          element.dispatchEvent(new Event(status.get(url) ? "load" : "error"));
        }
      },
    },
    __wega_submit_event: {
      value: (formId, submitterId) => {
        const form = wrap(formId);
        if (!form) return true;
        const event = new Event("submit", { bubbles: true, cancelable: true });
        event.submitter = submitterId === null ? null : wrap(submitterId);
        return !form.dispatchEvent(event);
      },
    },
    __wega_timers: {
      value: () => {
        for (const [id, timer] of [...timers]) {
          if (timer.due > host("now")) continue;
          if (timer.repeat) timer.due = host("now") + timer.delay;
          else timers.delete(id);
          try {
            typeof timer.fn === "function"
              ? timer.fn(...timer.args)
              : (0, eval)(String(timer.fn));
          } catch (error) {
            report(error);
          }
        }
      },
    },
    __wega_next_timer: {
      value: () =>
        Math.min(
          Infinity,
          ...[...timers.values()].map((t) => t.due - host("now")),
        ),
    },
  });
  // HTML [Replaceable] window.event backed by the dispatcher's current event.
  Object.defineProperty(globalThis, "event", {
    configurable: true,
    enumerable: true,
    get: () => web.event.getWegaCurrentEvent(),
    set(value) { Object.defineProperty(globalThis, "event", { value, writable: true, configurable: true, enumerable: true }); },
  });
  web.event.setWegaDomHooks({
    parent: (node, event, originalRoot) => {
      if (!node?._id) return null;
      if (node.nodeType === 9) return node.defaultView;
      return wrap(component("eventParent", node._id, event ? !!event.composed : true, originalRoot?._id || ""));
    },
    host: node => node?._id ? wrap(component("shadowHost", node._id)) : null,
    mode: node => shadowDescriptors.get(node)?.mode || null,
    slot: node => node?._id ? wrap(component("assignedSlotInternal", node._id)) : null,
  });
  Object.defineProperty(globalThis, "length", { get: () => 0, configurable: true });
  const classes = { Node, Element, Document, DocumentFragment, CharacterData, Text, Comment, CDATASection, ProcessingInstruction, DocumentType,
    Attr, NamedNodeMap, NodeList, HTMLCollection, DOMTokenList, MutationObserver, MutationRecord,
    Location, Event, EventTarget: BaseTarget, HTMLElement, HTMLHeadElement: HTMLElement,
    HTMLScriptElement, ShadowRoot, Range, Window, DOMImplementation, AbortSignal: web.AbortSignal };
  const eventTypeGetter = Object.getOwnPropertyDescriptor(Event.prototype, "type").get;
  const targetAdd = BaseTarget.prototype.addEventListener;
  Object.defineProperty(globalThis, "__wega_semantics", { configurable: true, value: {
    classes,
    // [LegacyUnforgeable] interfaces: members become own instance properties.
    unforgeable: { Location: [location] },
    brand(name, value) {
      if (value === null || (typeof value !== "object" && typeof value !== "function")) return false;
      const Ctor = classes[name] || globalThis[name];
      if (!Ctor || !apply(isPrototypeOf, Ctor.prototype, [value])) return false;
      if (name === "Event") { try { eventTypeGetter.call(value); return true; } catch { return false; } }
      if (name === "EventTarget") { try { Reflect.apply(targetAdd, value, ["__wega_brand", null]); return true; } catch { return false; } }
      if (Ctor === Node || apply(isPrototypeOf, Node.prototype, [Ctor.prototype])) return nodeHandles.has(value);
      if (name === "NodeList" || name === "HTMLCollection") return collectionData.has(value);
      if (name === "NamedNodeMap") return namedMapData.has(value);
      if (name === "DOMTokenList") return tokenData.has(value);
      if (name === "MutationObserver") return observerData.has(value);
      if (name === "MutationRecord") return recordData.has(value);
      if (name === "Location") return locationData.has(value);
      return true;
    },
    ids(value) {
      const id = nodeHandles.get(value);
      if (id) return [id];
      const attr = attrData.get(value); if (attr?.owner) return [nodeHandles.get(attr.owner)];
      const tokens = tokenData.get(value); if (tokens) return [nodeHandles.get(tokens.element)];
      const map = namedMapData.get(value); if (map) return [nodeHandles.get(map.element)];
      return [];
    },
    construct(name, args, newTarget) {
      if (name === "Text") return document.createTextNode(args[0]);
      if (name === "Comment") return document.createComment(args[0]);
      return Reflect.construct(classes[name], args, newTarget);
    },
    wrap,
  }});
})();
