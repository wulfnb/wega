# D3-A: read-only Elements inspector

Design committed before implementation, based on D0, D1/D2 designs and reports,
the implementation roadmap, D3 readiness audit and D3.0 report. Base `3f1ccb4`.
This slice supersedes the older roadmap's combined DOM/edit/style D3 scope:
**read-only DOM only**. D3-B/C are not part of this change.

## Prerequisites verified before implementation

Built `wega-engine` and `wega-content-bundle` with the engines lockfile, packaged
`/tmp/WegaD3A.app` and verified its hardened App Sandbox signature. Ran the existing
`inspector_sandbox -- --ignored --nocapture`: **1 passed**, including D3.0's
principal consistency refusals, grants, console, evaluation and detach revocation.
The outer execution sandbox initially refused the fixture's loopback bind; the
rerun allowed the test host's fixture, preserving the content App Sandbox.

Reproduced WebDriver's line 76 failure (`AssertionError: New tab`) using
`python3 scripts/native-tests.py --suite webdriver` with its original defaults.
The fixture received no HTTP request. The desktop discovers `target/debug/wega-engine`
(unsigned) and required launch refuses it without the explicit development opt-in.
With `WEGA_ENGINE_BINARY=engines/target/debug/wega-engine` and development opt-in:
**whole smoke passed**, 19.41 s. With the freshly signed helper and **no development
opt-in**: **whole smoke passed**, 14.99 s. Both runs fetched capabilities and second
fixtures and passed title, script title, cache, back/forward/refresh and header checks.
This is test launch configuration, not a title/DOM browser regression. The native
test wrapper now honors `WEGA_CONTENT_HELPER`, matching the required-worker suites.
Raw logs are `/tmp/wega-d3a-prereq-{sandbox,webdriver,webdriver-default,webdriver-sandbox}.log`;
final report will include durable evidence and rerun the fixed wrapper.

## Contract and identity audit

The one authoritative document is `View.document: Rc<RefCell<BlitzDocument>>`.
`DocumentView`/`NodeView` expose borrowed nodes, qualified names, attributes,
parents, exact child counts and O(1) `child_at` in the adapter. Missing contract:
borrowed **own** text/comment data; add `NodeView::text()` without descendant
concatenation. No second DOM, engine handles, script execution or layout work.

`NodeKey`'s documented contract forbids aliasing a dropped node. Verified actual
adapter `ToKey`/`ToNodeId` preserve all u64 bits, `NodeTree` uses slotmap `KeyData`
index plus version and `get_node` checks it. Adapter contract tests explicitly
drop/reuse a slot and reject the old key. Attribute/text edits and reparenting
preserve keys. A separate handle mapping would duplicate this existing generation
mechanism and require eviction semantics; none is needed for this adapter.
Wire `InspectorNodeId { document: DocumentContext, key: u64 }` names no adapter
type. Document stamp includes navigation generation and actual replacement revision.
Target/session/process authorization happens before resolution. Every node use also
checks the stamp and bounded ancestry to the current root. Detached nodes, template
fragments, shadow trees, anonymous layout boxes and child documents are not targets.
Connected light DOM only; iframe elements are inspectable, frame content is not.

## Protocol and budgets

Bump engine version to 24 and inspector version to 3; add `InspectorDom` capability
and explicit v3 command gating. Closed commands: `GetDomRoot`, `GetDomChildren`,
`DescribeDomNode`. Description is also selection validation; selection belongs to
the UI and confers no page authority. Child args carry node, epoch, offset and limit.
Continuations are bound to mutation epoch and rejected after mutation; node IDs
survive mutation independently. Responses carry document, epoch, parent/offset,
next offset, bounded metadata and truncation indicators. No recursive wire tree.

Hard caps: 64 nodes/page, 128 ancestry levels, 32 attributes/node, 256 UTF-8 bytes
per name/text/value, 64 KiB serialized response, 10 ms DOM work per request
(cooperative checks at each node/ancestor/attribute), existing 2 s transport
deadline and four pending requests per inspector. Direct indexing avoids scanning
an entire wide parent just to page near its end. Response byte accounting uses
JSON-escaped sizes, not raw string lengths. Budget truncation has a continuation;
depth/time exhaustion is an explicit error. No recursive traversal, subtree text,
query-selector scan, full-tree count, synchronous layout or forced paint.

`StatusSnapshot.mutation_epoch` from D3.0 is the change signal, polled at the existing
1 Hz interval only while tools are open. No new always-on observer or console hook.
Document replacement clears all DOM state. Epoch inequality marks cached rows
stale and refreshes root/expanded pages lazily, one request at a time; it does not
replace document identity or clear a surviving selection. A mutation storm must
not start a recursive full-tree rebuild. Stale IDs show a bounded notice and clear
selection. CSSOM in-sheet epoch gap is unchanged and outside read-only DOM scope.

## Browser and UI

Requests go through existing Page authority/DocumentRequests and ContentSessions.
Validate response bounds again before retaining them. UI cache: at most 512 rows
and 512 KiB serialized data, bounded expansion state, at most one DOM request in
flight. Evict/collapse cached branches when necessary; offer a clear reset action.
Rows are a presentation cache, never a DOM authority. Flatten visible rows
iteratively with a depth cap; virtualization keeps drawing proportional to the
viewport. Expand/collapse, child-page continuation, attributes/text metadata and
selection use plain egui text (no HTML evaluation). No editing or highlighting.

Reuse DevtoolsView lifecycle: dock follows active tab, detached window remains
pinned, close detaches, process replacement reattaches, navigation discards stale
requests. Revalidate target, document and live session at paint and request time,
including a navigation arriving between tick and paint. Detached window shares
the same Elements rendering and bounded cache.

## Staged validation and delivery

1. This design/prerequisite commit.
2. Protocol, borrowed DOM text contract and engine backend: shape/version/bounds,
   pagination, mutation/reparent/drop/reuse, root connectivity, depth/time/bytes,
   no flush and no JS, real-process and sandbox negative tests; commit.
3. Browser/UI: authority, cache/pending bounds, lazy epoch refresh, selection,
   lifecycle and native/detached window checks; commit.
4. Full engine, broker, WPT/CSS-WPT, architecture/vendor/boundary, locked metadata,
   formatting, strict clippy and unsafe diff checks; latency/RSS measurements for
   small, wide and deep DOMs plus D3.0 console/mutation checks; final report commit.

Report exact executed counts, measurements and limitations. macOS execution does
not certify Linux/Windows. D3-B needs a separate styles contract and the CSSOM epoch
gap fixed before editing. D3-C must resolve the existing depth-122 layout hang
before introducing any synchronous inspector layout. No push.
