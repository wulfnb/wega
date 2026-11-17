# Wega-owned authoritative tree: migration design

This is an incremental replacement, not a renamed reference document or a mirror.
The initial implementation is an independently constructible owner in `wega-dom`.
Production views must not switch until their concrete CSSOM/resource/input/VM
calls can use the new owner with existing observable behavior and security gates.

- Flat ownership: one document owns a map of nodes. Parent/child/template edges
  contain keys, never Rc nodes. Destruction, deep clone and subtree deletion use
  explicit work lists; dropping a very deep tree cannot recurse through owners.
- Keys: process-unique monotonically allocated u64 identities, never reused.
  Foreign-document and deleted-node lookups fail. JavaScript wrappers retain the
  exact key while detached nodes stay alive until explicitly collected.
- Parser: the existing standards-oriented html5ever `wega-html::Parser` writes
  directly through `DocumentMutation`; no new tokenizer/parser. Fragment context,
  namespaces, templates, foster parenting, malformed input and script pauses
  retain the same parser. XML construction needs its separate existing behavior.
- Mutation: validate every key and cycle before changing links. Reparenting moves
  nodes. One epoch per effective operation; creation detached is not a mutation.
  Explicit dirty state carries structure/style/layout invalidation to consumers.
- Scope: this milestone supplies node storage, namespaced attributes, text,
  templates/fragments, traversal, mutation, clone/drop and invalidation. It does
  not claim to replace stylesheet/resource/control ownership or the V8 host yet.
- Integration order: generic DOM readers/parser; stylesheet registry/CSSOM;
  authorized resource/cancellation/decoder state; control/editor/focus state;
  V8 concrete operations; frame document lifetimes and accessibility; then switch
  a view to exactly one new owner and run all runtime gates. Never run two trees
  in a view or silently fall back for an unsupported operation.
- Existing parser node budgets, 128 native layout depth, edit depth, CSS budgets,
  CSP redirect checks and 32-frame limit remain at their current enforcement
  points. Flat storage itself does not grant unlimited accepted browser depth.
- Event dispatch uses key paths and revalidates after callbacks; tree borrows may
  not span V8 callbacks. Frame principals/document tokens remain engine-owned.

Production removal remains blocked on the concrete calls inventoried in
`wega-blitz-dependency-inventory.md`. A standalone owner test is not evidence that
Wega's production runtime builds without reference crates.
