// The Web API implementations below are Deno's pinned, unmodified extensions.
((core) => {
  const load = (name) => core.loadExtScript(name);
  const domException = load("ext:deno_web/01_dom_exception.js");
  const event = load("ext:deno_web/02_event.js");
  const abort = load("ext:deno_web/03_abort_signal.js");
  const url = load("ext:deno_web/00_url.js");
  const encoding = load("ext:deno_web/08_text_encoding.js");
  const streams = load("ext:deno_web/06_streams.js");
  const file = load("ext:deno_web/09_file.js");
  const base64 = load("ext:deno_web/05_base64.js");
  const perf = load("ext:deno_web/15_performance.js");
  const messages = load("ext:deno_web/13_message_port.js");
  const timers = load("ext:deno_web/02_timers.js");
  const headers = load("ext:deno_fetch/20_headers.js");
  const request = load("ext:deno_fetch/23_request.js");
  const response = load("ext:deno_fetch/23_response.js");
  const form = load("ext:deno_fetch/21_formdata.js");
  // Wega does not collect or export runtime telemetry. Deno fetch checks this
  // disabled state before touching tracer/context APIs. Its sources stay unchanged.
  const internals = __bootstrap.internals;
  internals.__telemetry = {
    TRACING_ENABLED: false,
    PROPAGATORS: [],
    DID_NOT_ENTER: undefined,
    exitSpan(snapshot) {
      if (snapshot !== undefined) throw Error("Telemetry is disabled");
    },
  };
  internals.__telemetryUtil = {};
  const fetch = load("ext:deno_fetch/26_fetch.js");
  const storage = load("ext:deno_webstorage/01_webstorage.js");
  const crypto = load("ext:deno_crypto/00_crypto.js");
  const location = load("ext:deno_web/12_location.js");
  const console = load("ext:deno_web/01_console.js");
  // Consume each runtime exception before Deno's default first-error abort can
  // discard other callbacks in the same turn. Browser error handlers remain live.
  core.setReportExceptionCallback((error) => globalThis.__wega_report(error));
  core.setUnhandledPromiseRejectionHandler((promise, reason) => {
    core.ops.op_dom_read(
      "rejection", String(reason) + "\n" + String(reason?.stack || ""), "", "",
    );
    globalThis.dispatchEvent(new event.PromiseRejectionEvent("unhandledrejection", {
      promise, reason, cancelable: true,
    }));
    return true;
  });
  core.setHandledPromiseRejectionHandler((promise, reason) => {
    globalThis.dispatchEvent(new event.PromiseRejectionEvent("rejectionhandled", {
      promise, reason,
    }));
  });
  location.setLocationHref(core.ops.op_dom_read("url", "", "", ""));
  const websocket = core.createLazyLoader(
    "ext:deno_websocket/01_websocket.js",
  )();
  const globals = {
    DOMException: domException.DOMException,
    Event: event.Event,
    EventTarget: event.EventTarget,
    ErrorEvent: event.ErrorEvent,
    PromiseRejectionEvent: event.PromiseRejectionEvent,
    CustomEvent: event.CustomEvent,
    MessageEvent: event.MessageEvent,
    ProgressEvent: event.ProgressEvent,
    AbortController: abort.AbortController,
    AbortSignal: abort.AbortSignal,
    URL: url.URL,
    URLSearchParams: url.URLSearchParams,
    TextEncoder: encoding.TextEncoder,
    TextDecoder: encoding.TextDecoder,
    TextEncoderStream: encoding.TextEncoderStream,
    TextDecoderStream: encoding.TextDecoderStream,
    Blob: file.Blob,
    File: file.File,
    ReadableStream: streams.ReadableStream,
    WritableStream: streams.WritableStream,
    TransformStream: streams.TransformStream,
    Headers: headers.Headers,
    Request: request.Request,
    Response: response.Response,
    FormData: form.FormData,
    WebSocket: websocket.WebSocket,
    CryptoKey: crypto.CryptoKey,
    crypto: crypto.crypto,
    performance: perf.performance,
    structuredClone: messages.structuredClone,
    atob: base64.atob,
    btoa: base64.btoa,
  };
  const safe =
    (fn) =>
    (...args) => {
      try {
        return typeof fn === "function"
          ? Reflect.apply(fn, globalThis, args)
          : (0, eval)(String(fn));
      } catch (error) {
        __wega_report(error);
      }
    };
  const limit = core.ops.op_dom_read("limits", "", "", "").resourceBytes;
  let inFlight = 0;
  globals.fetch = async (input, init) => {
    if (inFlight >= 32) throw new TypeError("Network queue is full");
    inFlight++;
    try {
      if (typeof input === "string" || input instanceof url.URL)
        input = new url.URL(String(input), globalThis.location.href).href;
      const result = await fetch.fetch(input, init);
      if (Number(result.headers.get("content-length")) > limit) {
        await result.body?.cancel();
        throw new TypeError("Resource exceeds the size limit");
      }
      if (!result.body) return result;
      const reader = result.body.getReader();
      let used = 0;
      const body = new streams.ReadableStream({
        async pull(controller) {
          try {
            const part = await reader.read();
            if (part.done) {
              controller.close();
              return;
            }
            used += part.value.byteLength;
            if (used > limit) {
              await reader.cancel();
              throw new TypeError("Resource exceeds the size limit");
            }
            controller.enqueue(part.value);
          } catch (error) {
            controller.error(error);
          }
        },
        cancel(reason) {
          return reader.cancel(reason);
        },
      });
      const bounded = new response.Response(body, {
        status: result.status,
        statusText: result.statusText,
        headers: result.headers,
      });
      const inner = response.toInnerResponse(result);
      inner.body = response.toInnerResponse(bounded).body;
      return response.fromInnerResponse(inner, "immutable");
    } finally {
      inFlight--;
    }
  };
  // Deno timers run through its event loop, with P0 error reporting around each callback.
  const activeTimers = new Map();
  const schedule = (fn, delay, repeat, args) => {
    if (activeTimers.size >= 256) throw Error("Timer limit exceeded");
    let id;
    const callback = (...values) => {
      if (!repeat) activeTimers.delete(id);
      return safe(fn)(...values);
    };
    id = (repeat ? timers.setInterval : timers.setTimeout).call(
      globalThis,
      callback,
      delay,
      ...args,
    );
    activeTimers.set(id, true);
    return id;
  };
  globals.setTimeout = (fn, delay, ...args) => schedule(fn, delay, false, args);
  globals.setInterval = (fn, delay, ...args) => schedule(fn, delay, true, args);
  globals.clearTimeout = (id) => {
    activeTimers.delete(id);
    timers.clearTimeout.call(globalThis, id);
  };
  globals.clearInterval = (id) => {
    activeTimers.delete(id);
    timers.clearInterval.call(globalThis, id);
  };
  const infra = load("ext:deno_web/00_infra.js");
  const base64Encode = (bytes) => infra.forgivingBase64Encode(bytes);
  const base64Decode = (text) => infra.forgivingBase64Decode(text);
  const op = (...args) => {
    try { return core.ops.op_dom_read(...args); }
    catch (error) {
      const match = /^([A-Za-z]+Error):\s*(.*)/s.exec(error?.message || "");
      if (!match) throw error;
      const Native = { TypeError, RangeError, SyntaxError }[match[1]];
      throw Native ? new Native(match[2]) : new domException.DOMException(match[2], match[1]);
    }
  };
  const here = () => globalThis.location?.href || core.ops.op_dom_read("url", "", "", "");

  // Structured clone across realms. Values travel as V8 serializer bytes;
  // transferred platform objects are replaced by indexed markers and revived
  // by the receiving realm.
  const kinds = [];
  const marker = "__wegaTransfer";
  const plain = (value) => {
    const proto = Object.getPrototypeOf(value);
    return proto === Object.prototype || proto === null;
  };
  function swap(value, index, seen) {
    if (value === null || typeof value !== "object") return value;
    if (index.has(value)) return { [marker]: index.get(value) };
    if (seen.has(value)) return seen.get(value);
    if (Array.isArray(value)) {
      const copy = [];
      seen.set(value, copy);
      for (const item of value) copy.push(swap(item, index, seen));
      return copy;
    }
    if (!plain(value)) return value;
    const copy = {};
    seen.set(value, copy);
    for (const key of Object.keys(value)) copy[key] = swap(value[key], index, seen);
    return copy;
  }
  function revive(value, revived, seen) {
    if (value === null || typeof value !== "object" || seen.has(value)) return value;
    seen.add(value);
    if (Array.isArray(value) || plain(value)) {
      for (const key of Object.keys(value)) {
        const item = value[key];
        if (item && typeof item === "object" && plain(item) && Object.keys(item).length === 1 &&
            Number.isInteger(item[marker]) && item[marker] >= 0 && item[marker] < revived.length)
          value[key] = revived[item[marker]];
        else revive(item, revived, seen);
      }
    }
    return value;
  }
  const codec = {
    register(kind) { kinds.push(kind); },
    encode(value, transfer) {
      if (transfer && !Array.isArray(transfer) && typeof transfer === "object" && !(Symbol.iterator in transfer))
        transfer = transfer.transfer;
      const list = transfer ? [...transfer] : [];
      const index = new Map(), descriptors = [], buffers = [];
      for (const item of list) {
        if (index.has(item) || buffers.includes(item))
          throw new domException.DOMException("Duplicate transferable", "DataCloneError");
        if (item instanceof ArrayBuffer) { buffers.push(item); continue; }
        let descriptor = null;
        for (const kind of kinds) if ((descriptor = kind.describe(item))) break;
        if (!descriptor) throw new domException.DOMException("Value is not transferable", "DataCloneError");
        index.set(item, descriptors.length);
        descriptors.push(descriptor);
      }
      let bytes;
      try {
        const prepared = index.size ? swap(value, index, new Map()) : value;
        bytes = core.serialize(prepared, undefined, (message) => {
          throw new domException.DOMException(message, "DataCloneError");
        });
      } catch (error) {
        if (error instanceof domException.DOMException) throw error;
        throw new domException.DOMException(String(error?.message || error), "DataCloneError");
      }
      for (const descriptor of descriptors) descriptor.detach?.();
      // The serializer copied each buffer; detaching completes the transfer.
      for (const buffer of buffers) { try { buffer.transfer(); } catch { /* already detached */ } }
      return JSON.stringify({ v: base64Encode(bytes), t: descriptors.map((descriptor) => descriptor.wire) });
    },
    decode(text) {
      const envelope = typeof text === "string" ? JSON.parse(text) : text;
      if (!envelope || typeof envelope.v !== "string" || !Array.isArray(envelope.t))
        return { data: envelope, ports: [] };
      const revived = envelope.t.map((wire) => {
        const kind = kinds.find((candidate) => candidate.name === wire.k);
        return kind ? kind.revive(wire) : null;
      });
      const value = core.deserialize(base64Decode(envelope.v));
      let data = value;
      if (revived.length) {
        const root = { value };
        revive(root, revived, new Set());
        data = root.value;
      }
      return { data, ports: revived.filter((item) => item instanceof MessagePort) };
    },
    port: (id) => new MessagePort(portToken, id),
  };

  // Deno's MessageEvent only accepts its own port class in `ports`; attach
  // this engine's ports (and any source) to the event object directly.
  const messageEvent = (type, init = {}) => {
    const { ports = [], source = null, ...rest } = init;
    const ev = new event.MessageEvent(type, rest);
    Object.defineProperties(ev, {
      ports: { value: Object.freeze([...ports]), enumerable: true, configurable: true },
      source: { value: source, enumerable: true, configurable: true },
    });
    return ev;
  };
  const portToken = Symbol("port");
  const portIds = new WeakMap();
  let detachPort;
  class MessagePort extends event.EventTarget {
    #id;
    #closed = false;
    #onmessage = null;
    #onmessageerror = null;
    #timer = null;

    constructor(token, id) {
      if (token !== portToken) throw new TypeError("Illegal constructor");
      super();
      this.#id = id;
      portIds.set(this, id);
      event.setEventTargetData(this);
    }

    get onmessage() { return this.#onmessage; }
    set onmessage(fn) {
      this.#onmessage = typeof fn === "function" ? fn : null;
      this.start();
    }
    get onmessageerror() { return this.#onmessageerror; }
    set onmessageerror(fn) { this.#onmessageerror = typeof fn === "function" ? fn : null; }

    start() {
      if (this.#closed || this.#timer) return;
      this.#timer = timers.setInterval.call(globalThis, () => this.poll(), 5);
    }

    // Deliver every queued message now.
    poll() {
      if (this.#closed) return;
      let messages;
      try { messages = core.ops.op_dom_read("portPoll", String(this.#id), "", ""); } catch { return; }
      if (!Array.isArray(messages)) return;
      for (const message of messages) {
        let ev;
        try {
          const { data, ports } = codec.decode(message.data);
          ev = messageEvent("message", { data, ports });
        } catch {
          ev = messageEvent("messageerror");
        }
        const handler = ev.type === "message" ? this.#onmessage : this.#onmessageerror;
        try {
          if (handler) handler.call(this, ev);
        } catch (error) { globalThis.__wega_report?.(error); }
        this.dispatchEvent(ev);
      }
    }

    close() {
      if (this.#closed) return;
      this.#closed = true;
      if (this.#timer) {
        timers.clearInterval.call(globalThis, this.#timer);
        this.#timer = null;
      }
      try {
        core.ops.op_dom_read("portClose", String(this.#id), "", "");
      } catch (_) {}
    }

    static {
      // Transfer hands the port to another realm: this object goes inert
      // while the underlying channel end stays open for its new owner.
      detachPort = (port) => {
        portIds.delete(port);
        port.#closed = true;
        if (port.#timer) {
          timers.clearInterval.call(globalThis, port.#timer);
          port.#timer = null;
        }
      };
    }

    postMessage(message, transfer) {
      if (transfer && typeof transfer === "object" && !Array.isArray(transfer) && !(Symbol.iterator in transfer))
        transfer = transfer.transfer;
      if (transfer && [...transfer].includes(this))
        throw new domException.DOMException("A port cannot be transferred through itself", "DataCloneError");
      const data = codec.encode(message, transfer);
      if (this.#closed) return;
      core.ops.op_dom_read("portPost", String(this.#id), data, "[]");
    }
  }
  codec.register({
    name: "port",
    describe(value) {
      if (!(value instanceof MessagePort) || !portIds.has(value)) return null;
      const id = portIds.get(value);
      // The sending realm gives the port up; its entangled peer stays live.
      return { wire: { k: "port", id }, detach() { detachPort(value); } };
    },
    revive: (wire) => new MessagePort(portToken, wire.id),
  });

  class MessageChannel {
    constructor() {
      const ids = op("channelCreate", "", "", "");
      this.port1 = new MessagePort(portToken, ids.port1);
      this.port2 = new MessagePort(portToken, ids.port2);
    }
  }

  // A worker script named by a blob: URL is read in the creating realm; the
  // worker thread cannot reach this realm's blob store.
  async function scriptSource(address, inline) {
    if (inline) return String(inline);
    if (!address.startsWith("blob:")) return "";
    const blob = file.blobFromObjectUrl(address);
    if (!blob) throw new domException.DOMException("The worker script blob is not available", "NetworkError");
    return await blob.text();
  }
  function workerConfig(scriptURL, options, extra) {
    const address = new url.URL(String(scriptURL), here()).href;
    return {
      address,
      config: {
        url: address,
        name: String(options?.name ?? ""),
        module: options?.type === "module",
        ...extra,
      },
    };
  }

  class Worker extends event.EventTarget {
    #id = null;
    #port;
    #terminated = false;
    #onmessage = null;
    #onerror = null;
    #onmessageerror = null;
    #timer = null;

    constructor(scriptURL, options = {}) {
      super();
      event.setEventTargetData(this);
      if (arguments.length < 1) throw new TypeError("A worker script URL is required");
      const ids = op("channelCreate", "", "", "");
      const { address, config } = workerConfig(scriptURL, options, { port: ids.port2 });
      this.#port = new MessagePort(portToken, ids.port1);
      this.#port.addEventListener("message", (ev) => {
        this.#deliver(messageEvent("message", { data: ev.data, ports: [...ev.ports] }), this.#onmessage);
      });
      this.#port.addEventListener("messageerror", () => {
        this.#deliver(messageEvent("messageerror"), this.#onmessageerror);
      });
      this.#port.start();
      const start = (source) => {
        if (this.#terminated) return;
        this.#id = op("workerCreate", JSON.stringify({ ...config, source }), "", "");
        this.#timer = timers.setInterval.call(globalThis, () => this.poll(), 5);
      };
      if (address.startsWith("blob:") || options?.content) {
        // Validate synchronously, then start once the blob has been read.
        op("workerCheck", address, "", "");
        scriptSource(address, options?.content).then(start).catch((error) => {
          this.#deliver(new event.ErrorEvent("error", { message: String(error?.message || error) }), this.#onerror);
        });
      } else start("");
    }

    #deliver(ev, handler) {
      try {
        if (typeof handler === "function") handler.call(this, ev);
      } catch (error) { globalThis.__wega_report?.(error); }
      this.dispatchEvent(ev);
    }

    get onmessage() { return this.#onmessage; }
    set onmessage(fn) { this.#onmessage = fn; }
    get onerror() { return this.#onerror; }
    set onerror(fn) { this.#onerror = fn; }
    get onmessageerror() { return this.#onmessageerror; }
    set onmessageerror(fn) { this.#onmessageerror = fn; }

    // Deliver queued messages and worker errors now.
    poll() {
      if (this.#terminated) return;
      this.#port.poll();
      if (this.#id === null) return;
      let events;
      try { events = core.ops.op_dom_read("workerPoll", String(this.#id), "", ""); } catch { return; }
      if (!Array.isArray(events)) return;
      for (const entry of events) {
        if (entry.kind === "error") {
          const detail = JSON.parse(entry.payload);
          this.#deliver(new event.ErrorEvent("error", {
            message: String(detail.message || "Worker error"),
            filename: String(detail.filename || ""),
            cancelable: true,
          }), this.#onerror);
        } else if (entry.kind === "closed" && this.#timer) {
          timers.clearInterval.call(globalThis, this.#timer);
          this.#timer = null;
        }
      }
    }

    postMessage(message, transfer) {
      if (this.#terminated) return;
      this.#port.postMessage(message, transfer);
    }

    terminate() {
      if (this.#terminated) return;
      this.#terminated = true;
      if (this.#timer) {
        timers.clearInterval.call(globalThis, this.#timer);
        this.#timer = null;
      }
      this.#port.close();
      if (this.#id !== null) {
        try { core.ops.op_dom_read("workerTerminate", String(this.#id), "", ""); } catch (_) {}
      }
    }
  }

  class SharedWorker extends event.EventTarget {
    #port;
    #onerror = null;
    constructor(scriptURL, nameOrOptions = "") {
      super();
      event.setEventTargetData(this);
      if (arguments.length < 1) throw new TypeError("A worker script URL is required");
      const options = typeof nameOrOptions === "string" ? { name: nameOrOptions } : (nameOrOptions || {});
      const ids = op("channelCreate", "", "", "");
      const { address, config } = workerConfig(scriptURL, options, { port: ids.port2 });
      this.#port = new MessagePort(portToken, ids.port1);
      const fail = (error) => {
        const ev = new event.ErrorEvent("error", { message: String(error?.message || error) });
        try { this.#onerror?.call(this, ev); } catch (inner) { globalThis.__wega_report?.(inner); }
        this.dispatchEvent(ev);
      };
      const start = (source) => {
        const problem = op("sharedWorkerConnect", JSON.stringify({ ...config, source }), "", "");
        if (problem) fail(new Error(problem));
      };
      if (address.startsWith("blob:") || options.content) {
        op("workerCheck", address, "", "");
        scriptSource(address, options.content).then(start).catch(fail);
      } else start("");
    }
    get port() { return this.#port; }
    get onerror() { return this.#onerror; }
    set onerror(fn) { this.#onerror = fn; }
  }

  // ---- Service workers (document side) ------------------------------------
  // The host owns registrations and worker lifetimes; this realm mirrors their
  // state by polling and turns changes into the specified events.
  const swToken = Symbol("serviceWorker");
  const attribute = (target, type) => {
    let current = null;
    Object.defineProperty(target, "on" + type, {
      configurable: true, enumerable: true,
      get: () => current,
      set(value) {
        if (current) target.removeEventListener(type, current);
        current = typeof value === "function" ? value : null;
        if (current) target.addEventListener(type, current);
      },
    });
  };
  class ServiceWorker extends event.EventTarget {
    #id; #url; #state;
    constructor(token, info) {
      if (token !== swToken) throw new TypeError("Illegal constructor");
      super();
      event.setEventTargetData(this);
      this.#id = info.id; this.#url = info.script_url; this.#state = info.state;
      attribute(this, "statechange");
      attribute(this, "error");
    }
    get scriptURL() { return this.#url; }
    get state() { return this.#state; }
    postMessage(message, transfer) {
      op("serviceWorkerPost", String(this.#id), codec.encode(message, transfer), "");
    }
    static update(worker, state) {
      if (worker.#state === state) return;
      worker.#state = state;
      worker.dispatchEvent(new event.Event("statechange"));
    }
  }
  class ServiceWorkerRegistration extends event.EventTarget {
    #scope;
    constructor(token, scope) {
      if (token !== swToken) throw new TypeError("Illegal constructor");
      super();
      event.setEventTargetData(this);
      this.#scope = scope;
      this.installing = null; this.waiting = null; this.active = null;
      attribute(this, "updatefound");
    }
    get scope() { return this.#scope; }
    get updateViaCache() { return "imports"; }
    get navigationPreload() {
      return {
        enable: () => Promise.resolve(), disable: () => Promise.resolve(),
        setHeaderValue: () => Promise.resolve(),
        getState: () => Promise.resolve({ enabled: false, headerValue: "" }),
      };
    }
    async update() {
      const job = op("serviceWorkerUpdate", this.#scope, "", "");
      await container.job(job);
      return this;
    }
    async unregister() {
      const removed = op("serviceWorkerUnregister", this.#scope, "", "");
      container.sync();
      return removed;
    }
  }
  const stateName = (state) => String(state).toLowerCase();
  const container = new class ServiceWorkerContainer extends event.EventTarget {
    #timer = null;
    #registrations = new Map();
    #workers = new Map();
    #controller = null;
    #jobs = new Map();
    #ready = null;
    #readyResolve = null;
    constructor() {
      super();
      event.setEventTargetData(this);
      attribute(this, "controllerchange");
      attribute(this, "message");
      attribute(this, "messageerror");
    }
    #watch() {
      if (!this.#timer) this.#timer = timers.setInterval.call(globalThis, () => this.sync(), 20);
    }
    #worker(info) {
      if (!info) return null;
      let worker = this.#workers.get(info.id);
      if (!worker) {
        worker = new ServiceWorker(swToken, { ...info, state: stateName(info.state) });
        this.#workers.set(info.id, worker);
      } else ServiceWorker.update(worker, stateName(info.state));
      return worker;
    }
    // Apply the host's current view of this origin's registrations.
    sync() {
      let snapshot;
      try { snapshot = core.ops.op_dom_read("serviceWorkerPoll", "", "", ""); } catch { return; }
      const live = new Set();
      for (const info of snapshot.registrations) {
        live.add(info.scope);
        let registration = this.#registrations.get(info.scope);
        if (!registration) {
          registration = new ServiceWorkerRegistration(swToken, info.scope);
          this.#registrations.set(info.scope, registration);
        }
        const previous = registration.installing;
        registration.installing = this.#worker(info.installing);
        registration.waiting = this.#worker(info.waiting);
        registration.active = this.#worker(info.active);
        if (registration.installing && registration.installing !== previous)
          registration.dispatchEvent(new event.Event("updatefound"));
      }
      for (const scope of [...this.#registrations.keys()]) if (!live.has(scope)) this.#registrations.delete(scope);
      for (const info of snapshot.retired) {
        const worker = this.#workers.get(info.id);
        if (worker) { ServiceWorker.update(worker, "redundant"); this.#workers.delete(info.id); }
      }
      const controller = snapshot.controller ? this.#workers.get(snapshot.controller) || null : null;
      if (controller !== this.#controller) {
        this.#controller = controller;
        this.dispatchEvent(new event.Event("controllerchange"));
      }
      if (this.#readyResolve && snapshot.ready) {
        const registration = this.#registrations.get(snapshot.ready);
        if (registration?.active) { this.#readyResolve(registration); this.#readyResolve = null; }
      }
      for (const [id, waiter] of [...this.#jobs]) {
        const job = snapshot.jobs[id];
        if (!job) continue;
        this.#jobs.delete(id);
        if (job.error) {
          const match = /^([A-Za-z]+Error):\s*(.*)/s.exec(job.error);
          waiter.reject(match
            ? (match[1] === "TypeError" ? new TypeError(match[2]) : new domException.DOMException(match[2], match[1]))
            : new TypeError(job.error));
        } else {
          // The registration may already be gone again (a failed install).
          waiter.resolve(this.#registrations.get(job.scope) || new ServiceWorkerRegistration(swToken, job.scope));
        }
      }
      for (const message of snapshot.messages) {
        let ev;
        try {
          const { data, ports } = codec.decode(message.data);
          ev = messageEvent("message", {
            data, ports, origin: new url.URL(here()).origin,
            source: this.#workers.get(message.worker) || null,
          });
        } catch { ev = messageEvent("messageerror"); }
        this.dispatchEvent(ev);
      }
    }
    job(id) {
      this.#watch();
      return new Promise((resolve, reject) => this.#jobs.set(String(id), { resolve, reject }));
    }
    get controller() { this.#watch(); this.sync(); return this.#controller; }
    get ready() {
      this.#watch();
      if (!this.#ready) this.#ready = new Promise((resolve) => { this.#readyResolve = resolve; });
      return this.#ready;
    }
    async register(scriptURL, options = {}) {
      const address = new url.URL(String(scriptURL), here()).href;
      const scope = options?.scope !== undefined ? new url.URL(String(options.scope), here()).href : "";
      const job = op("serviceWorkerRegister", address, scope,
        JSON.stringify({ module: options?.type === "module" }));
      return await this.job(job);
    }
    async getRegistration(clientURL = "") {
      this.#watch(); this.sync();
      const target = new url.URL(String(clientURL), here()).href;
      let best;
      for (const [scope, registration] of this.#registrations)
        if (target.startsWith(scope) && (!best || scope.length > best.scope.length)) best = registration;
      return best;
    }
    async getRegistrations() {
      this.#watch(); this.sync();
      return [...this.#registrations.values()];
    }
    startMessages() { this.#watch(); }
    // Controlled documents are clients from load, before script asks.
    attach() {
      try { if (core.ops.op_dom_read("serviceWorkerControlled", "", "", "")) { this.#watch(); this.sync(); } } catch { /* opaque origin */ }
    }
  }();

  globals.MessagePort = MessagePort;
  globals.MessageChannel = MessageChannel;
  globals.Worker = Worker;
  globals.SharedWorker = SharedWorker;
  globals.ServiceWorkerRegistration = ServiceWorkerRegistration;
  globals.ServiceWorker = ServiceWorker;
  globals.serviceWorkerContainer = container;

  // ---- DevTools console capture and value previews (D2). ----
  // This runs in the page realm, so it relies only on intrinsics captured *now*, before any
  // page script, and never calls a method found on a page object or reads an accessor:
  // values are described through own data descriptors only, proxies are never inspected,
  // and JSON is assembled by hand (an object serialised with JSON.stringify would consult a
  // page-defined toJSON). Anything it cannot describe safely becomes a type-only preview.
  const devtools = (() => {
    const op = core.ops.op_dom_read;
    const isProxy = core.isProxy;
    const promiseDetails = core.getPromiseDetails;
    const callSite = core.currentUserCallSite;
    const gopd = Object.getOwnPropertyDescriptor;
    const gopn = Object.getOwnPropertyNames;
    const gpo = Object.getPrototypeOf;
    const hasOwn = Object.hasOwn;
    const sameValue = Object.is;
    const defineProperty = Object.defineProperty;
    const isArray = Array.isArray;
    const isView = ArrayBuffer.isView;
    const reflectApply = Reflect.apply;
    const slice = String.prototype.slice;
    const quote = JSON.stringify; // only ever applied to primitive strings
    const symbolToString = Symbol.prototype.toString;
    const bigintToString = BigInt.prototype.toString;
    const dateToISO = Date.prototype.toISOString;
    const regexpSource = gopd(RegExp.prototype, "source").get;
    const mapSize = gopd(Map.prototype, "size").get;
    const setSize = gopd(Set.prototype, "size").get;
    const typedLength = gopd(gpo(Uint8Array.prototype), "length").get;
    const protoError = Error.prototype;
    const protoDate = Date.prototype;
    const protoRegExp = RegExp.prototype;
    const protoMap = Map.prototype;
    const protoSet = Set.prototype;
    const protoPromise = Promise.prototype;
    const MAX_DEPTH = 3, MAX_PROPS = 8, MAX_NODES = 64, MAX_TEXT = 256;
    const UNAVAILABLE = '{"kind":"Unavailable","text":"unavailable"}';

    function clip(s, max) {
      return s.length > max ? reflectApply(slice, s, [0, max]) : s;
    }
    function leaf(kind, s, forceCut) {
      const cut = s.length > MAX_TEXT;
      return '{"kind":"' + kind + '","text":' + quote(clip(s, MAX_TEXT)) +
        (cut || forceCut ? ',"truncated":true' : "") + "}";
    }
    function composite(kind, s, props, more) {
      const cut = s.length > MAX_TEXT;
      return '{"kind":"' + kind + '","text":' + quote(clip(s, MAX_TEXT)) +
        (cut ? ',"truncated":true' : "") +
        (props === "" ? "" : ',"props":[' + props + "]") +
        (more > 0 ? ',"more":' + more : "") + "}";
    }
    function prop(key, json) {
      return '{"key":' + quote(clip(key, 128)) + ',"value":' + json + "}";
    }
    // Data value of an own property, or undefined plus a flag when it is an accessor.
    function dataOf(object, key) {
      const d = gopd(object, key);
      if (d === undefined) return { has: false };
      if (hasOwn(d, "value")) return { has: true, data: true, value: d.value };
      return { has: true, data: false, get: typeof d.get === "function", set: typeof d.set === "function" };
    }
    function accessor(slot) {
      return slot.get && slot.set ? "(getter/setter)" : slot.get ? "(getter)" : "(setter)";
    }
    function functionName(fn) {
      if (isProxy(fn)) return "";
      const slot = dataOf(fn, "name");
      return slot.data && typeof slot.value === "string" ? slot.value : "";
    }
    function className(object) {
      const proto = gpo(object);
      if (proto === null) return "Object: null prototype";
      const slot = dataOf(proto, "constructor");
      if (slot.data && typeof slot.value === "function") {
        const name = functionName(slot.value);
        if (name !== "") return name;
      }
      return "Object";
    }
    function chainString(object, key, fallback) {
      let at = object;
      for (let i = 0; i < 8 && at !== null && at !== undefined; i++) {
        const slot = dataOf(at, key);
        if (slot.has) return slot.data && typeof slot.value === "string" ? slot.value : "";
        at = gpo(at);
      }
      return fallback;
    }
    function kindOf(object) {
      let at = gpo(object);
      for (let i = 0; i < 32 && at !== null; i++) {
        if (at === protoError) return "Error";
        if (at === protoDate) return "Date";
        if (at === protoRegExp) return "RegExp";
        if (at === protoMap) return "Map";
        if (at === protoSet) return "Set";
        if (at === protoPromise) return "Promise";
        at = gpo(at);
      }
      return "";
    }

    function describe(value, depth, ctx) {
      ctx.nodes++;
      if (ctx.nodes > MAX_NODES) return leaf("Unavailable", "\u2026", true);
      switch (typeof value) {
        case "undefined": return leaf("Undefined", "undefined");
        case "boolean": return leaf("Boolean", value ? "true" : "false");
        case "number": return leaf("Number", sameValue(value, -0) ? "-0" : "" + value);
        case "string": return leaf("String", value);
        case "bigint": return leaf("BigInt", reflectApply(bigintToString, value, []) + "n");
        case "symbol": return leaf("Symbol", reflectApply(symbolToString, value, []));
        case "function": {
          if (isProxy(value)) return leaf("Proxy", "Proxy");
          return leaf("Function", "\u0192 " + functionName(value) + "()");
        }
        case "object": break;
        default: return leaf("Unavailable", "unknown");
      }
      if (value === null) return leaf("Null", "null");
      if (isProxy(value)) return leaf("Proxy", "Proxy");
      for (let i = 0; i < ctx.count; i++) {
        if (ctx.seen[i] === value) return leaf("Circular", "[Circular]");
      }
      const kind = kindOf(value);
      if (kind === "Error") {
        const name = chainString(value, "name", "Error");
        const own = dataOf(value, "message");
        const message = own.data && typeof own.value === "string" ? own.value : "";
        return leaf("Error", message === "" ? name : name + ": " + message);
      }
      if (kind === "Date") {
        try { return leaf("Date", reflectApply(dateToISO, value, [])); } catch { return leaf("Date", "Invalid Date"); }
      }
      if (kind === "RegExp") {
        try { return leaf("RegExp", "/" + reflectApply(regexpSource, value, []) + "/"); } catch { return leaf("RegExp", "RegExp"); }
      }
      if (kind === "Map" || kind === "Set") {
        try {
          const size = reflectApply(kind === "Map" ? mapSize : setSize, value, []);
          return leaf(kind, kind + "(" + size + ")");
        } catch { return leaf(kind, kind); }
      }
      if (kind === "Promise") {
        let state = "pending";
        try {
          const details = promiseDetails(value);
          state = details[0] === 1 ? "fulfilled" : details[0] === 2 ? "rejected" : "pending";
        } catch { /* unknown */ }
        return leaf("Promise", "Promise {<" + state + ">}");
      }
      if (isView(value)) {
        let length = "";
        try { length = "" + reflectApply(typedLength, value, []); } catch { /* DataView */ }
        return leaf("Object", className(value) + (length === "" ? "" : "(" + length + ")"));
      }
      ctx.seen[ctx.count++] = value;
      try {
        if (isArray(value)) {
          const lengthSlot = dataOf(value, "length");
          const length = lengthSlot.data && typeof lengthSlot.value === "number" ? lengthSlot.value : 0;
          const shown = length < MAX_PROPS ? length : MAX_PROPS;
          let props = "";
          if (depth + 1 < MAX_DEPTH) {
            for (let i = 0; i < shown; i++) {
              const slot = dataOf(value, "" + i);
              const json = !slot.has ? leaf("Unavailable", "<empty>")
                : slot.data ? describe(slot.value, depth + 1, ctx)
                : leaf("Unavailable", accessor(slot));
              props += (i === 0 ? "" : ",") + prop("" + i, json);
            }
            return composite("Array", "Array(" + length + ")", props, length - shown);
          }
          return composite("Array", "Array(" + length + ")", "", length);
        }
        const names = gopn(value);
        const total = names.length;
        const name = className(value);
        if (depth + 1 >= MAX_DEPTH) return composite("Object", name, "", total);
        const shown = total < MAX_PROPS ? total : MAX_PROPS;
        let props = "";
        for (let i = 0; i < shown; i++) {
          const key = names[i];
          const slot = dataOf(value, key);
          const json = slot.data ? describe(slot.value, depth + 1, ctx) : leaf("Unavailable", accessor(slot));
          props += (i === 0 ? "" : ",") + prop(key, json);
        }
        return composite("Object", name, props, total - shown);
      } finally {
        ctx.count--;
      }
    }
    function preview(value) {
      try {
        return describe(value, 0, { nodes: 0, seen: [], count: 0 });
      } catch {
        return UNAVAILABLE;
      }
    }

    // One-line summary for the message text: primitives only, never objects.
    function summary(args, n) {
      let text = "";
      for (let i = 0; i < n && text.length < 1024; i++) {
        const v = args[i];
        const t = typeof v;
        const part = t === "string" ? v
          : t === "number" || t === "boolean" || t === "undefined" ? "" + v
          : v === null ? "null"
          : t === "bigint" ? reflectApply(bigintToString, v, []) + "n"
          : t === "function" ? "\u0192"
          : "[" + t + "]";
        text += (i === 0 ? "" : " ") + part;
      }
      return clip(text, 1024);
    }
    let busy = false;
    let suppress = 0;
    function record(method, level, args, text) {
      if (busy) return;
      busy = true;
      try {
        const n = args.length < 16 ? args.length : 16;
        let list = "[";
        for (let i = 0; i < n; i++) list += (i === 0 ? "" : ",") + preview(args[i]);
        list += "]";
        const site = callSite();
        const url = site && typeof site.fileName === "string" ? site.fileName : "";
        op(
          "console",
          '{"m":' + quote(method) + ',"l":' + quote(level) + ',"t":' +
            quote(text === undefined ? summary(args, n) : clip(text, 1024)) +
            ',"a":' + list + ',"u":' + quote(clip(url, 1024)) +
            ',"n":' + ((site && site.lineNumber) >>> 0) + ',"c":' + ((site && site.columnNumber) >>> 0) + "}",
          "", "",
        );
      } catch { /* a missing host op (worker realms) or a full ring never breaks console.log */ } finally {
        busy = false;
      }
    }
    function fromFormatter(message, level) {
      if (suppress !== 0) return;
      let text = "" + message;
      if (text.length !== 0 && text[text.length - 1] === "\n") text = reflectApply(slice, text, [0, text.length - 1]);
      record("log", level === 0 ? "debug" : level === 2 ? "warn" : level === 3 ? "error" : "log", [], text);
    }
    function wrap(target) {
      for (const [method, level] of [["log", "log"], ["info", "info"], ["warn", "warn"], ["error", "error"], ["debug", "debug"]]) {
        const original = target[method];
        if (typeof original !== "function") continue;
        const wrapper = function(...args) {
          record(method, level, args, undefined);
          suppress++;
          try {
            return reflectApply(original, this, args);
          } finally {
            suppress--;
          }
        };
        defineProperty(wrapper, "name", { value: method, configurable: true });
        target[method] = wrapper;
      }
    }
    return { preview, fromFormatter, wrap };
  })();
  Object.defineProperty(globalThis, "__wega_preview", {
    configurable: true,
    value: (value) => devtools.preview(value),
  });

  globals.console = new console.Console((message, level) => {
    core.ops.op_dom_read("log", message, String(level), "");
    devtools.fromFormatter(message, level);
  });
  devtools.wrap(globals.console);
  Object.defineProperty(globalThis, "__wega_web", {
    configurable: true,
    value: { ...globals, event, messageEvent, brand: internals.webidlBrand, codec, base64Encode, base64Decode, op, here, timers, cacheResponse(entry) {
      const body = [204, 205, 304].includes(entry.status) ? null : new Uint8Array(entry.body);
      const result = new response.Response(body, {status: entry.status, statusText: entry.status_text, headers: entry.headers});
      const inner = response.toInnerResponse(result);
      inner.urlList = entry.url ? [entry.url] : [];
      return response.fromInnerResponse(inner, "immutable");
    } },
  });
  Object.defineProperty(globalThis, "__wega_install_web", {
    configurable: true,
    value: () => {
      Object.assign(globalThis, globals);
      delete globalThis.serviceWorkerContainer;
      Object.defineProperties(globalThis, {
        localStorage: { get: storage.localStorage, configurable: true },
        sessionStorage: { get: storage.sessionStorage, configurable: true },
      });
    },
  });
  // Deno's reporting code dispatches ErrorEvent on the global event target.
  event.setEventTargetData(globalThis);
  event.saveGlobalThisReference(globalThis);
})(Deno.core);
