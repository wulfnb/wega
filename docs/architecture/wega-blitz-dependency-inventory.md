# Blitz dependency inventory

## 2026-11-18 continuation

Production now owns `WegaDocument`: one existing `wega_dom::Document` with
resource/editor side tables keyed by Wega identities. View, V8, navigation,
HTML/XML parsing, lifecycle and frame references use that owner. Native rendering
is mandatory; no reference DOM or reference-rendering fallback exists in View.
The first 53 targeted owner/lifecycle/resource checks pass; complete acceptance
and security validation for this owner remains outstanding.

The actual normal browser graph still contains **four** reference packages:
`blitz-dom`, `blitz-paint`, `blitz-traits`, `wega-adapter-blitz`. The direct adapter
and DOM edges have not yet moved to dev-dependencies. The only remaining direct
`blitz-dom` production call installs the old SVG broker font database; networking
and form/navigation payloads still name `blitz-traits`. Component independence
is not production independence. Reference comparison harnesses must be adapted
to explicit dev-only oracles before those manifest edges are removed.

## Historical inventory before the owner transition

The table and original five-package evidence below describe the prior heads.

Start: `feature/dom-rounds-9-12`, clean `8523150487a953394920e69459ad7f374e57297c`.
No pushes authorized. DevTools D4 and inspector implementation excluded.

Machine evidence: `baseline/blitz-removal-2026-11-17/production-tree.txt`,
`test-tree.txt`, `metadata.json`, `blitz-source-references.txt` and
`inventory-commands.json`. The source index records individual imports,
re-exports, manifests, lockfiles, build/packaging and comparison references.

| Dependency / responsibility | Current owner and classification | Migration / removal strategy |
|---|---|---|
| `BlitzDocument` | Production `wega-adapter-blitz`, wraps `BaseDocument` by value and dereferences to it. `View`, `DomHost`, `FrameContext`, `ScriptHost`, native `NativeDocument` share `Rc<RefCell<BlitzDocument>>`. | Replace authoritative storage, not a type rename. Migrate concrete calls before switching owners; no mirror tree. |
| `blitz-dom` | Direct production dependency of engine and adapter. Owns slotmap node identities, tree, mutation side effects, Stylo CSSOM, resources, input/editor and reference geometry/accessibility. | Wega arena with stable non-reused keys; Wega mutation/invalidations; migrate CSSOM/resources/input behind explicit Wega contracts. |
| `blitz-html` | Production adapter one-shot HTML/XML construction; transitive `blitz-dom`. Also comparison/parser tests. | Existing `wega-html` html5ever tree sink is generic over `DocumentMutation`; instantiate with Wega owner. Preserve incremental scripts, fragments/templates and XML behavior. Replace parser oracle only after equivalent independent tests. |
| `blitz-paint` | Production adapter's `PaintSource`; reference renderer can be selected at runtime. Transitive `blitz-dom`, traits. | Native scene for all runtime surfaces and frames before optional development-only reference feature. |
| `blitz-traits` | Direct production engine/adapter networking/navigation/event contracts; transitive via DOM/paint/HTML. | Wega broker-facing contracts with identical authorization and per-hop CSP. Do not substitute direct networking. |
| `blitz-net`, `blitz-shell` | No engine manifest edge/import found; verify full metadata graph. | No runtime migration expected; retain evidence of absence. |
| `engines/vendor/blitz` | Mixed ownership: patched DOM/paint; independent Stylo, AnyRender/Skia and skia-bindings sources; license/build provenance. | Do not delete directory wholesale. Independent mature crates may stay, relocated with provenance when actual runtime Blitz removal passes. |
| V8/JavaScript | Adapter V8 is VM-only; concrete engine `dom.rs`, `script.rs`, `view.rs`, `frames.rs` still use Blitz IDs/document and direct node/control methods. | Generic or Wega-owned DOM host using one tree; preserve JS wrapper identity, synchronous geometry and frame access restrictions. |
| CSSOM/stylesheets | Authored rules, owner IDs, resource generations and CSSOM operations reside in Blitz DOM. Native CSS serializes these authored rules, not Stylo computed output. | Wega owner-backed ordered sheets, media/import state and mutation generation with existing CSSOM/security tests unchanged. |
| HTML nodes/mutations | `wega-dom` defines contracts; adapter implements mutation guard; generic html5ever parser already exists. Concrete owner remains Blitz. | Flat arena; iterative traversal/clone/drop; atomic validation, template fragments, parent links and mutation epochs; bounded depth policy retained. |
| Resource loading/images/fonts | Engine broker via Blitz net provider; native bridge reads decoded raster/SVG/fonts and sheet sources from Blitz. | Wega resource registry and bounded independent decoding, explicit cancellation/CSP/redirect policy. Preserve duplicate-discovery request counts. |
| Forms/selection/events | Native decoration uses Blitz `TextInputData` and editor; view dispatch mutates Blitz radio/file/focus state; V8 form operations use same owner. | Wega control state with existing editor/shaper primitives, focus/selection/events, no API narrowing. |
| Layout/paint/picking | Native CSS/layout/display list and native geometry are Wega-owned; native bridge still consumes Blitz owner/resources. Runtime reference path remains. | Retain native algorithms; remove bridge only after DOM/resources/control migration and Chrome/security checks. |
| Accessibility | Native AccessKit built from Wega layout plus shared DOM; reference accessibility still Blitz. | Wega tree and native geometry exclusively; preserve names/focus/actions/frame relationships. |
| Comparison/tests | Adapter tests and native/reference corpus, benchmark, geometry/parser/style tests instantiate Blitz. Font fixtures also reference vendor assets. | Keep verification value; migrate to Chrome/WPT/independent goldens before deleting reference sources. |
| Development tooling | `engines/v8-spike` excluded from workspace; architecture/dependency tools and vendor integrity/packaging scripts reference adapter/vendor paths. | Retire obsolete spike only after runtime independence; update architecture rules/provenance deliberately. |
| Root application | Root communicates with packaged `wega-engine`; engine has independent Cargo workspace/lockfile. Root lockfile is not runtime graph proof. | Check worker graph/build plus actual native view/sandbox; root build alone cannot prove independence. |

The native backend is opt-in (`WEGA_RENDER_BACKEND=native` / view selection).
Production constructors in `view.rs` create Blitz documents for blank/navigation
and share them with the native backend. Existing native rendering tests therefore
do **not** establish native-only construction or linking.

## Verified component migration

`wega-native` now owns CSS, selector/cascade syntax, layout, Parley shaping,
SVG geometry/serialization and display-list algorithms. The engine re-exports
these modules for existing callers. The crate's normal/build and test graphs
exclude all Blitz packages; its owner-to-raster test uses only `wega-dom::Document`.
Inline SVG decoding moved to the existing resource bridge with the same bounded
SVG decoder; the new algorithm crate never fetches or decodes resources.

`wega-dom::Document` supplies flat owned nodes and the html5ever mutation contract.
It is **not wired into production views yet**. The five production dependency
names remain: `blitz-dom`, `blitz-html`, `blitz-paint`, `blitz-traits`,
`wega-adapter-blitz`. The signed worker/browser is not native-only, even when its
native rendering backend is selected. No reference code/vendor source was deleted.

## Final graph/build evidence at `d4d2098`

- [Production graph](baseline/blitz-removal-2026-11-17/production-tree-final.txt)
  and [test graph](baseline/blitz-removal-2026-11-17/test-tree-final.txt).
- [Production gate failure](baseline/blitz-removal-2026-11-17/production-graph.json)
  lists all five remaining names; exit 1 is deliberate evidence of incomplete
  runtime independence, not an ignored or weakened test.
- [Independent component graph](baseline/blitz-removal-2026-11-17/independent-graph.json)
  passes; the actual independent component build and owner-to-raster tests pass.
- [Final import/source index](baseline/blitz-removal-2026-11-17/blitz-source-references-final.txt)
  retains exact concrete consumers and lockfile/build/tooling references.
- No `blitz-net` or `blitz-shell` package occurs in either final graph. Independent
  Stylo/AnyRender/Skia vendoring and font-fixture/license paths are separate from
  the remaining Blitz runtime edges; the vendor directory must not be deleted.

A normal no-default-features worker was also built and signed successfully. It
still has those production edges and therefore does not satisfy the native-only
browser build gate. No obsolete adapter/vendor/lockfile entry was removed merely
because the native algorithm component passed.
