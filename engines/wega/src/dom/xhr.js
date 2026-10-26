// XMLHttpRequest shares fetch's policy, cancellation and response-size limit.
class XMLHttpRequest extends EventTarget {
  constructor() {
    super();
    this.readyState = 0;
    this.status = 0;
    this.statusText = "";
    this.response = null;
    this.responseText = "";
    this.responseType = "";
    this.responseURL = "";
    this.withCredentials = false;
    this.timeout = 0;
    this.upload = new EventTarget();
    this._headers = new Headers();
    this._responseHeaders = new Headers();
  }
  _emit(type) {
    const event = new Event(type);
    try {
      if (typeof this["on" + type] === "function")
        this["on" + type].call(this, event);
    } catch (error) {
      __wega_report(error);
    }
    this.dispatchEvent(event);
  }
  _state(value) {
    this.readyState = value;
    this._emit("readystatechange");
  }
  open(method, url, async = true) {
    if (!async)
      throw new DOMException(
        "Synchronous XHR is not supported",
        "NotSupportedError",
      );
    this.abort();
    this._method = String(method);
    this._url = new URL(String(url), location.href).href;
    this._headers = new Headers();
    this._state(1);
  }
  setRequestHeader(name, value) {
    if (this.readyState !== 1 || this._sent)
      throw new DOMException("XHR is not opened", "InvalidStateError");
    this._headers.append(name, value);
  }
  getResponseHeader(name) {
    return this.readyState < 2 ? null : this._responseHeaders.get(name);
  }
  getAllResponseHeaders() {
    return this.readyState < 2
      ? ""
      : [...this._responseHeaders]
          .map(([k, v]) => k + ": " + v + "\r\n")
          .join("");
  }
  overrideMimeType(type) {
    this._mime = String(type);
  }
  abort() {
    if (this._controller) {
      this._controller.abort();
      this._controller = null;
    }
    this._sent = false;
    this.readyState = 0;
  }
  send(body = null) {
    if (this.readyState !== 1 || this._sent)
      throw new DOMException("XHR is not opened", "InvalidStateError");
    this._sent = true;
    const controller = (this._controller = new AbortController());
    this._emit("loadstart");
    let timedOut = false;
    const timer =
      this.timeout > 0
        ? setTimeout(() => {
            timedOut = true;
            controller.abort();
          }, this.timeout)
        : null;
    fetch(this._url, {
      method: this._method,
      headers: this._headers,
      body: ["GET", "HEAD"].includes(this._method.toUpperCase()) ? null : body,
      signal: controller.signal,
      credentials: this.withCredentials ? "include" : "same-origin",
    })
      .then(async (result) => {
        if (this._controller !== controller) return;
        this.status = result.status;
        this.statusText = result.statusText;
        this.responseURL = result.url;
        this._responseHeaders = result.headers;
        this._state(2);
        this._state(3);
        if (this.responseType === "arraybuffer")
          this.response = await result.arrayBuffer();
        else if (this.responseType === "blob")
          this.response = await result.blob();
        else {
          this.responseText = await result.text();
          this.response =
            this.responseType === "json"
              ? JSON.parse(this.responseText)
              : this.responseText;
        }
        if (this._controller !== controller) return;
        this._sent = false;
        this._state(4);
        this._emit("load");
        this._emit("loadend");
      })
      .catch((error) => {
        if (this._controller !== controller) return;
        this.status = 0;
        this._sent = false;
        this._state(4);
        this._emit(
          timedOut ? "timeout" : controller.signal.aborted ? "abort" : "error",
        );
        this._emit("loadend");
      })
      .finally(() => {
        if (timer !== null) clearTimeout(timer);
      });
  }
}
Object.assign(XMLHttpRequest, {
  UNSENT: 0,
  OPENED: 1,
  HEADERS_RECEIVED: 2,
  LOADING: 3,
  DONE: 4,
});
Object.assign(XMLHttpRequest.prototype, {
  UNSENT: 0,
  OPENED: 1,
  HEADERS_RECEIVED: 2,
  LOADING: 3,
  DONE: 4,
});
globalThis.XMLHttpRequest = XMLHttpRequest;
