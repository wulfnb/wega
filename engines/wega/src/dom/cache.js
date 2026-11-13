// Web API bindings only. Rust owns origin admission, SQLite and atomic batches.
(() => {
  const host = __wega_call;
  if (!host("cacheAllowed")) return;
  const restore = __wega_web.cacheResponse;
  const NativeRequest = Request, NativeResponse = Response;
  const handles = new WeakMap(), token = {};
  const byteLimit = 1024 * 1024;
  function call(operation) {
    try { return host("cacheCall", JSON.stringify(operation)); }
    catch (error) {
      const match = /^([A-Za-z]+Error):\s*(.*)/.exec(error.message || "");
      if (match) throw match[1] === "TypeError" ? new TypeError(match[2]) : new DOMException(match[2], match[1]);
      throw error;
    }
  }
  function id(cache) {
    if (!handles.has(cache)) throw new TypeError("Illegal invocation");
    return handles.get(cache);
  }
  function request(input) {
    if (input === undefined) throw new TypeError("A request is required");
    return input instanceof NativeRequest ? input : new NativeRequest(new URL(String(input), location.href).href);
  }
  function describe(req) {
    return {url: req.url, method: req.method, headers: [...req.headers]};
  }
  function options(value = {}) {
    value ??= {};
    return {ignoreSearch: !!value.ignoreSearch, ignoreMethod: !!value.ignoreMethod, ignoreVary: !!value.ignoreVary};
  }
  async function record(req, res) {
    if (!(res instanceof NativeResponse)) throw new TypeError("A Response is required");
    if (req.method !== "GET" || !/^https?:/.test(req.url) || res.status === 206 || res.status === 0 ||
        (res.headers.get("vary") || "").split(",").some(name => name.trim() === "*"))
      throw new TypeError("Response cannot be cached");
    if (res.bodyUsed || res.body?.locked) throw new TypeError("Response body has already been consumed or locked");
    const chunks = [];
    let length = 0;
    if (res.body) {
      const reader = res.body.getReader();
      try {
        for (;;) {
          const {done, value} = await reader.read();
          if (done) break;
          length += value.byteLength;
          if (length > byteLimit) {
            await reader.cancel();
            throw new DOMException("Cache response exceeds 1 MiB budget", "QuotaExceededError");
          }
          chunks.push(value);
        }
      } finally { reader.releaseLock(); }
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    return {request: describe(req), response: {
      url: res.url, method: req.method, status: res.status,
      status_text: res.statusText, headers: [...res.headers], body: [...bytes],
    }};
  }
  function query(cache, input, opts, kind) {
    return call({type: "query", id: id(cache), request: input === undefined ? null : describe(request(input)), options: options(opts), kind});
  }
  class Cache {
    constructor(key, handle) {
      if (key !== token) throw new TypeError("Illegal constructor");
      handles.set(this, handle);
    }
    async match(input, opts) {
      const entries = query(this, request(input), opts, "first");
      return entries.length ? restore(entries[0]) : undefined;
    }
    async matchAll(input, opts) { return Object.freeze(query(this, input, opts, "all").map(restore)); }
    async keys(input, opts) {
      return Object.freeze(query(this, input, opts, "keys").map(req => new NativeRequest(req.url, {method: req.method, headers: req.headers})));
    }
    async put(input, res) {
      const handle = id(this);
      const item = await record(request(input), res);
      call({type: "put", id: handle, records: [item]});
    }
    async add(input) { return this.addAll([input]); }
    async addAll(inputs) {
      const handle = id(this);
      const reqs = Array.from(inputs, request);
      if (reqs.length > 32) throw new DOMException("Cache batch exceeds 32 requests", "QuotaExceededError");
      for (const req of reqs) if (req.method !== "GET" || !/^https?:/.test(req.url)) throw new TypeError("Only HTTP GET requests can be added");
      const records = await Promise.all(reqs.map(async req => {
        const res = await fetch(req);
        if (!res.ok) throw new TypeError("Cache.addAll received an unsuccessful response");
        return record(req, res);
      }));
      call({type: "put", id: handle, records});
    }
    async delete(input, opts) {
      return call({type: "delete", id: id(this), request: describe(request(input)), options: options(opts)});
    }
    get [Symbol.toStringTag]() { return "Cache"; }
  }
  const storageBrand = new WeakSet();
  function storage(receiver) { if (!storageBrand.has(receiver)) throw new TypeError("Illegal invocation"); }
  class CacheStorage {
    constructor(key) {
      if (key !== token) throw new TypeError("Illegal constructor");
      storageBrand.add(this);
    }
    async open(name) { storage(this); if (!arguments.length) throw new TypeError("A cache name is required"); return new Cache(token, call({type: "open", name: String(name)})); }
    async has(name) { storage(this); if (!arguments.length) throw new TypeError("A cache name is required"); return call({type: "has", name: String(name)}); }
    async delete(name) { storage(this); if (!arguments.length) throw new TypeError("A cache name is required"); return call({type: "deleteCache", name: String(name)}); }
    async keys() { storage(this); return call({type: "names"}); }
    async match(input, opts = {}) {
      storage(this);
      opts ??= {};
      const entries = call({type: "query", id: null, name: opts.cacheName === undefined ? null : String(opts.cacheName), request: describe(request(input)), options: options(opts), kind: "first"});
      return entries.length ? restore(entries[0]) : undefined;
    }
    get [Symbol.toStringTag]() { return "CacheStorage"; }
  }
  Object.defineProperties(globalThis, {
    Cache: {value: Cache, configurable: true, writable: true},
    CacheStorage: {value: CacheStorage, configurable: true, writable: true},
    caches: {get: () => cacheStorage, configurable: true},
  });
  const cacheStorage = new CacheStorage(token);
})();
