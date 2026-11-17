# Wega Native Rendering Engine Design

## 1. Architectural Philosophy & Constraints

1. **Authoritative DOM**: Wega DOM (`wega-dom` / `DocumentView` / `NodeView`) is the sole authoritative DOM. The native engine does not duplicate DOM state or create secondary node identity graphs.
2. **Dual-Backend Support**: During migration, `RenderingBackend` allows seamless switching between `Blitz` (fallback/differential baseline) and `WegaNative` (native Wega CSS, Layout, and Display List).
3. **Graphics Output**: Translates native layout fragments into a Wega-owned display list, which maps to `anyrender::Scene` for rasterization by `wega-adapter-skia` via Skia.
4. **JavaScript & Browser Host**: V8 JavaScript runtime, browser shell, DevTools protocol, and iframe security architecture remain untouched.

## 2. Core Modules

### 2.1 CSS Engine (`native_engine/css.rs`)
- **Rule Representation**: `Rule`, `Selector`, `Declaration`, `Property`, `Value`.
- **Selectors**: Universal (`*`), Type (`div`, `p`), Class (`.btn`), ID (`#main`), Attribute (`[attr]`, `[attr=val]`), Child (`>`), Descendant (` `).
- **Specificity**: Standard 3-tuple `(id, class, type)` calculation and sorting.
- **Cascade**: User-agent base stylesheet + `<style>` element author rules + inline `style="..."` attributes + `!important` sorting + source order.
- **Inheritance**: Propagation of inheritable properties (`color`, `font-family`, `font-size`, `line-height`, `text-align`, `visibility`) down the DOM tree.
- **Computed Styles**: Strongly-typed `ComputedStyle` with layout dimensions, margins, paddings, borders, colors, and display models.

### 2.2 Layout Engine (`native_engine/layout.rs`)
- **Formatting Contexts**:
  - Block Formatting Context (BFC): Stacking, margin collapsing, width/height auto sizing, box-sizing.
  - Inline Formatting Context (IFC): Text measurement, word wrapping, line box formation, alignment.
  - Flexbox: Flex containers, flex-direction (row/column), justify-content, align-items, flex-grow/shrink/basis.
  - Positioning: Relative insets, absolute containing blocks, fixed viewport anchors.
  - Grid: Track sizing, grid-template rows/columns, grid placement.
  - Tables: Table rows, cells, basic tabular layout.
- **Box Model**: Margin box, border box, padding box, content box in CSS pixels.
- **Overflow & Scrolling**: Scroll size calculation, scroll offsets, clipping boundaries.

### 2.3 Display List & Painting (`native_engine/display_list.rs`)
- **Display Items**: Rectangles, borders, text runs, clips, transforms, opacity layers, nested iframe scenes.
- **Stacking Contexts**: Tree traversal adhering to CSS stacking context order (backgrounds/borders -> negative z-index -> block children -> float/inline children -> zero z-index -> positive z-index).
- **Scene Conversion**: Conversion of display list into `anyrender::Scene` for rasterization via `wega-adapter-skia`.

### 2.4 Browser & DevTools Integration (`native_engine/backend.rs`)
- **Backend Selector**: `RenderingBackend::Blitz` vs `RenderingBackend::WegaNative`.
- **Trait Implementation**: Provides `LayoutControl`, `GeometryAccess`, `HitTest`, `PaintSource`, and `StyleAccess` using native layout boxes.
- **Hit Testing**: Z-ordered element-at-point resolution for pointer events.
- **DevTools**: Box model inspection, geometry reporting, and hover highlighting without triggering layout hangs.
