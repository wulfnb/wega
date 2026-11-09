# Step 13 Contract: Close Rendering and Scrolling Gaps with Reference Tests

## 1. Overview & Objectives

Step 13 closes rendering, layout, and scrolling gaps across the standalone Wega engine by validating CSS/SVG/text/layout features against reference tests per the standalone execution plan:
- Audited supported layout families: Flexbox, Grid track sizing, gap properties, Table layout with `border-collapse`.
- Bidirectional text (RTL/LTR) shaping, computed direction, and bounding metrics for Arabic and Hebrew text.
- Scrolled and zoomed viewports, HiDPI scaling, and transformed pointer hit testing.
- SVG root element `viewBox` scaling, path rasterization, and pixel buffer paint validation.
- Reference tests and WPT regression suites validating layout consistency, style cascade, and rendering fidelity.

## 2. Test Suites & Coverage Additions

### Reference Test Suite (`engines/wega/tests/css.rs`)
1. **Flexbox & Grid Track Sizing with Gap (`flexbox_and_grid_track_sizing_with_gap`):**
   - Flex container with `display: flex; gap: 10px; width: 210px` distributing two equal `flex: 1` items (each 100px wide, spaced by 10px gap).
   - CSS Grid container with `display: grid; grid-template-columns: 100px 1fr; gap: 20px; width: 320px` correctly resolving explicit 100px tracks, 20px column gaps, and flexible 200px `1fr` tracks.
2. **Table Layout Cell & Border Collapse (`table_layout_cell_and_border_collapse`):**
   - Tables with `border-collapse: collapse; width: 200px` and cell borders.
   - Validates collapsed border distribution across adjacent table cells and equal cell dimension calculations.
3. **Bidirectional Text Layout (`bidirectional_text_arabic_hebrew_layout`):**
   - Computed `direction` resolution for `dir="rtl"` and `dir="ltr"`.
   - Complex RTL Arabic script shaping ("مرحبا بالعالم") with verified non-zero bounding metrics.
4. **Scrolled & Zoomed Viewport Layout and Hit Testing (`scrolled_zoomed_viewport_layout_and_hit_testing`):**
   - Fractional zoom levels and display scales (zoom 1.5, scale 2.0).
   - Layout geometry invariance under zoom (`getBoundingClientRect().width` in CSS pixels).
   - `view.scroll(0, 50)` offset management and pointer event hit testing triggering repaint flags.
5. **SVG ViewBox & Path Scaling (`svg_viewbox_and_path_scaling`):**
   - `<svg viewBox='0 0 50 50' style='width:150px;height:150px'>` geometry resolution.
   - Skia rasterization of vector circle elements producing non-empty RGBA pixel buffers.
6. **Prior CSS Suite Coverage (all 19 tests passing):**
   - Inline size container queries (`@container (min-width: ...)`).
   - Text color cascade (`color-scheme` root colors vs author colors).
   - HiDPI viewport units (`vh`, `vw` as CSS pixels).
   - Container scroll vs viewport scroll isolation (`overflow: auto`).
   - Sticky positioning edge clamping without accumulation.
   - Floats and clear layouts.
   - Line clamp (`-webkit-line-clamp`) and text ellipsis (`text-overflow: ellipsis`).
   - Skia color filters, blend modes, backdrop filters, and text shadows.
   - CSS transitions with active frame scheduling.

### WPT CSS Suite (`engines/wega/tests/css_wpt.rs`)
- `pinned_css_overflow_clip_reftest_matches_hidden_reference` — PASSED.
- `pinned_css_wpt_subset_has_no_regressions` — PASSED.

## 3. Verification & Evidence

- `cargo test --test css -p wega-engine --manifest-path engines/Cargo.toml`
  - Output: `19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.16s`
- `cargo test --test css_wpt -p wega-engine --manifest-path engines/Cargo.toml`
  - Output: `2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.01s`
- `cargo check --bin wega`
  - Output: Compiled cleanly with zero errors.
