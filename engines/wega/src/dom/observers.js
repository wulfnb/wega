// Frame-clock APIs read layout and scrolling from Blitz, never a JS-side layout tree.
(() => {
  const host = __wega_call;
  class DOMRectReadOnly {
    constructor(x = 0, y = 0, width = 0, height = 0) {
      this.x = Number(x);
      this.y = Number(y);
      this.width = Number(width);
      this.height = Number(height);
    }
    get top() {
      return Math.min(this.y, this.y + this.height);
    }
    get bottom() {
      return Math.max(this.y, this.y + this.height);
    }
    get left() {
      return Math.min(this.x, this.x + this.width);
    }
    get right() {
      return Math.max(this.x, this.x + this.width);
    }
    toJSON() {
      return {
        x: this.x,
        y: this.y,
        width: this.width,
        height: this.height,
        top: this.top,
        right: this.right,
        bottom: this.bottom,
        left: this.left,
      };
    }
    static fromRect(rect = {}) {
      return new this(rect.x, rect.y, rect.width, rect.height);
    }
  }
  class DOMRect extends DOMRectReadOnly {}
  class KeyboardEvent extends Event {
    constructor(type, options = {}) {
      super(type, options);
      for (const key of [
        "key",
        "code",
        "location",
        "repeat",
        "isComposing",
        "ctrlKey",
        "shiftKey",
        "altKey",
        "metaKey",
      ])
        Object.defineProperty(this, key, {
          value:
            options[key] ??
            (["key", "code"].includes(key)
              ? ""
              : key === "location"
                ? 0
                : false),
        });
    }
  }
  class InputEvent extends Event {
    constructor(type, options = {}) {
      super(type, options);
      this.data = options.data ?? null;
      this.inputType = options.inputType || "";
      this.isComposing = !!options.isComposing;
    }
  }
  class CustomEventFallback extends Event {
    constructor(type, options = {}) {
      super(type, options);
      this.detail = options.detail ?? null;
    }
  }
  class UIEvent extends Event {
    constructor(type, options = {}) {
      super(type, options);
      Object.defineProperty(this, "detail", { value: Number(options.detail ?? 0) });
      Object.defineProperty(this, "view", { value: options.view ?? null });
    }
  }
  class MouseEvent extends UIEvent {
    constructor(type, options = {}) {
      super(type, options);
      for (const [key, fallback] of [
        ["screenX", 0],
        ["screenY", 0],
        ["clientX", 0],
        ["clientY", 0],
        ["button", 0],
        ["buttons", 0],
        ["ctrlKey", false],
        ["shiftKey", false],
        ["altKey", false],
        ["metaKey", false],
        ["movementX", 0],
        ["movementY", 0],
        ["relatedTarget", null],
      ])
        Object.defineProperty(this, key, { value: options[key] ?? fallback });
    }
    get pageX() {
      return this.clientX + scrollX;
    }
    get pageY() {
      return this.clientY + scrollY;
    }
    get x() {
      return this.clientX;
    }
    get y() {
      return this.clientY;
    }
    get offsetX() {
      const rect = this.target?.getBoundingClientRect?.();
      return rect ? this.clientX - rect.left : this.clientX;
    }
    get offsetY() {
      const rect = this.target?.getBoundingClientRect?.();
      return rect ? this.clientY - rect.top : this.clientY;
    }
    getModifierState(key) {
      return (
        { Shift: this.shiftKey, Control: this.ctrlKey, Alt: this.altKey, Meta: this.metaKey }[
          key
        ] ?? false
      );
    }
  }
  class PointerEvent extends MouseEvent {
    constructor(type, options = {}) {
      super(type, options);
      for (const [key, fallback] of [
        ["pointerId", 1],
        ["pointerType", "mouse"],
        ["isPrimary", true],
        ["width", 1],
        ["height", 1],
        ["pressure", 0],
        ["tiltX", 0],
        ["tiltY", 0],
      ])
        Object.defineProperty(this, key, { value: options[key] ?? fallback });
    }
    getCoalescedEvents() {
      return [this];
    }
  }
  class WheelEvent extends MouseEvent {
    constructor(type, options = {}) {
      super(type, options);
      for (const key of ["deltaX", "deltaY", "deltaZ"])
        Object.defineProperty(this, key, { value: Number(options[key] ?? 0) });
      Object.defineProperty(this, "deltaMode", { value: Number(options.deltaMode ?? 0) });
    }
  }
  for (const [key, value] of [
    ["DOM_DELTA_PIXEL", 0],
    ["DOM_DELTA_LINE", 1],
    ["DOM_DELTA_PAGE", 2],
  ])
    Object.defineProperty(WheelEvent, key, { value });
  class FocusEvent extends UIEvent {
    constructor(type, options = {}) {
      super(type, options);
      Object.defineProperty(this, "relatedTarget", { value: options.relatedTarget ?? null });
    }
  }
  class SubmitEvent extends Event {
    constructor(type, options = {}) {
      super(type, options);
      this.submitter = options.submitter ?? null;
    }
  }
  class HashChangeEvent extends Event {
    constructor(type, options = {}) {
      super(type, options);
      Object.defineProperty(this, "oldURL", { value: String(options.oldURL ?? "") });
      Object.defineProperty(this, "newURL", { value: String(options.newURL ?? "") });
    }
  }
  class PopStateEvent extends Event {
    constructor(type, options = {}) {
      super(type, options);
      Object.defineProperty(this, "state", { value: options.state ?? null });
    }
  }
  const mouseTypes = new Set([
    "click",
    "dblclick",
    "mousedown",
    "mouseup",
    "mousemove",
    "mouseover",
    "mouseout",
    "mouseenter",
    "mouseleave",
    "contextmenu",
  ]);
  const notBubbling = new Set([
    "focus",
    "blur",
    "mouseenter",
    "mouseleave",
    "pointerenter",
    "pointerleave",
  ]);
  const notCancelable = new Set([
    "focus",
    "blur",
    "focusin",
    "focusout",
    "input",
    "mouseenter",
    "mouseleave",
    "pointerenter",
    "pointerleave",
    "compositionstart",
    "compositionupdate",
    "compositionend",
    "change",
  ]);
  Object.defineProperty(globalThis, "__wega_native_event", {
    value: (type, id, options = {}) => {
      const target = __wega_node(id);
      if (!target) return false;
      const init = {
        ...options,
        bubbles: !notBubbling.has(type),
        cancelable: !notCancelable.has(type),
        composed: true,
        view: globalThis,
        relatedTarget:
          options.relatedTarget == null ? null : __wega_node(options.relatedTarget),
        detail: type === "dblclick" ? 2 : type === "click" ? 1 : 0,
      };
      const event = type.startsWith("key")
        ? new KeyboardEvent(type, init)
        : ["input", "beforeinput"].includes(type)
          ? new InputEvent(type, init)
          : type === "wheel"
            ? new WheelEvent(type, init)
            : type.startsWith("pointer")
              ? new PointerEvent(type, init)
              : mouseTypes.has(type)
                ? new MouseEvent(type, init)
                : ["focus", "blur", "focusin", "focusout"].includes(type)
                  ? new FocusEvent(type, init)
                  : new Event(type, init);
      if (type.startsWith("composition")) event.data = options.data || "";
      return !target.dispatchEvent(event);
    },
  });
  const raf = new Map(),
    resize = new Set(),
    intersection = new Set(),
    media = new Set();
  let nextFrame = 0;
  globalThis.requestAnimationFrame = (callback) => {
    if (typeof callback !== "function")
      throw new TypeError("Callback required");
    if (raf.size >= 256) throw Error("Animation callback limit exceeded");
    const id = ++nextFrame;
    raf.set(id, callback);
    return id;
  };
  globalThis.cancelAnimationFrame = (id) => raf.delete(id);
  class ResizeObserver {
    constructor(callback) {
      if (typeof callback !== "function")
        throw new TypeError("Callback required");
      this._callback = callback;
      this._targets = new Map();
    }
    observe(target, options = {}) {
      if (!(target instanceof Element)) throw new TypeError("Expected Element");
      if (
        options.box &&
        !["content-box", "border-box", "device-pixel-content-box"].includes(
          options.box,
        )
      )
        throw new TypeError("Invalid box");
      this._targets.set(target, {
        last: null,
        box: options.box || "content-box",
      });
      resize.add(this);
    }
    unobserve(target) {
      this._targets.delete(target);
      if (!this._targets.size) resize.delete(this);
    }
    disconnect() {
      this._targets.clear();
      resize.delete(this);
    }
  }
  class IntersectionObserver {
    constructor(callback, options = {}) {
      if (typeof callback !== "function")
        throw new TypeError("Callback required");
      this._callback = callback;
      this.root = options.root ?? null;
      this.rootMargin = options.rootMargin || "0px";
      const parts = this.rootMargin.trim().split(/\s+/);
      if (
        parts.length > 4 ||
        parts.some((p) => !/^[-+]?(?:\d*\.)?\d+(px|%)$/.test(p))
      )
        throw new SyntaxError("Invalid rootMargin");
      this._margin =
        parts.length === 1
          ? [parts[0], parts[0], parts[0], parts[0]]
          : parts.length === 2
            ? [parts[0], parts[1], parts[0], parts[1]]
            : parts.length === 3
              ? [parts[0], parts[1], parts[2], parts[1]]
              : parts;
      this.rootMargin = this._margin.join(" ");
      this.thresholds = [
        ...new Set(
          Array.isArray(options.threshold)
            ? options.threshold
            : [options.threshold ?? 0],
        ),
      ]
        .map(Number)
        .sort((a, b) => a - b);
      if (this.thresholds.some((v) => !Number.isFinite(v) || v < 0 || v > 1))
        throw new RangeError("Invalid threshold");
      if (!this.thresholds.length) this.thresholds = [0];
      this._targets = new Map();
      this._records = [];
    }
    observe(target) {
      if (!(target instanceof Element)) throw new TypeError("Expected Element");
      this._targets.set(target, null);
      intersection.add(this);
    }
    unobserve(target) {
      this._targets.delete(target);
      if (!this._targets.size) intersection.delete(this);
    }
    disconnect() {
      this._targets.clear();
      this._records = [];
      intersection.delete(this);
    }
    takeRecords() {
      const records = this._records;
      this._records = [];
      return records;
    }
  }
  globalThis.matchMedia = (query) => {
    const result = new EventTarget();
    result.media = String(query);
    Object.defineProperty(result, "matches", {
      get: () => host("media", result.media),
    });
    result._matches = result.matches;
    result.addListener = (fn) => result.addEventListener("change", fn);
    result.removeListener = (fn) => result.removeEventListener("change", fn);
    media.add(result);
    return result;
  };
  Object.defineProperties(globalThis, {
    innerWidth: { get: () => host("viewport").width },
    innerHeight: { get: () => host("viewport").height },
    devicePixelRatio: { get: () => host("viewport").dpr },
    scrollX: { get: () => host("viewport").scrollX },
    scrollY: { get: () => host("viewport").scrollY },
    pageXOffset: { get: () => scrollX },
    pageYOffset: { get: () => scrollY },
  });
  globalThis.scrollTo = (x, y) => {
    if (typeof x === "object") ({ left: x = scrollX, top: y = scrollY } = x);
    host("scrollViewport", String(x || 0), String(y || 0));
    dispatchEvent(new Event("scroll"));
  };
  globalThis.scrollBy = (x, y) =>
    scrollTo(scrollX + Number(x || 0), scrollY + Number(y || 0));
  Object.defineProperties(Element.prototype, {
    scrollLeft: {
      get() {
        return host("scroll", this._id).x;
      },
      set(value) {
        host("scrollNode", this._id, String(value), String(this.scrollTop));
      },
    },
    scrollTop: {
      get() {
        return host("scroll", this._id).y;
      },
      set(value) {
        host("scrollNode", this._id, String(this.scrollLeft), String(value));
      },
    },
  });
  Element.prototype.scrollTo = function (x, y) {
    if (typeof x === "object")
      ({ left: x = this.scrollLeft, top: y = this.scrollTop } = x);
    host("scrollNode", this._id, String(x || 0), String(y || 0));
  };
  Element.prototype.scrollBy = function (x, y) {
    this.scrollTo(
      this.scrollLeft + Number(x || 0),
      this.scrollTop + Number(y || 0),
    );
  };
  Element.prototype.scrollIntoView = function () {
    host("scrollIntoView", this._id);
  };
  Object.defineProperty(Node.prototype, "baseURI", {
    get() {
      return this.ownerDocument?.URL || document.URL;
    },
  });
  const originalRect = Element.prototype.getBoundingClientRect;
  Element.prototype.getBoundingClientRect = function () {
    return DOMRect.fromRect(originalRect.call(this));
  };
  Object.assign(globalThis, {
    DOMRectReadOnly,
    DOMRect,
    KeyboardEvent,
    InputEvent,
    UIEvent,
    MouseEvent,
    PointerEvent,
    WheelEvent,
    FocusEvent,
    SubmitEvent,
    HashChangeEvent,
    PopStateEvent,
    ResizeObserver,
    IntersectionObserver,
  });
  if (typeof CustomEvent !== "function")
    globalThis.CustomEvent = CustomEventFallback;
  if (typeof performance !== "object")
    globalThis.performance = {
      now: () => host("now"),
      timeOrigin: Date.now() - host("now"),
    };
  const historyEntries = [{ state: null, url: location.href }];
  let historyIndex = 0;
  // The browser enables Back/Forward from this document's own entries.
  const reportHistory = () =>
    host("historyPosition", String(historyIndex), String(historyEntries.length));
  globalThis.history = {
    get length() {
      return historyEntries.length;
    },
    get state() {
      return historyEntries[historyIndex].state;
    },
    pushState(state, _unused, url) {
      const next =
        url == null ? location.href : new URL(String(url), location.href).href;
      host("history", next);
      historyEntries.splice(historyIndex + 1);
      historyEntries.push({
        state:
          typeof structuredClone === "function"
            ? structuredClone(state)
            : JSON.parse(JSON.stringify(state)),
        url: next,
      });
      historyIndex++;
      reportHistory();
    },
    replaceState(state, _unused, url) {
      const next =
        url == null ? location.href : new URL(String(url), location.href).href;
      host("history", next);
      historyEntries[historyIndex] = {
        state:
          typeof structuredClone === "function"
            ? structuredClone(state)
            : JSON.parse(JSON.stringify(state)),
        url: next,
      };
    },
    go(delta = 0) {
      delta = Number(delta) || 0;
      if (!delta) {
        location.reload();
        return;
      }
      // Outside this document's entries the browser's own history continues.
      if (!traverse(delta)) host("traverseHost", String(delta));
    },
    back() {
      this.go(-1);
    },
    forward() {
      this.go(1);
    },
  };
  const withoutHash = (url) => url.split("#")[0];
  // Move within this document's session history: popstate, and hashchange
  // when only the fragment differs.
  function traverse(delta) {
    const index = historyIndex + delta;
    if (index < 0 || index >= historyEntries.length) return false;
    const old = location.href;
    historyIndex = index;
    reportHistory();
    const next = historyEntries[index].url;
    if (withoutHash(old) === withoutHash(next) && old !== next) host("fragment", next);
    else host("history", next);
    dispatchEvent(new PopStateEvent("popstate", { state: historyEntries[index].state }));
    if (withoutHash(old) === withoutHash(next) && old !== next)
      dispatchEvent(new HashChangeEvent("hashchange", { oldURL: old, newURL: next }));
    return true;
  }
  function navigateFragment(url, replace) {
    const old = location.href;
    host("fragment", url);
    if (replace) historyEntries[historyIndex] = { state: null, url };
    else {
      historyEntries.splice(historyIndex + 1);
      historyEntries.push({ state: null, url });
      historyIndex++;
    }
    reportHistory();
    if (old !== url)
      dispatchEvent(new HashChangeEvent("hashchange", { oldURL: old, newURL: url }));
  }
  Object.defineProperties(globalThis, {
    __wega_traverse: { value: (delta) => traverse(Number(delta)) },
    __wega_navigate_fragment: { value: (url) => navigateFragment(String(url), false) },
    __wega_location_navigate: {
      value: (value, replace) => {
        const url = new URL(value, location.href).href;
        if (url.includes("#") && withoutHash(url) === withoutHash(location.href))
          navigateFragment(url, replace);
        else host("navigate", url);
      },
    },
    __wega_has_frame: { value: () => raf.size > 0 },
    __wega_frame: {
      value: (timestamp) => {
        const callbacks = [...raf.values()];
        raf.clear();
        for (const callback of callbacks) {
          try {
            callback(timestamp);
          } catch (error) {
            __wega_report(error);
          }
        }
        for (const observer of resize) {
          const entries = [];
          for (const [target, info] of observer._targets) {
            const box = host("box", target._id);
            const width =
              info.box === "border-box" ? box.offsetWidth : box.contentWidth;
            const height =
              info.box === "border-box" ? box.offsetHeight : box.contentHeight;
            const key =
              width +
              ":" +
              height +
              (info.box === "device-pixel-content-box"
                ? ":" + devicePixelRatio
                : "");
            if (key === info.last) continue;
            info.last = key;
            entries.push({
              target,
              contentRect: new DOMRect(
                box.paddingLeft,
                box.paddingTop,
                box.contentWidth,
                box.contentHeight,
              ),
              contentBoxSize: [
                { inlineSize: box.contentWidth, blockSize: box.contentHeight },
              ],
              borderBoxSize: [
                { inlineSize: box.offsetWidth, blockSize: box.offsetHeight },
              ],
              devicePixelContentBoxSize: [
                {
                  inlineSize: Math.round(box.contentWidth * devicePixelRatio),
                  blockSize: Math.round(box.contentHeight * devicePixelRatio),
                },
              ],
            });
          }
          if (entries.length)
            try {
              observer._callback(entries, observer);
            } catch (error) {
              __wega_report(error);
            }
        }
        for (const observer of intersection) {
          let root =
            observer.root?.getBoundingClientRect() ||
            new DOMRect(0, 0, innerWidth, innerHeight);
          const margins = observer._margin.map(
            (value) =>
              parseFloat(value) * (value.endsWith("%") ? root.width / 100 : 1),
          );
          root = new DOMRect(
            root.x - margins[3],
            root.y - margins[0],
            root.width + margins[1] + margins[3],
            root.height + margins[0] + margins[2],
          );
          for (const [target, last] of observer._targets) {
            const rect = target.getBoundingClientRect();
            const left = Math.max(rect.left, root.left),
              top = Math.max(rect.top, root.top),
              right = Math.min(rect.right, root.right),
              bottom = Math.min(rect.bottom, root.bottom);
            const isIntersecting =
              target.isConnected && right >= left && bottom >= top;
            const intersectionRect = isIntersecting
              ? new DOMRect(left, top, right - left, bottom - top)
              : new DOMRect();
            const ratio =
              rect.width * rect.height
                ? (intersectionRect.width * intersectionRect.height) /
                  (rect.width * rect.height)
                : isIntersecting
                  ? 1
                  : 0;
            const key =
              isIntersecting +
              ":" +
              observer.thresholds.filter((v) => ratio >= v).length;
            if (key === last) continue;
            observer._targets.set(target, key);
            observer._records.push({
              target,
              time: timestamp,
              rootBounds: root,
              boundingClientRect: rect,
              intersectionRect,
              isIntersecting,
              intersectionRatio: ratio,
            });
          }
          const records = observer.takeRecords();
          if (records.length)
            try {
              observer._callback(records, observer);
            } catch (error) {
              __wega_report(error);
            }
        }
        for (const result of media) {
          const matches = result.matches;
          if (matches !== result._matches) {
            result._matches = matches;
            const event = new Event("change");
            event.matches = matches;
            event.media = result.media;
            if (typeof result.onchange === "function")
              try {
                result.onchange(event);
              } catch (error) {
                __wega_report(error);
              }
            result.dispatchEvent(event);
          }
        }
      },
    },
  });
})();
