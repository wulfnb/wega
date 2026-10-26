// Typed native operations; no JSON serialization crosses the V8/DOM boundary.
((ops) => {
  Object.defineProperty(globalThis, "__wega_evaluate", {
    value: (source) =>
      ops.op_result(JSON.stringify((0, eval)(source)) ?? "null"),
  });
  // Run a classic script synchronously in the global scope (inserted inline
  // scripts). Unlike eval, this is a real script and is not subject to CSP eval.
  Object.defineProperty(globalThis, "__wega_run_script", {
    value: (source, name) => {
      const [, error] = ops.op_eval_context(String(source), String(name));
      if (error) throw error.thrown;
    },
  });
  Object.defineProperty(globalThis, "__wega_call", {
    value: (op, a = "", b = "", c = "", d = "") => {
      if (op === "setAttrNS")
        return ops.op_dom_set_attr_ns(
          String(a),
          String(b),
          String(c),
          String(d),
        );
      if (op === "setText") return ops.op_dom_set_text(String(a), String(b));
      if (op === "setAttr")
        return ops.op_dom_set_attr(String(a), String(b), String(c));
      return ops.op_dom_read(String(op), String(a), String(b), String(c));
    },
  });
})(Deno.core.ops);

// Page scripts must never access core ops (including stdout, which carries IPC).
delete globalThis.Deno;
delete globalThis.__bootstrap;
