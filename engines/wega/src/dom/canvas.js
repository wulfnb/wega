// Canvas 2D, OffscreenCanvas and WebGL bindings shared by documents and
// workers. Rust owns the surfaces; `host` reaches the caller's own engine.
Object.defineProperty(globalThis, "__wega_canvas", {
  configurable: true,
  value: ({ host, fail, isCanvasElement, isImageElement, drew = () => {} }) => {
  class CanvasGradient {
    constructor(type, coords) {
      this._type = type;
      this._coords = coords;
      this._stops = [];
    }
    addColorStop(offset, color) {
      const num = Number(offset);
      if (Number.isNaN(num) || num < 0 || num > 1) {
        fail("IndexSizeError", "Offset must be between 0 and 1");
      }
      this._stops.push({ offset: num, color: String(color) });
    }
  }

  class CanvasPattern {
    constructor(image, repetition) {
      this._image = image;
      this._repetition = repetition;
    }
    setTransform(_transform) {}
  }

  class ImageData {
    constructor(arg1, arg2, arg3) {
      if (arg1 instanceof Uint8ClampedArray) {
        this.data = arg1;
        this.width = arg2 >>> 0;
        this.height = arg3 ? (arg3 >>> 0) : ((arg1.length / 4 / this.width) >>> 0);
      } else {
        this.width = Math.max(1, arg1 >>> 0);
        this.height = Math.max(1, arg2 >>> 0);
        this.data = new Uint8ClampedArray(this.width * this.height * 4);
      }
      this.colorSpace = "srgb";
    }
  }

  class Path2D {
    constructor(path) {
      this._ops = [];
      if (path instanceof Path2D) {
        this._ops = [...path._ops];
      }
    }
    addPath(path) {
      if (path instanceof Path2D) {
        this._ops.push(...path._ops);
      }
    }
    closePath() { this._ops.push(["closePath"]); }
    moveTo(x, y) { this._ops.push(["moveTo", x, y]); }
    lineTo(x, y) { this._ops.push(["lineTo", x, y]); }
    quadraticCurveTo(cpx, cpy, x, y) { this._ops.push(["quadraticCurveTo", cpx, cpy, x, y]); }
    bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y) { this._ops.push(["bezierCurveTo", cp1x, cp1y, cp2x, cp2y, x, y]); }
    arcTo(x1, y1, x2, y2, radius) { this._ops.push(["arcTo", x1, y1, x2, y2, radius]); }
    rect(x, y, w, h) { this._ops.push(["rect", x, y, w, h]); }
    arc(x, y, radius, startAngle, endAngle, counterclockwise = false) {
      this._ops.push(["arc", x, y, radius, startAngle, endAngle, counterclockwise]);
    }
    ellipse(x, y, rx, ry, rotation, startAngle, endAngle, counterclockwise = false) {
      this._ops.push(["ellipse", x, y, rx, ry, rotation, startAngle, endAngle, counterclockwise]);
    }
  }

  // WebGL 1.0 & 2.0 implementations
  class WebGLObject {
    constructor(id) {
      this._id = id;
    }
  }
  class WebGLBuffer extends WebGLObject {}
  class WebGLShader extends WebGLObject {}
  class WebGLProgram extends WebGLObject {}
  class WebGLTexture extends WebGLObject {}
  class WebGLFramebuffer extends WebGLObject {}
  class WebGLRenderbuffer extends WebGLObject {}
  class WebGLUniformLocation {
    constructor(location, name) {
      this._location = location;
      this._name = name;
    }
  }

  const GL_DEPTH_BUFFER_BIT = 0x00000100;
  const GL_STENCIL_BUFFER_BIT = 0x00000400;
  const GL_COLOR_BUFFER_BIT = 0x00004000;
  const GL_POINTS = 0x0000;
  const GL_LINES = 0x0001;
  const GL_LINE_LOOP = 0x0002;
  const GL_LINE_STRIP = 0x0003;
  const GL_TRIANGLES = 0x0004;
  const GL_TRIANGLE_STRIP = 0x0005;
  const GL_TRIANGLE_FAN = 0x0006;
  const GL_ZERO = 0;
  const GL_ONE = 1;
  const GL_SRC_COLOR = 0x0300;
  const GL_ONE_MINUS_SRC_COLOR = 0x0301;
  const GL_SRC_ALPHA = 0x0302;
  const GL_ONE_MINUS_SRC_ALPHA = 0x0303;
  const GL_DST_ALPHA = 0x0304;
  const GL_ONE_MINUS_DST_ALPHA = 0x0305;
  const GL_DST_COLOR = 0x0306;
  const GL_ONE_MINUS_DST_COLOR = 0x0307;
  const GL_SRC_ALPHA_SATURATE = 0x0308;
  const GL_FUNC_ADD = 0x8006;
  const GL_BLEND_EQUATION = 0x8009;
  const GL_BLEND_EQUATION_RGB = 0x8009;
  const GL_BLEND_EQUATION_ALPHA = 0x883D;
  const GL_ARRAY_BUFFER = 0x8892;
  const GL_ELEMENT_ARRAY_BUFFER = 0x8893;
  const GL_STATIC_DRAW = 0x88E4;
  const GL_STREAM_DRAW = 0x88E0;
  const GL_DYNAMIC_DRAW = 0x88E8;
  const GL_CULL_FACE = 0x0B44;
  const GL_BLEND = 0x0BE2;
  const GL_DITHER = 0x0BD0;
  const GL_STENCIL_TEST = 0x0B90;
  const GL_DEPTH_TEST = 0x0B71;
  const GL_SCISSOR_TEST = 0x0C11;
  const GL_NO_ERROR = 0;
  const GL_INVALID_ENUM = 0x0500;
  const GL_INVALID_VALUE = 0x0501;
  const GL_INVALID_OPERATION = 0x0502;
  const GL_OUT_OF_MEMORY = 0x0505;
  const GL_CONTEXT_LOST_WEBGL = 0x9242;
  const GL_BYTE = 0x1400;
  const GL_UNSIGNED_BYTE = 0x1401;
  const GL_SHORT = 0x1402;
  const GL_UNSIGNED_SHORT = 0x1403;
  const GL_INT = 0x1404;
  const GL_UNSIGNED_INT = 0x1405;
  const GL_FLOAT = 0x1406;
  const GL_RGBA = 0x1908;
  const GL_RGB = 0x1907;
  const GL_FRAGMENT_SHADER = 0x8B30;
  const GL_VERTEX_SHADER = 0x8B31;
  const GL_COMPILE_STATUS = 0x8B81;
  const GL_DELETE_STATUS = 0x8B80;
  const GL_LINK_STATUS = 0x8B82;
  const GL_VALIDATE_STATUS = 0x8B83;
  const GL_TEXTURE_2D = 0x0DE1;

  class WebGLRenderingContextBase {
    constructor(canvas, webglId, isWebGL2 = false) {
      this._canvas = canvas;
      this._webglId = webglId;
      this._isWebGL2 = isWebGL2;
      this._contextLost = false;
    }

    get canvas() { return this._canvas; }
    get drawingBufferWidth() { return this._canvas.width; }
    get drawingBufferHeight() { return this._canvas.height; }

    _call(op, ...args) {
      if (this._canvas?._detached) fail("InvalidStateError", "Canvas is detached");
      return host("webglOp", String(this._webglId), op, JSON.stringify(args));
    }

    isContextLost() {
      return Boolean(this._call("isContextLost"));
    }

    getError() {
      return this._call("getError");
    }

    enable(cap) { this._call("enable", cap >>> 0); }
    disable(cap) { this._call("disable", cap >>> 0); }
    isEnabled(cap) { return Boolean(this._call("isEnabled", cap >>> 0)); }

    clearColor(r, g, b, a) { this._call("clearColor", Number(r) || 0, Number(g) || 0, Number(b) || 0, Number(a) || 0); }
    clearDepth(depth) { this._call("clearDepth", Number(depth) || 0); }
    clear(mask) { this._call("clear", mask >>> 0); }

    viewport(x, y, w, h) { this._call("viewport", x | 0, y | 0, w | 0, h | 0); }
    scissor(x, y, w, h) { this._call("scissor", x | 0, y | 0, w | 0, h | 0); }

    createBuffer() {
      const id = this._call("createBuffer");
      return id ? new WebGLBuffer(id) : null;
    }
    bindBuffer(target, buffer) {
      this._call("bindBuffer", target >>> 0, buffer ? buffer._id : 0);
    }
    bufferData(target, data, usage) {
      let bytes = [];
      if (data instanceof ArrayBuffer) {
        bytes = Array.from(new Uint8Array(data));
      } else if (ArrayBuffer.isView(data)) {
        bytes = Array.from(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
      } else if (typeof data === "number") {
        bytes = new Array(data).fill(0);
      }
      this._call("bufferData", target >>> 0, bytes, usage >>> 0);
    }

    createShader(type) {
      const id = this._call("createShader", type >>> 0);
      return id ? new WebGLShader(id) : null;
    }
    shaderSource(shader, source) {
      if (!shader) fail("TypeError", "Shader required");
      this._call("shaderSource", shader._id, String(source ?? ""));
    }
    compileShader(shader) {
      if (!shader) fail("TypeError", "Shader required");
      this._call("compileShader", shader._id);
    }
    getShaderParameter(shader, pname) {
      if (!shader) return null;
      return this._call("getShaderParameter", shader._id, pname >>> 0);
    }
    getShaderInfoLog(shader) {
      if (!shader) return "";
      return this._call("getShaderInfoLog", shader._id) || "";
    }

    createProgram() {
      const id = this._call("createProgram");
      return id ? new WebGLProgram(id) : null;
    }
    attachShader(program, shader) {
      if (!program || !shader) fail("TypeError", "Program and shader required");
      this._call("attachShader", program._id, shader._id);
    }
    linkProgram(program) {
      if (!program) fail("TypeError", "Program required");
      this._call("linkProgram", program._id);
    }
    useProgram(program) {
      this._call("useProgram", program ? program._id : 0);
    }
    getProgramParameter(program, pname) {
      if (!program) return null;
      return this._call("getProgramParameter", program._id, pname >>> 0);
    }
    getUniformLocation(program, name) {
      if (!program) return null;
      const loc = this._call("getUniformLocation", program._id, String(name));
      return loc !== null && loc !== undefined ? new WebGLUniformLocation(loc, String(name)) : null;
    }
    getAttribLocation(program, name) {
      if (!program) return -1;
      return this._call("getAttribLocation", program._id, String(name));
    }

    uniform1f(loc, x) { this._uniform(loc, [Number(x) || 0]); }
    uniform2f(loc, x, y) { this._uniform(loc, [Number(x) || 0, Number(y) || 0]); }
    uniform3f(loc, x, y, z) { this._uniform(loc, [Number(x) || 0, Number(y) || 0, Number(z) || 0]); }
    uniform4f(loc, x, y, z, w) { this._uniform(loc, [Number(x) || 0, Number(y) || 0, Number(z) || 0, Number(w) || 0]); }
    uniform1i(loc, x) { this._uniform(loc, [Number(x) || 0]); }
    uniform2i(loc, x, y) { this._uniform(loc, [Number(x) || 0, Number(y) || 0]); }
    uniform3i(loc, x, y, z) { this._uniform(loc, [Number(x) || 0, Number(y) || 0, Number(z) || 0]); }
    uniform4i(loc, x, y, z, w) { this._uniform(loc, [Number(x) || 0, Number(y) || 0, Number(z) || 0, Number(w) || 0]); }

    uniform1fv(loc, val) { this._uniform(loc, Array.from(val)); }
    uniform2fv(loc, val) { this._uniform(loc, Array.from(val)); }
    uniform3fv(loc, val) { this._uniform(loc, Array.from(val)); }
    uniform4fv(loc, val) { this._uniform(loc, Array.from(val)); }
    uniformMatrix2fv(loc, transpose, val) { this._uniform(loc, Array.from(val)); }
    uniformMatrix3fv(loc, transpose, val) { this._uniform(loc, Array.from(val)); }
    uniformMatrix4fv(loc, transpose, val) { this._uniform(loc, Array.from(val)); }

    _uniform(loc, values) {
      if (!loc) return;
      this._call("uniformValues", loc._location, values);
    }

    vertexAttribPointer(index, size, type, normalized, stride, offset) {
      this._call("vertexAttribPointer", index >>> 0, size | 0, type >>> 0, Boolean(normalized), stride | 0, offset | 0);
    }
    enableVertexAttribArray(index) {
      this._call("enableVertexAttribArray", index >>> 0);
    }
    disableVertexAttribArray(index) {
      this._call("disableVertexAttribArray", index >>> 0);
    }

    drawArrays(mode, first, count) {
      this._call("drawArrays", mode >>> 0, first | 0, count | 0);
    }
    drawElements(mode, count, type, offset) {
      this._call("drawElements", mode >>> 0, count | 0, type >>> 0, offset | 0);
    }

    createTexture() {
      const id = this._call("createTexture");
      return id ? new WebGLTexture(id) : null;
    }
    bindTexture(target, texture) {
      this._call("bindTexture", target >>> 0, texture ? texture._id : 0);
    }
    texImage2D(...args) {
      const target = args[0] >>> 0;
      const level = (args[1] | 0);
      const internalformat = (args[2] | 0);

      let isClean = true;
      let w = 0, h = 0, fmt = GL_RGBA, tp = GL_UNSIGNED_BYTE, bytes = [];

      // Check if last argument (or 6th argument) is an image/canvas/imagedata source
      const candidate = args[args.length - 1];
      const isSourceObj = candidate && (
        isImageElement(candidate) ||
        isCanvasElement(candidate) ||
        candidate instanceof OffscreenCanvas ||
        candidate instanceof ImageData ||
        candidate.tagName === "IMG" ||
        typeof candidate.src === "string" ||
        (typeof candidate === "object" && !(candidate instanceof ArrayBuffer) && !ArrayBuffer.isView(candidate) && !Array.isArray(candidate))
      );

      if (isSourceObj) {
        const source = candidate;
        if (args.length === 6) {
          fmt = args[3] >>> 0;
          tp = args[4] >>> 0;
        }
        if (isCanvasElement(source) || source instanceof OffscreenCanvas) {
          w = source.width;
          h = source.height;
          try {
            bytes = Array.from(source.getContext("2d").getImageData(0, 0, w, h).data);
          } catch (e) {
            isClean = false;
          }
        } else if (source instanceof ImageData) {
          w = source.width;
          h = source.height;
          bytes = Array.from(source.data);
        } else {
          // HTMLImageElement or element with src
          w = source.width || 1;
          h = source.height || 1;
          const crossOrigin = source.crossOrigin || (typeof source.getAttribute === "function" ? source.getAttribute("crossorigin") : null);
          const rawSrc = source.src || (typeof source.getAttribute === "function" ? source.getAttribute("src") : "") || "";
          const src = String(rawSrc);
          isClean = crossOrigin === "anonymous" || crossOrigin === "use-credentials" || (!src.startsWith("http://") && !src.startsWith("https://") && !src.startsWith("//"));
        }
      } else {
        w = args[3] | 0;
        h = args[4] | 0;
        fmt = (args[6] || GL_RGBA) >>> 0;
        tp = (args[7] || GL_UNSIGNED_BYTE) >>> 0;
        const pixels = args[8];
        if (pixels && pixels.buffer) {
          bytes = Array.from(new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength));
        } else if (Array.isArray(pixels)) {
          bytes = pixels;
        }
      }

      if (!isClean) {
        this._call("taint");
      }
      this._call("texImage2D", target, level, internalformat, w | 0, h | 0, 0, fmt, tp, bytes, Boolean(isClean));
    }

    readPixels(x, y, width, height, format, type, pixels) {
      if (this._canvas?._detached) fail("InvalidStateError", "Canvas is detached");
      try {
        const raw = this._call("readPixels", x | 0, y | 0, width | 0, height | 0);
        if (pixels && pixels.set) {
          pixels.set(new Uint8Array(raw));
        }
        return pixels;
      } catch (e) {
        if (String(e.message || e).includes("SecurityError")) {
          fail("SecurityError", "The operation is insecure");
        }
        throw e;
      }
    }

    getExtension(name) {
      if (name === "WEBGL_lose_context") {
        return {
          loseContext: () => {
            this._call("loseContext");
            if (typeof this._canvas.dispatchEvent === "function") {
              const ev = new Event("webglcontextlost", { cancelable: true });
              this._canvas.dispatchEvent(ev);
            }
          },
          restoreContext: () => {
            this._call("restoreContext");
            if (typeof this._canvas.dispatchEvent === "function") {
              const ev = new Event("webglcontextrestored");
              this._canvas.dispatchEvent(ev);
            }
          }
        };
      }
      return null;
    }
  }

  // Define WebGL constants on WebGLRenderingContextBase prototype
  const GL_CONSTS = {
    DEPTH_BUFFER_BIT: GL_DEPTH_BUFFER_BIT,
    STENCIL_BUFFER_BIT: GL_STENCIL_BUFFER_BIT,
    COLOR_BUFFER_BIT: GL_COLOR_BUFFER_BIT,
    POINTS: GL_POINTS,
    LINES: GL_LINES,
    LINE_LOOP: GL_LINE_LOOP,
    LINE_STRIP: GL_LINE_STRIP,
    TRIANGLES: GL_TRIANGLES,
    TRIANGLE_STRIP: GL_TRIANGLE_STRIP,
    TRIANGLE_FAN: GL_TRIANGLE_FAN,
    ZERO: GL_ZERO,
    ONE: GL_ONE,
    SRC_COLOR: GL_SRC_COLOR,
    ONE_MINUS_SRC_COLOR: GL_ONE_MINUS_SRC_COLOR,
    SRC_ALPHA: GL_SRC_ALPHA,
    ONE_MINUS_SRC_ALPHA: GL_ONE_MINUS_SRC_ALPHA,
    DST_ALPHA: GL_DST_ALPHA,
    ONE_MINUS_DST_ALPHA: GL_ONE_MINUS_DST_ALPHA,
    DST_COLOR: GL_DST_COLOR,
    ONE_MINUS_DST_COLOR: GL_ONE_MINUS_DST_COLOR,
    SRC_ALPHA_SATURATE: GL_SRC_ALPHA_SATURATE,
    FUNC_ADD: GL_FUNC_ADD,
    BLEND_EQUATION: GL_BLEND_EQUATION,
    ARRAY_BUFFER: GL_ARRAY_BUFFER,
    ELEMENT_ARRAY_BUFFER: GL_ELEMENT_ARRAY_BUFFER,
    STATIC_DRAW: GL_STATIC_DRAW,
    STREAM_DRAW: GL_STREAM_DRAW,
    DYNAMIC_DRAW: GL_DYNAMIC_DRAW,
    CULL_FACE: GL_CULL_FACE,
    BLEND: GL_BLEND,
    DITHER: GL_DITHER,
    STENCIL_TEST: GL_STENCIL_TEST,
    DEPTH_TEST: GL_DEPTH_TEST,
    SCISSOR_TEST: GL_SCISSOR_TEST,
    NO_ERROR: GL_NO_ERROR,
    INVALID_ENUM: GL_INVALID_ENUM,
    INVALID_VALUE: GL_INVALID_VALUE,
    INVALID_OPERATION: GL_INVALID_OPERATION,
    OUT_OF_MEMORY: GL_OUT_OF_MEMORY,
    CONTEXT_LOST_WEBGL: GL_CONTEXT_LOST_WEBGL,
    BYTE: GL_BYTE,
    UNSIGNED_BYTE: GL_UNSIGNED_BYTE,
    SHORT: GL_SHORT,
    UNSIGNED_SHORT: GL_UNSIGNED_SHORT,
    INT: GL_INT,
    UNSIGNED_INT: GL_UNSIGNED_INT,
    FLOAT: GL_FLOAT,
    RGBA: GL_RGBA,
    RGB: GL_RGB,
    FRAGMENT_SHADER: GL_FRAGMENT_SHADER,
    VERTEX_SHADER: GL_VERTEX_SHADER,
    COMPILE_STATUS: GL_COMPILE_STATUS,
    DELETE_STATUS: GL_DELETE_STATUS,
    LINK_STATUS: GL_LINK_STATUS,
    VALIDATE_STATUS: GL_VALIDATE_STATUS,
    TEXTURE_2D: GL_TEXTURE_2D,
  };

  class WebGLRenderingContext extends WebGLRenderingContextBase {}
  class WebGL2RenderingContext extends WebGLRenderingContextBase {}

  for (const [k, v] of Object.entries(GL_CONSTS)) {
    WebGLRenderingContext[k] = v;
    WebGLRenderingContext.prototype[k] = v;
    WebGL2RenderingContext[k] = v;
    WebGL2RenderingContext.prototype[k] = v;
  }

  class CanvasRenderingContext2DBase {
    constructor(canvas, canvasId, isOffscreen) {
      this._canvas = canvas;
      this._canvasId = canvasId;
      this._isOffscreen = isOffscreen;
      this._fillStyle = "#000000";
      this._strokeStyle = "#000000";
      this._lineWidth = 1;
      this._lineCap = "butt";
      this._lineJoin = "miter";
      this._miterLimit = 10;
      this._lineDashOffset = 0;
      this._globalAlpha = 1.0;
      this._globalCompositeOperation = "source-over";
      this._font = "10px sans-serif";
      this._textAlign = "start";
      this._textBaseline = "alphabetic";
      this._direction = "inherit";
      this._shadowBlur = 0;
      this._shadowColor = "rgba(0, 0, 0, 0)";
      this._shadowOffsetX = 0;
      this._shadowOffsetY = 0;
      this._imageSmoothingEnabled = true;
      this._imageSmoothingQuality = "low";
      this._filter = "none";
    }

    get canvas() { return this._canvas; }

    get fillStyle() { return this._fillStyle; }
    set fillStyle(val) {
      this._fillStyle = val;
      const str = typeof val === "string" ? val : (val?._stops?.[0]?.color || "#000000");
      this._exec("fillStyle", [str]);
    }

    get strokeStyle() { return this._strokeStyle; }
    set strokeStyle(val) {
      this._strokeStyle = val;
      const str = typeof val === "string" ? val : (val?._stops?.[0]?.color || "#000000");
      this._exec("strokeStyle", [str]);
    }

    get lineWidth() { return this._lineWidth; }
    set lineWidth(val) {
      const num = Number(val);
      if (num > 0) {
        this._lineWidth = num;
        this._exec("lineWidth", [num]);
      }
    }

    get lineCap() { return this._lineCap; }
    set lineCap(val) {
      if (["butt", "round", "square"].includes(val)) {
        this._lineCap = val;
        this._exec("lineCap", [val]);
      }
    }

    get lineJoin() { return this._lineJoin; }
    set lineJoin(val) {
      if (["round", "bevel", "miter"].includes(val)) {
        this._lineJoin = val;
        this._exec("lineJoin", [val]);
      }
    }

    get miterLimit() { return this._miterLimit; }
    set miterLimit(val) {
      const num = Number(val);
      if (num > 0) {
        this._miterLimit = num;
        this._exec("miterLimit", [num]);
      }
    }

    get globalAlpha() { return this._globalAlpha; }
    set globalAlpha(val) {
      const num = Number(val);
      if (!Number.isNaN(num) && num >= 0 && num <= 1) {
        this._globalAlpha = num;
        this._exec("globalAlpha", [num]);
      }
    }

    get globalCompositeOperation() { return this._globalCompositeOperation; }
    set globalCompositeOperation(val) {
      this._globalCompositeOperation = String(val);
      this._exec("globalCompositeOperation", [this._globalCompositeOperation]);
    }

    get font() { return this._font; }
    set font(val) {
      this._font = String(val);
      const m = /(\d+(?:\.\d+)?)\s*px\s*(.*)/.exec(this._font);
      const size = m ? parseFloat(m[1]) : 10;
      const family = m && m[2] ? m[2].trim() : "sans-serif";
      this._exec("font", [size, family]);
    }

    get textAlign() { return this._textAlign; }
    set textAlign(val) { this._textAlign = String(val); }

    get textBaseline() { return this._textBaseline; }
    set textBaseline(val) { this._textBaseline = String(val); }

    get direction() { return this._direction; }
    set direction(val) { this._direction = String(val); }

    get imageSmoothingEnabled() { return this._imageSmoothingEnabled; }
    set imageSmoothingEnabled(val) { this._imageSmoothingEnabled = Boolean(val); }

    get imageSmoothingQuality() { return this._imageSmoothingQuality; }
    set imageSmoothingQuality(val) { this._imageSmoothingQuality = String(val); }

    _exec(cmd, args = []) {
      if (this._canvas?._detached) {
        fail("InvalidStateError", "Canvas is detached");
      }
      const result = host("canvasExecute", String(this._canvasId), cmd, JSON.stringify(args));
      drew(this._canvas);
      return result;
    }

    save() { this._exec("save"); }
    restore() { this._exec("restore"); }
    reset() {
      this._exec("reset");
      this._fillStyle = "#000000";
      this._strokeStyle = "#000000";
      this._lineWidth = 1;
      this._lineCap = "butt";
      this._lineJoin = "miter";
      this._miterLimit = 10;
      this._globalAlpha = 1.0;
      this._globalCompositeOperation = "source-over";
      this._font = "10px sans-serif";
    }

    scale(x, y) { this._exec("scale", [Number(x) || 0, Number(y) || 0]); }
    rotate(angle) { this._exec("rotate", [Number(angle) || 0]); }
    translate(x, y) { this._exec("translate", [Number(x) || 0, Number(y) || 0]); }
    transform(a, b, c, d, e, f) {
      this._exec("transform", [Number(a) || 0, Number(b) || 0, Number(c) || 0, Number(d) || 0, Number(e) || 0, Number(f) || 0]);
    }
    setTransform(a, b, c, d, e, f) {
      if (typeof a === "object" && a !== null) {
        this.setTransform(a.a, a.b, a.c, a.d, a.e, a.f);
        return;
      }
      this._exec("setTransform", [Number(a) || 0, Number(b) || 0, Number(c) || 0, Number(d) || 0, Number(e) || 0, Number(f) || 0]);
    }
    resetTransform() { this._exec("resetTransform"); }
    getTransform() {
      return { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0, m11: 1, m12: 0, m21: 0, m22: 1, m41: 0, m42: 0 };
    }

    clearRect(x, y, w, h) { this._exec("clearRect", [Number(x) || 0, Number(y) || 0, Number(w) || 0, Number(h) || 0]); }
    fillRect(x, y, w, h) { this._exec("fillRect", [Number(x) || 0, Number(y) || 0, Number(w) || 0, Number(h) || 0]); }
    strokeRect(x, y, w, h) { this._exec("strokeRect", [Number(x) || 0, Number(y) || 0, Number(w) || 0, Number(h) || 0]); }

    beginPath() { this._exec("beginPath"); }
    closePath() { this._exec("closePath"); }
    moveTo(x, y) { this._exec("moveTo", [Number(x) || 0, Number(y) || 0]); }
    lineTo(x, y) { this._exec("lineTo", [Number(x) || 0, Number(y) || 0]); }
    quadraticCurveTo(cpx, cpy, x, y) { this._exec("quadraticCurveTo", [Number(cpx) || 0, Number(cpy) || 0, Number(x) || 0, Number(y) || 0]); }
    bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y) {
      this._exec("bezierCurveTo", [Number(cp1x) || 0, Number(cp1y) || 0, Number(cp2x) || 0, Number(cp2y) || 0, Number(x) || 0, Number(y) || 0]);
    }
    rect(x, y, w, h) { this._exec("rect", [Number(x) || 0, Number(y) || 0, Number(w) || 0, Number(h) || 0]); }
    arc(x, y, radius, startAngle, endAngle, counterclockwise = false) {
      this._exec("arc", [Number(x) || 0, Number(y) || 0, Number(radius) || 0, Number(startAngle) || 0, Number(endAngle) || 0, Boolean(counterclockwise)]);
    }
    ellipse(x, y, rx, ry, rotation, startAngle, endAngle, counterclockwise = false) {
      this._exec("ellipse", [Number(x) || 0, Number(y) || 0, Number(rx) || 0, Number(ry) || 0, Number(rotation) || 0, Number(startAngle) || 0, Number(endAngle) || 0, Boolean(counterclockwise)]);
    }

    fill(pathOrRule, rule) {
      if (pathOrRule instanceof Path2D) {
        for (const op of pathOrRule._ops) {
          this._exec(op[0], op.slice(1));
        }
      }
      this._exec("fill");
    }

    stroke(path) {
      if (path instanceof Path2D) {
        for (const op of path._ops) {
          this._exec(op[0], op.slice(1));
        }
      }
      this._exec("stroke");
    }

    clip(pathOrRule, rule) {
      if (pathOrRule instanceof Path2D) {
        for (const op of pathOrRule._ops) {
          this._exec(op[0], op.slice(1));
        }
      }
      this._exec("clip");
    }

    isPointInPath(x, y) {
      return Boolean(this._exec("isPointInPath", [Number(x) || 0, Number(y) || 0]));
    }
    isPointInStroke(x, y) {
      return false;
    }

    fillText(text, x, y, maxWidth) {
      this._exec("fillText", [String(text ?? ""), Number(x) || 0, Number(y) || 0, maxWidth !== undefined ? Number(maxWidth) : null]);
    }
    strokeText(text, x, y, maxWidth) {
      this._exec("strokeText", [String(text ?? ""), Number(x) || 0, Number(y) || 0, maxWidth !== undefined ? Number(maxWidth) : null]);
    }
    measureText(text) {
      const res = host("canvasMeasureText", String(this._canvasId), String(text ?? ""));
      return {
        width: res.width,
        actualBoundingBoxLeft: 0,
        actualBoundingBoxRight: res.width,
        fontBoundingBoxAscent: res.fontBoundingBoxAscent,
        fontBoundingBoxDescent: res.fontBoundingBoxDescent,
        actualBoundingBoxAscent: res.actualBoundingBoxAscent,
        actualBoundingBoxDescent: res.actualBoundingBoxDescent,
        emHeightAscent: res.fontBoundingBoxAscent,
        emHeightDescent: res.fontBoundingBoxDescent,
        hangingBaseline: res.fontBoundingBoxAscent * 0.8,
        alphabeticBaseline: 0,
        ideographicBaseline: -res.fontBoundingBoxDescent,
      };
    }

    createImageData(sw, sh) {
      return new ImageData(sw, sh);
    }

    getImageData(sx, sy, sw, sh) {
      if (this._canvas?._detached) fail("InvalidStateError", "Canvas is detached");
      try {
        const coords = JSON.stringify([sx >>> 0, sy >>> 0, sw >>> 0, sh >>> 0]);
        const pixels = host("canvasGetImageData", String(this._canvasId), coords);
        return new ImageData(new Uint8ClampedArray(pixels), sw >>> 0, sh >>> 0);
      } catch (e) {
        if (String(e.message || e).includes("SecurityError")) {
          fail("SecurityError", "The operation is insecure");
        }
        throw e;
      }
    }

    putImageData(imageData, dx, dy) {
      if (this._canvas?._detached) fail("InvalidStateError", "Canvas is detached");
      host("canvasPutImageData", String(this._canvasId), JSON.stringify({
        data: Array.from(imageData.data),
        dx: dx >>> 0,
        dy: dy >>> 0,
        w: imageData.width,
        h: imageData.height,
      }));
      drew(this._canvas);
    }

    drawImage(image, ...args) {
      if (!image) throw new TypeError("Image must be provided");
      if (isCanvasElement(image) || image instanceof OffscreenCanvas) {
        if (!image._canvasId) {
          image._canvasId = host("canvasCreate", String(image.width), String(image.height), image instanceof OffscreenCanvas ? "true" : "false");
        }
        host("canvasDrawImage", String(this._canvasId), String(image._canvasId), "clean");
      } else if (isImageElement(image)) {
        const crossOrigin = image.crossOrigin;
        const isClean = crossOrigin === "anonymous" || crossOrigin === "use-credentials" || !image.src?.startsWith("http");
        if (!isClean) {
          host("canvasTaint", String(this._canvasId));
        }
      }
    }

    createLinearGradient(x0, y0, x1, y1) {
      return new CanvasGradient("linear", { x0, y0, x1, y1 });
    }
    createRadialGradient(x0, y0, r0, x1, y1, r1) {
      return new CanvasGradient("radial", { x0, y0, r0, x1, y1, r1 });
    }
    createConicGradient(startAngle, x, y) {
      return new CanvasGradient("conic", { startAngle, x, y });
    }
    createPattern(image, repetition) {
      return new CanvasPattern(image, repetition);
    }
    setLineDash(segments) {}
    getLineDash() { return []; }
  }

  class CanvasRenderingContext2D extends CanvasRenderingContext2DBase {}
  class OffscreenCanvasRenderingContext2D extends CanvasRenderingContext2DBase {}

  class OffscreenCanvas {
    constructor(width, height, existingId = null) {
      this._width = Math.max(1, width >>> 0);
      this._height = Math.max(1, height >>> 0);
      this._detached = false;
      this._canvasId = existingId ?? host("canvasCreate", String(this._width), String(this._height), "true");
      this._context2d = null;
    }
    get width() { return this._width; }
    set width(val) {
      if (this._detached) fail("InvalidStateError", "Canvas is detached");
      this._width = Math.max(1, val >>> 0);
      host("canvasResize", String(this._canvasId), String(this._width), String(this._height));
      this._context2d?.reset();
    }
    get height() { return this._height; }
    set height(val) {
      if (this._detached) fail("InvalidStateError", "Canvas is detached");
      this._height = Math.max(1, val >>> 0);
      host("canvasResize", String(this._canvasId), String(this._width), String(this._height));
      this._context2d?.reset();
    }
    getContext(type) {
      if (this._detached) fail("InvalidStateError", "Canvas is detached");
      if (type === "2d") {
        if (this._webglCtx) return null;
        if (!this._context2d) {
          this._context2d = new OffscreenCanvasRenderingContext2D(this, this._canvasId, true);
        }
        return this._context2d;
      }
      if (type === "webgl" || type === "experimental-webgl") {
        if (this._context2d) return null;
        if (!this._webglId) {
          this._webglId = host("webglCreate", String(this.width), String(this.height), "webgl");
        }
        if (!this._webglCtx) {
          this._webglCtx = new WebGLRenderingContext(this, this._webglId, false);
        }
        return this._webglCtx;
      }
      if (type === "webgl2") {
        if (this._context2d) return null;
        if (!this._webglId) {
          this._webglId = host("webglCreate", String(this.width), String(this.height), "webgl2");
        }
        if (!this._webglCtx) {
          this._webglCtx = new WebGL2RenderingContext(this, this._webglId, true);
        }
        return this._webglCtx;
      }
      return null;
    }
    convertToBlob(options = {}) {
      if (this._detached) return Promise.reject(new DOMException("Canvas is detached", "InvalidStateError"));
      try {
        const type = options.type || "image/png";
        const bytes = host("canvasToBlob", String(this._canvasId), type);
        const blob = new Blob([new Uint8Array(bytes)], { type });
        return Promise.resolve(blob);
      } catch (e) {
        if (String(e.message || e).includes("SecurityError")) {
          return Promise.reject(new DOMException("The operation is insecure", "SecurityError"));
        }
        return Promise.reject(e);
      }
    }
    transfer() {
      this._detached = true;
      return { canvasId: this._canvasId, width: this._width, height: this._height };
    }
  }

  return { CanvasGradient, CanvasPattern, ImageData, Path2D, WebGLObject, WebGLBuffer, WebGLShader, WebGLProgram, WebGLTexture, WebGLFramebuffer, WebGLRenderbuffer, WebGLUniformLocation, WebGLRenderingContextBase, WebGLRenderingContext, WebGL2RenderingContext, CanvasRenderingContext2DBase, CanvasRenderingContext2D, OffscreenCanvasRenderingContext2D, OffscreenCanvas };
  },
});
