# Offline framework acceptance fixtures

Vendored bundles built from npm packages with esbuild 0.25.10, minified in production mode,
IIFE format, ES2022 target. They do not download any code at test time.

- jQuery 3.7.1
- jQuery 3.6.3's unmodified browser bundle (`jquery-3.6.3.min.js`), downloaded
  from `https://code.jquery.com/jquery-3.6.3.min.js` on 27 October 2026 to cover
  trak.in's actual browser script. MIT license notice is retained in the file;
  the full jQuery MIT license is already included alongside the other fixtures.
  SHA-256: `a6f3f0faea4b3d48e03176341bef0ed3151ffbf226d4c6635f1c6039c0500575`.
- React and ReactDOM 18.3.1
- Vue 3.5.22
- Preact 10.27.2
- Angular core/common/compiler/platform-browser 20.3.0, RxJS 7.8.2

All packages retain their MIT license files alongside the bundles. Legal notices
emitted by esbuild are preserved in the bundles. Angular uses its actual standalone
component runtime and compiler; this is not AngularJS.

Each HTML page mounts a counter, and the View test dispatches a click and checks its
rendered DOM text. The SVG fixture verifies actual raster pixels after mutation.

Rebuild: `npm ci --ignore-scripts && npm run build` in this directory. The sources
and dependency lockfile are included; node_modules is never committed.
