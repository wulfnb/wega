# css/specified

**Status: IMPLEMENTED** — crate `wega-css-specified`

## Purpose

CSS specified values as the CSSOM serializes them: validation plus Stylo's specified-value
serialization, so `element.style` and `cssText` read back exactly as before Stylo is removed.

## Responsibilities

- One parser per value type, by family (`length`, `number`, `color`, `font`, `text`,
  `background`, `align`, `box_values`, `easing`, `flags`, `grid`, `names`, `position`, `svg`)
- [`Specified`]: a serialized value, invalid, or not ported yet (the caller falls back)

## Expected dependencies

`cssparser` only. `wega-css-parser` maps each longhand to these parsers.

## Dependencies to avoid

`dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Stylo.

## Current status

Ported from Stylo `values/specified/*` and `values/generics/*` (MPL-2.0); each module names
its source. `wega-engine`'s `native_values_serialize_as_stylo_does` test checks every ported
longhand against Stylo. See [`css-engine-modularization-plan.md`](../../../docs/architecture/css-engine-modularization-plan.md).
