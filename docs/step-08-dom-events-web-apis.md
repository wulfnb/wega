# Step 08: Expand DOM, events and browser Web APIs by audited families

Branch: `feature/standalone-wega-engine`. Starts after Step 07 closes (generator + character-data family generated and checked in). This document records the subtask split, construct policy extensions per subtask and the migration order. Completion evidence per subtask is recorded in `docs/validation/standalone/progress.json`.

## Scope

Move **Node, Element, Document, collections, attributes, selectors, mutation, events, observers, location/navigation** and core **HTML element families** through the generated layer in small subtasks. Preserve existing Range/fragment/live-collection work. Complete event paths, capture/bubble/default actions and exception behavior.

The generator (from 07.a) currently admits a limited subset. Each subtask extends the generator only as far as the target family requires, then migrates that family, then closes out.

## 07.a baseline (generator — current state)

Admitted in pinned inputs:
- `interface` with inheritance, `[Exposed=Window]`, `attribute`/`readonly attribute` with `[Pure]`, `[Throws]`, `[NewObject]`, `[CEReactions]`, `[LegacyNullToEmptyString]`, `constructor` with `optional` args and defaults, operations, `includes` clauses
- Types: `DOMString`, `DOMString?`, `boolean`, `unsigned long`, `unsigned short`, `float`, `double`, `undefined`, interface identifiers
- Rejected (positioned failure): union types, variadic args, `any`, `const` members, `stringifier`, `iterable`, `getter`/`setter`/`deleter`, `[SameObject]`, `[Constant]`, dictionaries, `[Exposed=Worker]`, `callback`/`enum`/`typedef`, parameter extended attrs

## Subtask split

### 08.a — Generator: const members, [Constant], [SameObject], [SetterThrows]

**Depends on:** 07.a (complete).

Extend the parser and emitter for the constructs needed by Event, Attr and Node:
- `const unsigned short NODE_TYPE = 1;` → emit as static properties on the class and prototype
- `[Constant]` extended attribute → emit getter with `writable: false`
- `[SameObject]` extended attribute → emit a cached getter (JS-level `Object.defineProperty` with `get` that memoises on `this`)
- `[SetterThrows]` on attributes → document as a hook annotation (same as `[Throws]` but for setters)

**Target families:** Event (const members), Attr ([Constant]), Node ([SameObject] for `childNodes`, [Constant] for `nodeType`).

**Evidence:** New test cases in `tools/webidl/main.rs` (one per construct) verify parsing and deterministic emission; run `cargo test --bin wega-webidl`.

**IDL construct policy:** `const` members become `Object.defineProperty(Class, "CONST_NAME", { value, writable: false, enumerable: true, configurable: true })` on both class and prototype. `[Constant]` becomes `{ get, writable: false, enumerable: true, configurable: false }`. `[SameObject]` becomes `{ get, enumerable: true, configurable: true }` with a private `Symbol`-keyed cache field per instance. `[SetterThrows]` is treated as a hook annotation like `[Throws]`.

---

### 08.b — Migrate Event family (Event, EventTarget + EventInit)

**Depends on:** 08.a.

Pins `Event.webidl`, `EventTarget.webidl`, `EventHandler.webidl`, `EventListener.webidl` (interfaces only; dictionaries stay as JS objects or are handled by a later step). EventTarget is trivial (3 methods). Event needs:
- Constructor with optional dictionary arg (handled after dictionary support, or keep handwritten for now)
- Primitive attributes: `type`, `bubbles`, `cancelable`, `composed`, `isTrusted`, `eventPhase`, `defaultPrevented`, `timeStamp`
- Operations: `stopPropagation`, `stopImmediatePropagation`, `preventDefault`, `initEvent`, `composedPath` (returns `sequence<EventTarget>` — requires sequence support, defer or emit stub)
- Const members: `NONE`, `CAPTURING_PHASE`, `AT_TARGET`, `BUBBLING_PHASE`

**Approach:** Migrate EventTarget first (purely primitive operations, no generator extensions beyond 08.a). Then migrate Event with const members generated but delegate dictionary argument and sequence return to JS-layer stubs.

**Hooks:** `eventTarget.*`, `event.*` (20+ hooks covering constructor, getters, setters, operations).

**Evidence:** Pinned inputs parse, generator output matches, hook manifest covers all members.

---

### 08.c — Generator: dictionaries as parameters, enums, typedef

**Depends on:** 08.b.

- **Dictionaries as parameters** → emit a validation/conversion function that maps JS object properties to WebIDL types, with defaults
- **`enum` declarations** → emit as `Object.freeze({ VALUE1: "VALUE1", ... })` on the class
- **`typedef` declarations** → document (no JS emit needed; the type is just the underlying type)

**Target families:** Event (EventInit dictionary), Document (many dictionaries like ElementCreationOptions, ImportNodeOptions, ScrollToOptions, etc.).

**Evidence:** New test cases; run `cargo test --bin wega-webidl`.

---

### 08.d — Migrate Attr family

**Depends on:** 08.a (const/constant/SameObject/SetterThrows already covered).

`Attr.webidl` is a clean, small family (~25 lines):
- `[Constant] readonly attribute` (namespaceURI, prefix, localName, name, specified)
- `[CEReactions, Pure, SetterThrows] attribute DOMString value`
- `[Pure] readonly attribute Element? ownerElement`

No unions, no variadic, no interface-typed parameters beyond `ownerElement` (getter returns interface — handled by existing generator restriction: attribute returning interface is rejected, but `ownerElement` getter can be handled as a special case or deferred).

Actually, `ownerElement` returns `Element?` — an interface type. Our generator rejects interface-typed attributes. This needs either a generator extension or we keep `ownerElement` as a handwritten getter.

**Decision:** Defer interface-typed attributes to 08.f. Keep `ownerElement` as a handwritten accessor in the generated class (generator omits it, JS file adds it). This keeps the subtask focused.

**Hooks:** `attr.*` (~7 hooks).

**Evidence:** Generator output, hook manifest, parity check.

---

### 08.e — Generator: interface-typed parameters and [SameObject] on operations

**Depends on:** 08.d.

- **Interface-typed parameters** → emit `instanceof` checks and conversion helpers. E.g., `insertBefore(Node node, Node? child)` needs to verify `node instanceof Node` and unwrap/retrieve the internal ID.
- **Interface-typed return types** → emit `wrap()` call on the native ID.
- **`[SameObject]` on operation returns** → cached getter pattern.

**Target families:** Node (most operations take/return `Node`, `Element`, `Document`, etc.).

**Evidence:** New test cases with interface-typed parameters; run `cargo test --bin wega-webidl`.

---

### 08.f — Migrate Node family

**Depends on:** 08.a, 08.e.

`Node.webidl` (~80 lines) is the foundational family. It needs:
- Const members: `ELEMENT_NODE` through `NOTATION_NODE`, `DOCUMENT_POSITION_*` (covered by 08.a)
- `[Constant]` attributes: `nodeType`, `nodeName` (covered by 08.a)
- `[SameObject]` on `childNodes` (covered by 08.a)
- `[CEReactions]` mutation hooks: `normalize`, `cloneNode`, `insertBefore`, `appendChild`, `replaceChild`, `removeChild`, `nodeValue`, `textContent`
- Interface-typed parameters: `getRootNode`, `parentNode`, `parentElement`, `firstChild`, `lastChild`, `previousSibling`, `nextSibling`, `cloneNode`, `isEqualNode`, `isSameNode`, `compareDocumentPosition`, `contains`, `lookupPrefix`, `lookupNamespaceURI`, `isDefaultNamespace`, `insertBefore`, `appendChild`, `replaceChild`, `removeChild` (covered by 08.e)
- `[Throws]` on operations: `substringData` (if on Node; it's on CharacterData), `insertBefore`, `appendChild`, `replaceChild`, `removeChild`, `compareDocumentPosition`
- Dictionaries: `GetRootNodeOptions` for `getRootNode` (covered by 08.c)
- Interface-typed attribute: `ownerDocument` — emit as handwritten getter (same defer strategy as Attr.ownerElement)

**Scope:** Core Node operations only. Defer `getRootNode(dictionary)` to 08.c (dictionary support) or emit a simple `wrap(host("getRootNode", id))` that always returns the document root.

**Hooks:** `node.*` (~25 hooks for attributes + operations).

**Evidence:** Generator output, hook manifest, parity check against existing `Node` class in bindings.js.

---

### 08.g — Generator: union types, variadic arguments, partial interfaces, enums in inputs

**Depends on:** 08.f.

- **Union types** → emit type-dispatch conversion. E.g., `(DOMString or ElementCreationOptions) options` tries DOMString first, then validates as dictionary.
- **Variadic arguments** → emit `...rest` array parameter with type checks. Used by `Document.write`, `ChildNode.before/after/replaceWith`.
- **Partial interfaces** → merge multiple `partial interface X { ... }` declarations into a single IR before codegen.
- **Enum declarations in IDL** → emit `Object.freeze` map on the class.

**Evidence:** New test cases; run `cargo test --bin wega-webidl`.

---

### 08.h — Migrate Element family

**Depends on:** 08.g (partial interfaces, unions, variadic).

`Element.webidl` (~130 lines) plus includes:
```
Element includes ChildNode;
Element includes NonDocumentTypeChildNode;
Element includes ParentNode;
Element includes ActivatableElement;
Element includes ARIAMixin;
```

The `includes` clause injects members from mixins. The generator must:
1. Resolve includes: read mixin IDL files, extract members, inject into the interface's member list before codegen.
2. Handle mixin files that declare `interface mixin MixinName { ... }` — parse these separately and merge.

Element members:
- `[Constant]` attributes: `namespaceURI`, `prefix`, `localName`, `tagName` (covered by 08.a)
- `[CEReactions]` attributes: `id`, `className`, `slot` (covered)
- `[SameObject, PutForwards=value]` on `classList`, `attributes`, `part` (covered by 08.a)
- Operations: `hasAttributes`, `getAttribute`, `setAttribute`, `removeAttribute`, `getAttributeNames`, `closest`, `matches`, `getElementsByTagName`, etc. (primitive params — covered)
- Interface-typed params: `setAttributeNode(Attr attr)`, `removeAttributeNode(Attr oldAttr)` (covered by 08.e)
- Unions: `setAttribute(DOMString name, (TrustedType or DOMString) value)` (covered by 08.g)
- Variadic: `insertAdjacentHTML` (no, single arg), `setHTMLUnsafe` (no)
- Mixins (ChildNode): `before`, `after`, `replaceWith`, `remove`, `prepend`, `append`, `replaceChildren` — variadic + union params (covered by 08.g + includes support)
- Partial interfaces: scroll methods, DOM parsing (`innerHTML`, `outerHTML`), fullscreen, pointer capture, animations (covered by 08.g partial interface merge)

**Hooks:** `element.*` (~30+ hooks).

**Evidence:** Generator output, hook manifest, parity check against existing `HTMLElement`/`Element` in bindings.js.

---

### 08.i — Migrate Document family

**Depends on:** 08.g (partial, includes, variadic, unions).

`Document.webidl` (~120 lines) plus includes:
```
Document includes NonElementParentNode;
Document includes ParentNode;
Document includes GlobalEventHandlers;
Document includes DocumentOrShadowRoot;
```

Members include:
- Static operations: `parseHTMLUnsafe`, `parseHTML` — need generator support for `static` keyword
- Dictionaries: `ElementCreationOptions`, `ImportNodeOptions`, `SetHTMLUnsafeOptions` (covered by 08.c)
- Unions: `(DOMString or ElementCreationOptions)`, `(TrustedHTML or DOMString)`, `(WindowProxy or Element or HTMLCollection)` (covered by 08.g)
- Variadic: `write((TrustedHTML or DOMString)... text)`, `writeln` (covered by 08.g)
- Getters/indexed: `getter NamedPropertyValue(DOMString name)` — `document.all`-style dynamic properties. This is a `getter` member — currently rejected.
- Enums: `DocumentReadyState`, `DocumentVisibilityState` (covered by 08.g)
- `Partial interface` extensions: location/history, obsolete members, fullscreen, CSSOM-view, selection, animations (covered by 08.g)
- `includes` mixins: `GlobalEventHandlers` (adds `on*` attribute handlers) — need special handling (not pure IDL members, these are event handler IDL attributes)

**Decision for complex Document members:**
- `getter NamedPropertyValue` → defer to a later step or implement as a proxy handler
- `GlobalEventHandlers` includes → handle in JS layer (event handler IDL attributes are handled by existing `web.js` event wiring)
- `Promise<undefined> exitFullscreen()` → `Promise` returns are currently rejected; defer to a later step or emit stub
- `Static operations` → emit as static methods on the Document class

**Hooks:** `document.*` (~30+ hooks).

---

### 08.j — Migrate collections, mutation, observers and location

**Depends on:** 08.i.

Families:
- `NodeList`, `HTMLCollection`, `NamedNodeMap`, `HTMLAllCollection` — live collections with `length`, `item()`, `namedItem()`, indexed access. Need `getter` member support or emit JS-level class.
- `MutationObserver`, `MutationRecord` — observer with dictionary init, callback types.
- `Location` — `document.location`. Simple family with forwards attributes.
- `DOMTokenList` — already planned for 07.c.

**Approach:** Start with `Location` (simplest: forwards to `document.URL` with `href`, `origin`, etc.). Then `MutationObserver`/`MutationRecord`. Collections need `getter` member support — defer to a targeted generator extension.

---

### 08.k — Wire generated bindings into worker bootstrap

**Depends on:** 08.a through 08.j.

Replace handwritten wrappers for migrated families with generated output:
1. `build.rs` compiles generated `.js` files into the bootstrap snapshot after `bindings.js`.
2. `bindings.js` exposes the `__wega_dom` namespace; generated classes are installed on globals.
3. Handwritten wrappers for migrated families are removed from `bindings.js`.
4. `native_ops.rs` dispatches through generated hook manifest (same pattern as character-data).

**Evidence:** Existing DOM/Range/page suites pass unchanged; hook-manifest drift test rejects unregistered hooks.

---

### 08.l — Ledger update, evidence and progress closeout

**Depends on:** 08.k.

- Update `docs/standalone-feature-ledger.md` with migration status per family (F05 DOM row and related).
- Update `docs/validation/standalone/progress.json`.
- Close the contract for Step 08 in the execution plan.
- Evidence: passing DOM/CSS WPT subsets, real framework interactions (React, Vue element creation), negative security tests for stale nodes and reentrancy.

---

## Migration order rationale

```
08.a → 08.b → 08.c → 08.d → 08.e → 08.f → 08.g → 08.h → 08.i → 08.j → 08.k → 08.l
```

- **08.a–08.d**: Small generator extensions + small family (Event + Attr) — validates the approach with minimal risk.
- **08.e–08.f**: Interface-typed parameters + Node — the big leap. Node is foundational; if this works, everything else builds on it.
- **08.g**: Large generator extension (unions, variadic, partial, enums) — needed before Element/Document.
- **08.h–08.i**: Element + Document — the heaviest families, now that all constructs are available.
- **08.j**: Supporting families (collections, observers, location) — fills gaps.
- **08.k**: Bootstrap wiring — replaces handwritten wrappers end-to-end.
- **08.l**: Evidence and closeout.

---

## Closeout (3 November 2026)

Subtasks 08.a–08.l are delivered. Local evidence is recorded in
`docs/validation/standalone/progress.json` (`step_08_validation`); hosted
three-platform CI runs on the 08.l pull request.

**Delivered beyond the subtask list, found while wiring 08.k end to end:**

- `install()` keeps upstream non-configurable members (Deno `Event` constants,
  `[LegacyUnforgeable] isTrusted`); WebIDL constants are non-configurable.
- The pinned Deno event adapter adds `initEvent`, the spec `cancelBubble`
  setter, HTML `window.event` and DOM capture-at-target dispatch.
- `bindingCheck` validates node liveness only; each document owns its realm.
- DOM algorithms throw `DOMException` (JS errors only for Web IDL simple errors).
- Pre-insertion/replace validity, ChildNode/ParentNode conversion and viable
  siblings, and DOMTokenList update/replace steps follow the DOM specification.
- Value iterators use `%Array.prototype%` methods; indexed-getter interfaces
  without `iterable<>` (HTMLCollection, NamedNodeMap) expose only `@@iterator`.
- Location members are `[LegacyUnforgeable]` own properties with unforgeable
  `valueOf` and `@@toPrimitive`.

**Evidence:**

| Gate | Result |
|---|---|
| Pinned DOM WPT subset (`engines/wega/tests/wpt.rs`, revision `68d964a5`) | 35 files, 1685 cases, all pass; floor raised from 31 |
| Families with upstream WPT coverage | Event/EventTarget, CharacterData, Node, ChildNode, Element, Document, Attr/NamedNodeMap, NodeList, HTMLCollection, DOMTokenList (1420-case `Element-classlist`), MutationObserver, Location |
| Framework fixtures (`compat.rs`) | React, Vue, Preact and the other pinned fixtures pass |
| Generated-binding contracts (`dom.rs`) | Brand checks, illegal constructors, typed-argument rejection, constant descriptors, `[SameObject]`, hidden bootstrap internals, reentrant mutation during conversion and dispatch; proven by mutation of the brand check |
| Detached wrappers, DOM budgets | Existing `dom.rs` budget/detached regressions pass |
| Engine and protocol suites | 211 tests, all targets, 0 failures; strict Clippy and fmt clean |

**Explicit blockers (not counted as supported):**

| Blocker | Upstream WPT files affected | Owning step |
|---|---|---|
| `DOMImplementation` (`createDocument`, `createDocumentType`, `createHTMLDocument`), XML/XHTML documents | Document-createElement, Node-insertBefore, Node-replaceChild, Node-isEqualNode, Node-appendChild, ParentNode-append/prepend, Element-tagName | 08 follow-up |
| ProcessingInstruction and CDATASection nodes | MutationObserver-characterData, Node-isEqualNode | 08 follow-up |
| Per-tag HTML element interfaces beyond HTMLElement/HTMLHeadElement/HTMLScriptElement | Document-getElementById, Node-cloneNode | 08 follow-up (HTML element families) |
| Legacy platform object named properties | HTMLCollection-empty-name, HTMLCollection-own-props, namednodemap-supported-property-names | 08 follow-up |
| Namespace-aware attribute matching, `Attr.value` mutation records, `toggleAttribute` casing | Element-setAttribute, attributes, MutationObserver-attributes | 08 follow-up |
| MutationObserver childList records for `textContent`/`normalize` | MutationObserver-childList | 08 follow-up |
| Selectors `:invalid`, `:has()` | Element-closest | 13 |
| Fixtures that need iframes or XML documents | Node-removeChild, Node-contains, Node-textContent, location-protocol-setter-non-broken | 14 |

## Commands

```sh
cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- verify
cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- check
cargo test --locked --no-default-features --features webidl-tools --bin wega-webidl
```

---

## Risk and dependency notes

| Risk | Mitigation |
|------|-----------|
| Node has 50+ members; migration is a large rewrite | Each member maps to one hook; parity tests catch regressions per-method |
| `includes` mixins require reading additional IDL files | Add `load_mixin()` to the parser; includes are resolved at build time |
| `GlobalEventHandlers` adds 40+ `on*` attributes | Handled in `web.js` event wiring, not in the generator |
| `getter NamedPropertyValue(DOMString)` for `document.all` | Defer to proxy handler in `bindings.js` or targeted generator extension |
| `Promise` return types and callback interfaces | Deferred to a later step; stub as `undefined` with a ledger blocker |
| `TrustedHTML` / `TrustedType` union | Treated as `DOMString` for parity; type enforcement is a security step |
| Live collections (`childNodes`, `HTMLCollection`) require revision-aware caching | Existing `nodeList()` wrapper in bindings.js already handles this; generated code uses it |

---

## Completion evidence

**Done when:** each migrated family passes its selected upstream IDL/behavior tests and real framework interactions; wrapper GC, detached nodes, DOM budgets and reentrant mutation remain correct. No missing-feature success shims are counted.

| Evidence type | Source |
|---------------|--------|
| Deterministic regeneration | `cargo test --bin wega-webidl` |
| Hook manifest parity | Drift test in `cmd_generate(check)` |
| Framework interaction | React/Vue element creation, `document.createElement`, `appendChild` |
| Range/live-collection preservation | Existing `Range` tests in `engines/wega/tests/` |
| Reentrant mutation safety | Re-entrancy test in generated binding tests |
| Wrapper GC identity | Finalization test for detached nodes |
| Platform evidence | DOM/CSS WPT subsets on macOS, Linux, Windows |
