# Step 15 Contract: Implement Custom Elements and Shadow DOM

## 1. Overview & Objectives

Step 15 delivers the document-owned custom element and shadow DOM architecture for the standalone Wega engine per the execution plan:
- Rust-owned custom element registry (`Registry` in `engines/wega/src/components.rs`) managing definitions, constructors, upgrade lifecycle states (`Constructing`, `Custom`, `Failed`), and reaction queues.
- Standard custom element lifecycle callbacks (`connectedCallback`, `disconnectedCallback`, `adoptedCallback`, and `attributeChangedCallback` observing static `observedAttributes`).
- Rust-owned shadow root tree (`ShadowTree` in `engines/wega/src/components.rs`) supporting `open` and `closed` encapsulation modes, valid host element enforcement, and `delegatesFocus`.
- `<template>` element stamping with `template.content.cloneNode(true)` and `<slot>` distribution with `assignedNodes()` and `assignedElements()`.
- Shadow boundary event retargeting: `composed: true` events bubbling across boundaries with retargeted `target`, while non-composed events remain contained within the shadow root.

## 2. Architecture & Implementation

### Rust Registry & Shadow Tree (`engines/wega/src/components.rs`, `dom.rs`)
- `Registry`:
  - PCEN name validation (`valid_name`) enforcing ASCII lowercase initial characters, hyphen requirement, valid unicode character ranges, and exclusion of reserved names.
  - Constructor collision checks and maximum definition capacity limits.
  - State tracking per node (`Constructing`, `Custom`, `Failed`), preventing reentrancy or upgrade retries for failed constructors.
  - Bounded reaction queue with `drain()` dispatching lifecycle reactions in deterministic order.
- `ShadowTree`:
  - Validates shadow host tags (supports HTML content elements and custom elements; rejects void/replaced elements like `input`).
  - Open vs closed encapsulation: open roots reflect `element.shadowRoot`, closed roots return `null` externally.
  - `delegates_focus` attribute tracking and reverse host-to-root link management.

### DOM & Event Runtime (`engines/wega/src/dom/semantics.js`)
- `customElements.define`, `get`, `getName`, `upgrade`, and `whenDefined`.
- `HTMLElement.prototype.attachShadow` and `shadowRoot` reflection wired to Rust `component("shadowAttach")` and `component("shadowRoot")`.
- `HTMLTemplateElement.prototype.content` returning cloned document fragment.
- `HTMLSlotElement.prototype.assignedNodes` and `assignedElements` returning slotted nodes.
- Pinned Deno event dispatcher adapted for native shadow parent traversal, slot assignment, and composed event path calculation.

## 3. Verification & Evidence

- Unit tests in `engines/wega/tests/custom_elements.rs`:
  - `custom_element_definition_and_instantiation` — PASSED
  - `custom_element_lifecycle_connected_disconnected_and_attribute_changed` — PASSED
  - `custom_element_when_defined_and_late_upgrade` — PASSED
  - `shadow_root_open_and_closed_encapsulation` — PASSED
  - `shadow_root_slot_assignment_named_default_and_flatten` — PASSED
  - `event_retargeting_and_composed_path_across_shadow_boundary` — PASSED
  - `security_and_spec_conformance_fail_closed` — PASSED
  - `disconnected_upgrade_failure_and_definition_callbacks_are_stable` — PASSED
  - `invalid_registration_does_not_read_user_callbacks_and_closed_paths_hide_internals` — PASSED
  - `component_library_template_cloning_and_nested_shadow_dom` — PASSED
  - `custom_element_observed_attributes_and_property_reflection` — PASSED
  - `delegates_focus_and_event_composed_path_retargeting` — PASSED
  - Total: 12/12 custom element tests passing.
- Protocol regression suite:
  - 33/33 tests passing.
- Browser host check:
  - `cargo check --bin wega`: compiled cleanly with zero errors.
