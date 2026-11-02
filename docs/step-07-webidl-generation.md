# Step 07: WebIDL generation pipeline

Branch: `feature/standalone-wega-engine`. Started 2 November 2026, after Step 06
acceptance at `efe5a87`. This document records the subtask split, the supported
IDL construct policy and the binding/hook conventions. Completion evidence per
subtask is recorded in `docs/validation/standalone/progress.json`.

The pipeline replaces handwritten interface wrappers with deterministic,
generated binding code plus Rust-implemented semantic hooks. Generation is a
tool, not a runtime: outputs are checked in, byte-compared in tests, and only
then embedded in the worker bootstrap.

## Subtask split

### 07.a — Pinned IDL inputs and deterministic generator

- `tools/webidl/` Rust tool (`wega-webidl` binary, `webidl-tools` feature):
  WebIDL subset parser with positioned errors, pinned-input manifest
  verification, family configuration, deterministic JS emitter and a
  machine-readable semantic-hook manifest.
- `tools/webidl/idls.json` pins every IDL file the pipeline consumes with
  sha256, spec origin and license; `tools/webidl/families.json` declares each
  generated family (files, native base class, base-provided mixins, output
  paths).
- First pinned family: `character-data` (`CharacterData`, `Text`, `Comment`);
  generated output checked in under `engines/wega/src/dom/generated/`.
- Unsupported IDL constructs fail generation explicitly with a positioned
  diagnostic. The generator emits only what a family uses.
- Evidence: determinism test (regenerate → byte-identical), manifest hash
  verification, explicit-failure fixtures, `cargo test --bin wega-webidl`.
- The generated file is **not** wired into the worker bootstrap in 07.a.

### 07.b — Migrate the character-data family to generated bindings

- Rust semantic hooks in `Dom` (`charData.*`), dispatched through one generic
  `op_dom_hook` op; hook names come from the family manifest.
- `bindings.js` exposes the `__wega_dom` namespace (base class, brand,
  wrap/reaction helpers, hook bridge); the handwritten `CharacterData`,
  `Text` and `Comment` classes are removed and the generated classes are
  installed on the same globals.
- build.rs compiles the generated file into the bootstrap snapshot after
  `bindings.js`.
- Evidence: existing DOM/Range/page suites pass unchanged; new tests cover
  constructors, error names, brand checks, `LegacyNullToEmptyString` and pure
  getter non-invalidation. A hook-manifest drift test rejects unregistered
  hooks.

### 07.c — DOMTokenList family

Pins `DOMTokenList.webidl`; extends the generator with variadic arguments,
optional arguments, stringifiers, iterable members and nullable getters. The
live list wrapper is keyed by (element, attribute) with re-entrancy through
mutation observers during `add`/`toggle`.

### 07.d — Wrapper identity, GC and re-entrancy closeout

Brand symbols and `Illegal invocation` checks across the generated surface,
same-realm `instanceof` behavior, weak wrapper cache reuse, node-lifetime
rules for generated wrappers and re-entrant native call rules. Closes the
contract, ledger and progress records for Step 07.

## Supported construct policy (07.a)

Supported in pinned inputs: `interface` with inheritance, `[Exposed=Window]`,
`attribute`/`readonly attribute` with `[Pure]`, `[Throws]`, `[NewObject]`,
`[CEReactions]`, `[LegacyNullToEmptyString]`, `constructor` with `optional`
arguments and defaults, operations, `includes` clauses (members must be
provided by the native base), and the types `DOMString`, `DOMString?`,
`boolean`, `unsigned long`, `unsigned short`, `float`, `double`, `undefined`
and interface identifiers.

Explicit generation failures (positioned diagnostic, non-zero exit):
`Worker` exposure, `const` members, `stringifier`, `iterable<...>`, variadic
arguments, `getter`/`setter`/`deleter` members, union types, dictionaries,
`callback`/`enum`/`typedef` declarations, dictionary-valued argument defaults,
`[Constant]`, `[SameObject]` and any unknown extended attribute or type.
07.c lifts the stringifier/iterable/variadic/getter/optional restrictions for
the DOMTokenList family only, with the same explicit-failure behavior for the
rest.

## Conventions

- **One hook per IDL member**, named `<family>.<camelCaseMember>` (e.g.
  `charData.substring`). The hook manifest is the single source of truth; the
  Rust dispatch table must cover exactly the manifest hooks (drift test).
- **Hooks own validation and mutation** in Rust, including UTF-16 code-unit
  offset semantics (WebIDL offsets are JS string indices, not UTF-8 bytes).
  A hook returns the structured result the remaining standard steps need
  (e.g. the old data, the effective clamped count, a new node id plus its
  info). Hook errors are exactly `"ExceptionName: message"`; the generated
  binding rethrows them through the handwritten `fail` helper so error names
  match the current DOMException behavior.
- **Reactions stay in the JS layer.** Range boundary rebalancing and
  MutationObserver records operate on JS-owned objects; the generated code
  calls named namespace reaction helpers after each hook. The manifest
  records which hook triggers which reaction.
- **Conversions are emitted by the generator**, once per type a family uses
  (`DOMString`, `unsigned long`, `boolean`, nullable and legacy-null variants),
  matching WebIDL conversion semantics.
- **Wrapper identity:** generated classes extend the native base (handwritten
  `Node`) through the `__wega_dom` namespace, carry a per-interface brand
  symbol set in the constructor, and brand-check every member. Wrappers keep
  the existing `wrap()` proxy/cache/retain/finalize lifecycle; `instanceof`
  follows the prototype chain.
- **Exposure:** 07.a only admits `[Exposed=Window]`; the emitted install is
  guarded by `dom.exposure("Window")`. Multi-realm and `Worker` exposure are
  explicit generator failures until 07.d defines the realm model.
- **Determinism:** stable ordering (IDL declaration order), no timestamps,
  pinned sha256 recorded in each output header. `wega-webidl check`
  regenerates in memory and byte-compares the checked-in outputs.

## Commands

```sh
cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- verify
cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- check
cargo test --locked --no-default-features --features webidl-tools --bin wega-webidl
```
