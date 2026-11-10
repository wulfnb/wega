# Step 19 Contract: WebGL and Resilient Composition

## 1. Overview & Objectives

Step 19 delivers hardware/software accelerated WebGL rendering and resilient viewport composition for Wega:
- **WebGL 1.0 & WebGL 2.0 Subsystem** (`engines/wega/src/webgl/`):
  - Constants & state enumeration: Complete OpenGL ES 2.0 / 3.0 WebGL tokens (buffers, shaders, programs, textures, framebuffers, renderbuffers, blending, depth/stencil tests, formats, errors).
  - WebGL resource management: Buffers (`WebGLBuffer`), shaders (`WebGLShader` with source code reflection), programs (`WebGLProgram` with shader linking, uniform & attribute introspection), textures (`WebGLTexture`), framebuffers (`WebGLFramebuffer`), and renderbuffers (`WebGLRenderbuffer`).
  - WebGL state machine (`WebGLContext`):
    - Drawing buffer allocation and viewport/scissor state.
    - Clear color/depth/stencil operations (`clearColor`, `clearDepth`, `clearStencil`, `clear`).
    - Capability flags (`enable`, `disable`, `isEnabled`).
    - Buffer uploads (`bufferData`) and vertex attribute pointers (`vertexAttribPointer`, `enableVertexAttribArray`).
    - Shader compilation, program linking, and uniform uploads (`uniform1f`..`4f`, `uniform1i`, `uniformMatrix2fv`..`4fv`).
    - Draw calls (`drawArrays`, `drawElements`).
    - Texture uploads (`texImage2D`) with cross-origin taint propagation.
    - Pixel readback (`readPixels`) with strict origin security checks throwing `SecurityError` on tainted buffers.
    - Data serialization (`to_data_url`) converting WebGL drawing buffer contents to standard PNG data URIs.
    - Context loss and restoration via the `WEBGL_lose_context` extension (`loseContext`, `restoreContext`).
  - WebGL engine & memory accounting (`WebGLEngine`):
    - Aggregate WebGL pixel memory allocation tracking against engine limits (`Limits::max_pixels * 8`).
    - Rejects allocations exceeding quotas with `QuotaExceededError`.
    - Automatically reclaims allocated quota when contexts are destroyed.
- **Resilient Viewport Composition & Screenshots** (`engines/wega/src/view.rs`):
  - `View::screenshot(width, height, scale, zoom, rect)`: resilient rasterization compositing DOM and canvas contents, applying device scale factor, CSS zoom, and clipping to requested subregions.
- **JavaScript & DOM Semantics** (`engines/wega/src/dom/semantics.js`):
  - `WebGLRenderingContextBase`, `WebGLRenderingContext`, `WebGL2RenderingContext`.
  - Extension support including `getExtension("WEBGL_lose_context")`.
  - Canvas context isolation: strictly blocks requesting a 2D context on a canvas that already initialized a WebGL context (and vice versa).
  - Origin cleanliness tracking on `texImage2D` when loading cross-origin HTMLImageElement resources.

## 2. Architecture & Implementation

### Module Layout (`engines/wega/src/webgl/`)
- `constants.rs`:
  - GLenum definitions covering standard WebGL 1.0 and 2.0 constants.
- `resources.rs`:
  - Resource representations for buffers, shaders, programs, textures, framebuffers, and renderbuffers.
  - GLSL reflection parsing uniforms, types, and attribute locations.
- `context.rs`:
  - `WebGLContext`: encapsulates drawing buffer, clear states, viewports, bound resources, uniforms, attributes, origin cleanliness, and lost context flags.
- `engine.rs`:
  - `WebGLEngine`: manages context lifecycle by `ContextId`, enforces aggregate memory quotas, and dispatches JSON operations from the host bridge.
- `mod.rs`:
  - Re-exports module types. Exposed as `pub mod webgl;` in `engines/wega/src/lib.rs`.

### DOM & Host Bridge Integration
- `Dom` (`engines/wega/src/dom.rs`):
  - Added `pub webgl_engine: RefCell<WebGLEngine>` to DOM environment.
  - Registered host handlers: `webglCreate`, `webglResize`, `webglDestroy`, `webglGetMemory`, and `webglOp` (dispatching operations).
- `semantics.js`:
  - Standard DOM classes: `WebGLRenderingContextBase`, `WebGLRenderingContext`, `WebGL2RenderingContext`, `WebGLBuffer`, `WebGLShader`, `WebGLProgram`, `WebGLTexture`, `WebGLFramebuffer`, `WebGLRenderbuffer`, `WebGLUniformLocation`.
  - Extension registration and error handling (`fail("SecurityError", ...)`).
- `view.rs`:
  - Added `View::screenshot(width, height, scale, zoom, rect)` to capture viewport screenshots with scaling and subregion cropping.

## 3. Verification & Evidence

- Integration test suite `engines/wega/tests/webgl.rs`:
  - `webgl_context_creation_and_attributes` — PASSED
  - `webgl_memory_accounting_and_quota_enforcement` — PASSED
  - `webgl_cross_origin_texture_taints_and_blocks_readback` — PASSED
  - `webgl_context_loss_and_restoration_extension` — PASSED
  - `webgl_shaders_program_linking_and_introspection` — PASSED
  - `webgl_buffer_upload_and_draw_arrays` — PASSED
  - `webgl_clear_color_and_pixel_readback` — PASSED
  - `view_resilient_screenshot_composition_and_zoom` — PASSED
- `cargo test --manifest-path engines/Cargo.toml --lib -p wega-engine` — 29 unit tests PASSED.
- `cargo check --bin wega` — PASSED with 0 errors (Servo compatibility intact).
