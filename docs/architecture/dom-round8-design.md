# Round 8 design: event routing, focus and input boundary

Written before implementation. Evidence is the source tree at `4ae5c65` plus a behaviour probe (focus/trusted events) run against that build.

## 1. Who owns what today (audit)

| Concern | Real owner | Evidence |
|---|---|---|
| Listener registration, capture/target/bubble, `once`, `passive`, `stopPropagation`, `stopImmediatePropagation`, `preventDefault`, `composedPath`, retargeting, `isTrusted` storage | **Deno `deno_web` `02_event.js`** (`Event`, `EventTarget`, `dispatch`), used because `semantics.js:18` finds `__wega_web`. Wega's `Node extends EventTarget` supplies `parentNode`, `getRootNode`, shadow `host` and calls `setEventTargetData` (`semantics.js:578`). | `semantics.js:18,578,4736`; `web.js:5-65`; `deno_web 02_event.js` `getParent`/`dispatch` |
| JS fallback dispatcher (`FallbackEvent`, `FallbackEventTarget`) | Wega, **dead code** unless `__wega_web` is absent (no such configuration exists in the engine) | `semantics.js:141-265`, selected at `:266` |
| Creating trusted native events in JS | Wega: `__wega_native_event(type, nodeId, options)` (`observers.js:227`) | creates the event object and calls `target.dispatchEvent`; never marks it trusted |
| Rust → JS routing | Wega, **11 string-built calls** to `__wega_native_event` in `view.rs` (lines 1153, 1518, 1821, 1898, 1926, 1960, 2045, 2073, 2224, 2236, 2248), each formatting `u64.to_string()` ids by hand; plus `__wega_submit_event` (1845, 2195) | grep |
| Hit-testing and hover state | **Blitz** (`set_hover_to`, `clear_hover`, `get_hover_node_id`); Wega calls them at `view.rs:1536-1542` | |
| Focus state | **Blitz** (`focus_node_id`, `pub(crate)`; `get_focussed_node_id()` falls back to the root element). Wega reads it at `view.rs:1943,2213`, `dom.rs:1183` (`document.activeElement`) and writes it at `dom.rs:1301` (`element.focus()`) and `view.rs:1120` (accessibility). JS keeps a second, unused-for-reads `_active` (`semantics.js:1626-1632`). | |
| Focus/blur events | **Blitz generates** `Focus/Blur/FocusIn/FocusOut` in `handle_dom_event` for click and Tab; Wega forwards them from `generated_events` (`view.rs:1868`). JS `focus()`/`blur()` dispatch their own partial events. | probe |
| Active / mousedown node (click synthesis) | Blitz (`active_node`, `unactive_node`, `set_mousedown_node_id`, `view.rs:1573-1584`) | |
| Default actions | Mixed: Wega (`default_click` for submit/reset/file/new-window/fragment, implicit submission, clipboard shortcuts, checkbox toggle in `key`, `view.rs:1699-1868, 1936+`) then Blitz (`handle_dom_event`, 5 call sites) for text editing, selection, focus, link activation | |
| Shadow DOM metadata (host, root, slots) | Wega `components.rs` + `dom.rs` ops (`shadowAttach`, `componentRoot`, `assignedSlot`) | |
| Document replacement / unload | Wega `view.rs` `set_document` (new `BaseDocument`, new `Rc<RefCell>`, new script host) | |

Existing functionality versus missing platform behaviour, measured by the probe on the unmodified build:

| Behaviour | Result |
|---|---|
| click: `mousedown, focus, focusin, click`; click elsewhere: `blur, focusout, focus, focusin` (Chrome order) | works |
| Tab moves focus with `keydown`, `blur, focusout, focus, focusin` | works |
| `isTrusted` on native events | **false (bug)** |
| `relatedTarget` on focus events | **null (gap)** |
| `el.focus()` | dispatches only `focus` — no `blur` on the previous element, no `focusin` **(bug)** |
| `el.blur()` | dispatches `blur`; `document.activeElement` is still the element **(bug)** |
| click on a non-focusable element | focus is not moved to the body **(gap, Blitz default action)** |

## 2. Decision: three separate concerns, no single large trait

1. **Dispatch semantics** stay where they are (deno_web). Nothing in Rust re-implements capture/bubble. No contract is needed and none is added.
2. **Native input target selection and focus/hover state**: a small contract `wega_dom::input` — `FocusState` (read: `focused`, `keyboard_target`, `hovered`) and `InputControl` (write: `set_focus`, `clear_focus`, `pointer_moved`, `pointer_left`) — implemented once by `BlitzDocument`. It reuses `NodeKey`, rejects stale keys, exposes no Blitz event type and allocates nothing. Blitz stays the owner of the state (text editing and the caret depend on it); Wega reads and writes it only through the contract, so there is **no second authoritative focus state**. The JS `_active` copy is removed.
3. **Default actions** stay engine-side (`default_click`, `key`) and in Blitz's `handle_dom_event` (text editing, selection, click synthesis from mousedown/mouseup). They are documented, not abstracted: they are policy of the engine, with a single implementation.

Plus one Wega-owned *routing* type in the engine (not the contract crate): `NativeEvent { kind, target: NodeKey, related: Option<NodeKey>, init }` and one function that turns it into the single JS call, replacing the 11 hand-formatted strings. It lives in the engine because its only consumer is the engine's JS bridge.

### Where the contract lives: `wega-dom`, not a new `wega-events`

Real dependency direction: the contract is consumed by `wega-engine` and implemented by `wega-adapter-blitz`; both already depend on `wega-dom`; `wega-html` does not need it. A `wega-events` crate would have the same two dependents and nothing else, and would need `wega-dom` for `NodeKey` anyway, so it would only add a manifest and an edge. Dispatch semantics (the part that could justify an independent crate) are not Rust code. If a second implementation of event semantics ever appears in Rust, extract then.

### Focus transitions: one Wega-owned path

Blitz changes focus as a side effect of its default actions and of `set_focus_to`. The engine observes `FocusState::focused()` before and after every operation that can change focus (pointer down/up defaults, key defaults, `element.focus()`, `element.blur()`, accessibility focus) and emits `blur`, `focusout`, `focus`, `focusin` (with `relatedTarget`) itself, once. Blitz-generated focus events are dropped at the forwarding point so nothing is dispatched twice. Document replacement clears focus without events.

## 3. Scope fences

No rewrite of the JS event system; no Shadow DOM conformance claim beyond tests that pass; no layout/CSS/render changes; no new unsafe; Blitz and Deno stay pinned.
