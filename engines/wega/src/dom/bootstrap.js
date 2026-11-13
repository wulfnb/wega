// Factories are eagerly compiled in build.rs; profile/origin/DOM state is created here.
for (const name of ["web", "bridge", "canvas", "semantics", "observers", "install", "cache", "bindings", "generated", "xhr", "forms"]) {
  const key = "__wega_bootstrap_" + name;
  globalThis[key]();
  delete globalThis[key];
}

// Native DOM dispatch and Deno's mutable event internals are bootstrap-only.
// The installed bindings hold private closures rather than page-visible handles.
delete globalThis.__wega_call;
delete globalThis.__wega_web;
delete globalThis.__wega_install_web;

delete globalThis.__wega_canvas;
delete globalThis.__wega_bootstrap_worker;
delete globalThis.__wega_dom;
delete globalThis.__wega_semantics;
delete globalThis.__wega_guard_binding;
