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
  location.setLocationHref(core.ops.op_dom_read("url", "", "", ""));
  const websocket = core.createLazyLoader(
    "ext:deno_websocket/01_websocket.js",
  )();
  const globals = {
    DOMException: domException.DOMException,
    Event: event.Event,
    EventTarget: event.EventTarget,
    ErrorEvent: event.ErrorEvent,
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
      Object.defineProperties(bounded, {
        url: { value: result.url },
        redirected: { value: result.redirected },
        type: { value: result.type },
      });
      return bounded;
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
  globals.console = new console.Console((message) =>
    core.ops.op_dom_read("log", message, "", ""),
  );
  Object.defineProperty(globalThis, "__wega_web", {
    value: { ...globals, event },
  });
  Object.defineProperty(globalThis, "__wega_install_web", {
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
