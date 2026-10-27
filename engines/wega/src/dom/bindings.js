// Wega contributors, 2026. JS wrappers hold generational IDs into Blitz's single DOM.
(() => {
  const host = globalThis.__wega_call;
  const web = typeof __wega_web === "object" ? __wega_web : null;
  const missing = new Set(),
    cache = new Map(),
    connected = new Map();
  const weak =
    typeof WeakRef === "function" && typeof FinalizationRegistry === "function";
  let document;
  const fail = (name, message) => {
    const error = new Error(message);
    error.name = name;
    throw error;
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
  const rejections = new Map();
  Object.defineProperties(globalThis, {
    __wega_reject: {
      value: (promise, reason, handled) => {
        if (handled) rejections.delete(promise);
        else if (rejections.size < 256) rejections.set(promise, reason);
      },
    },
    __wega_rejections: {
      value: () => {
        for (const reason of rejections.values()) report(reason);
        rejections.clear();
      },
    },
  });
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
      return this._path || [];
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
      event.target = this;
      const path = [this];
      let parent = this.parentNode;
      while (parent) {
        path.push(parent);
        parent = parent.parentNode;
      }
      if (path.at(-1)?.nodeType === 9) path.push(globalThis);
      event._path = path;
      const invoke = (target, capture, phase) => {
        event.currentTarget = target;
        event.eventPhase = phase;
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
    const Ctor =
      info.nodeType === 9
        ? Document
        : info.nodeType === 11
          ? DocumentFragment
          : info.nodeType === 3
            ? Text
            : info.nodeType === 8
              ? Comment
              : info.namespaceURI === "http://www.w3.org/2000/svg"
                ? SVGElement
                : htmlConstructors[info.localName] || HTMLElement;
    result = new Proxy(new Ctor(id, info), {
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
    cache.set(id, weak ? new WeakRef(result) : result);
    host("retain", id);
    finalizer?.register(result, id);
    if (owner) result._owner = owner;
    if (host("connected", id)) connected.set(id, result);
    if (info.nodeType === 1) {
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
    return new Proxy(target, {
      get(target, key, receiver) {
        if (key === "length") return length();
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
  }
  class NodeList {
    get length() {
      throw new TypeError("Illegal invocation");
    }
    item(index) {
      return this[Number(index)] || null;
    }
  }
  class HTMLCollection {
    get length() {
      throw new TypeError("Illegal invocation");
    }
    item(index) {
      return this[Number(index)] || null;
    }
    namedItem(name) {
      return (
        [...this].find(
          (n) => n.id === name || n.getAttribute("name") === name,
        ) || null
      );
    }
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
  class Node extends BaseTarget {
    constructor(id, info) {
      super();
      // Deno allocates listener data lazily; a node must take part in event
      // paths (delegation) even before it has listeners of its own.
      web?.event.setEventTargetData(this);
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
      return this.getRootNode().nodeType === 9;
    }
    hasChildNodes() {
      return this.childNodes.length > 0;
    }
    getRootNode() {
      let root = this;
      while (root.parentNode) root = root.parentNode;
      return root;
    }
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
    insertBefore(child, reference) {
      if (!(child instanceof Node)) throw new TypeError("Expected a Node");
      if (reference !== null && reference?.parentNode !== this) {
        fail("NotFoundError", "Reference is not a child");
      }
      if (child.contains(this)) {
        fail("HierarchyRequestError", "Cannot create a DOM cycle");
      }
      if (child.nodeType === 11) {
        for (const node of [...child.childNodes]) {
          this.insertBefore(node, reference);
        }
        return child;
      }
      const oldParent = child.parentNode;
      if (child === reference) return child;
      if (oldParent) removedRangeNode(child);
      const index = reference
        ? nodeIndex(reference) -
          Number(oldParent === this && nodeIndex(child) < nodeIndex(reference))
        : this.childNodes.length - Number(oldParent === this);
      insertedRangeNode(this, index);
      host("insert", this._id, child._id, reference ? reference._id : "");
      child._owner = this.ownerDocument || this;
      retainTree(child, this.getRootNode() === document);
      if (oldParent) {
        notify(oldParent, "childList", {
          removedNodes: [child],
          addedNodes: [],
        });
      }
      notify(this, "childList", { removedNodes: [], addedNodes: [child] });
      if (this.getRootNode() === document) {
        prepareInserted(child);
        if (isScript(this)) prepareScript(this);
      }
      return child;
    }
    removeChild(child) {
      if (child?.parentNode !== this) {
        fail("NotFoundError", "Node is not a child");
      }
      removedRangeNode(child);
      retainTree(child, false);
      host("remove", child._id);
      notify(this, "childList", { removedNodes: [child], addedNodes: [] });
      return child;
    }
    replaceChild(child, old) {
      if (old?.parentNode !== this)
        fail("NotFoundError", "Node is not a child");
      if (child === old) return old;
      this.insertBefore(child, old);
      this.removeChild(old);
      return old;
    }
    replaceChildren(...values) {
      for (const child of [...this.childNodes]) this.removeChild(child);
      this.append(...values);
    }
    append(...values) {
      for (const value of values)
        this.appendChild(
          value instanceof Node
            ? value
            : (this.ownerDocument || this).createTextNode(String(value)),
        );
    }
    prepend(...values) {
      const anchor = this.firstChild;
      for (const value of values)
        this.insertBefore(
          value instanceof Node
            ? value
            : (this.ownerDocument || this).createTextNode(String(value)),
          anchor,
        );
    }
    before(...values) {
      if (!this.parentNode) return;
      for (const value of values)
        this.parentNode.insertBefore(
          value instanceof Node
            ? value
            : this.ownerDocument.createTextNode(String(value)),
          this,
        );
    }
    after(...values) {
      if (!this.parentNode) return;
      const anchor = this.nextSibling;
      for (const value of values)
        this.parentNode.insertBefore(
          value instanceof Node
            ? value
            : this.ownerDocument.createTextNode(String(value)),
          anchor,
        );
    }
    replaceWith(...values) {
      this.before(...values);
      this.remove();
    }
    remove() {
      this.parentNode?.removeChild(this);
    }
    cloneNode(deep = false) {
      return wrap(host("clone", this._id, String(!!deep)), this.ownerDocument);
    }
    normalize() {
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
    Object.defineProperty(Node, name, { value });
    Object.defineProperty(Node.prototype, name, { value });
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
    }
    removeAttribute(name) {
      const old = this.getAttribute(name);
      host("removeAttr", this._id, String(name));
      if (String(name).startsWith("on")) this[name] = null;
      if (old !== null)
        notify(this, "attributes", {
          attributeName: String(name),
          oldValue: old,
        });
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
    }
    removeAttributeNS(namespace, name) {
      const attr = [...this.attributes].find(
        (a) => a.namespaceURI === namespace && a.localName === name,
      );
      if (attr) this.removeAttribute(attr.name);
    }
    get attributes() {
      const element = this;
      const read = () =>
        host("attributes", element._id).map((attr) => ({
          nodeType: 2,
          nodeName: attr.name,
          name: attr.name,
          localName: attr.localName,
          namespaceURI: attr.namespaceURI,
          ownerElement: element,
          specified: true,
          get value() {
            return element.getAttributeNS(attr.namespaceURI, attr.localName);
          },
          set value(value) {
            element.setAttributeNS(attr.namespaceURI, attr.name, value);
          },
        }));
      return new Proxy(
        {},
        {
          get(_target, key) {
            const attrs = read();
            if (key === "length") return attrs.length;
            if (key === Symbol.iterator)
              return function* () {
                yield* read();
              };
            if (key === "item") return (i) => read()[i] || null;
            if (key === "getNamedItem")
              return (name) => read().find((a) => a.name === name) || null;
            if (typeof key === "string" && /^\d+$/.test(key))
              return attrs[Number(key)];
            return attrs.find((a) => a.name === key);
          },
        },
      );
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
    get classList() {
      const element = this;
      return {
        add(...names) {
          for (const name of names)
            if (!name || /\s/.test(name))
              fail("InvalidCharacterError", "Invalid class token");
          element.className = [
            ...new Set([
              ...element.className.split(/\s+/).filter(Boolean),
              ...names,
            ]),
          ].join(" ");
        },
        remove(...names) {
          element.className = element.className
            .split(/\s+/)
            .filter((n) => !names.includes(n))
            .join(" ");
        },
        contains(name) {
          return element.className.split(/\s+/).includes(String(name));
        },
        toggle(name, force) {
          const add = force === undefined ? !this.contains(name) : !!force;
          if (add) this.add(name);
          else this.remove(name);
          return add;
        },
        replace(old, value) {
          if (!this.contains(old)) return false;
          this.remove(old);
          this.add(value);
          return true;
        },
        get value() {
          return element.className;
        },
        get length() {
          return element.className.split(/\s+/).filter(Boolean).length;
        },
        [Symbol.iterator]: function* () {
          yield* element.className.split(/\s+/).filter(Boolean);
        },
      };
    }
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
      return host("rect", this._id);
    }
    getClientRects() {
      return host("box", this._id).hasBoxes
        ? [this.getBoundingClientRect()]
        : [];
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
      this.toggleAttribute("checked", !!value);
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
  }
  class HTMLElement extends Element {}
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

  class HTMLIFrameElement extends HTMLElement {}
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
    else if (node.firstElementChild)
      for (const script of node.querySelectorAll("script"))
        prepareScript(script);
  }
  class HTMLTemplateElement extends HTMLElement {
    get content() {
      return wrap(host("templateContent", this._id), this.ownerDocument);
    }
  }
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
    script: HTMLScriptElement,
    template: HTMLTemplateElement,
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
      return this === document ? globalThis : null;
    }
    get location() {
      return this === document ? globalThis.location : null;
    }
    set location(value) {
      if (this === document) globalThis.location.href = String(value);
    }
    get URL() {
      return host("url");
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
    get characterSet() {
      return "UTF-8";
    }
    get compatMode() {
      return "CSS1Compat";
    }
    get title() {
      return this.querySelector("title")?.textContent || "";
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
    createElement(name) {
      name = String(name).toLowerCase();
      if (!/^[a-z_][\w.:-]*$/i.test(name))
        fail("InvalidCharacterError", "Invalid element name");
      const element = wrap(host("create", name), this);
      if (name === "script") element._forceAsync = true;
      return element;
    }
    createElementNS(namespace, name) {
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
      return {
        createHTMLDocument: (title) => {
          const doc = wrap(host("createDocument"));
          const html = doc.createElement("html");
          doc.appendChild(html);
          html.append(doc.createElement("head"), doc.createElement("body"));
          doc.title = title || "";
          doc.readyState = "complete";
          return doc;
        },
        hasFeature: () => true,
      };
    }
    importNode(node, deep = false) {
      const clone = node.cloneNode(deep);
      clone._owner = this;
      return clone;
    }
    adoptNode(node) {
      node.remove();
      node._owner = this;
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
  let mutationObservers = new Set(),
    scheduled = false;
  function notify(target, type, detail = {}) {
    if (!mutationObservers.size) return;
    for (const observer of mutationObservers)
      for (const [root, options] of observer._targets) {
        if (root !== target && !(options.subtree && root.contains(target)))
          continue;
        if (!options[type]) continue;
        if (
          type === "attributes" &&
          options.attributeFilter &&
          !options.attributeFilter.includes(detail.attributeName)
        )
          continue;
        observer._records.push({
          type,
          target,
          addedNodes: [],
          removedNodes: [],
          previousSibling: null,
          nextSibling: null,
          attributeName: null,
          attributeNamespace: null,
          ...detail,
          oldValue: (
            type === "attributes"
              ? options.attributeOldValue
              : options.characterDataOldValue
          )
            ? (detail.oldValue ?? null)
            : null,
        });
        break;
      }
    if (!scheduled) {
      scheduled = true;
      (globalThis.queueMicrotask || ((fn) => Promise.resolve().then(fn)))(
        () => {
          scheduled = false;
          for (const observer of mutationObservers) {
            const records = observer.takeRecords();
            if (records.length)
              try {
                observer._callback(records, observer);
              } catch (error) {
                report(error);
              }
          }
        },
      );
    }
  }
  class MutationObserver {
    constructor(callback) {
      if (typeof callback !== "function")
        throw new TypeError("Callback required");
      this._callback = callback;
      this._targets = new Map();
      this._records = [];
    }
    observe(target, options) {
      if (
        !options.childList &&
        !options.attributes &&
        !options.characterData &&
        !options.attributeOldValue &&
        !options.characterDataOldValue &&
        !options.attributeFilter
      )
        throw new TypeError("Observation type required");
      this._targets.set(target, {
        ...options,
        attributes:
          options.attributes ??
          !!(options.attributeOldValue || options.attributeFilter),
        characterData: options.characterData ?? !!options.characterDataOldValue,
      });
      mutationObservers.add(this);
    }
    disconnect() {
      this._targets.clear();
      this._records = [];
      mutationObservers.delete(this);
    }
    takeRecords() {
      const records = this._records;
      this._records = [];
      return records;
    }
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
  ])
    defineHandler(globalThis, name);
  Object.assign(globalThis, {
    Node,
    Element,
    HTMLElement,
    SVGElement,
    HTMLIFrameElement,
    HTMLInputElement,
    HTMLTextAreaElement,
    HTMLSelectElement,
    HTMLOptionElement,
    HTMLButtonElement,
    HTMLFormElement,
    HTMLAnchorElement,
    HTMLImageElement,
    HTMLScriptElement,
    HTMLTemplateElement,
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
    document,
    window: globalThis,
    self: globalThis,
    top: globalThis,
    parent: globalThis,
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
      warn: (...v) => host("log", v.map(String).join(" ")),
      error: (...v) => host("log", v.map(String).join(" ")),
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
  const location = {
    get href() {
      return host("url");
    },
    set href(value) {
      __wega_location_navigate(String(value), false);
    },
    assign(value) {
      __wega_location_navigate(String(value), false);
    },
    replace(value) {
      __wega_location_navigate(String(value), true);
    },
    reload() {
      host("navigate", host("url"));
    },
    toString() {
      return this.href;
    },
  };
  for (const key of [
    "origin",
    "protocol",
    "host",
    "hostname",
    "port",
    "pathname",
    "search",
    "hash",
  ])
    Object.defineProperty(location, key, {
      get() {
        return new URL(this.href)[key];
      },
      set(value) {
        if (key === "origin") return;
        const url = new URL(this.href);
        url[key] = String(value);
        __wega_location_navigate(url.href, false);
      },
    });
  Object.defineProperty(globalThis, "location", { value: location });
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
})();
