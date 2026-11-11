// Worker global scopes: dedicated, shared and service. Runs once per worker
// realm after the Web API factories. Returns the host's private dispatcher;
// nothing here leaves a native handle reachable from worker script.
const host = __wega_call;
const web = __wega_web;
const { event, codec } = web;
const info = host("workerInfo");
const kind = info.kind;
const fail = (name, message) => { throw new DOMException(message, name); };
const rethrow = (error) => {
  const match = /^([A-Za-z]+Error):\s*(.*)/s.exec(error?.message || "");
  if (!match) throw error;
  const Native = { TypeError, RangeError, SyntaxError }[match[1]];
  throw Native ? new Native(match[2]) : new DOMException(match[2], match[1]);
};
const emit = (name, payload = null) => host("workerEmit", name, JSON.stringify(payload));

// ---- Global scope interfaces -------------------------------------------
class WorkerGlobalScope extends event.EventTarget {
  constructor() { fail("TypeError", "Illegal constructor"); }
}
class DedicatedWorkerGlobalScope extends WorkerGlobalScope {}
class SharedWorkerGlobalScope extends WorkerGlobalScope {}
class ServiceWorkerGlobalScope extends WorkerGlobalScope {}
const Scope = { dedicated: DedicatedWorkerGlobalScope, shared: SharedWorkerGlobalScope, service: ServiceWorkerGlobalScope }[kind];
Object.setPrototypeOf(globalThis, Scope.prototype);
// Deno's EventTarget methods accept only branded receivers.
Object.defineProperty(globalThis, web.brand, { value: web.brand });

const address = new URL(info.url);
const location = Object.freeze({
  href: address.href, origin: info.origin, protocol: address.protocol, host: address.host,
  hostname: address.hostname, port: address.port, pathname: address.pathname,
  search: address.search, hash: address.hash, toString() { return address.href; },
});
const base = info.base;
const navigator = Object.freeze({
  userAgent: "Mozilla/5.0 (compatible; Wega/0.2; Blitz) AppleWebKit/537.36 (KHTML, like Gecko)",
  appName: "Netscape", appVersion: "5.0 (compatible; Wega/0.2; Blitz)", product: "Gecko",
  platform: "", language: "en-US", languages: Object.freeze(["en-US", "en"]),
  hardwareConcurrency: 4,
  get onLine() { return info.online; },
});
const define = (name, value) => Object.defineProperty(globalThis, name, { value, writable: true, configurable: true, enumerable: false });
const readonly = (name, get) => Object.defineProperty(globalThis, name, { get, configurable: true, enumerable: true });
readonly("self", () => globalThis);
readonly("location", () => location);
readonly("navigator", () => navigator);
readonly("origin", () => info.origin);
readonly("crossOriginIsolated", () => false);
if (kind !== "service") readonly("name", () => info.name);
for (const [name, value] of Object.entries({ WorkerGlobalScope, DedicatedWorkerGlobalScope, SharedWorkerGlobalScope, ServiceWorkerGlobalScope }))
  if (name === "WorkerGlobalScope" || value === Scope) define(name, value);
// Nested workers and document-only entry points are not part of this scope.
for (const name of ["Worker", "SharedWorker", "ServiceWorkerRegistration", "serviceWorkerContainer", "localStorage", "sessionStorage"])
  delete globalThis[name];

// Event handler IDL attributes.
const handlers = new Map();
function handler(target, type) {
  Object.defineProperty(target, "on" + type, {
    configurable: true, enumerable: true,
    get() { return handlers.get(type)?.value || null; },
    set(value) {
      const previous = handlers.get(type);
      if (previous) target.removeEventListener(type, previous.wrapper);
      if (typeof value !== "function") { handlers.delete(type); return; }
      const wrapper = (ev) => value.call(target, ev);
      handlers.set(type, { value, wrapper });
      target.addEventListener(type, wrapper);
    },
  });
}
for (const type of ["error", "message", "messageerror", "unhandledrejection", "rejectionhandled", "online", "offline", "languagechange"])
  handler(globalThis, type);

// ---- Errors -------------------------------------------------------------
let reporting = false;
define("__wega_report", (error) => {
  const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
  let handled = false;
  if (!reporting) {
    reporting = true;
    try {
      const ev = new event.ErrorEvent("error", { message, filename: address.href, error, cancelable: true });
      handled = !globalThis.dispatchEvent(ev);
    } catch { /* a failing error handler must not recurse */ }
    reporting = false;
  }
  if (!handled) emit("error", { message, filename: address.href, stack: String(error?.stack || "") });
});
define("reportError", (error) => globalThis.__wega_report(error));

// ---- Script loading -----------------------------------------------------
const run = __wega_run_script;
define("importScripts", (...urls) => {
  for (const url of urls) {
    let script;
    try { script = host("importScript", String(url)); } catch (error) { rethrow(error); }
    run(script.source, script.url);
  }
});
define("close", () => host("workerClose"));

// ---- Canvas --------------------------------------------------------------
const pending = new Set();
function commit() {
  for (const canvas of pending) {
    if (canvas._placeholder && !canvas._detached)
      try { host("frameCommit", String(canvas._canvasId), String(canvas._placeholder)); } catch { /* resized away */ }
  }
  pending.clear();
}
const canvas = __wega_canvas({
  host, fail,
  isCanvasElement: () => false,
  isImageElement: () => false,
  drew(target) {
    if (!target?._placeholder) return;
    if (!pending.size) setTimeout(commit, 0);
    pending.add(target);
  },
});
for (const name of ["OffscreenCanvas", "OffscreenCanvasRenderingContext2D", "ImageData", "Path2D", "CanvasGradient", "CanvasPattern",
  "WebGLRenderingContext", "WebGL2RenderingContext", "WebGLBuffer", "WebGLShader", "WebGLProgram", "WebGLTexture",
  "WebGLFramebuffer", "WebGLRenderbuffer", "WebGLUniformLocation"])
  define(name, canvas[name]);
// A canvas transferred from a document keeps drawing into this realm's own
// surface; every committed frame is copied to its placeholder element.
codec.register({
  name: "canvas",
  describe(value) {
    if (!(value instanceof canvas.OffscreenCanvas)) return null;
    if (value._detached) fail("DataCloneError", "OffscreenCanvas is detached");
    if (value._context2d || value._webglCtx) fail("DataCloneError", "OffscreenCanvas with a context cannot be transferred");
    return { wire: { k: "canvas", w: value.width, h: value.height, p: value._placeholder || 0 }, detach() { value._detached = true; } };
  },
  revive(wire) {
    const result = new canvas.OffscreenCanvas(wire.w, wire.h);
    if (wire.p) result._placeholder = wire.p;
    return result;
  },
});
if (kind === "dedicated") {
  let frame = 0;
  const callbacks = new Map();
  define("requestAnimationFrame", (callback) => {
    if (typeof callback !== "function") fail("TypeError", "A callback is required");
    const id = ++frame;
    callbacks.set(id, setTimeout(() => { callbacks.delete(id); callback(performance.now()); }, 16));
    return id;
  });
  define("cancelAnimationFrame", (id) => { clearTimeout(callbacks.get(id)); callbacks.delete(id); });
}

// ---- Messaging ------------------------------------------------------------
let started = () => {};
if (kind === "dedicated") {
  const port = codec.port(info.port);
  port.addEventListener("message", (ev) => {
    globalThis.dispatchEvent(web.messageEvent("message", { data: ev.data, ports: [...ev.ports] }));
  });
  port.addEventListener("messageerror", () => globalThis.dispatchEvent(web.messageEvent("messageerror")));
  define("postMessage", (message, transfer) => port.postMessage(message, transfer));
  started = () => port.start();
}
if (kind === "shared") handler(globalThis, "connect");

// ---- Service worker scope ---------------------------------------------------
let service = null;
if (kind === "service") {
  const extendable = new WeakMap();
  class ExtendableEvent extends event.Event {
    constructor(type, init = {}) {
      super(type, init);
      extendable.set(this, { promises: [], active: true });
    }
    waitUntil(promise) {
      const state = extendable.get(this);
      if (!state) fail("TypeError", "Illegal invocation");
      if (!state.active) fail("InvalidStateError", "The event is no longer extendable");
      state.promises.push(Promise.resolve(promise));
    }
  }
  // Wait for every lifetime promise, including ones added while waiting.
  async function settle(ev) {
    const state = extendable.get(ev);
    let seen = 0, failed = false;
    while (seen < state.promises.length) {
      const batch = state.promises.slice(seen);
      seen = state.promises.length;
      for (const result of await Promise.allSettled(batch)) if (result.status === "rejected") failed = true;
    }
    state.active = false;
    return !failed;
  }
  const fetchState = new WeakMap();
  class FetchEvent extends ExtendableEvent {
    constructor(type, init = {}) {
      super(type, init);
      if (!(init.request instanceof Request)) fail("TypeError", "FetchEvent requires a request");
      fetchState.set(this, { request: init.request, clientId: String(init.clientId || ""), resultingClientId: String(init.resultingClientId || ""), response: null, dispatching: true });
    }
    get request() { return fetchState.get(this).request; }
    get clientId() { return fetchState.get(this).clientId; }
    get resultingClientId() { return fetchState.get(this).resultingClientId; }
    get preloadResponse() { return Promise.resolve(undefined); }
    get handled() { return Promise.resolve(undefined); }
    respondWith(response) {
      const state = fetchState.get(this);
      if (!state.dispatching) fail("InvalidStateError", "respondWith() must be called while the event is dispatched");
      if (state.response) fail("InvalidStateError", "respondWith() was already called");
      state.response = Promise.resolve(response);
      this.waitUntil(state.response.catch(() => {}));
      this.stopImmediatePropagation();
    }
  }
  class ExtendableMessageEvent extends ExtendableEvent {
    #init;
    constructor(type, init = {}) { super(type, init); this.#init = init; }
    get data() { return this.#init.data ?? null; }
    get origin() { return this.#init.origin ?? ""; }
    get lastEventId() { return ""; }
    get source() { return this.#init.source ?? null; }
    get ports() { return this.#init.ports ?? []; }
  }
  class Client {
    #id; #url;
    constructor(token, id, url) { if (token !== extendable) fail("TypeError", "Illegal constructor"); this.#id = id; this.#url = url; }
    get id() { return this.#id; }
    get url() { return this.#url; }
    get type() { return "window"; }
    get frameType() { return "top-level"; }
    postMessage(message, transfer) {
      emit("swClientMessage", { client: this.#id, data: codec.encode(message, transfer) });
    }
  }
  class WindowClient extends Client {
    get visibilityState() { return "visible"; }
    get focused() { return true; }
    focus() { return Promise.resolve(this); }
    navigate() { return Promise.reject(new TypeError("Client navigation is not supported")); }
  }
  const listClients = () => host("swClients").map((entry) => new WindowClient(extendable, entry.id, entry.url));
  class Clients {
    async get(id) { return listClients().find((client) => client.id === String(id)); }
    async matchAll(options = {}) {
      const all = listClients();
      return options?.includeUncontrolled ? all : all.filter((client) => host("swControls", client.url));
    }
    async claim() { emit("swClaim"); }
    async openWindow() { fail("InvalidAccessError", "openWindow requires a notification click"); }
  }
  const clients = new Clients();
  const registration = Object.freeze({
    get scope() { return info.scope; },
    get active() { return null; },
    get installing() { return null; },
    get waiting() { return null; },
    update() { emit("swUpdate"); return Promise.resolve(registration); },
    unregister() { emit("swUnregister"); return Promise.resolve(true); },
    navigationPreload: Object.freeze({
      enable: () => Promise.resolve(), disable: () => Promise.resolve(),
      setHeaderValue: () => Promise.resolve(), getState: () => Promise.resolve({ enabled: false, headerValue: "" }),
    }),
  });
  readonly("clients", () => clients);
  readonly("registration", () => registration);
  define("skipWaiting", () => { emit("swSkipWaiting"); return Promise.resolve(); });
  for (const [name, value] of Object.entries({ ExtendableEvent, FetchEvent, ExtendableMessageEvent, Client, WindowClient, Clients })) define(name, value);
  for (const type of ["install", "activate", "fetch", "notificationclick", "push", "sync"]) handler(globalThis, type);
  // Fetch handlers must be registered during initial evaluation.
  let fetchListeners = 0, evaluated = false;
  const add = globalThis.addEventListener;
  Object.defineProperty(Scope.prototype, "addEventListener", {
    configurable: true, writable: true,
    value(type, ...rest) {
      if (type === "fetch" && !evaluated) fetchListeners++;
      return add.call(this, type, ...rest);
    },
  });
  const decodeBody = web.base64Decode, encodeBody = web.base64Encode;
  service = {
    loaded() {
      evaluated = true;
      emit("swEvaluated", { fetch: fetchListeners > 0 });
    },
    async lifecycle(type) {
      const ev = new ExtendableEvent(type);
      globalThis.dispatchEvent(ev);
      const ok = await settle(ev);
      emit(type === "install" ? "swInstalled" : "swActivated", { ok });
    },
    async fetch(job) {
      let outcome = { id: job.id, kind: "fallback" };
      try {
        const init = { method: job.method, headers: job.headers, redirect: "manual" };
        if (job.body && !["GET", "HEAD"].includes(job.method)) init.body = decodeBody(job.body);
        const request = new Request(job.url, init);
        const ev = new FetchEvent("fetch", { request, clientId: job.client, cancelable: true });
        globalThis.dispatchEvent(ev);
        const state = fetchState.get(ev);
        state.dispatching = false;
        if (state.response) {
          const response = await state.response;
          if (!(response instanceof Response)) throw new TypeError("respondWith() needs a Response");
          if (response.type === "error") throw new TypeError("respondWith() received a network error");
          const bytes = new Uint8Array(await response.arrayBuffer());
          outcome = {
            id: job.id, kind: "response", status: response.status, statusText: response.statusText,
            headers: [...response.headers], url: response.url || job.url, redirected: response.redirected,
            body: encodeBody(bytes),
          };
        }
        emit("swFetch", outcome);
        await settle(ev);
        return;
      } catch (error) {
        outcome = { id: job.id, kind: "error", message: String(error?.message || error) };
      }
      emit("swFetch", outcome);
    },
    message(job) {
      const { data, ports } = codec.decode(job.data);
      const source = job.client ? new WindowClient(extendable, job.client, job.url) : null;
      const ev = new ExtendableMessageEvent("message", { data, ports, origin: info.origin, source });
      globalThis.dispatchEvent(ev);
      settle(ev);
    },
  };
}

// ---- Host dispatch ----------------------------------------------------------
const secret = info.secret;
Object.defineProperty(globalThis, "__wega_worker_dispatch", {
  value: (token, name, payload) => {
    if (token !== secret) fail("TypeError", "Illegal invocation");
    const value = JSON.parse(payload);
    try {
      if (name === "loaded") { started(); service?.loaded(); }
      else if (name === "connect") {
        const port = codec.port(value.port);
        globalThis.dispatchEvent(web.messageEvent("connect", { data: "", ports: [port], source: port }));
      }
      else if (name === "install" || name === "activate") service.lifecycle(name);
      else if (name === "fetch") service.fetch(value);
      else if (name === "message") service.message(value);
      else if (name === "online") { info.online = !!value; globalThis.dispatchEvent(new event.Event(value ? "online" : "offline")); }
    } catch (error) { globalThis.__wega_report(error); }
  },
});
void base;
return undefined;
