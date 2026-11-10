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
      "error", String(reason) + "\n" + String(reason?.stack || ""), "", "",
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
  class MessagePort extends event.EventTarget {
    #id;
    #closed = false;
    #onmessage = null;
    #timer = null;

    constructor(id) {
      super();
      this.#id = id;
      event.setEventTargetData(this);
      this.#timer = timers.setInterval.call(globalThis, () => {
        if (this.#closed) return;
        try {
          const messages = core.ops.op_dom_read("portPoll", String(this.#id), "", "");
          if (Array.isArray(messages)) {
            for (const msg of messages) {
              const data = typeof msg.data === "string" ? JSON.parse(msg.data) : msg.data;
              const ev = new event.MessageEvent("message", { data });
              if (typeof this.#onmessage === "function") {
                this.#onmessage.call(this, ev);
              }
              this.dispatchEvent(ev);
            }
          }
        } catch (_) {}
      }, 5);
    }

    get onmessage() { return this.#onmessage; }
    set onmessage(fn) {
      this.#onmessage = fn;
      this.start();
    }

    start() {}

    poll() {
      if (this.#closed) return;
      try {
        const messages = core.ops.op_dom_read("portPoll", String(this.#id), "", "");
        if (Array.isArray(messages)) {
          for (const msg of messages) {
            const data = typeof msg.data === "string" ? JSON.parse(msg.data) : msg.data;
            const ev = new event.MessageEvent("message", { data });
            if (typeof this.#onmessage === "function") {
              this.#onmessage.call(this, ev);
            }
            this.dispatchEvent(ev);
          }
        }
      } catch (_) {}
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

    postMessage(message, transfer = []) {
      if (this.#closed) return;
      if (Array.isArray(transfer)) {
        for (const item of transfer) {
          if (item instanceof ArrayBuffer) {
            Object.defineProperty(item, "byteLength", { value: 0, configurable: true });
            Object.defineProperty(item, "detached", { value: true, configurable: true });
          }
        }
      }
      const transferredPortIds = Array.isArray(transfer)
        ? transfer.filter(x => x instanceof MessagePort).map(p => p.#id)
        : [];
      core.ops.op_dom_read(
        "portPost",
        String(this.#id),
        JSON.stringify(message),
        JSON.stringify(transferredPortIds)
      );
    }
  }

  class MessageChannel {
    constructor() {
      const ids = core.ops.op_dom_read("channelCreate", "", "", "");
      this.port1 = new MessagePort(ids.port1);
      this.port2 = new MessagePort(ids.port2);
    }
  }

  class Worker extends event.EventTarget {
    #id;
    #terminated = false;
    #onmessage = null;
    #onerror = null;
    #timer = null;

    constructor(scriptURL, options = {}) {
      super();
      event.setEventTargetData(this);
      const urlStr = String(scriptURL);
      const scriptContent = options.content || "";
      this.#id = core.ops.op_dom_read("workerCreate", urlStr, scriptContent, "");

      this.#timer = timers.setInterval.call(globalThis, () => {
        if (this.#terminated) return;
        try {
          const events = core.ops.op_dom_read("workerPoll", String(this.#id), "", "");
          if (Array.isArray(events)) {
            for (const ev of events) {
              if (ev.Message !== undefined) {
                const data = JSON.parse(ev.Message);
                const me = new event.MessageEvent("message", { data });
                if (typeof this.#onmessage === "function") {
                  this.#onmessage.call(this, me);
                }
                this.dispatchEvent(me);
              } else if (ev.Error !== undefined) {
                const ee = new event.ErrorEvent("error", { message: ev.Error });
                if (typeof this.#onerror === "function") {
                  this.#onerror.call(this, ee);
                }
                this.dispatchEvent(ee);
              }
            }
          }
        } catch (_) {}
      }, 5);
    }

    get onmessage() { return this.#onmessage; }
    set onmessage(fn) { this.#onmessage = fn; }

    get onerror() { return this.#onerror; }
    set onerror(fn) { this.#onerror = fn; }

    poll() {
      if (this.#terminated) return;
      try {
        const events = core.ops.op_dom_read("workerPoll", String(this.#id), "", "");
        if (Array.isArray(events)) {
          for (const ev of events) {
            if (ev.Message !== undefined) {
              const data = JSON.parse(ev.Message);
              const me = new event.MessageEvent("message", { data });
              if (typeof this.#onmessage === "function") {
                this.#onmessage.call(this, me);
              }
              this.dispatchEvent(me);
            } else if (ev.Error !== undefined) {
              const ee = new event.ErrorEvent("error", { message: ev.Error });
              if (typeof this.#onerror === "function") {
                this.#onerror.call(this, ee);
              }
              this.dispatchEvent(ee);
            }
          }
        }
      } catch (_) {}
    }

    postMessage(message, transfer = []) {
      if (this.#terminated) return;
      if (Array.isArray(transfer)) {
        for (const item of transfer) {
          if (item instanceof ArrayBuffer) {
            Object.defineProperty(item, "byteLength", { value: 0, configurable: true });
            Object.defineProperty(item, "detached", { value: true, configurable: true });
          }
        }
      }
      core.ops.op_dom_read("workerPost", String(this.#id), JSON.stringify(message), "");
    }

    terminate() {
      if (this.#terminated) return;
      this.#terminated = true;
      if (this.#timer) {
        timers.clearInterval.call(globalThis, this.#timer);
        this.#timer = null;
      }
      try {
        core.ops.op_dom_read("workerTerminate", String(this.#id), "", "");
      } catch (_) {}
    }
  }

  class SharedWorker extends event.EventTarget {
    #port;
    constructor(scriptURL, nameOrOptions = "") {
      super();
      event.setEventTargetData(this);
      const name = typeof nameOrOptions === "string" ? nameOrOptions : (nameOrOptions.name || "");
      const content = typeof nameOrOptions === "object" && nameOrOptions.content ? nameOrOptions.content : "";
      const res = core.ops.op_dom_read("sharedWorkerConnect", String(scriptURL), name, content);
      this.#port = new MessagePort(res.docPort);
    }
    get port() { return this.#port; }
  }

  class ServiceWorkerRegistration {
    constructor(info) {
      this.scope = info.scope;
      this.installing = null;
      this.waiting = null;
      this.active = { scriptURL: info.script_url, state: (info.state || "activated").toLowerCase() };
    }

    async unregister() {
      return core.ops.op_dom_read("serviceWorkerUnregister", this.scope, "", "");
    }
  }

  const serviceWorkerContainer = new class extends event.EventTarget {
    constructor() {
      super();
      event.setEventTargetData(this);
    }

    async register(scriptURL, options = {}) {
      const scope = options.scope ? String(options.scope) : "";
      const info = core.ops.op_dom_read("serviceWorkerRegister", String(scriptURL), scope, "");
      return new ServiceWorkerRegistration(info);
    }

    async getRegistration(clientURL = "") {
      const info = core.ops.op_dom_read("serviceWorkerGetRegistration", String(clientURL), "", "");
      if (!info) return undefined;
      return new ServiceWorkerRegistration(info);
    }
  }();

  globals.MessagePort = MessagePort;
  globals.MessageChannel = MessageChannel;
  globals.Worker = Worker;
  globals.SharedWorker = SharedWorker;
  globals.ServiceWorkerRegistration = ServiceWorkerRegistration;
  globals.serviceWorkerContainer = serviceWorkerContainer;
  globals.structuredClone = (value, options = {}) => {
    if (options && Array.isArray(options.transfer)) {
      for (const item of options.transfer) {
        if (item instanceof ArrayBuffer) {
          Object.defineProperty(item, "byteLength", { value: 0, configurable: true });
          Object.defineProperty(item, "detached", { value: true, configurable: true });
        }
      }
    }
    return JSON.parse(JSON.stringify(value));
  };

  globals.console = new console.Console((message, level) =>
    core.ops.op_dom_read("log", message, String(level), ""),
  );
  Object.defineProperty(globalThis, "__wega_web", {
    configurable: true,
    value: { ...globals, event },
  });
  Object.defineProperty(globalThis, "__wega_install_web", {
    configurable: true,
    value: () => {
      Object.assign(globalThis, globals);
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
