// Factories are eagerly compiled in build.rs; profile/origin/DOM state is created here.
for (const name of ["web", "bridge", "bindings", "observers", "install", "xhr"]) {
  const key = "__wega_bootstrap_" + name;
  globalThis[key]();
  delete globalThis[key];
}
