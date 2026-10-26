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
  Object.defineProperty(globalThis, "__wega_native_event", {
    value: (type, id, options) => {
      const target = __wega_node(id);
      const bubbles = !["focus", "blur"].includes(type);
      const cancelable = ["keydown", "keyup", "beforeinput"].includes(type);
      const event = type.startsWith("key")
        ? new KeyboardEvent(type, { ...options, bubbles, cancelable })
        : ["input", "beforeinput"].includes(type)
          ? new InputEvent(type, { ...options, bubbles, cancelable })
          : new Event(type, { bubbles, cancelable });
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
      const index = historyIndex + Number(delta);
      if (index < 0 || index >= historyEntries.length) return;
      historyIndex = index;
      host("history", historyEntries[index].url);
      const event = new Event("popstate");
      event.state = historyEntries[index].state;
      dispatchEvent(event);
    },
    back() {
      this.go(-1);
    },
    forward() {
      this.go(1);
    },
  };
  Object.defineProperties(globalThis, {
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
